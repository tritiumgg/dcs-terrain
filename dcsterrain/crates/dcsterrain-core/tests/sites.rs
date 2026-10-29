//! Opt-in: a FARP-like `find_sites` over a region of each real terrain.
//! Every candidate passes when scored alone with `score_site` at its
//! orientation, with the same terms; no two lie nearer than the spacing;
//! and the answer equals a brute-force search over every site. Set
//! `DCSTERRAIN_INSTALL` to the DCS install root; without it these pass
//! without reading anything, which is what CI does. Run them optimized,
//! `cargo test --release --test sites -- --nocapture`, to see the times.
//! `DCSTERRAIN_THREADS` caps the threads. The install is only read; the
//! cache they build is a scratch directory.

use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use serde_json::json;

use dcsterrain_core::cache::Cache;
use dcsterrain_core::decode::install;
use dcsterrain_core::query::criteria::{Mode, evaluate};
use dcsterrain_core::query::{Terrain, find_sites, lattice, resolve, score_site};
use dcsterrain_core::types::{Candidate, FindSitesRequest, Footprint, ScoreSiteRequest};

fn install_root() -> Option<PathBuf> {
    match std::env::var_os("DCSTERRAIN_INSTALL") {
        Some(p) => Some(PathBuf::from(p)),
        None => {
            eprintln!("DCSTERRAIN_INSTALL is not set; skipped");
            None
        }
    }
}

/// A fresh directory under the system temp, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir =
            std::env::temp_dir().join(format!("dcsterrain-sites-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Every site evaluated, each position's best passing orientation by the
/// weighted sum, sorted best first and by x then z, and suppressed against
/// every one kept.
fn brute(t: &Terrain, q: &FindSitesRequest) -> Vec<Candidate> {
    let src = t.source();
    let area = resolve(src, &q.region, "region").unwrap();
    let cands = lattice(&area, q.step.unwrap(), u32::MAX).unwrap();
    let e = evaluate(src, &cands, &q.footprint, &q.criteria, Mode::Every).unwrap();
    let mut best: Vec<Candidate> = Vec::new();
    for sites in e.sites.chunks(q.footprint.orientations_deg.len()) {
        let mut top: Option<Candidate> = None;
        for s in sites.iter().filter(|s| s.pass) {
            let score: f64 = s
                .terms
                .iter()
                .map(|t| q.weights.get(&t.name).copied().unwrap_or(1.0) * t.score)
                .sum();
            if top.as_ref().is_none_or(|t| score > t.score) {
                top = Some(Candidate {
                    x: s.x,
                    z: s.z,
                    orientation_deg: s.orientation_deg,
                    score,
                    terms: s.terms.clone(),
                });
            }
        }
        best.extend(top);
    }
    best.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then(a.x.total_cmp(&b.x))
            .then(a.z.total_cmp(&b.z))
    });
    let mut kept: Vec<Candidate> = Vec::new();
    for c in best {
        if kept.len() == q.limit as usize {
            break;
        }
        if kept
            .iter()
            .all(|k| (k.x - c.x).hypot(k.z - c.z) >= q.spacing)
        {
            kept.push(c);
        }
    }
    kept
}

/// A 20 by 20 km region around each terrain's first placed airdrome, a
/// FARP's criteria every 100 m, 1 km apart.
#[test]
fn farp_sites_pass_alone_and_keep_their_spacing_on_every_terrain() {
    let Some(root) = install_root() else { return };
    let threads = std::env::var("DCSTERRAIN_THREADS")
        .ok()
        .and_then(|v| v.parse().ok());
    dcsterrain_core::threads::init(threads);
    let build = install::read_build(&root).unwrap();
    eprintln!("DCS {} ({})", build.version, build.timestamp);
    let footprint = json!({"width": 60, "depth": 40, "orientations_deg": [0, 90]});
    let criteria = json!([
        {"name": "slope_max", "params": {"deg": 5}},
        {"name": "span_max", "params": {"m": 3}},
        {"name": "surface_in", "params": {"classes": ["land"]}},
        {"name": "cover_in", "params": {"classes": ["plain", "field"]}},
        {"name": "dist_road_band", "params": {"min": 50, "max": 2000}},
        {"name": "dist_water_min", "params": {"m": 200}},
        {"name": "dist_airdrome_band", "params": {"min": 3000, "max": 15000}},
        {"name": "scenery_in_footprint_max", "params": {"max": 0}},
        {"name": "clearing_max", "params": {"radius": 150, "max": 10}},
        {"name": "tpi_class_in", "params": {"classes": ["flat", "lower", "valley"]}}
    ]);
    let spacing = 1000.0;
    for listed in install::terrains(&root).unwrap() {
        let Some(id) = listed.id else { continue };
        let cache = TempDir::new(&id);
        let t = Terrain::open(&root, &id, &Cache::new(&cache.0), true).unwrap();
        let Terrain::Install(i) = &t else {
            unreachable!("an install terrain")
        };
        let tables = i.tables().unwrap();
        let Some(a) = tables.airdromes.iter().find(|a| a.x.is_some()) else {
            eprintln!("{id}: no airdrome placed; skipped");
            continue;
        };
        let (x, z) = (a.x.unwrap(), a.z.unwrap());
        // The scenery index and the road graph are cache entries; build
        // them first, so the times below are the search's.
        i.scenery().unwrap().classes();
        i.roads().unwrap();
        let q: FindSitesRequest = serde_json::from_value(json!({
            "region": {"Box": {"min_x": x - 10_000.0, "min_z": z - 10_000.0,
                               "max_x": x + 10_000.0, "max_z": z + 10_000.0}},
            "footprint": footprint, "criteria": criteria,
            "step": 100, "spacing": spacing, "limit": 50,
            "weights": {"slope_max": 2, "dist_road_band": 0.5}
        }))
        .unwrap();
        // Once to read the tiles, then timed.
        find_sites(&t, q.clone()).unwrap();
        let started = Instant::now();
        let r = find_sites(&t, q.clone()).unwrap();
        let ms = started.elapsed().as_secs_f64() * 1e3;
        let started = Instant::now();
        for c in &r.candidates {
            let s = score_site(
                &t,
                ScoreSiteRequest {
                    x: c.x,
                    z: c.z,
                    // At the candidate's orientation: `score_site` picks its own
                    // without the weights.
                    footprint: Footprint {
                        orientations_deg: vec![c.orientation_deg],
                        ..q.footprint.clone()
                    },
                    criteria: q.criteria.clone(),
                    geojson: false,
                },
            )
            .unwrap();
            assert!(!s.hard_fail, "{id}: {c:?} fails alone: {s:?}");
            assert_eq!(s.terms, c.terms, "{id}: {c:?}");
            let w: f64 = s
                .terms
                .iter()
                .map(|t| q.weights.get(&t.name).copied().unwrap_or(1.0) * t.score)
                .sum();
            assert!((w - c.score).abs() < 1e-9, "{id}: {c:?}");
        }
        let rescore_ms = started.elapsed().as_secs_f64() * 1e3;
        for (k, a) in r.candidates.iter().enumerate() {
            for b in &r.candidates[k + 1..] {
                let d = (a.x - b.x).hypot(a.z - b.z);
                assert!(d >= spacing, "{id}: {d} m between {a:?} and {b:?}");
            }
        }
        let started = Instant::now();
        assert_eq!(r.candidates, brute(&t, &q), "{id}");
        let brute_ms = started.elapsed().as_secs_f64() * 1e3;
        let best = r.candidates.first().map_or(f64::NAN, |c| c.score);
        eprintln!(
            "  {id:<20} {:>6} candidates, {:>5} pass, {:>3} tiles skipped; {:>2} returned, best \
             {best:.3}; find_sites {ms:>6.0} ms, rescored {rescore_ms:>6.0} ms, brute {brute_ms:>6.0} ms",
            r.stats.cells_considered,
            r.stats.cells_passed,
            r.stats.tiles_skipped,
            r.candidates.len(),
        );
    }
}
