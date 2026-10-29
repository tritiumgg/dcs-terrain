//! `describe`: what the terrain is, which files answer for it, what its
//! cache holds, and how much of it there is.

use super::{Terrain, refused};
use crate::check;
use crate::grid::Grid;
use crate::source::Source;
use crate::synth;
use crate::types::{
    BoundingBox, CacheEntry, Counts, DescribeRequest, DescribeResponse, Issue, TerrainFile,
    Validation,
};

/// The grid's extent: whole cells around everything the mesh covers.
fn extent(g: &Grid) -> BoundingBox {
    let c = f64::from(g.cell_size);
    BoundingBox {
        min_x: g.origin_x,
        min_z: g.origin_z,
        max_x: g.origin_x + f64::from(g.height) * c,
        max_z: g.origin_z + f64::from(g.width) * c,
    }
}

/// The counts the tables give; the rest are filled by the caller.
fn table_counts(src: &dyn Source) -> Result<Counts, Vec<Issue>> {
    let t = src.tables().map_err(refused)?;
    let n = |k: usize| Some(k as u64);
    Ok(Counts {
        airdromes: n(t.airdromes.len()),
        runways: n(t.runways.len()),
        stands: n(t.stands.len()),
        beacons: n(t.beacons.len()),
        towns: n(t.towns.len()),
        nodes: n(t.nodes.len()),
        road_edges: None,
        scenery_objects: None,
    })
}

pub fn describe(terrain: &Terrain, _: DescribeRequest) -> Result<DescribeResponse, Vec<Issue>> {
    let provenance = terrain.provenance();
    let mut counts = table_counts(terrain.source())?;
    Ok(match terrain {
        Terrain::Synth(s) => {
            let form = s.form();
            counts.road_edges = Some(form.roads().len() as u64);
            counts.scenery_objects = Some(form.scenery().len() as u64);
            DescribeResponse {
                provenance,
                dcs_build_timestamp: synth::DCS_BUILD_TIMESTAMP.to_string(),
                extent: Some(extent(&s.grid())),
                files: Vec::new(),
                cache: Vec::new(),
                counts,
                validation: None,
            }
        }
        Terrain::Install(i) => {
            // Reading the road graph or the scene index costs more than a
            // describe may, so those are counted only once read.
            counts.road_edges = i.roads_read().map(|g| g.edges.len() as u64);
            counts.scenery_objects = i.scenery_read().map(|s| s.index().pools[0].count);
            let mut problems = Vec::new();
            let cache = check::cache_report(i.cache(), &mut problems)
                .entries
                .into_iter()
                .map(|e| CacheEntry {
                    name: e.name,
                    state: e.state,
                    why: e.why,
                    size: e.size,
                })
                .collect();
            // `check` reports figures that cannot be read; here they are
            // only absent.
            let validation = check::read_validation(i.cache().dir(), &mut problems)
                .and_then(|v| serde_json::from_value::<Validation>(v).ok());
            DescribeResponse {
                provenance,
                dcs_build_timestamp: i.map().build.timestamp.clone(),
                extent: Some(extent(&i.grid())),
                files: i
                    .map()
                    .files
                    .iter()
                    .map(|f| {
                        // Only the surface file's header is read to open a
                        // terrain; the others are read by what needs them.
                        let s = &i.map().surface;
                        let read = s.path == f.path;
                        TerrainFile {
                            role: f.role.clone(),
                            path: f.path.clone(),
                            class: s.class.clone().filter(|_| read),
                            version: s.version.filter(|_| read),
                            size: f.size,
                        }
                    })
                    .collect(),
                cache,
                counts,
                validation,
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::super::answer;
    use super::super::tests::synth;
    use super::*;
    use serde_json::{Value, json};

    #[test]
    fn describes_the_synthetic_terrain() {
        let v = answer(&synth(), "describe", &json!({})).unwrap();
        assert_eq!(v["terrain"], "synth");
        assert_eq!(v["dcs_build"], synth::DCS_BUILD);
        assert_eq!(v["terrain_fingerprint"], "synth-1-70-50");
        assert_eq!(v["cell_size"], 50.0);
        assert!(v["elapsed_ms"].is_u64());
        assert_eq!(
            v["extent"],
            json!({"min_x": -30000.0, "min_z": -45000.0, "max_x": 40000.0, "max_z": 25000.0})
        );
        assert_eq!(v["counts"]["road_edges"], 9);
        assert_eq!(v["counts"]["scenery_objects"], synth::SCENERY_COUNT);
        assert_eq!(v["counts"]["airdromes"], 1);
        assert_eq!(v["counts"]["stands"], synth::STAND_COUNT);
        assert_eq!(v["counts"]["towns"], synth::TOWNS_COUNT);
        assert_eq!(v["counts"]["nodes"], synth::NODES_COUNT);
        assert_eq!(v["validation"], Value::Null);
        let back: DescribeResponse = serde_json::from_value(v).unwrap();
        assert!(back.files.is_empty() && back.cache.is_empty());
    }
}
