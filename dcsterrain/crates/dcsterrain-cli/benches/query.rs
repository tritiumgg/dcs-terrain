//! The query benchmark: every built operation, the per-query layers, the
//! horizon, the roads, opening a terrain and one call of the binary end to
//! end, over the synthetic terrain and, opt-in, over an install.
//!
//! ```text
//! cargo bench -p dcsterrain-cli --bench query [-- <filter>...]
//! ```
//!
//! A filter keeps the cases whose `terrain/case` name contains it. The
//! settings are environment variables:
//!
//! - `DCSTERRAIN_INSTALL`: the DCS install root. Without it only the
//!   synthetic terrain runs.
//! - `DCSTERRAIN_BENCH_TERRAINS`: the install terrains, comma separated;
//!   `Caucasus,GermanyCW` when unset.
//! - `DCSTERRAIN_BENCH_ROUNDS`: rounds per case after the warm-up, 3 when
//!   unset.
//! - `DCSTERRAIN_BENCH_OS`: `warm`, `cold` or `both` (the default). Cold
//!   drops the terrain's files and the cache's from the OS cache before each
//!   round, by opening each without buffering, which needs no privilege but
//!   works only on Windows.
//! - `DCSTERRAIN_BENCH_CACHE`: the dcsterrain cache it builds and reads,
//!   kept between runs so only the first run builds it; a directory under
//!   the system temp when unset. The install is only read.
//! - `DCSTERRAIN_BENCH_OUT`: where to write the figures as JSON.
//! - `DCSTERRAIN_BENCH_BASELINE`: figures an earlier run wrote, to print
//!   each median against.
//!
//! A round is one process's worth of work: a terrain opened, then each of
//! the case's requests answered against it, so blocks and tiles read for
//! one request may serve the next, as in a batch. A `fresh` case opens the
//! terrain for every request, as one call of the binary does. Every round
//! asks the same requests, drawn from a seeded generator, so runs compare.

use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use dcsterrain_core::cache::Cache;
use dcsterrain_core::query::{self, Terrain};
use dcsterrain_core::source::Source;
use dcsterrain_core::synth::{self, Synth};
use serde_json::{Value, json};

/// Counts every allocation, so a figure can say how many a request made.
struct Counting;

static ALLOCS: AtomicU64 = AtomicU64::new(0);
static ALLOC_BYTES: AtomicU64 = AtomicU64::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        ALLOC_BYTES.fetch_add(l.size() as u64, Ordering::Relaxed);
        unsafe { System.alloc(l) }
    }

    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        ALLOC_BYTES.fetch_add(l.size() as u64, Ordering::Relaxed);
        unsafe { System.alloc_zeroed(l) }
    }

    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }

    unsafe fn realloc(&self, p: *mut u8, l: Layout, size: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        ALLOC_BYTES.fetch_add(size as u64, Ordering::Relaxed);
        unsafe { System.realloc(p, l, size) }
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

fn allocs() -> (u64, u64) {
    (
        ALLOCS.load(Ordering::Relaxed),
        ALLOC_BYTES.load(Ordering::Relaxed),
    )
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

    fn pick<'a, T>(&mut self, v: &'a [T]) -> &'a T {
        &v[(self.next() * v.len() as f64) as usize % v.len()]
    }
}

/// What a case draws its requests from: the terrain's land, found once.
struct Ground {
    /// `[x, z]` of points on land, spread over the whole terrain.
    land: Vec<[f64; 2]>,
    bounds: [f64; 4],
}

impl Ground {
    fn of(src: &dyn Source) -> Ground {
        let g = src.grid();
        let c = f64::from(g.cell_size);
        let bounds = [
            g.origin_x,
            g.origin_z,
            g.origin_x + f64::from(g.height) * c,
            g.origin_z + f64::from(g.width) * c,
        ];
        let mut u = Unit(0x5eed_0001);
        let p: Vec<[f64; 2]> = (0..8192)
            .map(|_| {
                [
                    bounds[0] + u.next() * (bounds[2] - bounds[0]),
                    bounds[1] + u.next() * (bounds[3] - bounds[1]),
                ]
            })
            .collect();
        let found = src.points(&p).expect("the terrain answers points");
        let land = p
            .into_iter()
            .zip(found)
            .filter_map(|(p, g)| g.filter(|g| g.water == 0).map(|_| p))
            .collect::<Vec<_>>();
        assert!(!land.is_empty(), "no land found");
        Ground { land, bounds }
    }

    fn inside(&self, p: [f64; 2]) -> bool {
        let m = 2000.0;
        (self.bounds[0] + m..self.bounds[2] - m).contains(&p[0])
            && (self.bounds[1] + m..self.bounds[3] - m).contains(&p[1])
    }

    /// A land point `1..40` km from `a`, or `a` moved 1 km when none is
    /// found.
    fn far_from(&self, u: &mut Unit, a: [f64; 2], lo: f64, hi: f64) -> [f64; 2] {
        for _ in 0..64 {
            let d = lo + u.next() * (hi - lo);
            let r = u.next() * std::f64::consts::TAU;
            let b = [a[0] + d * r.cos(), a[1] + d * r.sin()];
            if self.inside(b) {
                return b;
            }
        }
        [a[0] + lo, a[1]]
    }
}

fn xz(p: [f64; 2]) -> Value {
    json!({"x": p[0], "z": p[1]})
}

fn circle(p: [f64; 2], radius: f64) -> Value {
    json!({"Circle": {"x": p[0], "z": p[1], "radius": radius}})
}

/// How a case's requests reach the terrain.
#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// One terrain per round, every request against it.
    Held,
    /// A terrain opened for every request.
    Fresh,
    /// Only the opening, nothing asked.
    Open,
    /// The binary, run once per request.
    Cli,
    /// An empty dcsterrain cache, then the request: every entry it needs
    /// built.
    Index,
}

struct Case {
    name: String,
    op: &'static str,
    mode: Mode,
    requests: Vec<Value>,
    /// Remove the horizon tiles from the cache before each round.
    cold_horizon: bool,
}

const LAYERS: &[&str] = &[
    "valid",
    "slope",
    "aspect",
    "roughness",
    "tpi_300",
    "tpi_2000",
    "dist_road",
    "dist_rail",
    "dist_water",
    "dist_builtup",
    "count_building",
    "count_industrial",
    "tree_density",
    "share_forest",
];

fn cases(ground: &Ground, install: bool) -> Vec<Case> {
    let mut u = Unit(0x9e37_79b9_7f4a_7c15);
    let mut out = Vec::new();
    let mut case = |name: &str, op: &'static str, mode: Mode, requests: Vec<Value>| {
        out.push(Case {
            name: name.to_string(),
            op,
            mode,
            requests,
            cold_horizon: false,
        });
    };
    let land = &ground.land;
    let n = |k: usize, u: &mut Unit, f: &mut dyn FnMut(&mut Unit) -> Value| {
        (0..k).map(|_| f(u)).collect::<Vec<_>>()
    };

    case("open", "describe", Mode::Open, vec![json!({}); 10]);
    case("describe", "describe", Mode::Held, vec![json!({}); 10]);
    let point = n(
        20,
        &mut u,
        &mut |u| json!({"points": [xz(*u.pick(land))], "layers": ["height", "water", "surface", "cover"]}),
    );
    case("sample_point", "sample", Mode::Held, point.clone());
    case("sample_point/fresh", "sample", Mode::Fresh, point.clone());
    let scattered = |u: &mut Unit, k: usize| {
        let p: Vec<Value> = (0..k).map(|_| xz(*u.pick(land))).collect();
        json!({"points": p, "layers": ["height"], "max_rows": 10_000})
    };
    case(
        "sample_100_scattered",
        "sample",
        Mode::Held,
        n(5, &mut u, &mut |u| scattered(u, 100)),
    );
    case(
        "sample_10k_scattered",
        "sample",
        Mode::Held,
        n(2, &mut u, &mut |u| scattered(u, 10_000)),
    );
    let clustered = n(3, &mut u, &mut |u| {
        let c = *u.pick(land);
        let p: Vec<Value> = (0..10_000)
            .map(|_| {
                xz([
                    c[0] + (u.next() - 0.5) * 5000.0,
                    c[1] + (u.next() - 0.5) * 5000.0,
                ])
            })
            .collect();
        json!({"points": p, "layers": ["height"], "max_rows": 10_000})
    });
    case("sample_10k_clustered", "sample", Mode::Held, clustered);
    let region = n(
        3,
        &mut u,
        &mut |u| json!({"region": circle(*u.pick(land), 2500.0), "step": 50, "layers": ["height", "water", "cover"]}),
    );
    case("sample_region_5km", "sample", Mode::Held, region);
    for &layer in LAYERS {
        let r = n(
            3,
            &mut u,
            &mut |u| json!({"region": circle(*u.pick(land), 1000.0), "step": 50, "layers": [layer]}),
        );
        case(&format!("layer_{layer}"), "sample", Mode::Held, r);
    }
    let enclosure = n(
        2,
        &mut u,
        &mut |u| json!({"region": circle(*u.pick(land), 1000.0), "step": 50, "layers": ["enclosure"]}),
    );
    case("layer_enclosure", "sample", Mode::Held, enclosure.clone());
    out.push(Case {
        name: "horizon_first_touch".into(),
        op: "sample",
        mode: Mode::Held,
        requests: enclosure[..1].to_vec(),
        cold_horizon: true,
    });
    let mut case = |name: &str, op: &'static str, mode: Mode, requests: Vec<Value>| {
        out.push(Case {
            name: name.to_string(),
            op,
            mode,
            requests,
            cold_horizon: false,
        });
    };
    let geo = n(5, &mut u, &mut |u| {
        let p: Vec<Value> = (0..100).map(|_| xz(*u.pick(land))).collect();
        json!({"to_latlon": p})
    });
    case("geo_100", "geo", Mode::Held, geo);
    let mut kinds = vec!["road", "rail", "town", "airdrome", "beacon", "node"];
    if install {
        kinds.push("scenery");
    }
    for kind in kinds {
        let r = n(20, &mut u, &mut |u| {
            let p = u.pick(land);
            json!({"kind": kind, "x": p[0], "z": p[1]})
        });
        case(&format!("nearest_{kind}"), "nearest", Mode::Held, r);
    }
    let boxes = n(10, &mut u, &mut |u| {
        let p = u.pick(land);
        let s = 500.0;
        json!({"region": {"Box": {"min_x": p[0] - s, "min_z": p[1] - s, "max_x": p[0] + s, "max_z": p[1] + s}}, "max_rows": 100_000})
    });
    case("scenery_in_1km", "scenery_in", Mode::Held, boxes.clone());
    case("scenery_in_1km/fresh", "scenery_in", Mode::Fresh, boxes);
    let lines = n(50, &mut u, &mut |u| {
        let a = *u.pick(land);
        let b = ground.far_from(u, a, 1000.0, 40_000.0);
        let eye = *u.pick(&[2.0, 10.0, 100.0]);
        json!({"a": {"x": a[0], "z": a[1], "alt_agl": eye}, "b": {"x": b[0], "z": b[1], "alt_agl": 2.0}})
    });
    case("visible", "visible", Mode::Held, lines.clone());
    case(
        "visible/fresh",
        "visible",
        Mode::Fresh,
        lines[..10].to_vec(),
    );
    for (name, radius, k) in [
        ("viewshed_10km", 10_000.0, 3),
        ("viewshed_20km", 20_000.0, 2),
    ] {
        let r = n(k, &mut u, &mut |u| {
            let p = u.pick(land);
            json!({"observer": {"x": p[0], "z": p[1], "alt_agl": 10.0}, "radius": radius, "target_alt_agl": 2.0})
        });
        case(name, "viewshed", Mode::Held, r);
    }
    case("cli_describe", "describe", Mode::Cli, vec![json!({}); 5]);
    case("cli_visible", "visible", Mode::Cli, lines[..5].to_vec());
    case(
        "index_empty_cache",
        "describe",
        Mode::Index,
        vec![json!({})],
    );
    out
}

/// The terrain a bench runs over.
#[derive(Clone)]
enum Target {
    Synth,
    Install { root: PathBuf, id: String },
}

impl Target {
    fn name(&self) -> &str {
        match self {
            Target::Synth => synth::TERRAIN_ID,
            Target::Install { id, .. } => id,
        }
    }

    fn open(&self, cache: &Cache) -> Terrain {
        match self {
            Target::Synth => Terrain::Synth(Box::new(
                Synth::new(
                    synth::DEFAULT_SEED,
                    synth::DEFAULT_SIZE_KM,
                    synth::CELL_SIZE_M,
                )
                .expect("the synthetic terrain"),
            )),
            Target::Install { root, id } => {
                Terrain::open(root, id, cache, true).expect("the terrain opens")
            }
        }
    }

    /// Every file of the terrain and of its cache, to drop from the OS
    /// cache.
    fn files(&self, t: &Terrain, cache: &Cache) -> Vec<PathBuf> {
        let mut out = Vec::new();
        if let (Target::Install { root, .. }, Terrain::Install(i)) = (self, t) {
            out.extend(i.map().files.iter().map(|f| root.join(&f.path)));
            out.push(root.join(&i.map().surface.path));
            walk(&cache.dir(self.name()), &mut |p, _| {
                out.push(p.to_path_buf())
            });
        }
        out
    }
}

fn walk(dir: &Path, f: &mut dyn FnMut(&Path, u64)) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let p = e.path();
        match e.metadata() {
            Ok(m) if m.is_dir() => walk(&p, f),
            Ok(m) => f(&p, m.len()),
            Err(_) => {}
        }
    }
}

/// Drops `p` from the OS cache: opening a file without buffering makes
/// Windows write back and discard the pages it holds of it.
#[cfg(windows)]
fn purge(p: &Path) -> bool {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_FLAG_NO_BUFFERING: u32 = 0x2000_0000;
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_NO_BUFFERING)
        .open(p)
        .is_ok()
}

#[cfg(not(windows))]
fn purge(_: &Path) -> bool {
    false
}

/// One request's cost.
#[derive(Clone, Copy)]
struct Sample {
    ms: f64,
    allocs: u64,
    bytes: u64,
}

struct Figures {
    samples: Vec<Sample>,
    /// The median of each round.
    rounds: Vec<f64>,
    /// The warm-up round's median: cold process caches, and for a
    /// first-touch case a cold dcsterrain cache.
    first: f64,
    errors: usize,
}

fn percentile(v: &[f64], p: f64) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    let mut v = v.to_vec();
    v.sort_unstable_by(f64::total_cmp);
    v[((v.len() - 1) as f64 * p).round() as usize]
}

fn timed(f: impl FnOnce() -> bool) -> (Sample, bool) {
    let (a0, b0) = allocs();
    let t = Instant::now();
    let ok = f();
    let ms = t.elapsed().as_secs_f64() * 1e3;
    let (a1, b1) = allocs();
    (
        Sample {
            ms,
            allocs: a1 - a0,
            bytes: b1 - b0,
        },
        ok,
    )
}

struct Bench {
    target: Target,
    cache: Cache,
    cli: PathBuf,
}

impl Bench {
    /// One round of `case`: its samples and errors.
    fn round(&self, case: &Case) -> (Vec<Sample>, usize) {
        let mut out = Vec::new();
        let mut errors = 0;
        let ask = |t: &Terrain, r: &Value| query::answer(t, case.op, r).is_ok();
        match case.mode {
            Mode::Held => {
                let t = self.target.open(&self.cache);
                for r in &case.requests {
                    let (s, ok) = timed(|| ask(&t, r));
                    out.push(s);
                    errors += usize::from(!ok);
                }
            }
            Mode::Fresh => {
                for r in &case.requests {
                    let (s, ok) = timed(|| ask(&self.target.open(&self.cache), r));
                    out.push(s);
                    errors += usize::from(!ok);
                }
            }
            Mode::Open => {
                for _ in &case.requests {
                    out.push(
                        timed(|| {
                            let _t = self.target.open(&self.cache);
                            true
                        })
                        .0,
                    );
                }
            }
            Mode::Cli => {
                for r in &case.requests {
                    let (s, ok) = timed(|| self.cli(case.op, r));
                    out.push(s);
                    errors += usize::from(!ok);
                }
            }
            Mode::Index => {
                let dir = std::env::temp_dir()
                    .join(format!("dcsterrain-bench-index-{}", std::process::id()));
                let _ = std::fs::remove_dir_all(&dir);
                let cache = Cache::new(&dir);
                for r in &case.requests {
                    let (s, ok) = timed(|| ask(&self.target.open(&cache), r));
                    out.push(s);
                    errors += usize::from(!ok);
                }
                let _ = std::fs::remove_dir_all(&dir);
            }
        }
        (out, errors)
    }

    fn cli(&self, op: &str, r: &Value) -> bool {
        let mut c = Command::new(&self.cli);
        c.arg("query").arg("--terrain").arg(self.target.name());
        if let Target::Install { root, .. } = &self.target {
            c.arg("--install")
                .arg(root)
                .arg("--cache")
                .arg(self.cache.root())
                .arg("--allow-crs-residual");
        }
        c.arg(op).arg(r.to_string());
        c.output().map(|o| o.status.success()).unwrap_or(false)
    }

    fn purge(&self) -> bool {
        let t = self.target.open(&self.cache);
        let files = self.target.files(&t, &self.cache);
        drop(t);
        !files.is_empty() && files.iter().all(|p| purge(p))
    }

    fn run(&self, case: &Case, rounds: usize, cold_os: bool) -> Figures {
        let clear_horizon = || {
            if case.cold_horizon {
                let _ = std::fs::remove_dir_all(self.cache.dir(self.target.name()).join("horizon"));
            }
        };
        clear_horizon();
        if cold_os {
            self.purge();
        }
        let (warm_up, _) = self.round(case);
        let first = percentile(&warm_up.iter().map(|s| s.ms).collect::<Vec<_>>(), 0.5);
        let rounds = if case.mode == Mode::Index { 0 } else { rounds };
        let mut samples = Vec::new();
        let mut medians = Vec::new();
        let mut errors = 0;
        for _ in 0..rounds {
            clear_horizon();
            if cold_os {
                self.purge();
            }
            let (s, e) = self.round(case);
            medians.push(percentile(&s.iter().map(|s| s.ms).collect::<Vec<_>>(), 0.5));
            samples.extend(s);
            errors += e;
        }
        if samples.is_empty() {
            samples = warm_up;
            medians.push(first);
        }
        Figures {
            samples,
            rounds: medians,
            first,
            errors,
        }
    }
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

/// The bytes of each kind of cache entry under `dir`: a file by its name,
/// a directory of tiles by the directory's.
fn sizes(dir: &Path) -> BTreeMap<String, (u64, u64)> {
    let mut out: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    walk(dir, &mut |p, len| {
        let rel = p.strip_prefix(dir).unwrap_or(p);
        let mut parts = rel.components();
        let first = parts
            .next()
            .map(|c| c.as_os_str().to_string_lossy().into_owned());
        let kind = match (first, parts.next()) {
            (Some(d), Some(_)) => format!("{d}/"),
            (Some(f), None) => f,
            _ => return,
        };
        let e = out.entry(kind).or_default();
        e.0 += 1;
        e.1 += len;
    });
    out
}

fn main() {
    // The pool the binary starts, so a case times what a call of it does.
    dcsterrain_core::threads::init(None);
    let args: Vec<String> = std::env::args()
        .skip(1)
        .filter(|a| !a.starts_with("--"))
        .collect();
    let rounds: usize = env("DCSTERRAIN_BENCH_ROUNDS").map_or(3, |v| v.parse().expect("rounds"));
    let os = env("DCSTERRAIN_BENCH_OS").unwrap_or_else(|| "both".into());
    let cache_dir = env("DCSTERRAIN_BENCH_CACHE").map_or_else(
        || std::env::temp_dir().join("dcsterrain-bench-cache"),
        PathBuf::from,
    );
    let cache = Cache::new(&cache_dir);
    let mut targets = vec![Target::Synth];
    if let Some(root) = env("DCSTERRAIN_INSTALL") {
        let ids = env("DCSTERRAIN_BENCH_TERRAINS").unwrap_or_else(|| "Caucasus,GermanyCW".into());
        for id in ids.split(',') {
            targets.push(Target::Install {
                root: PathBuf::from(&root),
                id: id.trim().to_string(),
            });
        }
    } else {
        eprintln!("DCSTERRAIN_INSTALL is not set; the synthetic terrain only");
    }
    let baseline: Option<Value> = env("DCSTERRAIN_BENCH_BASELINE").map(|p| {
        serde_json::from_str(&std::fs::read_to_string(&p).expect("the baseline reads"))
            .expect("the baseline is JSON")
    });
    let base = |key: &str, field: &str| {
        baseline
            .as_ref()
            .and_then(|b| b["cases"][key][field].as_f64())
    };

    let mut cases_out = serde_json::Map::new();
    let mut sizes_out = serde_json::Map::new();
    let mut ratios: Vec<f64> = Vec::new();
    println!(
        "| case | os | n | median ms | p95 ms | first ms | rounds ms | allocs | KiB | err |{}",
        if baseline.is_some() {
            " base ms | × |"
        } else {
            ""
        }
    );
    println!(
        "|---|---|---|---|---|---|---|---|---|---|{}",
        if baseline.is_some() { "---|---|" } else { "" }
    );
    for target in &targets {
        let bench = Bench {
            target: target.clone(),
            cache: cache.clone(),
            cli: PathBuf::from(env!("CARGO_BIN_EXE_dcsterrain")),
        };
        let install = matches!(target, Target::Install { .. });
        let keep = |name: &str| {
            let full = format!("{}/{name}", target.name());
            args.is_empty() || args.iter().any(|a| full.contains(a.as_str()))
        };
        let all = {
            let t = target.open(&cache);
            cases(&Ground::of(t.source()), install)
        };
        let todo: Vec<&Case> = all.iter().filter(|c| keep(&c.name)).collect();
        if todo.is_empty() {
            continue;
        }
        let modes: &[bool] = match (install, os.as_str()) {
            (false, _) | (true, "warm") => &[false],
            (true, "cold") => &[true],
            _ => &[false, true],
        };
        for &cold in modes {
            if cold && !bench.purge() {
                eprintln!("cannot drop files from the OS cache here; cold skipped");
                continue;
            }
            for case in &todo {
                let f = bench.run(case, rounds, cold);
                let ms: Vec<f64> = f.samples.iter().map(|s| s.ms).collect();
                let median = percentile(&ms, 0.5);
                let p95 = percentile(&ms, 0.95);
                let al = percentile(
                    &f.samples
                        .iter()
                        .map(|s| s.allocs as f64)
                        .collect::<Vec<_>>(),
                    0.5,
                );
                let kib = percentile(
                    &f.samples.iter().map(|s| s.bytes as f64).collect::<Vec<_>>(),
                    0.5,
                ) / 1024.0;
                let os_name = if !install {
                    "-"
                } else if cold {
                    "cold"
                } else {
                    "warm"
                };
                let key = format!("{}/{}/{}", target.name(), case.name, os_name);
                let spread = format!(
                    "{:.2}–{:.2}",
                    f.rounds.iter().copied().fold(f64::INFINITY, f64::min),
                    f.rounds.iter().copied().fold(f64::NEG_INFINITY, f64::max)
                );
                let mut row = format!(
                    "| {}/{} | {os_name} | {} | {median:.3} | {p95:.3} | {:.3} | {spread} | {al:.0} | {kib:.0} | {} |",
                    target.name(),
                    case.name,
                    ms.len(),
                    f.first,
                    f.errors
                );
                if baseline.is_some() {
                    match base(&key, "median_ms") {
                        Some(b) => {
                            row += &format!(" {b:.3} | {:.2} |", b / median);
                            if median > 0.0 && b > 0.0 {
                                ratios.push(b / median);
                            }
                        }
                        None => row += " - | - |",
                    }
                }
                println!("{row}");
                cases_out.insert(
                    key,
                    json!({
                        "n": ms.len(),
                        "median_ms": median,
                        "p95_ms": p95,
                        "first_ms": f.first,
                        "round_medians_ms": f.rounds,
                        "allocs": al,
                        "alloc_kib": kib,
                        "errors": f.errors,
                    }),
                );
            }
        }
        if install {
            let s = sizes(&cache.dir(target.name()));
            let mut m = serde_json::Map::new();
            for (kind, (files, bytes)) in s {
                eprintln!("{}: {kind} {files} files, {bytes} bytes", target.name());
                m.insert(kind, json!({"files": files, "bytes": bytes}));
            }
            sizes_out.insert(target.name().to_string(), Value::Object(m));
        }
    }
    if !ratios.is_empty() {
        let g = (ratios.iter().map(|r| r.ln()).sum::<f64>() / ratios.len() as f64).exp();
        println!(
            "\ngeometric mean speedup over {} cases: {g:.2}×",
            ratios.len()
        );
    }
    if let Some(p) = env("DCSTERRAIN_BENCH_OUT") {
        let out = json!({"cases": cases_out, "sizes": sizes_out, "rounds": rounds});
        std::fs::write(&p, serde_json::to_string_pretty(&out).unwrap()).expect("the figures write");
        eprintln!("wrote {p}");
    }
}
