//! One request against one terrain: the terrain opened, the operation found,
//! the request read, the answer stamped.

use std::path::Path;
use std::time::Instant;

use serde_json::Value;

use crate::cache::Cache;
use crate::grid::Grid;
use crate::source::install::Install;
use crate::source::{Refusal, Source};
use crate::synth::{self, Synth};
use crate::types::{
    BoundingBox, Counts, DescribeRequest, DescribeResponse, Issue, OPERATIONS, Provenance,
    TerrainFile,
};

/// Every answer is on the 50 m grid.
pub const CELL_SIZE_M: f64 = 50.0;

/// A terrain a request can be asked of.
pub enum Terrain {
    /// The synthetic terrain, in memory.
    Synth(Synth),
    /// A terrain of a DCS install, read in place.
    Install(Box<Install>),
}

impl Terrain {
    /// The install terrain whose `entry.lua` declares `id`, its cache
    /// entries in `cache`.
    pub fn open(install: &Path, id: &str, cache: &Cache) -> Result<Terrain, Refusal> {
        Ok(Terrain::Install(Box::new(Install::open(
            install, id, cache,
        )?)))
    }

    pub fn source(&self) -> &dyn Source {
        match self {
            Terrain::Synth(s) => s,
            Terrain::Install(i) => i.as_ref(),
        }
    }

    fn provenance(&self) -> Provenance {
        let i = self.source().identity();
        Provenance {
            terrain: i.terrain.clone(),
            dcs_build: i.dcs_build.clone(),
            terrain_fingerprint: i.fingerprint.clone(),
            cell_size: CELL_SIZE_M,
            elapsed_ms: 0,
        }
    }
}

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

/// Answers one request, `op` with its JSON, timing it from here.
pub fn answer(terrain: &Terrain, op: &str, request: &Value) -> Result<Value, Vec<Issue>> {
    let started = Instant::now();
    if !OPERATIONS.iter().any(|o| o.name == op) {
        let names: Vec<_> = OPERATIONS.iter().map(|o| o.name).collect();
        return Err(vec![Issue::new(
            "unknown_operation",
            &format!(
                "no operation `{op}`; the operations are {}",
                names.join(", ")
            ),
            "",
        )]);
    }
    let mut out = match op {
        "describe" => {
            read::<DescribeRequest>(request)?;
            to_value(describe(terrain))
        }
        _ => {
            return Err(vec![Issue::new(
                "not_built",
                &format!("`{op}` is planned and not built yet"),
                "",
            )]);
        }
    };
    out["elapsed_ms"] = (started.elapsed().as_millis() as u64).into();
    Ok(out)
}

fn read<T: serde::de::DeserializeOwned>(request: &Value) -> Result<T, Vec<Issue>> {
    serde_json::from_value(request.clone())
        .map_err(|e| vec![Issue::new("invalid_request", &e.to_string(), "")])
}

fn to_value(r: impl serde::Serialize) -> Value {
    serde_json::to_value(r).expect("a response serializes")
}

/// What the terrain is and what is known of it. An install terrain's counts
/// are null until its tables have been read.
pub fn describe(terrain: &Terrain) -> DescribeResponse {
    let provenance = terrain.provenance();
    match terrain {
        Terrain::Synth(s) => {
            let form = s.form();
            DescribeResponse {
                provenance,
                dcs_build_timestamp: synth::DCS_BUILD_TIMESTAMP.to_string(),
                extent: Some(extent(&s.grid())),
                files: Vec::new(),
                counts: Counts {
                    airdromes: Some(1),
                    runways: Some(1),
                    stands: Some(synth::STAND_COUNT.into()),
                    beacons: Some(synth::BEACON_COUNT.into()),
                    towns: Some(synth::TOWNS_COUNT.into()),
                    nodes: Some(synth::NODES_COUNT.into()),
                    road_edges: Some(form.roads().len() as u64),
                    scenery_objects: Some(form.scenery().len() as u64),
                },
                validation: None,
            }
        }
        Terrain::Install(i) => DescribeResponse {
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
            counts: Counts::default(),
            validation: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn synth() -> Terrain {
        Terrain::Synth(Synth::new(synth::DEFAULT_SEED, synth::DEFAULT_SIZE_KM, 50).unwrap())
    }

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
        assert_eq!(v["validation"], Value::Null);
        let back: DescribeResponse = serde_json::from_value(v).unwrap();
        assert!(back.files.is_empty());
    }

    #[test]
    fn refuses_an_unknown_operation_a_planned_one_and_a_bad_request() {
        let issue = |op: &str, req: Value| answer(&synth(), op, &req).unwrap_err()[0].code.clone();
        assert_eq!(issue("nope", json!({})), "unknown_operation");
        assert_eq!(issue("visible", json!({})), "not_built");
        assert_eq!(issue("describe", json!([1])), "invalid_request");
    }
}
