//! Opt-in: 10 000 points scattered over a whole terrain, answered block by
//! block at the default block budget, against the 3 s target on GermanyCW.
//! Set `DCSTERRAIN_INSTALL` to the DCS install root; without it this passes
//! without reading anything, which is what CI does. Run it with `--release`
//! for the times. The install is only read; the cache is a scratch
//! directory.

use std::path::PathBuf;
use std::time::Instant;

use dcsterrain_core::cache::Cache;
use dcsterrain_core::source::Source;
use dcsterrain_core::source::install::Install;

fn install_root() -> Option<PathBuf> {
    match std::env::var_os("DCSTERRAIN_INSTALL") {
        Some(p) => Some(PathBuf::from(p)),
        None => {
            eprintln!("DCSTERRAIN_INSTALL is not set; skipped");
            None
        }
    }
}

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `n` points spread evenly at random over the grid, the same each run.
fn scattered(src: &dyn Source, n: usize) -> Vec<[f64; 2]> {
    let g = src.grid();
    let c = f64::from(g.cell_size);
    let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut unit = || {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (state >> 11) as f64 / (1u64 << 53) as f64
    };
    (0..n)
        .map(|_| {
            [
                g.origin_x + unit() * f64::from(g.height) * c,
                g.origin_z + unit() * f64::from(g.width) * c,
            ]
        })
        .collect()
}

#[test]
fn ten_thousand_scattered_points_answer_within_the_target() {
    let Some(root) = install_root() else { return };
    let build = dcsterrain_core::decode::install::read_build(&root).unwrap();
    for (id, target_s) in [("GermanyCW", 3.0), ("Caucasus", f64::INFINITY)] {
        let dir = Scratch(
            std::env::temp_dir().join(format!("dcsterrain-points-{}-{id}", std::process::id())),
        );
        let t = Install::open(&root, id, &Cache::new(&dir.0)).unwrap();
        let pts = scattered(&t, 10_000);
        let started = Instant::now();
        let got = t.points(&pts).unwrap();
        let s = started.elapsed().as_secs_f64();
        let known = got.iter().filter(|g| g.is_some()).count();
        eprintln!(
            "{id} on {}: 10 000 scattered points in {s:.2} s, {known} on the mesh",
            build.version
        );
        assert!(known > 1000, "{id}: only {known} points on the mesh");
        // A sample of them, one at a time, agrees.
        for (p, g) in pts.iter().zip(&got).step_by(97) {
            assert_eq!(*g, t.point(p[0], p[1]).unwrap(), "{id} {p:?}");
        }
        if cfg!(not(debug_assertions)) {
            assert!(s < target_s, "{id}: {s:.2} s, over the {target_s} s target");
        }
    }
}
