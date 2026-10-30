//! Opt-in: the FARP and SAM siting examples replayed on Caucasus against
//! a live probe of DCS 2.9.29.27278, the golden oracle, and the same
//! request shapes once on every other terrain as a regression net. Set
//! `DCSTERRAIN_INSTALL` to the DCS install root, and `DCSTERRAIN_TERRAINS`
//! to a comma-separated list of ids to take only those in the net; without
//! the install this passes without reading anything, which is what CI does.
//! Run it with `--release` and `--nocapture` for the figures, and
//! `DCSTERRAIN_THREADS` to cap the threads. The install is only read; the
//! cache is a scratch directory.
//!
//! The tolerances are constants, each with its reason. The probe's
//! candidates lie on a 100 m lattice, so a candidate within one lattice
//! step of its match in each axis is the same one; the two sets are paired
//! by an exact bottleneck assignment, not by rank, so a swap of two
//! near-equal candidates is no failure. The probe's counts may differ by
//! the cells whose value lies at a threshold, or whose class the build
//! since then changed.

use std::path::{Path, PathBuf};
use std::time::Instant;

use serde_json::{Value, json};

use dcsterrain_core::cache::Cache;
use dcsterrain_core::decode::install;
use dcsterrain_core::decode::rn4::KIND_ROAD;
use dcsterrain_core::query::{Terrain, answer};
use dcsterrain_core::source::Ground;
use dcsterrain_core::threads::cpu_cycles;

fn install_root() -> Option<PathBuf> {
    match std::env::var_os("DCSTERRAIN_INSTALL") {
        Some(p) => Some(PathBuf::from(p)),
        None => {
            eprintln!("DCSTERRAIN_INSTALL is not set; skipped");
            None
        }
    }
}

/// The install root with the threads capped and the build reported.
fn setup() -> Option<PathBuf> {
    let root = install_root()?;
    let threads = std::env::var("DCSTERRAIN_THREADS")
        .ok()
        .and_then(|v| v.parse().ok());
    dcsterrain_core::threads::init(threads);
    let build = install::read_build(&root).unwrap();
    eprintln!("DCS {} ({})", build.version, build.timestamp);
    Some(root)
}

/// A fresh directory under the system temp, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir =
            std::env::temp_dir().join(format!("dcsterrain-replay-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn open(root: &Path, id: &str, cache: &Scratch) -> Terrain {
    Terrain::open(root, id, &Cache::new(&cache.0), true).unwrap()
}

/// `f`'s value, wall-clock milliseconds and CPU megacycles.
fn timed<T>(f: impl FnOnce() -> T) -> (T, f64, f64) {
    let (s, c) = (Instant::now(), cpu_cycles());
    let v = f();
    let ms = s.elapsed().as_secs_f64() * 1e3;
    (v, ms, cpu_cycles().saturating_sub(c) as f64 / 1e6)
}

fn call(t: &Terrain, op: &str, q: &Value) -> Value {
    answer(t, op, q).unwrap_or_else(|e| panic!("{op} {q}: {e:?}"))
}

fn xz(v: &Value) -> [f64; 2] {
    [v["x"].as_f64().unwrap(), v["z"].as_f64().unwrap()]
}

/// The probe's lattice step.
const STEP: f64 = 100.0;

/// A candidate within one lattice step of its match in each axis is the
/// same candidate: the same or a neighboring lattice point.
const CELL: f64 = STEP;

/// The probe's eight FARP candidates in its order: `x, z`, then the span
/// and the road distance it measured, in meters.
const FARP_ORACLE: [[f64; 4]; 8] = [
    [-286787.0, 677859.0, 0.01, 233.0],
    [-275487.0, 684259.0, 0.13, 218.0],
    [-287787.0, 684759.0, 0.12, 233.0],
    [-283387.0, 679459.0, 0.15, 211.0],
    [-287487.0, 675859.0, 0.16, 206.0],
    [-294387.0, 676359.0, 0.14, 235.0],
    [-282187.0, 693859.0, 0.17, 222.0],
    [-280487.0, 688559.0, 0.15, 257.0],
];

/// The probe's rejections in the order it tested them, then the cells kept.
const FARP_COUNTS: [(&str, usize); 5] = [
    ("water", 1220),
    ("span over 2.5 m", 3170),
    ("inside 3 km of the airdrome", 2721),
    ("outside 200 m to 3 km of a road", 7205),
    ("kept", 26085),
];

/// Each count within this share of the probe's: the cells whose value
/// lies at a threshold, or whose class the build since changed. A cell
/// counts at the first test it fails, so a stage's count also carries the
/// stages before it.
const COUNT_SHARE: f64 = 0.005;

/// The probe printed spans to the centimeter and road distances to the
/// meter; a remeasured value within twice that is the same.
const SPAN_M: f64 = 0.02;
const ROAD_M: f64 = 2.0;

/// The probe's five SAM candidates, site 1 first.
const SAM_ORACLE: [[f64; 2]; 5] = [
    [-293887.0, 682059.0],
    [-294487.0, 674859.0],
    [-294487.0, 690459.0],
    [-277987.0, 673059.0],
    [-286987.0, 671259.0],
];

/// The probe's first sighting by site 1 of an ingress from due west, in
/// meters from the airdrome, and the tolerance: the probe's rounding to
/// 0.1 km, 50 m either way, and the 100 m step this route is sampled at.
const UNMASK_M: f64 = 7800.0;
const UNMASK_TOL_M: f64 = 150.0;

/// Kutaisi's airdrome id on Caucasus, checked against its `name_id`.
const KUTAISI: i64 = 25;

/// Pairs `a` with `b` so that the farthest pair, by the larger of the axis
/// gaps, is as near as it can be: an exhaustive bottleneck assignment with
/// pruning, exact and fast for the handful of candidates here. Returns that
/// distance and, for each of `a`, its partner in `b`; infinite when `b` has
/// fewer.
fn assign(a: &[[f64; 2]], b: &[[f64; 2]]) -> (f64, Vec<usize>) {
    struct Search<'a> {
        a: &'a [[f64; 2]],
        b: &'a [[f64; 2]],
        used: Vec<bool>,
        cur: Vec<usize>,
        best: (f64, Vec<usize>),
    }
    fn go(s: &mut Search, worst: f64) {
        if worst >= s.best.0 {
            return;
        }
        let k = s.cur.len();
        if k == s.a.len() {
            s.best = (worst, s.cur.clone());
            return;
        }
        for j in 0..s.b.len() {
            if s.used[j] {
                continue;
            }
            let (p, q) = (s.a[k], s.b[j]);
            let d = (p[0] - q[0]).abs().max((p[1] - q[1]).abs());
            s.used[j] = true;
            s.cur.push(j);
            go(s, worst.max(d));
            s.cur.pop();
            s.used[j] = false;
        }
    }
    let mut s = Search {
        a,
        b,
        used: vec![false; b.len()],
        cur: Vec::new(),
        best: (f64::INFINITY, Vec::new()),
    };
    go(&mut s, 0.0);
    s.best
}

/// The probe's FARP request as `find_sites` asks it: a 20 km square about
/// the airdrome every 100 m, 120 by 80 m at 0 and 90 degrees, land, span at
/// most 2.5 m, 200 m to 3 km from a road, 3 km or more from the airdrome;
/// ranked by `span / 2.5 + road / 3000`, which the weights give up to a
/// constant, 2 km apart, eight.
fn farp_request(x: f64, z: f64, airdrome: &Value) -> Value {
    json!({
        "region": {"Box": {"min_x": x - 10_000.0, "min_z": z - 10_000.0,
                           "max_x": x + 10_000.0, "max_z": z + 10_000.0}},
        "footprint": {"width": 120, "depth": 80, "orientations_deg": [0, 90]},
        "criteria": [
            {"name": "surface_in", "params": {"classes": ["land"]}},
            {"name": "span_max", "params": {"m": 2.5}},
            {"name": "dist_road_band", "params": {"min": 200, "max": 3000}},
            {"name": "dist_airdrome_band",
             "params": {"min": 3000, "max": 1.0e6, "airdrome": airdrome}}
        ],
        "step": 100, "spacing": 2000, "limit": 8,
        "weights": {"valid": 0, "surface_in": 0, "dist_airdrome_band": 0,
                    "span_max": 1, "dist_road_band": 2800.0 / 3000.0}
    })
}

/// `n` points `r` from `(x, z)` at bearings `from_deg` onward every
/// `step_deg`, clockwise from north.
fn arc(x: f64, z: f64, r: f64, from_deg: f64, step_deg: f64, n: usize) -> Vec<[f64; 2]> {
    (0..n)
        .map(|k| {
            let b = (from_deg + step_deg * k as f64).to_radians();
            [x + r * b.cos(), z + r * b.sin()]
        })
        .collect()
}

/// The seven western points: 10 km out at bearings 225 to 315 degrees,
/// every 15. The probe named 225 and 270 among them and a target 137 m
/// above sea level at 10 km on 270 from site 1, which is 100 m over the
/// ground there; so the arc is about the site.
fn western(x: f64, z: f64) -> Vec<[f64; 2]> {
    arc(x, z, 10_000.0, 225.0, 15.0, 7)
}

/// The airdrome and 24 points on a 5 km ring about it, every 15 degrees.
fn protected(x: f64, z: f64) -> Vec<[f64; 2]> {
    let mut v = vec![[x, z]];
    v.extend(arc(x, z, 5000.0, 0.0, 15.0, 24));
    v
}

/// The probe's SAM request as `find_sites` asks it: every 300 m from 4 to
/// 15 km of the airdrome, land, a road within 500 m, ranked by the share of
/// the protected points at 100 m above ground a 6 m mast sees, then the
/// share of the western arc that does not see it, 5 km apart, five. The arc
/// here is about the airdrome: a criterion's observers are fixed points.
fn sam_request(x: f64, z: f64, airdrome: &Value) -> Value {
    let pt = |p: &[f64; 2]| json!({"x": p[0], "z": p[1], "alt_agl": 100.0});
    json!({
        "region": {"Box": {"min_x": x - 15_000.0, "min_z": z - 15_000.0,
                           "max_x": x + 15_000.0, "max_z": z + 15_000.0}},
        "footprint": {"width": 30, "depth": 30, "orientations_deg": [0]},
        "criteria": [
            {"name": "surface_in", "params": {"classes": ["land"]}},
            {"name": "dist_road_band", "params": {"min": 0, "max": 500}},
            {"name": "dist_airdrome_band",
             "params": {"min": 4000, "max": 15000, "airdrome": airdrome}},
            {"name": "visible_to", "params": {
                "targets": protected(x, z).iter().map(pt).collect::<Vec<_>>(),
                "min_fraction": 0.0, "own_alt_agl": 6.0}},
            {"name": "not_visible_from", "params": {
                "observers": western(x, z).iter().map(pt).collect::<Vec<_>>(),
                "max_fraction": 1.0, "own_alt_agl": 6.0}}
        ],
        "step": 300, "spacing": 5000, "limit": 5,
        "weights": {"valid": 0, "surface_in": 0, "dist_road_band": 0,
                    "dist_airdrome_band": 0, "visible_to": 1000, "not_visible_from": 1}
    })
}

/// `find_sites` with `q`, timed, and every candidate scored alone at its
/// orientation: it passes, with the same terms.
fn search_and_rescore(t: &Terrain, id: &str, what: &str, q: &Value) -> Vec<[f64; 2]> {
    let (r, ms, mc) = timed(|| call(t, "find_sites", q));
    let cands = r["candidates"].as_array().unwrap();
    for c in cands {
        let mut fp = q["footprint"].clone();
        fp["orientations_deg"] = json!([c["orientation_deg"]]);
        let s = call(
            t,
            "score_site",
            &json!({"x": c["x"], "z": c["z"], "footprint": fp, "criteria": q["criteria"]}),
        );
        assert_eq!(
            s["hard_fail"],
            json!(false),
            "{id} {what}: {c} fails alone: {s}"
        );
        assert_eq!(s["terms"], c["terms"], "{id} {what}: {c}");
    }
    eprintln!(
        "  {id:<20} {what}: {:>6} pass, {} returned, {ms:>6.0} ms, {mc:>7.0} Mcycles",
        r["stats"]["cells_passed"],
        cands.len()
    );
    cands.iter().map(xz).collect()
}

/// Each placed airdrome in the tables' order: its `name_id`, position,
/// and how a request names it.
fn airdromes(t: &Terrain) -> Vec<(String, f64, f64, Value)> {
    let Terrain::Install(i) = t else {
        unreachable!("an install terrain")
    };
    i.tables()
        .unwrap()
        .airdromes
        .iter()
        .filter_map(|a| {
            let name = a.id.map_or_else(|| json!(a.name_id), |id| json!(id));
            Some((a.name_id.clone(), a.x?, a.z?, name))
        })
        .collect()
}

/// Kutaisi's position and how a request names it.
fn kutaisi(t: &Terrain) -> (f64, f64, Value) {
    let (name_id, x, z, name) = airdromes(t)
        .into_iter()
        .find(|a| a.3 == json!(KUTAISI))
        .unwrap();
    assert!(
        name_id.eq_ignore_ascii_case("Kutaisi"),
        "airdrome {KUTAISI} is {name_id}"
    );
    (x, z, name)
}

/// The probe's FARP search recomputed from the mesh's point heights and
/// the road geometry, as the probe asked the game: the cascade of
/// rejections, then the eight best 2 km apart. Then `find_sites` with the
/// same criteria, whose layers are coarser.
#[test]
fn farp_example_on_caucasus() {
    let Some(root) = setup() else { return };
    let cache = Scratch::new("farp");
    let t = open(&root, "Caucasus", &cache);
    let (ax, az, name) = kutaisi(&t);
    let Terrain::Install(i) = &t else {
        unreachable!("an install terrain")
    };
    let roads = i.roads().unwrap();
    let src = t.source();

    // The probe's lattice: every 100 m from 10 km south and west of the
    // airdrome's reference point, 201 by 201.
    let mut cells = Vec::with_capacity(201 * 201);
    for r in 0..=200 {
        for c in 0..=200 {
            cells.push([
                ax - 10_000.0 + f64::from(r) * STEP,
                az - 10_000.0 + f64::from(c) * STEP,
            ]);
        }
    }
    // The center, then the corners of 120 by 80 m at 0 and at 90 degrees.
    // The probe says only that it tested two orientations. Of the spans
    // tried, the best of the two, each alone and the corners without the
    // center, only both orientations together gave its count of steep
    // cells and its eight; the others missed it by 190 to 620 cells and the
    // eight by kilometers.
    let around = |p: [f64; 2]| {
        let mut v = vec![p];
        for (h, w) in [(60.0, 40.0), (40.0, 60.0)] {
            for (sx, sz) in [(-1.0, -1.0), (-1.0, 1.0), (1.0, -1.0), (1.0, 1.0)] {
                v.push([p[0] + sx * h, p[1] + sz * w]);
            }
        }
        v
    };
    // Each kept cell: its position, span, road distance and ranking key.
    type Kept = ([f64; 2], f64, f64, f64);
    let ((counts, mut kept, missing), ms, mc) = timed(|| {
        let pts: Vec<[f64; 2]> = cells.iter().flat_map(|&p| around(p)).collect();
        let ground = src.points(&pts).unwrap();
        let mut counts = [0usize; 5];
        let mut missing = 0;
        let mut kept: Vec<Kept> = Vec::new();
        for (k, &p) in cells.iter().enumerate() {
            let Some(g) = ground[k * 9..k * 9 + 9]
                .iter()
                .map(Option::as_ref)
                .collect::<Option<Vec<&Ground>>>()
            else {
                missing += 1;
                continue;
            };
            if g[0].water != 0 {
                counts[0] += 1;
                continue;
            }
            let span = g.iter().map(|g| g.height).fold(f64::MIN, f64::max)
                - g.iter().map(|g| g.height).fold(f64::MAX, f64::min);
            if span > 2.5 {
                counts[1] += 1;
                continue;
            }
            if (p[0] - ax).hypot(p[1] - az) < 3000.0 {
                counts[2] += 1;
                continue;
            }
            let road = roads
                .nearest(KIND_ROAD, p, 1)
                .first()
                .map_or(f64::INFINITY, |(_, q)| (q[0] - p[0]).hypot(q[1] - p[1]));
            if !(200.0..=3000.0).contains(&road) {
                counts[3] += 1;
                continue;
            }
            counts[4] += 1;
            kept.push((p, span, road, span / 2.5 + road / 3000.0));
        }
        (counts, kept, missing)
    });
    eprintln!(
        "recomputed over {} cells, {} point heights: {ms:.0} ms, {mc:.0} Mcycles",
        cells.len(),
        cells.len() * 9
    );
    for ((what, probe), got) in FARP_COUNTS.iter().zip(counts) {
        eprintln!("  {what:<32} {got:>6}, probe {probe:>6}");
    }
    kept.sort_by(|a, b| {
        a.3.total_cmp(&b.3)
            .then(a.0[0].total_cmp(&b.0[0]))
            .then(a.0[1].total_cmp(&b.0[1]))
    });
    let mut top: Vec<Kept> = Vec::new();
    for c in kept {
        if top.len() == FARP_ORACLE.len() {
            break;
        }
        if top
            .iter()
            .all(|k| (k.0[0] - c.0[0]).hypot(k.0[1] - c.0[1]) >= 2000.0)
        {
            top.push(c);
        }
    }
    let oracle: Vec<[f64; 2]> = FARP_ORACLE.iter().map(|o| [o[0], o[1]]).collect();
    let got: Vec<[f64; 2]> = top.iter().map(|c| c.0).collect();
    let (worst, pairs) = assign(&oracle, &got);
    eprintln!("  the eight paired at worst {worst:.0} m apart in each axis");
    for (o, &j) in FARP_ORACLE.iter().zip(&pairs) {
        let (p, span, road, _) = top[j];
        eprintln!(
            "  ({:.0}, {:.0}) rank {}: span {span:.2} m, probe {:.2}; road {road:.0} m, probe {:.0}",
            p[0],
            p[1],
            j + 1,
            o[2],
            o[3]
        );
    }
    assert_eq!(missing, 0, "cells with a point off the mesh");
    for ((what, probe), got) in FARP_COUNTS.iter().zip(counts) {
        let off = (got as f64 - *probe as f64).abs() / *probe as f64;
        assert!(off <= COUNT_SHARE, "{what}: {got}, probe {probe}");
    }
    assert!(worst <= CELL, "a FARP candidate {worst} m from the probe's");
    for (o, &j) in FARP_ORACLE.iter().zip(&pairs) {
        let (p, span, road, _) = top[j];
        assert!(
            (span - o[2]).abs() <= SPAN_M,
            "span at {p:?}: {span}, probe {}",
            o[2]
        );
        assert!(
            (road - o[3]).abs() <= ROAD_M,
            "road at {p:?}: {road}, probe {}",
            o[3]
        );
    }

    // `find_sites` asks the same, but its span reads heights rounded to the
    // meter on a 50 m raster, one orientation at a time, and its road
    // distance is from cell center to cell center; its eight are reported,
    // not compared.
    let q = farp_request(ax, az, &name);
    let found = search_and_rescore(&t, "Caucasus", "FARP find_sites", &q);
    eprintln!(
        "  find_sites: {} of the probe's eight within a cell",
        within(&oracle, &found)
    );
}

/// How many of `a` have a point of `b` within a cell in each axis.
fn within(a: &[[f64; 2]], b: &[[f64; 2]]) -> usize {
    a.iter()
        .filter(|p| {
            b.iter()
                .any(|q| (p[0] - q[0]).abs().max((p[1] - q[1]).abs()) <= CELL)
        })
        .count()
}

/// The probe's SAM site 1, masked from all seven western points, seeing
/// every protected point, and first seeing an ingress from due west where
/// the probe did. Then `find_sites` with the probe's criteria.
#[test]
fn sam_example_on_caucasus() {
    let Some(root) = setup() else { return };
    let cache = Scratch::new("sam");
    let t = open(&root, "Caucasus", &cache);
    let (ax, az, name) = kutaisi(&t);
    let [sx, sz] = SAM_ORACLE[0];
    let site = json!({"x": sx, "z": sz, "alt_agl": 6.0});
    let (masked, ms, mc) = timed(|| {
        western(sx, sz)
            .iter()
            .map(|p| {
                let b = json!({"x": p[0], "z": p[1], "alt_agl": 100.0});
                call(&t, "visible", &json!({"a": site, "b": b}))
            })
            .collect::<Vec<_>>()
    });
    eprintln!("site 1 to the western arc: {ms:.1} ms, {mc:.1} Mcycles");
    for (k, v) in masked.iter().enumerate() {
        eprintln!(
            "  bearing {:>3}: visible {}, clearance {:.1} m",
            225 + 15 * k,
            v["visible"],
            v["clearance_m"].as_f64().unwrap()
        );
    }
    let seen = protected(ax, az)
        .iter()
        .filter(|p| {
            let b = json!({"x": p[0], "z": p[1], "alt_agl": 100.0});
            call(&t, "visible", &json!({"a": site, "b": b}))["visible"] == json!(true)
        })
        .count();
    eprintln!("  sees {seen} of the 25 protected points; the probe saw all");
    let (u, ms, mc) = timed(|| {
        call(
            &t,
            "unmask_profile",
            &json!({"site": site, "step": 100, "route": [
                {"x": ax, "z": az - 60_000.0, "alt_agl": 100.0},
                {"x": ax, "z": az, "alt_agl": 100.0}
            ]}),
        )
    });
    let from_airdrome = 60_000.0 - u["first_seen_dist_along_route"].as_f64().unwrap();
    eprintln!(
        "  an ingress from due west first seen {from_airdrome:.0} m from the airdrome, \
         probe {UNMASK_M:.0}: {ms:.0} ms, {mc:.0} Mcycles"
    );
    for v in &masked {
        assert_eq!(v["visible"], json!(false), "site 1 seen from the west: {v}");
    }
    assert_eq!(seen, 25, "site 1 sees {seen} of the protected points");
    assert!(
        (from_airdrome - UNMASK_M).abs() <= UNMASK_TOL_M,
        "first seen {from_airdrome} m out, probe {UNMASK_M}"
    );

    // Reported, not compared: a criterion's observers are fixed points, so
    // its western arc is about the airdrome, not about each site.
    let q = sam_request(ax, az, &name);
    let found = search_and_rescore(&t, "Caucasus", "SAM find_sites", &q);
    eprintln!(
        "  find_sites: {} of the probe's five within a cell",
        within(&SAM_ORACLE, &found)
    );
}

/// Both requests once on every other terrain, about its first airdrome
/// with a FARP site: each answers with candidates, and every one passes
/// scored alone.
#[test]
fn the_example_requests_on_every_other_terrain() {
    let Some(root) = setup() else { return };
    let only: Option<Vec<String>> = std::env::var("DCSTERRAIN_TERRAINS")
        .ok()
        .map(|s| s.split(',').map(str::to_string).collect());
    let mut ran = 0;
    for listed in install::terrains(&root).unwrap() {
        let Some(id) = listed.id else { continue };
        if id == "Caucasus" || only.as_ref().is_some_and(|o| !o.contains(&id)) {
            continue;
        }
        let cache = Scratch::new(&id);
        let t = open(&root, &id, &cache);
        let Terrain::Install(i) = &t else {
            unreachable!("an install terrain")
        };
        // The road graph is a cache entry; built first, so the times are
        // the searches'.
        i.roads().unwrap();
        // The first airdrome with a FARP site about it, so that the check
        // is not empty on a terrain whose first is on a small island.
        let mut found = false;
        for (name_id, x, z, name) in airdromes(&t) {
            let what = format!("FARP about {name_id}");
            if search_and_rescore(&t, &id, &what, &farp_request(x, z, &name)).is_empty() {
                continue;
            }
            let what = format!("SAM about {name_id}");
            let sam = search_and_rescore(&t, &id, &what, &sam_request(x, z, &name));
            assert!(!sam.is_empty(), "{id}: no SAM site about {name_id}");
            found = true;
            break;
        }
        assert!(found, "{id}: no airdrome with a FARP site about it");
        ran += 1;
    }
    assert!(
        ran > 0 || only.is_none(),
        "DCSTERRAIN_TERRAINS names no installed terrain but Caucasus"
    );
}
