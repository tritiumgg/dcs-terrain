//! What a packed file says about itself: its `meta`, its `grid` table and how
//! many rows each vector table holds.
//!
//! Read straight from the file, with no reader behind it yet.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Instant;

use rusqlite::{Connection, OpenFlags};

use super::tables::VECTOR;
use crate::types::{DescribeResponse, GridInfo, Provenance};

pub fn describe(path: &Path) -> Result<DescribeResponse, String> {
    let started = Instant::now();
    let fail = |e: rusqlite::Error| format!("{}: {e}", path.display());
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(fail)?;
    conn.execute_batch("PRAGMA query_only = 1").map_err(fail)?;

    // A NULL key is one the file does not know, and is left out.
    let meta: BTreeMap<String, String> = conn
        .prepare("SELECT key, value FROM meta WHERE value IS NOT NULL")
        .and_then(|mut s| s.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect())
        .map_err(fail)?;
    let grids = conn
        .prepare(
            "SELECT name, cell_size, origin_x, origin_z, width, height, dtype, tile_size,
               nodata, unit, derived_from, params FROM grid ORDER BY name",
        )
        .and_then(|mut s| {
            s.query_map([], |r| {
                Ok(GridInfo {
                    name: r.get(0)?,
                    cell_size: r.get(1)?,
                    origin_x: r.get(2)?,
                    origin_z: r.get(3)?,
                    width: r.get(4)?,
                    height: r.get(5)?,
                    dtype: r.get(6)?,
                    tile_size: r.get(7)?,
                    nodata: r.get(8)?,
                    unit: r.get(9)?,
                    derived_from: r.get(10)?,
                    params: r.get(11)?,
                })
            })?
            .collect()
        })
        .map_err(fail)?;
    // A table a later pack adds is counted once it is in the list; one the
    // file lacks is left out rather than counted as empty.
    let mut row_counts = BTreeMap::new();
    for table in VECTOR {
        let present: bool = conn
            .query_row(
                "SELECT count(*) > 0 FROM sqlite_schema WHERE type = 'table' AND name = ?1",
                [table],
                |r| r.get(0),
            )
            .map_err(fail)?;
        if present {
            let n: i64 = conn
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
                .map_err(fail)?;
            row_counts.insert(table.to_string(), n as u64);
        }
    }

    let key = |k: &str| {
        meta.get(k)
            .cloned()
            .ok_or_else(|| format!("{}: meta has no {k}", path.display()))
    };
    let cell_size = key("cell_size")?
        .parse()
        .map_err(|e| format!("{}: cell_size: {e}", path.display()))?;
    let layers_dropped = serde_json::from_str(&key("layers_dropped")?)
        .map_err(|e| format!("{}: layers_dropped: {e}", path.display()))?;
    let validation: BTreeMap<String, String> = meta
        .iter()
        .filter(|(k, _)| k.starts_with("validation_"))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    Ok(DescribeResponse {
        provenance: Provenance {
            theatre: key("theatre_id")?,
            dcs_build: key("dcs_build")?,
            cell_size,
            elapsed_ms: started.elapsed().as_millis() as u64,
        },
        meta,
        grids,
        row_counts,
        layers_dropped,
        validation: (!validation.is_empty()).then_some(validation),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::{CELL_SIZE_M, Extract};
    use crate::pack::grids::{BASE, cells};
    use crate::pack::{Options, TILE_SIZE, pack};
    use crate::scratch::Scratch;
    use crate::synth;

    /// The synthetic file at the default cell size and at an explicit 100:
    /// every grid where the manifest puts it, at the size asked for.
    #[test]
    fn matches_the_manifest_at_50_and_100_m() {
        let s = Scratch::new();
        let dir = s.0.join("extract");
        let t =
            synth::Theatre::new(synth::DEFAULT_SEED, synth::DEFAULT_SIZE_KM, CELL_SIZE_M).unwrap();
        synth::write(&t, &dir).unwrap();
        let m = Extract::open(&dir).unwrap().manifest;

        for cell_size in [None, Some(100)] {
            let out = s.0.join(format!("{cell_size:?}.sqlite"));
            let mut opts = Options::default();
            if let Some(c) = cell_size {
                opts.cell_size = c;
            }
            pack(&dir, Some(&out), &opts).unwrap();
            let d = describe(&out).unwrap();
            let c = cell_size.unwrap_or(50);

            assert_eq!(d.provenance.theatre, m.theatre);
            assert_eq!(d.provenance.dcs_build, m.dcs_build);
            assert_eq!(d.provenance.cell_size, f64::from(c));
            let g = m.grid;
            let meta = |k: &str| d.meta.get(k).map(String::as_str);
            assert_eq!(
                meta("dcs_build_timestamp"),
                Some(m.dcs_build_timestamp.as_str())
            );
            assert_eq!(meta("grid_origin_x"), Some(g.origin_x.to_string().as_str()));
            assert_eq!(meta("grid_origin_z"), Some(g.origin_z.to_string().as_str()));
            let (w, h) = (cells(g.width, c), cells(g.height, c));
            assert_eq!(meta("grid_width"), Some(w.to_string().as_str()));
            assert_eq!(meta("grid_height"), Some(h.to_string().as_str()));
            assert_eq!(meta("validation_at"), None);
            assert_eq!(d.validation, None);
            assert!(d.layers_dropped.is_empty());
            let rows = |t: &str| d.row_counts.get(t).copied();
            assert_eq!(d.row_counts.len(), VECTOR.len());
            assert_eq!(rows("airdrome"), Some(1));
            assert_eq!(rows("stand"), Some(u64::from(synth::STAND_COUNT)));
            assert_eq!(rows("beacon"), Some(u64::from(synth::BEACON_COUNT)));
            assert_eq!(rows("radio"), Some(u64::from(synth::RADIO_COUNT)));
            assert_eq!(
                rows("poi"),
                Some(u64::from(synth::TOWNS_COUNT + 2 * synth::NODES_COUNT))
            );

            let mut names: Vec<&str> = d.grids.iter().map(|g| g.name.as_str()).collect();
            names.sort_unstable();
            let mut base = BASE.to_vec();
            base.sort_unstable();
            assert_eq!(names, base);
            for gi in &d.grids {
                let l = &m.layers[&gi.name];
                assert_eq!(gi.cell_size, f64::from(c), "{}", gi.name);
                assert_eq!((gi.origin_x, gi.origin_z), (g.origin_x, g.origin_z));
                assert_eq!((gi.width, gi.height), (w, h));
                assert_eq!(
                    (gi.dtype.as_str(), gi.tile_size),
                    (l.dtype.as_str(), TILE_SIZE)
                );
                assert_eq!(gi.nodata, Some(l.nodata.to_string()));
                assert_eq!(gi.unit.as_deref(), Some(l.unit.as_str()));
                assert_eq!(gi.derived_from, None);
                let params: serde_json::Value =
                    serde_json::from_str(gi.params.as_deref().unwrap()).unwrap();
                assert_eq!(params.as_object().unwrap().is_empty(), c == 50);
            }
        }
        // The synthetic theatre spans 70 km on each axis.
        assert_eq!(cells(m.grid.width, 100), 700);
    }
}
