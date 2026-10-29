//! What `dcsterrain check` reports about one terrain: each of its files and
//! whether a decoder reads it, what the cache holds, the tables and the
//! scenery catalog, the last validation figures, and every problem found.
//!
//! A file no decoder reads refuses only what reads its role (ADR 0001), so
//! the report goes on past it and says what each refusal kept from being
//! read.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use crate::cache::{self, Cache, Entry, State, TerrainCache};
use crate::classes::{Class, Row};
use crate::decode::install::{self, Listed};
use crate::decode::lua::tables::Crs;
use crate::source::Refusal;
use crate::source::install::{HORIZON, Install, MAX_CRS_RESIDUAL_M, Tables};

/// Everything `check` found about one terrain.
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub terrain: String,
    pub folder: String,
    pub install: String,
    pub dcs_build: Option<String>,
    pub dcs_build_timestamp: Option<String>,
    pub files: Vec<FileReport>,
    /// What the cache held for the terrain before `check` read anything;
    /// `None` when the terrain cannot be opened.
    pub cache: Option<CacheReport>,
    pub tables: Option<TablesReport>,
    pub catalog: Option<CatalogReport>,
    /// The terrain's `validation.json`, as the live validation sortie wrote
    /// it.
    pub validation: Option<Value>,
    /// Each thing that keeps part of the terrain from being read, each
    /// naming the file or entry at fault. `check` fails when there is one.
    pub problems: Vec<String>,
}

/// One terrain file, with the class and version its header states.
#[derive(Debug, Clone, Serialize)]
pub struct FileReport {
    pub role: String,
    pub path: String,
    pub class: Option<String>,
    pub version: Option<u32>,
    pub readable: bool,
    pub why: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CacheReport {
    pub dir: String,
    pub entries: Vec<EntryReport>,
    /// Horizon tiles computed so far, each made the first time a query
    /// touches it, and their bytes.
    pub horizon_tiles: usize,
    pub horizon_size: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct EntryReport {
    pub name: String,
    /// `current`, `missing` or `stale`.
    pub state: String,
    /// Why a stale entry will be rebuilt or re-stamped when next used.
    pub why: Option<String>,
    pub size: Option<u64>,
}

/// The tables `tables.json` holds, counted.
#[derive(Debug, Clone, Serialize)]
pub struct TablesReport {
    pub airdromes: usize,
    pub runways: usize,
    pub stands: usize,
    pub beacons: usize,
    pub radio: usize,
    pub towns: usize,
    pub nodes: usize,
    pub crs: Crs,
    /// The shared scripts the Lua files ran for their constants.
    pub lua_included: Vec<String>,
    /// Each name a Lua file read that nothing assigned.
    pub lua_unresolved: Vec<String>,
}

/// The scenery classes: totals, the models placed most and the tallest.
#[derive(Debug, Clone, Serialize)]
pub struct CatalogReport {
    pub models: usize,
    pub objects: u64,
    pub counts_by_class: BTreeMap<Class, u64>,
    /// The override keys naming no model, each with its file: a model
    /// renamed or removed.
    pub unmatched: Vec<(String, String)>,
    pub most_placed: Vec<Row>,
    /// The tallest models placed at least once, effects left out.
    pub tallest: Vec<Row>,
}

/// Checks the terrain whose `entry.lua` declares `id`, listing the `top`
/// models placed most and the `tallest` tallest. Refuses only an install or
/// id it cannot find; everything else it finds is in the report.
///
/// Reading the tables and the scenery builds `tables.json` and the scene
/// index when the cache lacks them, as the first query would; the road graph
/// is never built, since its betweenness takes minutes on a large terrain.
pub fn check(
    root: &Path,
    id: &str,
    cache: &Cache,
    top: usize,
    tallest: usize,
) -> Result<Report, Refusal> {
    let folder = install::find(root, id)?;
    let survey = install::survey(
        root,
        &Listed {
            dir: folder.clone(),
            id: Some(id.to_string()),
        },
    );
    let build = install::read_build(root).ok();
    let mut report = Report {
        terrain: id.to_string(),
        folder,
        install: root.display().to_string(),
        dcs_build: build.as_ref().map(|b| b.version.clone()),
        dcs_build_timestamp: build.map(|b| b.timestamp),
        files: survey
            .files
            .into_iter()
            .map(|f| FileReport {
                role: f.role,
                path: f.path,
                class: f.class,
                version: f.version,
                readable: f.readable,
                why: f.why,
            })
            .collect(),
        cache: None,
        tables: None,
        catalog: None,
        validation: None,
        problems: Vec::new(),
    };
    // A file the terrain's refusal already names is said once.
    let refusal = survey.refusal;
    report.problems.extend(
        report
            .files
            .iter()
            .filter_map(|f| f.why.clone())
            .filter(|w| refusal.as_ref() != Some(w)),
    );
    report.validation = read_validation(&cache.dir(id), &mut report.problems);
    if let Some(why) = refusal {
        report.problems.push(why);
        return Ok(report);
    }

    let map = match install::open(root, id) {
        Ok(m) => m,
        Err(e) => {
            report.problems.push(e.0);
            return Ok(report);
        }
    };
    let terrain = cache.terrain(root, &map);
    report.cache = Some(cache_report(&terrain, &mut report.problems));

    // A role whose file no decoder reads is reported once, above, and not
    // again as each thing it keeps from being read.
    let unreadable = |roles: &[&str]| {
        report
            .files
            .iter()
            .any(|f| !f.readable && roles.contains(&f.role.as_str()))
    };
    // The Lua files and `assets.assets` have no header to refuse; a fault in
    // one refuses the tables when they are read.
    let tables_unreadable = unreadable(&["airfield_rn4", "airfield_rn5"]);
    let scene_unreadable = unreadable(&["scene"]);

    if !tables_unreadable {
        match Tables::open(&terrain) {
            Ok(t) => {
                if let Some(m) = t.crs.max_m.filter(|&m| m > MAX_CRS_RESIDUAL_M) {
                    report.problems.push(format!(
                        "the projection fitted to {} beacons misses one by {m:.2} m, over the \
                         {MAX_CRS_RESIDUAL_M} m allowed",
                        t.crs.n.unwrap_or(0)
                    ));
                }
                report.tables = Some(tables_report(t));
            }
            Err(e) => report.problems.push(e.0),
        }
    }
    if !scene_unreadable {
        let catalog = Install::open(root, id, cache).and_then(|i| i.scenery()?.catalog());
        match catalog {
            Ok(c) => {
                // A smoke volume is drawn in the air, hundreds of meters
                // tall, and would fill the list a reviewer reads for a tall
                // structure in the wrong class.
                let tallest = c
                    .tallest(c.rows.len())
                    .into_iter()
                    .filter(|r| r.assignment.class.is_structure())
                    .take(tallest)
                    .cloned()
                    .collect();
                report.catalog = Some(CatalogReport {
                    models: c.rows.len(),
                    objects: c.rows.iter().map(|r| r.count).sum(),
                    counts_by_class: c.counts_by_class,
                    unmatched: c.unmatched,
                    most_placed: c.rows.into_iter().take(top).collect(),
                    tallest,
                });
            }
            Err(e) => report.problems.push(e.0),
        }
    }
    Ok(report)
}

fn tables_report(t: Tables) -> TablesReport {
    TablesReport {
        airdromes: t.airdromes.len(),
        runways: t.runways.len(),
        stands: t.stands.len(),
        beacons: t.beacons.len(),
        radio: t.radio.len(),
        towns: t.towns.len(),
        nodes: t.nodes.len(),
        crs: t.crs,
        lua_included: t.lua_included,
        lua_unresolved: t.lua_unresolved,
    }
}

/// The state of each entry `index` builds, and the horizon tiles so far.
pub(crate) fn cache_report(terrain: &TerrainCache, problems: &mut Vec<String>) -> CacheReport {
    let dir = terrain.dir();
    let entries = cache::entries()
        .into_iter()
        .map(|e| {
            let (state, why) = match terrain.state(e) {
                Ok(State::Current) => ("current", None),
                Ok(State::Missing) => ("missing", None),
                Ok(State::Stale(why)) => ("stale", Some(why)),
                Err(r) => {
                    problems.push(format!("{}: {}", e.name(), r.0));
                    ("stale", Some(r.0))
                }
            };
            EntryReport {
                name: e.name().to_string(),
                state: state.to_string(),
                why,
                size: fs::metadata(dir.join(e.name())).ok().map(|m| m.len()),
            }
        })
        .collect();
    let tiles: Vec<u64> = fs::read_dir(dir.join(HORIZON.name()))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.metadata().ok().filter(|m| m.is_file()))
        .map(|m| m.len())
        .collect();
    CacheReport {
        dir: dir.display().to_string(),
        entries,
        horizon_tiles: tiles.len(),
        horizon_size: tiles.iter().sum(),
    }
}

/// The validation figures, `None` when there are none; one that cannot be
/// read is a problem, since nothing can rebuild it.
pub(crate) fn read_validation(dir: &Path, problems: &mut Vec<String>) -> Option<Value> {
    let path = dir.join(cache::VALIDATION_FILE);
    let bytes = match fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            problems.push(format!("{} cannot be read: {e}", path.display()));
            return None;
        }
    };
    match serde_json::from_slice(&bytes) {
        Ok(v) => Some(v),
        Err(e) => {
            problems.push(format!("{} is not JSON: {e}", path.display()));
            None
        }
    }
}
