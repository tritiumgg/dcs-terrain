//! Opt-in: the exact sightline over the mesh's triangles against the
//! half-cell raster walk, on many random sightlines over Caucasus and
//! GermanyCW: how often they disagree on whether the ends see each other,
//! how far their clearances differ, and what each costs. Set
//! `DCSTERRAIN_INSTALL` to the DCS install root, and `DCSTERRAIN_SIGHTLINES`
//! to the number of sightlines if not 1000; without the install this passes
//! without reading anything, which is what CI does. Run it with `--release`
//! and `--nocapture` for the figures. The install is only read; the cache is
//! a scratch directory.

use std::path::PathBuf;
use std::time::Instant;

use dcsterrain_core::cache::Cache;
use dcsterrain_core::query::sightline::{Heights, Sight, exact, walk};
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

/// A generator of numbers in 0..1, the same each run.
struct Unit(u64);

impl Unit {
    fn next(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// Which lines a row of figures is over.
type Keep = fn(&Line) -> bool;

/// One sightline: its ends on the mesh, `[x, z, alt]`.
struct Line {
    a: [f64; 3],
    b: [f64; 3],
    eye_agl: f64,
    target_agl: f64,
}

/// `n` sightlines of 1 to 40 km, in any direction, whose ends both stand on
/// the mesh: the observer 2, 10, 100 or 1000 m above its ground, the target
/// on its ground or 2 m above it.
fn lines(src: &dyn Source, n: usize) -> Vec<Line> {
    let g = src.grid();
    let c = f64::from(g.cell_size);
    let mut u = Unit(0x9e37_79b9_7f4a_7c15);
    let mut out = Vec::new();
    while out.len() < n {
        let mut ends = Vec::new();
        let mut agl = Vec::new();
        for _ in 0..256 {
            let a = [
                g.origin_x + u.next() * f64::from(g.height) * c,
                g.origin_z + u.next() * f64::from(g.width) * c,
            ];
            let (d, r) = (
                1000.0 + u.next() * 39_000.0,
                u.next() * std::f64::consts::TAU,
            );
            ends.push(a);
            ends.push([a[0] + d * r.cos(), a[1] + d * r.sin()]);
            let eye = [2.0, 10.0, 100.0, 1000.0][(u.next() * 4.0) as usize % 4];
            agl.push((eye, if u.next() < 0.5 { 0.0 } else { 2.0 }));
        }
        let ground = src.points(&ends).unwrap();
        for (k, (eye, target)) in agl.into_iter().enumerate() {
            if let (Some(ga), Some(gb)) = (ground[2 * k], ground[2 * k + 1]) {
                let (a, b) = (ends[2 * k], ends[2 * k + 1]);
                out.push(Line {
                    a: [a[0], a[1], ga.height + eye],
                    b: [b[0], b[1], gb.height + target],
                    eye_agl: eye,
                    target_agl: target,
                });
            }
        }
    }
    out.truncate(n);
    out
}

fn percentile(v: &[f64], p: f64) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    let mut v = v.to_vec();
    v.sort_unstable_by(f64::total_cmp);
    v[((v.len() - 1) as f64 * p).round() as usize]
}

/// A sightline as `visible` steps it, with a fresh set of raster windows,
/// the way one request makes it.
fn raster(t: &Install, l: &Line) -> Sight {
    let step = f64::from(t.grid().cell_size) / 2.0;
    walk(&mut Heights::new(t), l.a, l.b, step).unwrap()
}

fn mesh(t: &Install, l: &Line) -> Sight {
    exact(
        &t.along([l.a[0], l.a[1]], [l.b[0], l.b[1]]).unwrap(),
        l.a,
        l.b,
    )
}

/// What `f` finds on each line, and the milliseconds it takes.
fn timed(ls: &[Line], mut f: impl FnMut(&Line) -> Sight) -> (Vec<Sight>, Vec<f64>) {
    ls.iter()
        .map(|l| {
            let s = Instant::now();
            let r = f(l);
            (r, s.elapsed().as_secs_f64() * 1e3)
        })
        .unzip()
}

fn ms(v: &[f64]) -> String {
    format!(
        "p50 {:.2} p99 {:.2} max {:.2}, total {:.0}",
        percentile(v, 0.5),
        percentile(v, 0.99),
        percentile(v, 1.0),
        v.iter().sum::<f64>()
    )
}

#[test]
fn the_exact_sightline_against_the_raster() {
    let Some(root) = install_root() else { return };
    let build = dcsterrain_core::decode::install::read_build(&root).unwrap();
    let n: usize = std::env::var("DCSTERRAIN_SIGHTLINES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1000);
    for id in ["Caucasus", "GermanyCW"] {
        let dir = Scratch(
            std::env::temp_dir().join(format!("dcsterrain-sightline-{}-{id}", std::process::id())),
        );
        let cache = Cache::new(&dir.0);
        let open = || Install::open(&root, id, &cache).unwrap();
        let ls = lines(&open(), n);
        // One pass of each through a terrain opened for it alone warms the
        // OS cache; then each method is timed through a terrain of its own,
        // its block and tile caches empty at first, and again warm.
        {
            let t = open();
            for l in &ls {
                raster(&t, l);
                mesh(&t, l);
            }
        }
        let t = open();
        let (_, raster_cold) = timed(&ls, |l| raster(&t, l));
        let (r, raster_warm) = timed(&ls, |l| raster(&t, l));
        let t = open();
        let (_, mesh_cold) = timed(&ls, |l| mesh(&t, l));
        let (m, mesh_warm) = timed(&ls, |l| mesh(&t, l));
        let pieces: Vec<f64> = (ls.iter())
            .map(|l| t.along([l.a[0], l.a[1]], [l.b[0], l.b[1]]).unwrap().len() as f64)
            .collect();
        let km: f64 = (ls.iter())
            .map(|l| (l.b[0] - l.a[0]).hypot(l.b[1] - l.a[1]) / 1000.0)
            .sum();
        eprintln!(
            "{id} on {}: {n} sightlines, {km:.0} km in all",
            build.version
        );
        eprintln!("  raster ms, cold caches: {}", ms(&raster_cold));
        eprintln!("  raster ms, warm:        {}", ms(&raster_warm));
        eprintln!("  exact ms, cold caches:  {}", ms(&mesh_cold));
        eprintln!("  exact ms, warm:         {}", ms(&mesh_warm));
        eprintln!(
            "  triangles crossed per line: p50 {:.0} p99 {:.0} max {:.0}",
            percentile(&pieces, 0.5),
            percentile(&pieces, 0.99),
            percentile(&pieces, 1.0)
        );
        // The exact least gap over only the span the raster steps, from its
        // first step to its last, so the two clearances measure the same
        // thing: the exact one otherwise reaches the ends.
        let step = f64::from(t.grid().cell_size) / 2.0;
        let span: Vec<Sight> = (ls.iter())
            .map(|l| {
                let k = ((l.b[0] - l.a[0]).hypot(l.b[1] - l.a[1]) / step).ceil();
                let at = |f: f64| [0, 1, 2].map(|i| l.a[i] + f * (l.b[i] - l.a[i]));
                let (a, b) = (at(1.0 / k), at(1.0 - 1.0 / k));
                exact(&t.along([a[0], a[1]], [b[0], b[1]]).unwrap(), a, b)
            })
            .collect();
        let rows: Vec<(&Line, &Sight, &Sight, &Sight)> = (ls.iter().zip(&r).zip(&m).zip(&span))
            .map(|(((l, r), m), s)| (l, r, m, s))
            .collect();
        let groups: [(&str, Keep); 7] = [
            ("all", |_| true),
            ("eye 2 m", |l| l.eye_agl == 2.0),
            ("eye 10 m", |l| l.eye_agl == 10.0),
            ("eye 100 m", |l| l.eye_agl == 100.0),
            ("eye 1000 m", |l| l.eye_agl == 1000.0),
            ("target 0 m", |l| l.target_agl == 0.0),
            ("target 2 m", |l| l.target_agl == 2.0),
        ];
        for (name, keep) in groups {
            let of: Vec<_> = rows.iter().filter(|(l, ..)| keep(l)).collect();
            let count = |f: &dyn Fn(&Sight, &Sight) -> bool| {
                of.iter().filter(|(_, r, m, _)| f(r, m)).count()
            };
            let hidden_raster = count(&|r, _| r.blocking.is_some());
            let hidden_exact = count(&|_, m| m.blocking.is_some());
            let only_raster = count(&|r, m| r.blocking.is_some() && m.blocking.is_none());
            let only_exact = count(&|r, m| r.blocking.is_none() && m.blocking.is_some());
            // Where both see across, how far the raster's least gap is from
            // the exact one over the same span, and from the exact one
            // reaching the ends.
            let both: Vec<_> = (of.iter())
                .filter(|(_, r, m, _)| r.blocking.is_none() && m.blocking.is_none())
                .collect();
            let diff = |same_span: bool| -> Vec<f64> {
                (both.iter())
                    .filter_map(|(_, r, m, s)| {
                        let e = if same_span { s } else { m };
                        Some((r.clearance? - e.clearance?).abs())
                    })
                    .collect()
            };
            let (same, ends) = (diff(true), diff(false));
            eprintln!(
                "  {name:>10}: {} lines, hidden raster {hidden_raster} exact {hidden_exact}; \
                 hidden by one only: raster {only_raster}, exact {only_exact}; \
                 clearance |diff| same span p50 {:.2} p99 {:.2} max {:.2} m, \
                 to the ends p50 {:.2} p99 {:.2} m",
                of.len(),
                percentile(&same, 0.5),
                percentile(&same, 0.99),
                percentile(&same, 1.0),
                percentile(&ends, 0.5),
                percentile(&ends, 0.99),
            );
        }
        let raised: Vec<_> = rows.iter().filter(|(l, ..)| l.target_agl > 0.0).collect();
        let agree = (raised.iter())
            .filter(|(_, r, m, _)| r.blocking.is_some() == m.blocking.is_some())
            .count();
        assert!(
            agree * 10 > raised.len() * 9,
            "{id}: only {agree} of {} agree",
            raised.len()
        );
    }
}
