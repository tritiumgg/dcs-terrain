//! The query benchmark: every operation, the per-query layers, the horizon,
//! the roads, opening a terrain and calls of the binary end to end, over the
//! synthetic terrain and, opt-in, over an install.
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
//!   unset, or 9 with `DCSTERRAIN_BENCH_BOUNDS`.
//! - `DCSTERRAIN_BENCH_OS`: `warm`, `cold` or `both` (the default). Cold
//!   drops the terrain's files and the cache's from the OS cache before each
//!   round, by opening each without buffering, which needs no privilege but
//!   works only on Windows.
//! - `DCSTERRAIN_BENCH_CACHE`: the dcsterrain cache it builds and reads,
//!   kept between runs so only the first run builds it; a directory under
//!   the system temp when unset. The install is only read. `towns` and
//!   `chokepoints` answer only from what `dcsterrain index` built, so run
//!   that over this directory first.
//! - `DCSTERRAIN_BENCH_OUT`: where to write the figures as JSON.
//! - `DCSTERRAIN_BENCH_BASELINE`: figures an earlier run wrote, to print
//!   each median against.
//! - `DCSTERRAIN_BENCH_BOUNDS`: any value runs only the cases with a target,
//!   and fails if one spends more than 3× its target's cycles.
//!   `mise run bench-bounds` sets it.
//!
//! A round is one process's worth of work: a terrain opened, then each of
//! the case's requests answered against it, so blocks and tiles read for
//! one request may serve the next, as in a batch. A `fresh` case opens the
//! terrain for every request, as one call of the binary does. Every round
//! asks the same requests, drawn from a seeded generator, so runs compare.
//!
//! A verdict judges cycles, every thread's added up, which a busy machine
//! changes far less than the wall clock: a thread waiting for a core costs
//! none. The wall clock stands beside them for a reader, and is the only
//! figure that sees a disk. The figure judged is the median of the rounds'
//! medians, the warm-up round dropped because it fills the process's caches;
//! a median ignores the odd slow round, so the outliers are counted and
//! kept. Its interval is the order statistics that hold the true median with
//! at least 95 % confidence, whatever the distribution, for 6 rounds or more.

use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use dcsterrain_core::cache::Cache;
use dcsterrain_core::query::{self, Terrain};
use dcsterrain_core::synth::{self, Synth};
use dcsterrain_core::threads::{self, cpu_cycles, cycle_hz};
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
    /// Ids of the airdromes nearest the first land point.
    airdromes: Vec<Value>,
}

impl Ground {
    fn of(t: &Terrain) -> Ground {
        let src = t.source();
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
        let near = json!({"kind": "airdrome", "x": land[0][0], "z": land[0][1], "n": 8});
        let found = query::answer(t, "nearest", &near).expect("the terrain has airdromes");
        let airdromes = found["results"].as_array().expect("results").iter();
        let airdromes: Vec<Value> = airdromes.map(|a| a["id"].clone()).collect();
        assert!(!airdromes.is_empty(), "no airdromes found");
        Ground {
            land,
            bounds,
            airdromes,
        }
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
    /// The binary, run once a round with every request as a line of stdin.
    Batch,
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
    let lattice = n(3, &mut u, &mut |u| {
        let p = u.pick(land);
        let s = 2500.0;
        json!({"region": {"Box": {"min_x": p[0] - s, "min_z": p[1] - s, "max_x": p[0] + s, "max_z": p[1] + s}},
               "step": 50, "layers": ["height"], "max_rows": 20_000})
    });
    case("sample_10k_lattice", "sample", Mode::Held, lattice);
    let long_lines = n(50, &mut u, &mut |u| {
        let a = *u.pick(land);
        let b = ground.far_from(u, a, 10_000.0, 40_000.0);
        json!({"a": {"x": a[0], "z": a[1], "alt_agl": 10}, "b": {"x": b[0], "z": b[1], "alt_agl": 2}})
    });
    case("visible_10_40km", "visible", Mode::Held, long_lines.clone());
    case(
        "visible_10_40km/fresh",
        "visible",
        Mode::Fresh,
        long_lines[..10].to_vec(),
    );
    let site = |p: [f64; 2]| json!({"x": p[0], "z": p[1], "alt_agl": 10});
    let floors = n(
        2,
        &mut u,
        &mut |u| json!({"site": site(*u.pick(land)), "radius": 20_000, "geojson": true}),
    );
    case("radar_floor_20km", "radar_floor", Mode::Held, floors);
    let exposure = n(1, &mut u, &mut |u| {
        let c = *u.pick(land);
        let sites: Vec<Value> = (0..10)
            .map(|_| json!({"observer": site(ground.far_from(u, c, 1000.0, 30_000.0)), "radius": 20_000}))
            .collect();
        json!({"sites": sites, "target_alt_agl": 30})
    });
    case("exposure_10_sites", "exposure", Mode::Held, exposure);
    let coverage = n(1, &mut u, &mut |u| {
        let p = *u.pick(land);
        json!({"site": site(p), "envelope": {"radius": 20_000, "alt_bands_agl": [30, 100]},
               "asset": circle(p, 5000.0)})
    });
    case("coverage_20km", "coverage", Mode::Held, coverage);
    let spectrum = n(
        2,
        &mut u,
        &mut |u| json!({"site": site(*u.pick(land)), "radius": 20_000, "alt_bands_agl": [30, 100]}),
    );
    case(
        "approach_spectrum_20km",
        "approach_spectrum",
        Mode::Held,
        spectrum,
    );
    let unmask = n(5, &mut u, &mut |u| {
        let a = *u.pick(land);
        let (b, c) = (
            ground.far_from(u, a, 2000.0, 10_000.0),
            ground.far_from(u, a, 2000.0, 10_000.0),
        );
        let leg = |p: [f64; 2]| json!({"x": p[0], "z": p[1], "alt_agl": 100});
        json!({"site": site(a), "route": [leg(b), leg(c)]})
    });
    case("unmask_profile", "unmask_profile", Mode::Held, unmask);
    let square = |p: [f64; 2], s: f64| json!({"Box": {"min_x": p[0] - s, "min_z": p[1] - s, "max_x": p[0] + s, "max_z": p[1] + s}});
    let footprint = json!({"width": 60, "depth": 40});
    let geometric = json!([
        {"name": "slope_max", "params": {"deg": 5}},
        {"name": "dist_road_band", "params": {"min": 50, "max": 2000}},
    ]);
    let sites = n(2, &mut u, &mut |u| {
        json!({"region": square(*u.pick(land), 10_000.0), "footprint": footprint,
                        "criteria": geometric, "step": 100, "spacing": 1000, "limit": 50})
    });
    case("find_sites_20km", "find_sites", Mode::Held, sites);
    let los = n(1, &mut u, &mut |u| {
        let c = *u.pick(land);
        let observers: Vec<Value> = (0..36)
            .map(|_| site(ground.far_from(u, c, 2000.0, 15_000.0)))
            .collect();
        let criteria = json!([
            {"name": "slope_max", "params": {"deg": 5}},
            {"name": "not_visible_from", "params": {"observers": observers, "max_fraction": 0.1}},
        ]);
        json!({"region": square(c, 10_000.0), "footprint": footprint, "criteria": criteria,
               "step": 100, "spacing": 1000, "limit": 50})
    });
    case("find_sites_los_36", "find_sites", Mode::Held, los);
    let masked = json!([
        {"name": "masked_from", "params": {"bearing_deg": 0, "half_width_deg": 45, "min_elev_deg": 1}},
    ]);
    let horizon = json!({"region": square(land[0], 10_000.0), "footprint": footprint,
                         "criteria": masked, "step": 100, "spacing": 1000, "limit": 50});
    out.push(Case {
        name: "find_sites_horizon_first_touch".into(),
        op: "find_sites",
        mode: Mode::Held,
        requests: vec![horizon.clone()],
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
    // The synthetic terrain keeps no horizons, so every touch is the first.
    if install {
        case(
            "find_sites_horizon",
            "find_sites",
            Mode::Held,
            vec![horizon],
        );
    }
    let score = n(10, &mut u, &mut |u| {
        let p = u.pick(land);
        json!({"x": p[0], "z": p[1], "footprint": footprint, "criteria": geometric})
    });
    case("score_site", "score_site", Mode::Held, score);
    let pairs = n(10, &mut u, &mut |u| {
        let a = *u.pick(land);
        let b = ground.far_from(u, a, 45_000.0, 55_000.0);
        json!({"network": "roads", "from": {"x": a[0], "z": a[1]}, "to": {"x": b[0], "z": b[1]}})
    });
    case("route_50km", "route", Mode::Held, pairs.clone());
    let alternatives = pairs[..3].iter().map(|r| {
        let mut r = r.clone();
        r["k"] = json!(3);
        r
    });
    case(
        "route_alternatives_50km",
        "route_alternatives",
        Mode::Held,
        alternatives.collect(),
    );
    let chokepoints = n(
        2,
        &mut u,
        &mut |u| json!({"region": circle(*u.pick(land), 20_000.0), "network": "roads", "limit": 20}),
    );
    case("chokepoints_20km", "chokepoints", Mode::Held, chokepoints);
    let reach = n(2, &mut u, &mut |u| {
        let p = u.pick(land);
        json!({"x": p[0], "z": p[1], "max_slope_deg": 30, "network": "roads"})
    });
    case("trafficability", "trafficability", Mode::Held, reach);
    let info = n(20, &mut u, &mut |u| json!({"points": [xz(*u.pick(land))]}));
    case("point_info", "point_info", Mode::Held, info.clone());
    case(
        "point_info/fresh",
        "point_info",
        Mode::Fresh,
        info[..5].to_vec(),
    );
    let fields: Vec<Value> = ground
        .airdromes
        .iter()
        .map(|a| json!({"airdrome": a}))
        .collect();
    case("airfield_info", "airfield_info", Mode::Held, fields.clone());
    case("airfield_info/fresh", "airfield_info", Mode::Fresh, fields);
    let placements = n(1, &mut u, &mut |u| {
        let shapes = [
            json!("Point"),
            json!({"Rect": {"width": 60, "depth": 40}}),
            json!({"Circle": {"radius": 30}}),
        ];
        let p: Vec<Value> = (0..1000)
            .map(|i| {
                let at = u.pick(land);
                json!({"id": i.to_string(), "x": at[0], "z": at[1], "heading": 0, "shape": shapes[i % 3]})
            })
            .collect();
        json!({"placements": p})
    });
    case(
        "validate_1000",
        "validate_placements",
        Mode::Held,
        placements,
    );
    case("towns", "towns", Mode::Held, vec![json!({"limit": 50}); 3]);
    case("cli_describe", "describe", Mode::Cli, vec![json!({}); 5]);
    case("cli_sample", "sample", Mode::Cli, point[..5].to_vec());
    case("cli_visible", "visible", Mode::Cli, lines[..5].to_vec());
    let lines_100: Vec<Value> = lines
        .iter()
        .map(|r| ("visible", r))
        .chain(point.iter().cycle().take(50).map(|r| ("sample", r)))
        .enumerate()
        .map(|(id, (op, request))| json!({"id": id, "op": op, "request": request}))
        .collect();
    case("cli_batch_100", "", Mode::Batch, lines_100);
    case(
        "index_empty_cache",
        "sample",
        Mode::Index,
        vec![json!({"points": [xz(land[0])], "layers": ["height", "dist_road", "count_building"]})],
    );
    out
}

/// A case's target in milliseconds, and whether it is stated with the OS
/// cache cold. Targets are stated cold unless they say otherwise; `visible`
/// within a batch, a call end to end and the cache build say otherwise.
const TARGETS: &[(&str, f64, bool)] = &[
    ("open", 20.0, true),
    ("describe", 20.0, true),
    ("sample_10k_lattice", 100.0, true),
    ("sample_10k_scattered", 3000.0, true),
    ("visible_10_40km", 1.0, false),
    ("visible_10_40km/fresh", 20.0, true),
    ("cli_describe", 50.0, false),
    ("cli_sample", 50.0, false),
    ("cli_visible", 50.0, false),
    ("viewshed_20km", 300.0, true),
    ("radar_floor_20km", 300.0, true),
    ("find_sites_20km", 500.0, true),
    ("exposure_10_sites", 3000.0, true),
    ("find_sites_los_36", 10_000.0, true),
    ("find_sites_horizon_first_touch", 5000.0, true),
    ("find_sites_horizon", 500.0, true),
    ("route_50km", 500.0, true),
    ("validate_1000", 3000.0, true),
    ("point_info/fresh", 150.0, true),
    ("airfield_info/fresh", 20.0, true),
    ("index_empty_cache", 10_000.0, false),
];

/// The least time spent on a case that any counter here must see: a case
/// shorter than the CPU clock's tick reads zero with the counter alive.
const COUNTABLE_MS: f64 = 10.0;

/// The most memory a call of the binary may hold.
const TARGET_PEAK_MIB: f64 = 150.0;

/// The headroom the bounds leave: the machine running them need not be the
/// one the targets were measured on.
const BOUND_FACTOR: f64 = 3.0;

fn target(case: &str) -> Option<(f64, bool)> {
    let found = TARGETS.iter().find(|t| t.0 == case);
    found.map(|t| (t.1, t.2))
}

/// The cases whose answer is computed over the pool on any source: the sweep
/// of `viewshed` and `radar_floor`, the sites of `exposure`, the cells of a
/// new horizon. Elsewhere only an install's reads of blocks fan out, for blocks
/// not held, and two steps of a road build, within a build that averages at
/// most one core. That leaves those cases one thread's work.
const PARALLEL_CASES: &[&str] = &[
    "viewshed_20km",
    "radar_floor_20km",
    "exposure_10_sites",
    "find_sites_horizon_first_touch",
];

/// The cores a parallel case's budget counts (ADR 0007): a target is time a
/// user waits, which such a case spends on every core at once.
const PARALLEL_CORES: f64 = 8.0;

/// The cycles `case` may spend in `ms`, in millions: one core's time, or
/// [`PARALLEL_CORES`]' where the case runs on them.
fn budget_mc(case: &str, ms: f64) -> f64 {
    let cores = if PARALLEL_CASES.contains(&case) {
        PARALLEL_CORES
    } else {
        1.0
    };
    ms * 1e-3 * cycle_hz() / 1e6 * cores
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
    /// Millions of cycles, every thread's.
    mc: f64,
    allocs: u64,
    bytes: u64,
    /// The most memory a child process held, 0 for work done in this one.
    peak: u64,
}

struct Figures {
    samples: Vec<Sample>,
    /// The median of each round.
    rounds: Vec<f64>,
    rounds_mc: Vec<f64>,
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

/// The interval between two order statistics of `v` that holds its
/// distribution's median with at least 95 % confidence once `v` has 6
/// values: the rank of the median among them is binomial, whatever the
/// distribution. Under 6 it is only the range.
fn median_interval(v: &[f64]) -> (f64, f64) {
    let mut v = v.to_vec();
    v.sort_unstable_by(f64::total_cmp);
    let n = v.len();
    let mut below = 0.5f64.powi(n as i32);
    let mut tail = below;
    let mut k = 0;
    loop {
        below *= (n - k) as f64 / (k + 1) as f64;
        if tail + below > 0.025 {
            return (v[k], v[n - 1 - k]);
        }
        tail += below;
        k += 1;
    }
}

/// How many of `v` lie over 3 scaled median absolute deviations from their
/// median, the scale that estimates a standard deviation.
fn outliers(v: &[f64]) -> usize {
    let m = percentile(v, 0.5);
    let deviations: Vec<f64> = v.iter().map(|x| (x - m).abs()).collect();
    let mad = 1.4826 * percentile(&deviations, 0.5);
    deviations.iter().filter(|d| **d > 3.0 * mad).count()
}

fn timed(f: impl FnOnce() -> bool) -> (Sample, bool) {
    let (a0, b0) = allocs();
    let (t, c) = (Instant::now(), cpu_cycles());
    let ok = f();
    let mc = cpu_cycles().saturating_sub(c) as f64 / 1e6;
    let ms = t.elapsed().as_secs_f64() * 1e3;
    let (a1, b1) = allocs();
    (
        Sample {
            ms,
            mc,
            allocs: a1 - a0,
            bytes: b1 - b0,
            peak: 0,
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
        let ask = |t: &Terrain, r: &Value| match query::answer(t, case.op, r) {
            Ok(_) => true,
            Err(issues) => {
                eprintln!("{}: {}: {}", case.name, issues[0].code, issues[0].message);
                false
            }
        };
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
                    let (s, ok, _) = self.called(&[case.op, &r.to_string()], "");
                    out.push(s);
                    errors += usize::from(!ok);
                }
            }
            Mode::Batch => {
                let mut input = String::new();
                for r in &case.requests {
                    input.push_str(&format!("{r}\n"));
                }
                let (s, ok, printed) = self.called(&[], &input);
                let answered = printed
                    .lines()
                    .filter_map(|l| serde_json::from_str::<Value>(l).ok())
                    .filter(|a| a["result"].is_object())
                    .count();
                out.push(s);
                errors += usize::from(!ok) + case.requests.len().saturating_sub(answered);
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

    /// One run of the binary's `query`, with `input` on its stdin, and what
    /// it printed.
    fn called(&self, args: &[&str], input: &str) -> (Sample, bool, String) {
        let mut c = Command::new(&self.cli);
        c.arg("query").arg("--terrain").arg(self.target.name());
        if let Target::Install { root, .. } = &self.target {
            c.arg("--install")
                .arg(root)
                .arg("--cache")
                .arg(self.cache.root())
                .arg("--allow-crs-residual");
        }
        c.args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let t = Instant::now();
        let mut child = c.spawn().expect("the binary runs");
        let mut stdin = child.stdin.take().expect("a pipe");
        let input = input.to_string();
        let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()));
        let mut printed = String::new();
        let mut stdout = child.stdout.take().expect("a pipe");
        let drained = stdout.read_to_string(&mut printed).is_ok();
        let (status, cost) = threads::wait_costed(child).expect("the binary is waited for");
        let ms = t.elapsed().as_secs_f64() * 1e3;
        let wrote = writer.join().expect("the writer runs").is_ok();
        let s = Sample {
            ms,
            mc: cost.cycles as f64 / 1e6,
            allocs: 0,
            bytes: 0,
            peak: cost.peak_bytes,
        };
        (s, status.success() && drained && wrote, printed)
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
        let (warm_up, warm_up_errors) = self.round(case);
        let median = |s: &[Sample], f: fn(&Sample) -> f64| {
            percentile(&s.iter().map(f).collect::<Vec<_>>(), 0.5)
        };
        let first = median(&warm_up, |s| s.ms);
        let rounds = if case.mode == Mode::Index { 0 } else { rounds };
        let mut samples = Vec::new();
        let mut medians = Vec::new();
        let mut medians_mc = Vec::new();
        let mut errors = warm_up_errors;
        for _ in 0..rounds {
            clear_horizon();
            if cold_os {
                self.purge();
            }
            let (s, e) = self.round(case);
            medians.push(median(&s, |s| s.ms));
            medians_mc.push(median(&s, |s| s.mc));
            samples.extend(s);
            errors += e;
        }
        if samples.is_empty() {
            medians.push(first);
            medians_mc.push(median(&warm_up, |s| s.mc));
            samples = warm_up;
        }
        Figures {
            samples,
            rounds: medians,
            rounds_mc: medians_mc,
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
    threads::init(None);
    let args: Vec<String> = std::env::args()
        .skip(1)
        .filter(|a| !a.starts_with("--"))
        .collect();
    let bounds = env("DCSTERRAIN_BENCH_BOUNDS").is_some();
    let rounds: usize = env("DCSTERRAIN_BENCH_ROUNDS")
        .map_or(if bounds { 9 } else { 3 }, |v| v.parse().expect("rounds"));
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
    let mut over: Vec<String> = Vec::new();
    println!("cycles are counted at {:.0} MHz", cycle_hz() / 1e6);
    println!(
        "| case | os | n | median Mc | 95 % Mc | outliers | median ms | p95 ms | first ms | target ms | budget Mc | verdict | peak MiB | allocs | KiB | err |{}",
        if baseline.is_some() {
            " base Mc | × |"
        } else {
            ""
        }
    );
    println!(
        "|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|{}",
        if baseline.is_some() { "---|---|" } else { "" }
    );
    for terrain in &targets {
        let bench = Bench {
            target: terrain.clone(),
            cache: cache.clone(),
            cli: PathBuf::from(env!("CARGO_BIN_EXE_dcsterrain")),
        };
        let install = matches!(terrain, Target::Install { .. });
        let keep = |name: &str| {
            let full = format!("{}/{name}", terrain.name());
            (!bounds || target(name).is_some())
                && (args.is_empty() || args.iter().any(|a| full.contains(a.as_str())))
        };
        let all = {
            let t = terrain.open(&cache);
            cases(&Ground::of(&t), install)
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
                let median = percentile(&f.rounds, 0.5);
                let median_mc = percentile(&f.rounds_mc, 0.5);
                let (lo_mc, hi_mc) = median_interval(&f.rounds_mc);
                let odd = outliers(&f.rounds_mc);
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
                let peak = f.samples.iter().map(|s| s.peak).max().unwrap_or(0);
                let peak_mib = peak as f64 / (1024.0 * 1024.0);
                let os_name = if !install {
                    "-"
                } else if cold {
                    "cold"
                } else {
                    "warm"
                };
                let key = format!("{}/{}/{}", terrain.name(), case.name, os_name);
                let goal = target(&case.name);
                // Only the OS cache state a target is stated in is judged.
                let judged = goal.filter(|g| !install || g.1 == cold);
                let budget = judged.map(|g| budget_mc(&case.name, g.0));
                let verdict = match (judged, budget) {
                    (Some(g), Some(b)) if median_mc > b => format!(
                        "MISS {:.2}×{}",
                        median_mc / b,
                        if median > g.0 { ", wall too" } else { "" }
                    ),
                    (Some(g), Some(_)) if median > g.0 => "ok, wall over".to_string(),
                    (Some(_), Some(_)) => "ok".to_string(),
                    _ => "-".to_string(),
                };
                let verdict = if peak_mib > TARGET_PEAK_MIB {
                    format!("{verdict}, memory over")
                } else {
                    verdict
                };
                if bounds {
                    let ms = goal.map_or(f64::INFINITY, |g| g.0);
                    let limit = budget_mc(&case.name, ms) * BOUND_FACTOR;
                    if median_mc > limit {
                        over.push(format!("{key}: {median_mc:.1} Mc over {limit:.1} Mc"));
                    }
                    let spent_ms: f64 = f.samples.iter().map(|s| s.ms).sum();
                    let spent_mc: f64 = f.samples.iter().map(|s| s.mc).sum();
                    if spent_mc <= 0.0 && spent_ms >= COUNTABLE_MS {
                        over.push(format!("{key}: no cycles were counted"));
                    }
                    if peak_mib > TARGET_PEAK_MIB * BOUND_FACTOR {
                        over.push(format!("{key}: {peak_mib:.0} MiB"));
                    }
                    if f.errors > 0 {
                        over.push(format!("{key}: {} requests failed", f.errors));
                    }
                }
                let mut row = format!(
                    "| {}/{} | {os_name} | {} | {median_mc:.3} | {lo_mc:.3}–{hi_mc:.3} | {odd} | {median:.3} | {p95:.3} | {:.3} | {} | {} | {verdict} | {peak_mib:.1} | {al:.0} | {kib:.0} | {} |",
                    terrain.name(),
                    case.name,
                    ms.len(),
                    f.first,
                    judged.map_or("-".to_string(), |g| format!("{}", g.0)),
                    budget.map_or("-".to_string(), |b| format!("{b:.1}")),
                    f.errors
                );
                if baseline.is_some() {
                    match base(&key, "median_mc") {
                        Some(b) => {
                            row += &format!(" {b:.3} | {:.2} |", b / median_mc);
                            if median_mc > 0.0 && b > 0.0 {
                                ratios.push(b / median_mc);
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
                        "median_mc": median_mc,
                        "median_mc_95": [lo_mc, hi_mc],
                        "outlier_rounds": odd,
                        "median_ms": median,
                        "p95_ms": p95,
                        "first_ms": f.first,
                        "round_medians_ms": f.rounds,
                        "round_medians_mc": f.rounds_mc,
                        "target_ms": judged.map(|g| g.0),
                        "budget_mc": budget,
                        "verdict": verdict,
                        "peak_mib": peak_mib,
                        "allocs": al,
                        "alloc_kib": kib,
                        "errors": f.errors,
                    }),
                );
            }
        }
        if install {
            let s = sizes(&cache.dir(terrain.name()));
            let mut m = serde_json::Map::new();
            for (kind, (files, bytes)) in s {
                eprintln!("{}: {kind} {files} files, {bytes} bytes", terrain.name());
                m.insert(kind, json!({"files": files, "bytes": bytes}));
            }
            sizes_out.insert(terrain.name().to_string(), Value::Object(m));
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
        let out = json!({
            "cases": cases_out,
            "sizes": sizes_out,
            "rounds": rounds,
            "cycle_hz": cycle_hz(),
        });
        std::fs::write(&p, serde_json::to_string_pretty(&out).unwrap()).expect("the figures write");
        eprintln!("wrote {p}");
    }
    assert!(over.is_empty(), "over the bounds:\n{}", over.join("\n"));
}
