//! The towns or nodes nearest a point, from `poi` and its R-tree.
//!
//! Only the point-of-interest kinds, read straight from the file; the other
//! kinds need the road graph, the scenery and a reader, and come with them.

use std::path::Path;
use std::time::Instant;

use rusqlite::{Connection, OpenFlags, params_from_iter};

use crate::types::{Nearby, NearestKind, NearestRequest, NearestResponse, Provenance};

/// The most results one request may ask for.
pub const MAX_N: u32 = 50;
/// Half the side of the first box searched; it doubles until it holds enough.
const FIRST_REACH_M: f64 = 2000.0;

pub fn nearest(path: &Path, req: &NearestRequest) -> Result<NearestResponse, String> {
    let started = Instant::now();
    if !(1..=MAX_N).contains(&req.n) {
        return Err(format!("n is {}, not 1 to {MAX_N}", req.n));
    }
    if !(req.x.is_finite() && req.z.is_finite()) {
        return Err("x and z must be finite".into());
    }
    let kinds: &[&str] = match req.kind {
        NearestKind::Town => &["town"],
        NearestKind::Node => &["node_red", "node_blue"],
        k => return Err(format!("nearest {k:?} is not built yet")),
    };
    let fail = |e: rusqlite::Error| format!("{}: {e}", path.display());
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(fail)?;
    conn.execute_batch("PRAGMA query_only = 1").map_err(fail)?;

    let marks = vec!["?"; kinds.len()].join(", ");
    let total: i64 = conn
        .query_row(
            &format!("SELECT count(*) FROM poi WHERE kind IN ({marks})"),
            params_from_iter(kinds),
            |r| r.get(0),
        )
        .map_err(fail)?;
    let want = (req.n as usize).min(total as usize);
    let mut boxed = conn
        .prepare(&format!(
            "SELECT p.id, p.kind, p.name, p.x, p.z FROM poi_idx i JOIN poi p ON p.id = i.id
             WHERE i.minx <= ?1 AND i.maxx >= ?2 AND i.minz <= ?3 AND i.maxz >= ?4
               AND p.kind IN ({marks})"
        ))
        .map_err(fail)?;

    let mut reach = FIRST_REACH_M;
    let results = loop {
        let bounds = [req.x + reach, req.x - reach, req.z + reach, req.z - reach];
        let args = bounds
            .iter()
            .map(|b| b as &dyn rusqlite::ToSql)
            .chain(kinds.iter().map(|k| k as &dyn rusqlite::ToSql));
        let mut found: Vec<(i64, Nearby)> = boxed
            .query_map(params_from_iter(args), |r| {
                let (x, z): (f64, f64) = (r.get(3)?, r.get(4)?);
                let id: i64 = r.get(0)?;
                Ok((
                    id,
                    Nearby {
                        id: id.to_string(),
                        kind: r.get(1)?,
                        name: r.get(2)?,
                        x,
                        z,
                        distance: (x - req.x).hypot(z - req.z),
                    },
                ))
            })
            .and_then(Iterator::collect)
            .map_err(fail)?;
        let all = found.len() == total as usize;
        // A row in the box's corner may be farther than one just outside its
        // side, so only those within the inscribed circle are certain.
        if !all {
            found.retain(|(_, n)| n.distance <= reach);
        }
        if all || found.len() >= want {
            found.sort_by(|(ia, a), (ib, b)| a.distance.total_cmp(&b.distance).then(ia.cmp(ib)));
            found.truncate(want);
            break found.into_iter().map(|(_, n)| n).collect();
        }
        reach *= 2.0;
    };

    let meta = |k: &str| {
        conn.query_row("SELECT value FROM meta WHERE key = ?1", [k], |r| {
            r.get::<_, Option<String>>(0)
        })
        .map_err(fail)?
        .ok_or_else(|| format!("{}: meta has no {k}", path.display()))
    };
    let cell_size = meta("cell_size")?
        .parse()
        .map_err(|e| format!("{}: cell_size: {e}", path.display()))?;
    Ok(NearestResponse {
        provenance: Provenance {
            theatre: meta("theatre_id")?,
            dcs_build: meta("dcs_build")?,
            cell_size,
            elapsed_ms: started.elapsed().as_millis() as u64,
        },
        results,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use serde_json::Value;

    use super::*;
    use crate::pack::{Options, pack};
    use crate::scratch::Scratch;
    use crate::synth;

    fn packed(s: &Scratch) -> (PathBuf, PathBuf) {
        let dir = s.0.join("extract");
        let t = synth::Theatre::new(synth::DEFAULT_SEED, synth::DEFAULT_SIZE_KM, 50).unwrap();
        synth::write(&t, &dir).unwrap();
        let out = s.0.join("x.sqlite");
        pack(&dir, Some(&out), &Options::default()).unwrap();
        (dir, out)
    }

    fn req(kind: NearestKind, x: f64, z: f64, n: u32) -> NearestRequest {
        NearestRequest {
            kind,
            x,
            z,
            n,
            class: None,
        }
    }

    fn read(dir: &std::path::Path, name: &str) -> Vec<Value> {
        let v: Value = serde_json::from_str(&fs::read_to_string(dir.join(name)).unwrap()).unwrap();
        v.as_array().unwrap().clone()
    }

    fn xz(v: &Value) -> (f64, f64) {
        (v["x"].as_f64().unwrap(), v["z"].as_f64().unwrap())
    }

    #[test]
    fn finds_each_town_from_beside_it() {
        let s = Scratch::new();
        let (dir, out) = packed(&s);
        for town in read(&dir, "towns.json") {
            let (x, z) = xz(&town);
            let r = nearest(&out, &req(NearestKind::Town, x + 30.0, z - 40.0, 1)).unwrap();
            assert_eq!(r.provenance.theatre, synth::THEATRE);
            assert_eq!(r.results.len(), 1);
            let n = &r.results[0];
            assert_eq!(n.kind, "town");
            assert_eq!(n.name.as_deref(), town["name"].as_str());
            assert_eq!((n.x, n.z), (x, z));
            assert!((n.distance - 50.0).abs() < 1e-9, "{}", n.distance);
        }
    }

    /// From a node's red side the nearest is that side, then its blue side,
    /// and a node is never offered as a town.
    #[test]
    fn finds_nodes_by_side_and_keeps_kinds_apart() {
        let s = Scratch::new();
        let (dir, out) = packed(&s);
        let nodes = read(&dir, "nodes.json");
        let first = &nodes[0];
        let (x, z) = xz(&first["red"]);
        let r = nearest(&out, &req(NearestKind::Node, x, z, 2)).unwrap();
        let kinds: Vec<&str> = r.results.iter().map(|n| n.kind.as_str()).collect();
        assert_eq!(kinds, ["node_red", "node_blue"]);
        assert_eq!(r.results[0].distance, 0.0);
        assert_eq!(r.results[1].name.as_deref(), first["name"].as_str());
        assert_eq!((r.results[1].x, r.results[1].z), xz(&first["blue"]));

        let towns = nearest(&out, &req(NearestKind::Town, x, z, MAX_N)).unwrap();
        assert!(towns.results.iter().all(|n| n.kind == "town"));
    }

    /// Asking for more than there are returns all of them, nearest first,
    /// however far the search has to reach.
    #[test]
    fn returns_every_row_in_order_when_asked_for_more() {
        let s = Scratch::new();
        let (dir, out) = packed(&s);
        let far = 1.0e7;
        let r = nearest(&out, &req(NearestKind::Node, far, far, MAX_N)).unwrap();
        assert_eq!(r.results.len(), 2 * read(&dir, "nodes.json").len());
        assert!(r.results.windows(2).all(|w| w[0].distance <= w[1].distance));
    }

    /// The R-tree answer equals a scan of the whole table, from points
    /// spread across the theatre.
    #[test]
    fn agrees_with_a_brute_force_scan() {
        let s = Scratch::new();
        let (dir, out) = packed(&s);
        let mut all: Vec<(f64, f64)> = read(&dir, "towns.json").iter().map(xz).collect();
        let size = f64::from(synth::DEFAULT_SIZE_KM) * 1000.0;
        let (x0, z0) = (f64::from(synth::ORIGIN_X_M), f64::from(synth::ORIGIN_Z_M));
        for i in 0..7 {
            for j in 0..7 {
                let (x, z) = (
                    x0 + size * f64::from(i) / 6.0,
                    z0 + size * f64::from(j) / 6.0,
                );
                all.sort_by(|a, b| {
                    let d = |p: &(f64, f64)| (p.0 - x).hypot(p.1 - z);
                    d(a).total_cmp(&d(b))
                });
                let r = nearest(&out, &req(NearestKind::Town, x, z, 2)).unwrap();
                let got: Vec<(f64, f64)> = r.results.iter().map(|n| (n.x, n.z)).collect();
                assert_eq!(got, all[..2], "from {x}, {z}");
            }
        }
    }

    /// A town in the first box's corner is farther than one just past its
    /// side, so the search must not stop at the box.
    #[test]
    fn a_row_past_the_box_side_beats_one_in_its_corner() {
        let s = Scratch::new();
        let dir = s.0.join("extract");
        let t = synth::Theatre::new(synth::DEFAULT_SEED, synth::DEFAULT_SIZE_KM, 50).unwrap();
        synth::write(&t, &dir).unwrap();
        let (x, z) = (1000.0, 2000.0);
        let town = |name: &str, dx: f64, dz: f64| {
            serde_json::json!({"name": name, "display_name": name, "lat": 0, "lon": 0,
                "x": x + dx, "z": z + dz})
        };
        let near = FIRST_REACH_M * 0.95;
        let towns = serde_json::json!([
            town("CORNER", near, near),
            town("SIDE", FIRST_REACH_M * 1.05, 0.0),
            town("FAR", 30_000.0, 30_000.0),
        ]);
        fs::write(dir.join("towns.json"), towns.to_string()).unwrap();
        let out = s.0.join("x.sqlite");
        pack(&dir, Some(&out), &Options::default()).unwrap();
        let r = nearest(&out, &req(NearestKind::Town, x, z, 1)).unwrap();
        assert_eq!(r.results[0].name.as_deref(), Some("SIDE"));
    }

    #[test]
    fn refuses_what_it_cannot_answer() {
        let s = Scratch::new();
        let (_, out) = packed(&s);
        for (r, why) in [
            (req(NearestKind::Town, 0.0, 0.0, 0), "n is 0"),
            (req(NearestKind::Town, 0.0, 0.0, MAX_N + 1), "n is 51"),
            (req(NearestKind::Town, f64::NAN, 0.0, 1), "finite"),
            (req(NearestKind::Road, 0.0, 0.0, 1), "not built yet"),
        ] {
            let e = nearest(&out, &r).unwrap_err();
            assert!(e.contains(why), "{e}");
        }
    }
}
