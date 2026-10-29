//! Opt-in: the siting criteria over a region of each real terrain, pruned
//! and exhaustive, which must give the same sites. Set `DCSTERRAIN_INSTALL`
//! to the DCS install root; without it these pass without reading anything,
//! which is what CI does. Run them optimized,
//! `cargo test --release --test criteria -- --nocapture`, to see the tiles
//! skipped and the times. `DCSTERRAIN_THREADS` caps the threads. The
//! install is only read; the cache they build is a scratch directory.

use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use serde_json::json;

use dcsterrain_core::cache::Cache;
use dcsterrain_core::decode::install;
use dcsterrain_core::query::criteria::{Evaluation, Mode, evaluate};
use dcsterrain_core::query::{lattice, resolve};
use dcsterrain_core::source::install::Install;
use dcsterrain_core::types::{BoundingBox, Criterion, Footprint, Region};

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
            std::env::temp_dir().join(format!("dcsterrain-criteria-{name}-{}", std::process::id()));
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

fn criteria(v: serde_json::Value) -> Vec<Criterion> {
    serde_json::from_value(v).unwrap()
}

/// Evaluates pruned, then exhaustively, asserts the two agree, and prints
/// the pruned run's tiles and both times.
fn compare(
    t: &Install,
    label: &str,
    cands: &[[f64; 2]],
    fp: &Footprint,
    cs: &[Criterion],
    warm: bool,
) {
    // Without a horizon to compute, both runs read the same tiles; read
    // them once first, so neither pays for the other's cold reads. With
    // one, the pruned run goes first and computes the horizon cold.
    if warm {
        evaluate(t, cands, fp, cs, Mode::Every).unwrap();
    }
    let started = Instant::now();
    let pruned = evaluate(t, cands, fp, cs, Mode::Prune).unwrap();
    let pruned_ms = started.elapsed().as_secs_f64() * 1e3;
    let started = Instant::now();
    let every = evaluate(t, cands, fp, cs, Mode::Every).unwrap();
    let every_ms = started.elapsed().as_secs_f64() * 1e3;
    let passed: Vec<_> = every.sites.iter().filter(|s| s.pass).collect();
    let Evaluation { sites, stats } = &pruned;
    assert_eq!(sites.len(), passed.len(), "{label}");
    for (p, e) in sites.iter().zip(passed) {
        assert_eq!(p, e, "{label}");
    }
    assert_eq!(stats.candidates_passed, every.stats.candidates_passed);
    eprintln!(
        "  {label:<10} {:>6} candidates, {:>5} pass; tiles {:>3}, skipped {:>3}; \
         pruned {pruned_ms:>8.0} ms, every {every_ms:>8.0} ms",
        stats.candidates, stats.candidates_passed, stats.tiles, stats.tiles_skipped,
    );
}

/// A 20 by 20 km region around each terrain's first placed airdrome: a
/// FARP's geometric criteria every 100 m, then the horizon and sightline
/// criteria every 500 m.
#[test]
fn pruning_agrees_with_every_site_on_every_terrain() {
    let Some(root) = install_root() else { return };
    let threads = std::env::var("DCSTERRAIN_THREADS")
        .ok()
        .and_then(|v| v.parse().ok());
    dcsterrain_core::threads::init(threads);
    let build = install::read_build(&root).unwrap();
    eprintln!("DCS {} ({})", build.version, build.timestamp);
    let footprint = Footprint {
        width: 60.0,
        depth: 40.0,
        orientations_deg: vec![0.0, 90.0],
    };
    let geometric = criteria(json!([
        {"name": "slope_max", "params": {"deg": 5}},
        {"name": "span_max", "params": {"m": 3}},
        {"name": "surface_in", "params": {"classes": ["land"]}},
        {"name": "cover_in", "params": {"classes": ["plain", "field"]}},
        {"name": "dist_road_band", "params": {"min": 50, "max": 2000}},
        {"name": "dist_water_min", "params": {"m": 200}},
        {"name": "dist_airdrome_band", "params": {"min": 3000, "max": 15000}},
        {"name": "scenery_in_footprint_max", "params": {"max": 0}},
        {"name": "clearing_max", "params": {"radius": 150, "max": 10}},
        {"name": "cover_fraction_min", "params": {"class": "forest", "radius": 300, "min": 0.0}},
        {"name": "dist_cover_band", "params": {"class": "town", "min": 500, "max": 5000}},
        {"name": "tpi_class_in", "params": {"classes": ["flat", "lower", "valley"]}}
    ]));
    for listed in install::terrains(&root).unwrap() {
        let Some(id) = listed.id else { continue };
        let cache = TempDir::new(&id);
        let t = Install::open(&root, &id, &Cache::new(&cache.0)).unwrap();
        let tables = t.tables().unwrap();
        let Some(a) = tables.airdromes.iter().find(|a| a.x.is_some()) else {
            eprintln!("{id}: no airdrome placed; skipped");
            continue;
        };
        let (x, z) = (a.x.unwrap(), a.z.unwrap());
        let name = a.name_id.clone();
        // The scenery index and the road graph are cache entries; build
        // them first, so the times below are the criteria's.
        let started = Instant::now();
        t.scenery().unwrap().classes();
        t.roads().unwrap();
        eprintln!(
            "{id}: around {name} at ({x:.0}, {z:.0}); scenery and roads ready in {:.1} s",
            started.elapsed().as_secs_f64()
        );
        let area = resolve(
            &t,
            &Region::Box(BoundingBox {
                min_x: x - 10_000.0,
                min_z: z - 10_000.0,
                max_x: x + 10_000.0,
                max_z: z + 10_000.0,
            }),
            "region",
        )
        .unwrap();
        let every_100 = lattice(&area, 100.0, u32::MAX).unwrap();
        compare(&t, "geometric", &every_100, &footprint, &geometric, true);
        // Observers and a target on three sides of the airdrome, 8 km out.
        let around = [[x + 8000.0, z], [x, z + 8000.0], [x - 8000.0, z]];
        let observers: Vec<_> = around
            .iter()
            .map(|p| json!({"x": p[0], "z": p[1], "alt_agl": 10}))
            .collect();
        let sight = criteria(json!([
            {"name": "slope_max", "params": {"deg": 10}},
            {"name": "masked_from",
             "params": {"bearing_deg": 0, "half_width_deg": 30, "min_elev_deg": 1}},
            {"name": "enclosure_min", "params": {"deg": 0.5}},
            {"name": "not_visible_from",
             "params": {"observers": observers, "max_fraction": 0.34}},
            {"name": "visible_to",
             "params": {"targets": [{"x": x, "z": z, "alt_agl": 30}], "min_fraction": 1}}
        ]));
        let every_500 = lattice(&area, 500.0, u32::MAX).unwrap();
        compare(&t, "sight", &every_500, &footprint, &sight, false);
    }
}
