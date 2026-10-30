//! The plane is accepted and the lake and the hill's flank rejected; the
//! answer equals a brute-force search, and its spacing holds.

use std::sync::OnceLock;

use serde_json::{Value, json};

use super::super::answer;
use super::super::criteria::{PARALLEL_LINES, evaluate};
use super::super::geojson::tests::assert_valid;
use super::super::tests::synth as open_synth;
use super::*;
use crate::synth;

fn synth() -> &'static Terrain {
    static T: OnceLock<Terrain> = OnceLock::new();
    T.get_or_init(open_synth)
}

/// DCS x and z of a point given north and east of the synthetic grid's
/// origin.
fn at(north: f64, east: f64) -> [f64; 2] {
    [
        f64::from(synth::ORIGIN_X_M) + north,
        f64::from(synth::ORIGIN_Z_M) + east,
    ]
}

/// A box `half` meters each way about a point north and east of the origin.
fn around(north: f64, east: f64, half: f64) -> Value {
    let [x, z] = at(north, east);
    json!({"Box": {"min_x": x - half, "min_z": z - half, "max_x": x + half, "max_z": z + half}})
}

fn find(v: Value) -> FindSitesResponse {
    find_sites(synth(), serde_json::from_value(v).unwrap()).unwrap()
}

fn score(x: f64, z: f64, footprint: &Value, criteria: &Value) -> ScoreSiteResponse {
    let q = json!({"x": x, "z": z, "footprint": footprint, "criteria": criteria});
    score_site(synth(), serde_json::from_value(q).unwrap()).unwrap()
}

fn refusal(v: Value) -> Issue {
    find_sites(synth(), serde_json::from_value(v).unwrap()).unwrap_err()[0].clone()
}

/// Every pair of candidates `spacing` or more apart.
fn spaced(c: &[Candidate], spacing: f64) {
    for (i, a) in c.iter().enumerate() {
        for b in &c[i + 1..] {
            let d = (a.x - b.x).hypot(a.z - b.z);
            assert!(d >= spacing, "{d} m between {a:?} and {b:?}");
        }
    }
}

/// Each candidate scored alone gives the same orientation, terms and score.
fn rescored(c: &[Candidate], footprint: &Value, criteria: &Value) {
    for k in c {
        let s = score(k.x, k.z, footprint, criteria);
        assert!(!s.hard_fail, "{k:?}: {s:?}");
        assert_eq!(s.orientation_deg, k.orientation_deg, "{k:?}");
        assert_eq!(s.terms, k.terms, "{k:?}");
        assert!((s.score - k.score).abs() < 1e-9, "{k:?}: {s:?}");
    }
}

/// Points scattered by a fixed linear congruential generator.
fn scatter(n: usize, side: f64) -> Vec<[f64; 2]> {
    let mut s: u64 = 0x2545_f491_4f6c_dd1d;
    let mut next = || {
        s = s.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        (s >> 11) as f64 / (1u64 << 53) as f64
    };
    (0..n)
        .map(|_| [next() * side - side / 2.0, next() * side])
        .collect()
}

/// Greedy suppression the obvious way: each against every one kept.
fn suppress_brute(ranked: &[[f64; 2]], spacing: f64, limit: usize) -> Vec<usize> {
    let mut kept: Vec<usize> = Vec::new();
    for (i, p) in ranked.iter().enumerate() {
        if kept.len() == limit {
            break;
        }
        if kept
            .iter()
            .all(|&k| (ranked[k][0] - p[0]).hypot(ranked[k][1] - p[1]) >= spacing)
        {
            kept.push(i);
        }
    }
    kept
}

#[test]
fn suppression_equals_brute_force() {
    let mut pts = scatter(3000, 20_000.0);
    // Points on a lattice, exactly a spacing apart, and repeated.
    for i in 0..20 {
        pts.push([f64::from(i) * 250.0, 0.0]);
        pts.push([f64::from(i) * 250.0, 0.0]);
    }
    for spacing in [0.0, 1.0, 37.5, 250.0, 1_000.0, 1e9] {
        for limit in [1, 7, 200, usize::MAX] {
            assert_eq!(
                suppress(&pts, spacing, limit),
                suppress_brute(&pts, spacing, limit),
                "spacing {spacing}, limit {limit}"
            );
        }
    }
}

const SQUARE: f64 = 100.0;

fn square() -> Value {
    json!({"width": SQUARE, "depth": SQUARE, "orientations_deg": [0, 90]})
}

#[test]
fn the_plane_is_accepted_and_the_lake_rejected() {
    let criteria = json!([
        {"name": "slope_max", "params": {"deg": 3}},
        {"name": "surface_in", "params": {"classes": ["land"]}}
    ]);
    let spacing = 500.0;
    let r = find(json!({
        "region": around(synth::LAKE_NORTH_M, synth::LAKE_EAST_M, 4000.0),
        "footprint": square(), "criteria": criteria,
        "spacing": spacing, "limit": 200
    }));
    assert!(r.candidates.len() > 20, "{}", r.candidates.len());
    let [lx, lz] = at(synth::LAKE_NORTH_M, synth::LAKE_EAST_M);
    for c in &r.candidates {
        let d = (c.x - lx).hypot(c.z - lz);
        assert!(
            d > synth::LAKE_RADIUS_M,
            "{c:?} is {d} m from the lake's center"
        );
    }
    spaced(&r.candidates, spacing);
    rescored(&r.candidates, &square(), &criteria);
    // Some candidates lie in each quarter about the lake: the plane is
    // accepted all around it.
    for (sx, sz) in [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
        assert!(
            r.candidates
                .iter()
                .any(|c| (c.x - lx) * sx > 0.0 && (c.z - lz) * sz > 0.0),
            "none in quarter {sx} {sz}"
        );
    }
    assert!(r.stats.cells_passed < r.stats.cells_considered);
    let s = score(lx, lz, &square(), &criteria);
    assert!(s.hard_fail, "{s:?}");
    assert!(s.terms.iter().any(|t| t.name == "surface_in" && !t.pass));
}

/// The closed form's steepest slope in degrees over a footprint about
/// `p`, the footprint sampled every 10 m.
fn steepest(p: [f64; 2], half: f64) -> f64 {
    let ground = |north: f64, east: f64| {
        let hill = |h: f64, n0: f64, e0: f64| {
            let d2 = (north - n0).powi(2) + (east - e0).powi(2);
            h * (-d2 / (2.0 * synth::HILL_SIGMA_M.powi(2))).exp()
        };
        synth::PLANE_SLOPE * north
            + hill(
                synth::HILL_A_HEIGHT_M,
                synth::HILL_A_NORTH_M,
                synth::HILL_A_EAST_M,
            )
            + hill(
                synth::HILL_B_HEIGHT_M,
                synth::HILL_B_NORTH_M,
                synth::HILL_B_EAST_M,
            )
    };
    let n0 = p[0] - f64::from(synth::ORIGIN_X_M);
    let e0 = p[1] - f64::from(synth::ORIGIN_Z_M);
    let mut worst = 0.0f64;
    let k = (half / 10.0) as i32;
    for i in -k..=k {
        for j in -k..=k {
            let (n, e) = (n0 + f64::from(i) * 10.0, e0 + f64::from(j) * 10.0);
            let gn = (ground(n + 1.0, e) - ground(n - 1.0, e)) / 2.0;
            let ge = (ground(n, e + 1.0) - ground(n, e - 1.0)) / 2.0;
            worst = worst.max(gn.hypot(ge).atan().to_degrees());
        }
    }
    worst
}

#[test]
fn the_hill_flank_is_rejected() {
    let criteria = json!([{"name": "slope_max", "params": {"deg": 3}}]);
    let r = find(json!({
        "region": around(synth::HILL_A_NORTH_M, synth::HILL_A_EAST_M, 5000.0),
        "footprint": square(), "criteria": criteria,
        "spacing": 300, "limit": 200
    }));
    assert!(!r.candidates.is_empty());
    let [hx, hz] = at(synth::HILL_A_NORTH_M, synth::HILL_A_EAST_M);
    for c in &r.candidates {
        // The raster's slope is a 50 m cell's; the closed form's is the
        // point's, so allow it half a degree.
        let s = steepest([c.x, c.z], SQUARE / 2.0);
        assert!(s <= 3.5, "{c:?} stands on {s}°");
        // Between 400 m and 2.8 km from the top the hill is steeper than
        // 3° everywhere, the other hill and the plane included.
        let d = (c.x - hx).hypot(c.z - hz);
        assert!(!(400.0..2800.0).contains(&d), "{c:?} is {d} m from the top");
    }
    spaced(&r.candidates, 300.0);
    rescored(&r.candidates, &square(), &criteria);
    let flank = at(
        synth::HILL_A_NORTH_M + synth::HILL_SIGMA_M,
        synth::HILL_A_EAST_M,
    );
    assert!(score(flank[0], flank[1], &square(), &criteria).hard_fail);
}

/// The weighted sum of each orientation of each position, the best kept,
/// sorted and suppressed, written out without the code under test.
fn brute(q: &FindSitesRequest) -> Vec<Candidate> {
    let src = synth().source();
    let area = resolve(src, &q.region, "region").unwrap();
    let cands = lattice(&area, q.step.unwrap(), u32::MAX).unwrap();
    let e = evaluate(src, &cands, &q.footprint, &q.criteria, Mode::Every).unwrap();
    let w = |t: &Term| q.weights.get(&t.name).copied().unwrap_or(1.0);
    let mut best: Vec<Candidate> = Vec::new();
    let n = q.footprint.orientations_deg.len();
    for sites in e.sites.chunks(n) {
        let mut top: Option<Candidate> = None;
        for s in sites.iter().filter(|s| s.pass) {
            let score: f64 = s.terms.iter().map(|t| w(t) * t.score).sum();
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
            .partial_cmp(&a.score)
            .unwrap()
            .then(a.x.partial_cmp(&b.x).unwrap())
            .then(a.z.partial_cmp(&b.z).unwrap())
    });
    let at: Vec<[f64; 2]> = best.iter().map(|c| [c.x, c.z]).collect();
    suppress_brute(&at, q.spacing, q.limit as usize)
        .into_iter()
        .map(|k| best[k].clone())
        .collect()
}

#[test]
fn find_sites_equals_a_brute_force_search() {
    // Across the north–south road, the river and the plane beside them,
    // with three orientations and uneven weights.
    let q = json!({
        "region": around(48_000.0, synth::ROAD_NS_EAST_M, 3000.0),
        "footprint": {"width": 120, "depth": 40, "orientations_deg": [0, 45, 90]},
        "criteria": [
            {"name": "slope_max", "params": {"deg": 6}},
            {"name": "surface_in", "params": {"classes": ["land"]}},
            {"name": "dist_road_band", "params": {"min": 0, "max": 1500}},
            {"name": "span_max", "params": {"m": 8}}
        ],
        "step": 150, "spacing": 400, "limit": 60,
        "weights": {"slope_max": 2.0, "dist_road_band": 0.5, "valid": 0}
    });
    let req: FindSitesRequest = serde_json::from_value(q.clone()).unwrap();
    let r = find(q);
    assert!(r.candidates.len() > 10, "{}", r.candidates.len());
    assert_eq!(r.candidates, brute(&req));
    spaced(&r.candidates, 400.0);
    // A limit cuts the same ranking short.
    let mut short = req.clone();
    short.limit = 5;
    let s = find_sites(synth(), short.clone()).unwrap();
    assert_eq!(s.candidates, r.candidates[..5]);
}

#[test]
fn ties_rank_by_x_then_z() {
    // On the plane every site passes and scores the same.
    let q = json!({
        "region": around(30_000.0, 40_000.0, 1000.0),
        "footprint": {"width": 20, "depth": 20},
        "criteria": [{"name": "surface_in", "params": {"classes": ["land"]}}],
        "step": 100, "spacing": 1000, "limit": 200
    });
    let r = find(q.clone());
    let req: FindSitesRequest = serde_json::from_value(q.clone()).unwrap();
    // The lattice runs x, then z: suppressed in that order.
    let area = resolve(synth().source(), &req.region, "region").unwrap();
    let all = lattice(&area, 100.0, u32::MAX).unwrap();
    let want: Vec<[f64; 2]> = suppress_brute(&all, 1000.0, 200)
        .into_iter()
        .map(|k| all[k])
        .collect();
    let got: Vec<[f64; 2]> = r.candidates.iter().map(|c| [c.x, c.z]).collect();
    assert_eq!(got[0], at(29_000.0, 39_000.0));
    assert_eq!(got, want);
    assert_eq!(find(q), r);
}

#[test]
fn a_site_keeps_its_best_orientation() {
    // A long footprint 150 m west of the river, across it or along it.
    let [x, z] = at(20_000.0, synth::RIVER_EAST_M - 150.0);
    // Wider than a cell, so it holds cells besides its own.
    let footprint = json!({"width": 60, "depth": 400, "orientations_deg": [90, 0]});
    let criteria = json!([{"name": "dist_water_min", "params": {"m": 50}}]);
    let s = score(x, z, &footprint, &criteria);
    // The water term reads the site's own cell, so both tie and the first
    // given is kept.
    assert_eq!(s.orientation_deg, 90.0);
    // Across the river, the footprint reaches into it; along it, it stays
    // on land.
    let criteria = json!([{"name": "surface_in", "params": {"classes": ["land"]}}]);
    let s = score(x, z, &footprint, &criteria);
    assert_eq!(s.orientation_deg, 0.0, "{s:?}");
    assert!(!s.hard_fail);
    let along = s.terms.iter().find(|t| t.name == "surface_in").unwrap();
    assert_eq!(along.score, 1.0);
}

#[test]
fn refuses_what_it_cannot_answer() {
    let base = json!({
        "region": around(30_000.0, 40_000.0, 1000.0),
        "footprint": {"width": 20, "depth": 20},
        "criteria": [{"name": "slope_max", "params": {"deg": 3}}],
        "spacing": 100
    });
    let with = |k: &str, v: Value| {
        let mut q = base.clone();
        q[k] = v;
        refusal(q)
    };
    assert_eq!(with("limit", json!(0)).path, "limit");
    assert_eq!(with("limit", json!(201)).path, "limit");
    assert_eq!(with("step", json!(49)).path, "step");
    assert_eq!(with("spacing", json!(-1)).path, "spacing");
    assert_eq!(with("weights", json!({"slope": 1})).path, "weights.slope");
    assert_eq!(
        with("weights", json!({"slope_max": -1})).path,
        "weights.slope_max"
    );
    assert_eq!(with("weights", json!({"valid": -1})).path, "weights.valid");
    assert_eq!(
        with("weights", json!({"slope_max": 1001})).path,
        "weights.slope_max"
    );
    let big = with("region", around(30_000.0, 40_000.0, 60_000.0));
    assert_eq!(
        (big.code.as_str(), big.path.as_str()),
        ("too_large", "region")
    );
}

#[test]
fn answers_as_geojson_through_the_operation_list() {
    let v = answer(
        synth(),
        "find_sites",
        &json!({
            "region": around(30_000.0, 40_000.0, 1000.0),
            "footprint": {"width": 60, "depth": 30, "orientations_deg": [30]},
            "criteria": [{"name": "slope_max", "params": {"deg": 3}}],
            "spacing": 500, "geojson": true
        }),
    )
    .unwrap();
    let g = &v["geojson"];
    assert_valid(g);
    let c = v["candidates"].as_array().unwrap();
    let f = g["features"].as_array().unwrap();
    assert_eq!(f.len(), 2 * c.len());
    for (n, pair) in f.chunks(2).enumerate() {
        assert_eq!(pair[0]["geometry"]["type"], "Point");
        assert_eq!(pair[1]["geometry"]["type"], "Polygon");
        for p in pair {
            assert_eq!(p["properties"]["rank"], json!(n + 1));
            assert_eq!(p["properties"]["score"], c[n]["score"]);
        }
    }
    let v = answer(
        synth(),
        "score_site",
        &json!({
            "x": c[0]["x"], "z": c[0]["z"],
            "footprint": {"width": 60, "depth": 30, "orientations_deg": [30]},
            "criteria": [{"name": "slope_max", "params": {"deg": 3}}],
            "geojson": true
        }),
    )
    .unwrap();
    assert_valid(&v["geojson"]);
    assert_eq!(v["geojson"]["features"].as_array().unwrap().len(), 2);
    assert_eq!(v["score"], c[0]["score"]);
}

#[test]
fn the_best_orientation_passes_then_scores_most_by_the_weights() {
    let t = |name: &str, score: f64| Term {
        name: name.to_string(),
        value: score,
        pass: true,
        score,
    };
    let site = |deg: f64, a: f64, b: f64, pass: bool| Site {
        x: 0.0,
        z: 0.0,
        orientation_deg: deg,
        terms: vec![t("a", a), t("b", b)],
        pass,
    };
    let sites = [
        site(0.0, 1.0, 1.0, false),
        site(45.0, 0.9, 0.2, true),
        site(90.0, 0.3, 0.7, true),
        site(135.0, 0.3, 0.7, true),
    ];
    let w = |a: f64, b: f64| BTreeMap::from([("a".to_string(), a), ("b".to_string(), b)]);
    // Unweighted, 45° sums 1.1 against 1.0; the failing 0° never wins.
    assert_eq!(best(&sites, &BTreeMap::new()).0.orientation_deg, 45.0);
    // Weighting b three times turns it to 90°, ahead of 135° on the tie.
    let (s, v) = best(&sites, &w(1.0, 3.0));
    assert_eq!(s.orientation_deg, 90.0);
    assert!((v - 2.4).abs() < 1e-12, "{v}");
    // Only failing sites: the best of them.
    assert_eq!(best(&sites[..1], &w(1.0, 1.0)).0.orientation_deg, 0.0);
}

/// The answer with the pool at `threads`, whose own one thread takes the
/// same order as the calling thread would.
fn on_threads(threads: usize, q: &Value) -> FindSitesResponse {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .unwrap();
    pool.install(|| find(q.clone()))
}

#[test]
fn sightlines_answer_the_same_on_any_number_of_threads() {
    let [ox, oz] = at(synth::HILL_A_NORTH_M, synth::HILL_A_EAST_M - 4000.0);
    let observers: Vec<Value> = (0..6)
        .map(|k| json!({"x": ox - 500.0 * f64::from(k), "z": oz, "alt_agl": 2}))
        .collect();
    // Across the hill sites score apart; on the plane every one sees the
    // same and they tie, so the order of the ties shows too.
    let hill = around(synth::HILL_A_NORTH_M, synth::HILL_A_EAST_M, 1500.0);
    let plane = around(30_000.0, 40_000.0, 1000.0);
    let mut tied = false;
    for region in [hill, plane] {
        let q = json!({
            "region": region,
            "footprint": {"width": 20, "depth": 20, "orientations_deg": [0, 90]},
            "criteria": [
                {"name": "not_visible_from",
                 "params": {"observers": observers, "max_fraction": 0.5}},
                {"name": "visible_to",
                 "params": {"targets": [{"x": ox, "z": oz + 9000.0, "alt_agl": 10}],
                            "min_fraction": 0}},
            ],
            "step": 100, "spacing": 0, "limit": 200
        });
        let one = on_threads(1, &q);
        // No test but `valid` comes before the lines and the region is all
        // land, so every position draws at least its observers' lines.
        let lines = one.stats.cells_considered as usize * observers.len();
        assert!(lines >= PARALLEL_LINES, "{lines} lines stay on one thread");
        assert!(!one.candidates.is_empty());
        // Under the line count each site alone stays on the calling thread.
        rescored(&one.candidates, &q["footprint"], &q["criteria"]);
        let mut pairs = one.candidates.windows(2);
        tied |= pairs.any(|w| w[0].score == w[1].score);
        for threads in [2, 3, 8] {
            // Every bit: the debug form keeps a float's shortest exact digits.
            let many = on_threads(threads, &q);
            assert_eq!(format!("{many:?}"), format!("{one:?}"));
        }
    }
    assert!(tied, "no two sites tied");
}
