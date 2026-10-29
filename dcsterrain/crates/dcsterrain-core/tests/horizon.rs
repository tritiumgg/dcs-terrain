//! Opt-in: times the horizon over Caucasus, the first touch and from the
//! cache. Set `DCSTERRAIN_INSTALL` to the DCS install root; without it this
//! passes without reading anything, which is what CI does. Run it
//! optimized, `cargo test --release --test horizon -- --nocapture`, to see
//! the times. The install is only read; the cache is a scratch directory.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use dcsterrain_core::cache::Cache;
use dcsterrain_core::decode::install;
use dcsterrain_core::grid::Window;
use dcsterrain_core::horizon::RANGE_M;
use dcsterrain_core::layers::{Cells, Derived, derive};
use dcsterrain_core::source::install::Install;
use dcsterrain_core::source::{Layer, Source};

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
            std::env::temp_dir().join(format!("dcsterrain-horizon-{name}-{}", std::process::id()));
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

fn size_of(dir: &Path) -> (u64, usize) {
    fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.metadata().ok())
        .fold((0, 0), |(b, n), m| (b + m.len(), n + 1))
}

fn ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1e3
}

/// 20 by 20 km around three airdromes, the enclosure derived on a fresh
/// install with an empty cache, again on the same install, and on a fresh
/// install over the tiles kept; then one cell in a new area. The heights
/// within range of the first region are read once on their own beforehand,
/// in another install, to tell the march from reading the ground cold.
#[test]
fn the_horizon_over_caucasus_first_touched_and_cached() {
    let Some(root) = install_root() else { return };
    let build = install::read_build(&root).unwrap();
    eprintln!("DCS {} ({})", build.version, build.timestamp);
    let id = "Caucasus";
    let dir = TempDir::new(id);
    let cache = Cache::new(&dir.0);
    let open = || Install::open(&root, id, &cache).unwrap();
    let t = open();
    let g = t.grid();
    let tables = t.tables().unwrap();
    let sites: Vec<(String, f64, f64)> = tables
        .airdromes
        .iter()
        .filter_map(|a| Some((a.name_id.clone(), a.x?, a.z?)))
        .step_by(7)
        .take(3)
        .collect();
    assert_eq!(sites.len(), 3);
    let region = |x: f64, z: f64| Cells {
        rows: 400,
        cols: 400,
        ..Cells::covering(&g, [x - 10_000.0, z - 10_000.0, 0.0, 0.0])
    };

    // The ground alone: the window a batch reads around the first region.
    let (_, x, z) = &sites[0];
    let pad = (RANGE_M / f64::from(g.cell_size)) as u32 + 1;
    let at = region(*x, *z);
    let started = Instant::now();
    let w = Window::read(
        &open(),
        Layer::Height,
        at.row0 - i64::from(pad),
        at.col0 - i64::from(pad),
        400 + 2 * pad,
        400 + 2 * pad,
    )
    .unwrap();
    eprintln!(
        "  heights within range of a region, {} cells, cold: {:.0} ms",
        w.data.len(),
        ms(started)
    );

    let bits = |v: &[f32]| v.iter().map(|x| x.to_bits()).collect::<Vec<_>>();
    for (name, x, z) in &sites {
        let at = region(*x, *z);
        let t = open();
        let started = Instant::now();
        let first = derive(&t, Derived::Enclosure, at).unwrap();
        let first_ms = ms(started);
        let started = Instant::now();
        let again = derive(&t, Derived::Enclosure, at).unwrap();
        let again_ms = ms(started);
        let started = Instant::now();
        let fresh = derive(&open(), Derived::Enclosure, at).unwrap();
        let fresh_ms = ms(started);
        assert!(bits(&again.values) == bits(&first.values), "{name}");
        assert!(bits(&fresh.values) == bits(&first.values), "{name}");
        let known: Vec<f64> = first
            .values
            .iter()
            .filter(|v| !v.is_nan())
            .map(|&v| f64::from(v))
            .collect();
        let mean = known.iter().sum::<f64>() / known.len().max(1) as f64;
        let most = known.iter().copied().fold(f64::MIN, f64::max);
        eprintln!(
            "{name} at ({x:.0}, {z:.0}): first touch {first_ms:.0} ms, again {again_ms:.1} ms, \
             fresh install over the cache {fresh_ms:.1} ms; {} of 160000 defined, mean {mean:.2}°, \
             most {most:.2}°",
            known.len()
        );
        assert!(known.len() > 100_000, "{name}");
    }

    // One cell somewhere no region has touched: one tile and its window.
    let (_, x, z) = &sites[0];
    let one = Cells {
        rows: 1,
        cols: 1,
        ..Cells::covering(&g, [x + 40_000.0, z + 40_000.0, 0.0, 0.0])
    };
    let started = Instant::now();
    let v = derive(&open(), Derived::Enclosure, one).unwrap();
    eprintln!(
        "one cell, first touch: {:.0} ms ({:?}°)",
        ms(started),
        v.values
    );

    let (bytes, files) = size_of(&dir.0.join(id).join("horizon"));
    eprintln!(
        "horizon/: {files} tiles, {:.1} MiB",
        bytes as f64 / (1 << 20) as f64
    );
}
