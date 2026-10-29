//! One request against one terrain: the terrain opened, the operation found,
//! the request read, the answer stamped.
//!
//! An operation is a function from the terrain and its request struct to
//! its response struct or a list of issues, listed in `HANDLERS` under the
//! name `types::OPERATIONS` gives it.

use std::path::Path;
use std::time::Instant;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::cache::Cache;
use crate::source::install::Install;
use crate::source::{Refusal, Source};
use crate::synth::Synth;
use crate::types::{Issue, OPERATIONS, Provenance};

mod describe;
mod region;

pub use describe::describe;
pub use region::{Area, resolve};

/// Every answer is on the 50 m grid.
pub const CELL_SIZE_M: f64 = 50.0;

/// A terrain a request can be asked of.
pub enum Terrain {
    /// The synthetic terrain, in memory.
    Synth(Box<Synth>),
    /// A terrain of a DCS install, read in place.
    Install(Box<Install>),
}

impl Terrain {
    /// The install terrain whose `entry.lua` declares `id`, its cache
    /// entries in `cache`. A terrain whose projection misses a beacon it
    /// was fitted to by more than the residual allowed is refused unless
    /// `allow_crs_residual`, as `index` refuses it.
    pub fn open(
        install: &Path,
        id: &str,
        cache: &Cache,
        allow_crs_residual: bool,
    ) -> Result<Terrain, Refusal> {
        let i = Install::open(install, id, cache)?;
        i.tables()?
            .check_crs(&i.identity().terrain, allow_crs_residual)?;
        Ok(Terrain::Install(Box::new(i)))
    }

    pub fn source(&self) -> &dyn Source {
        match self {
            Terrain::Synth(s) => s.as_ref(),
            Terrain::Install(i) => i.as_ref(),
        }
    }

    /// What answered, with no time taken yet: `answer` stamps that.
    pub fn provenance(&self) -> Provenance {
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

type Handler = fn(&Terrain, &Value) -> Result<Value, Vec<Issue>>;

/// The operations built so far, by the name `OPERATIONS` lists each under.
const HANDLERS: &[(&str, Handler)] = &[
    ("describe", |t, r| run(t, r, describe)),
];

/// Reads the request as `Q`, answers it with `f`, and writes the answer.
fn run<Q: DeserializeOwned, A: Serialize>(
    terrain: &Terrain,
    request: &Value,
    f: fn(&Terrain, Q) -> Result<A, Vec<Issue>>,
) -> Result<Value, Vec<Issue>> {
    let q = serde_json::from_value(request.clone())
        .map_err(|e| vec![Issue::new("invalid_request", &e.to_string(), "")])?;
    Ok(serde_json::to_value(f(terrain, q)?).expect("a response serializes"))
}

/// A refusal from the terrain, as the issue an operation returns.
pub(crate) fn refused(e: Refusal) -> Vec<Issue> {
    vec![Issue::new("refused", &e.0, "")]
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
    let Some((_, handler)) = HANDLERS.iter().find(|(name, _)| *name == op) else {
        return Err(vec![Issue::new(
            "not_built",
            &format!("`{op}` is planned and not built yet"),
            "",
        )]);
    };
    let mut out = handler(terrain, request)?;
    out["elapsed_ms"] = (started.elapsed().as_millis() as u64).into();
    Ok(out)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::synth;
    use serde_json::json;

    pub fn synth() -> Terrain {
        Terrain::Synth(Box::new(
            Synth::new(
                synth::DEFAULT_SEED,
                synth::DEFAULT_SIZE_KM,
                synth::CELL_SIZE_M,
            )
            .unwrap(),
        ))
    }

    #[test]
    fn every_handler_is_a_listed_operation() {
        for (name, _) in HANDLERS {
            assert!(OPERATIONS.iter().any(|o| o.name == *name), "{name}");
        }
    }

    #[test]
    fn refuses_an_unknown_operation_a_planned_one_and_a_bad_request() {
        let issue = |op: &str, req: Value| answer(&synth(), op, &req).unwrap_err()[0].code.clone();
        assert_eq!(issue("nope", json!({})), "unknown_operation");
        assert_eq!(issue("visible", json!({})), "not_built");
        assert_eq!(issue("describe", json!([1])), "invalid_request");
    }
}
