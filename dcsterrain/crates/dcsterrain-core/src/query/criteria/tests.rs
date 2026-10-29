//! Each criterion passes where the synthetic terrain's closed form says it
//! should and fails where it says it should not; pruning changes no answer.

use std::sync::OnceLock;

use serde_json::json;

use super::*;
use crate::synth::{self, Synth};
use crate::types::Footprint;

fn synth() -> &'static Synth {
    static S: OnceLock<Synth> = OnceLock::new();
    S.get_or_init(|| {
        Synth::new(
            synth::DEFAULT_SEED,
            synth::DEFAULT_SIZE_KM,
            synth::CELL_SIZE_M,
        )
        .unwrap()
    })
}

/// DCS x and z of a point given north and east of the synthetic grid's
/// origin.
fn at(north: f64, east: f64) -> [f64; 2] {
    [
        f64::from(synth::ORIGIN_X_M) + north,
        f64::from(synth::ORIGIN_Z_M) + east,
    ]
}

fn square(side: f64) -> Footprint {
    Footprint {
        width: side,
        depth: side,
        orientations_deg: vec![0.0],
    }
}

fn criterion(v: serde_json::Value) -> Criterion {
    serde_json::from_value(v).unwrap()
}

/// The term of `c` at the site, a `side` meter square at north.
fn term(p: [f64; 2], side: f64, c: &Criterion) -> Term {
    let e = evaluate(
        synth(),
        &[p],
        &square(side),
        std::slice::from_ref(c),
        Mode::Every,
    )
    .unwrap();
    let site = &e.sites[0];
    site.terms
        .iter()
        .find(|t| t.name == name(c))
        .unwrap()
        .clone()
}

/// `c` passes at `yes` and fails at `no`, each a `side` meter square.
fn passes_and_fails(c: serde_json::Value, side: f64, yes: [f64; 2], no: [f64; 2]) -> (Term, Term) {
    let c = criterion(c);
    let (y, n) = (term(yes, side, &c), term(no, side, &c));
    assert!(y.pass, "{} should pass at {yes:?}: {y:?}", name(&c));
    assert!(!n.pass, "{} should fail at {no:?}: {n:?}", name(&c));
    for t in [&y, &n] {
        assert!((0.0..=1.0).contains(&t.score), "{t:?}");
    }
    (y, n)
}

// Places on the synthetic terrain, north and east of its origin.
const PLAIN: (f64, f64) = (30_000.0, 40_000.0);
fn plain() -> [f64; 2] {
    at(PLAIN.0, PLAIN.1)
}
fn hill_top() -> [f64; 2] {
    at(synth::HILL_A_NORTH_M, synth::HILL_A_EAST_M)
}
/// One sigma north of the big hill's top, where its flank is steepest.
fn hill_flank() -> [f64; 2] {
    at(
        synth::HILL_A_NORTH_M + synth::HILL_SIGMA_M,
        synth::HILL_A_EAST_M,
    )
}

#[test]
fn valid_fails_on_the_sea_and_off_the_mesh() {
    passes_and_fails(
        json!({"name": "valid"}),
        20.0,
        plain(),
        at(20_000.0, 10_000.0),
    );
    passes_and_fails(json!({"name": "valid"}), 20.0, plain(), at(500.0, 40_000.0));
}

#[test]
fn slope_max_passes_the_plane_and_fails_the_hill_flank() {
    let (y, n) = passes_and_fails(
        json!({"name": "slope_max", "params": {"deg": 3}}),
        100.0,
        plain(),
        hill_flank(),
    );
    // A 1 % plane is 0.57°; the flank of a 600 m hill of sigma 1.5 km is
    // atan(0.4 / sqrt(e)) at its steepest, less the plane rising against it:
    // 13.1°.
    assert!((y.value - 0.01f64.atan().to_degrees()).abs() < 0.1, "{y:?}");
    let flank = (0.4 / 0.5f64.exp() - 0.01).atan().to_degrees();
    assert!((n.value - flank).abs() < 0.3, "{n:?}");
}

#[test]
fn span_max_passes_the_plane_and_fails_the_hill_flank() {
    let (y, _) = passes_and_fails(
        json!({"name": "span_max", "params": {"m": 5}}),
        100.0,
        plain(),
        hill_flank(),
    );
    // The plane rises 1 m over 100 m north.
    assert!((y.value - 1.0).abs() < 0.6, "{y:?}");
}

#[test]
fn surface_in_fails_a_footprint_over_the_river() {
    passes_and_fails(
        json!({"name": "surface_in", "params": {"classes": ["land"]}}),
        100.0,
        plain(),
        at(30_000.0, synth::RIVER_EAST_M + 40.0),
    );
}

#[test]
fn surface_enum_in_fails_a_footprint_on_the_road() {
    passes_and_fails(
        json!({"name": "surface_enum_in", "params": {"values": [1]}}),
        60.0,
        plain(),
        at(synth::ROAD_EW_NORTH_M + 20.0, 20_000.0),
    );
}

#[test]
fn cover_in_passes_on_the_field_and_fails_off_it() {
    let field = at(21_500.0, 61_500.0);
    passes_and_fails(
        json!({"name": "cover_in", "params": {"classes": ["field"]}}),
        200.0,
        field,
        plain(),
    );
    // A footprint across the field's edge has a cell off it.
    let edge = at(synth::FIELD_NORTH_M, 61_500.0);
    let c = criterion(json!({"name": "cover_in", "params": {"classes": ["field"]}}));
    let t = term(edge, 200.0, &c);
    assert!(!t.pass && t.value > 0.3 && t.value < 0.7, "{t:?}");
}

#[test]
fn cover_fraction_min_passes_in_the_forest() {
    let (y, n) = passes_and_fails(
        json!({"name": "cover_fraction_min",
               "params": {"class": "forest", "radius": 300, "min": 0.9}}),
        20.0,
        at(40_500.0, 55_500.0),
        plain(),
    );
    assert_eq!(y.value, 1.0);
    assert_eq!(n.value, 0.0);
}

#[test]
fn dist_cover_band_passes_beside_the_forest() {
    let (y, n) = passes_and_fails(
        json!({"name": "dist_cover_band",
               "params": {"class": "forest", "min": 50, "max": 300}}),
        20.0,
        at(synth::FOREST_SOUTH_M - 175.0, 57_000.0),
        plain(),
    );
    // Cell centre to cell centre: 150 or 200 m.
    assert!((150.0..=200.0).contains(&y.value), "{y:?}");
    assert!(n.value > 300.0, "{n:?}");
}

#[test]
fn dist_road_and_rail_bands_pass_beside_each() {
    passes_and_fails(
        json!({"name": "dist_road_band", "params": {"min": 0, "max": 200}}),
        20.0,
        at(synth::ROAD_EW_NORTH_M + 100.0, 20_000.0),
        plain(),
    );
    let rail = synth::ROAD_EW_NORTH_M - synth::RAIL_SOUTH_OFFSET_M;
    passes_and_fails(
        json!({"name": "dist_rail_band", "params": {"min": 0, "max": 200}}),
        20.0,
        at(rail - 100.0, 20_000.0),
        plain(),
    );
    // Too near fails as too far does.
    passes_and_fails(
        json!({"name": "dist_road_band", "params": {"min": 500, "max": 2000}}),
        20.0,
        at(synth::ROAD_EW_NORTH_M + 1000.0, 20_000.0),
        at(synth::ROAD_EW_NORTH_M + 100.0, 20_000.0),
    );
}

#[test]
fn dist_water_min_fails_beside_the_river() {
    let (y, _) = passes_and_fails(
        json!({"name": "dist_water_min", "params": {"m": 1000}}),
        20.0,
        plain(),
        at(30_000.0, synth::RIVER_EAST_M - 300.0),
    );
    // The river is 5 km east of the plain's point, past twice the minimum.
    assert_eq!(y.score, 1.0);
}

#[test]
fn dist_airdrome_band_passes_near_the_runway() {
    let runway = (synth::RUNWAY_NORTH_M, synth::RUNWAY_EAST_M);
    let (y, _) = passes_and_fails(
        json!({"name": "dist_airdrome_band", "params": {"min": 0, "max": 3000}}),
        20.0,
        at(runway.0 + 1000.0, runway.1),
        plain(),
    );
    assert!((y.value - 1000.0).abs() < 1e-6, "{y:?}");
    let c = criterion(json!({"name": "dist_airdrome_band",
                             "params": {"min": 0, "max": 3000, "airdrome_id": 99}}));
    let e = evaluate(synth(), &[plain()], &square(20.0), &[c], Mode::Every).unwrap_err();
    assert_eq!(e[0].code, "unknown_airdrome");
}

#[test]
fn scenery_max_counts_the_town_buildings() {
    let town = at(synth::SCENERY_TOWN_NORTH_M, synth::SCENERY_TOWN_EAST_M);
    let (_, n) = passes_and_fails(
        json!({"name": "scenery_max",
               "params": {"class": "building", "radius": 1500, "max": 0}}),
        20.0,
        plain(),
        town,
    );
    assert_eq!(n.value, f64::from(synth::SCENERY_TOWN_COUNT));
    let c = criterion(json!({"name": "scenery_max",
                             "params": {"class": "effect", "radius": 100, "max": 0}}));
    let e = evaluate(synth(), &[plain()], &square(20.0), &[c], Mode::Every).unwrap_err();
    assert_eq!(e[0].code, "unknown_class");
}

#[test]
fn scenery_in_footprint_max_fails_over_the_industrial_pair() {
    let (_, n) = passes_and_fails(
        json!({"name": "scenery_in_footprint_max", "params": {"max": 0}}),
        300.0,
        plain(),
        at(
            synth::SCENERY_INDUSTRIAL_NORTH_M,
            synth::SCENERY_INDUSTRIAL_EAST_M,
        ),
    );
    assert_eq!(n.value, 2.0);
}

#[test]
fn clearing_max_fails_beside_a_pole() {
    let pole = at(
        synth::ROAD_EW_NORTH_M + synth::SCENERY_POLE_NORTH_OFFSET_M,
        synth::SCENERY_POLE_FIRST_EAST_M + 5.0 * synth::SCENERY_POLE_SPACING_M,
    );
    let (_, n) = passes_and_fails(
        json!({"name": "clearing_max", "params": {"max": 0}}),
        20.0,
        plain(),
        [pole[0] + 100.0, pole[1]],
    );
    assert_eq!(n.value, 1.0);
}

#[test]
fn tpi_class_in_finds_the_ridge_on_the_hill_top() {
    passes_and_fails(
        json!({"name": "tpi_class_in", "params": {"classes": ["ridge"], "threshold_m": 3}}),
        20.0,
        hill_top(),
        plain(),
    );
    passes_and_fails(
        json!({"name": "tpi_class_in", "params": {"classes": ["flat"]}}),
        20.0,
        plain(),
        hill_top(),
    );
}

/// East and west of the big hill, on its east-west line.
fn east_of_hill() -> [f64; 2] {
    at(synth::HILL_A_NORTH_M, synth::HILL_A_EAST_M + 2000.0)
}
fn west_of_hill() -> [f64; 2] {
    at(synth::HILL_A_NORTH_M, synth::HILL_A_EAST_M - 4000.0)
}

#[test]
fn horizon_criteria_see_the_big_hill() {
    // Within 2 km east of the hill the sector to the west rises 8 to 9°;
    // 4 km west of it the sector to the west is the plane.
    passes_and_fails(
        json!({"name": "masked_from",
               "params": {"bearing_deg": 270, "half_width_deg": 20, "min_elev_deg": 5}}),
        20.0,
        east_of_hill(),
        west_of_hill(),
    );
    // Everything stands below the hill's top, and a site beside the hill
    // has it above.
    passes_and_fails(
        json!({"name": "enclosure_min", "params": {"deg": 0}}),
        20.0,
        east_of_hill(),
        hill_top(),
    );
}

#[test]
fn sightline_criteria_find_the_hill_between() {
    let [ox, oz] = west_of_hill();
    let near = at(synth::HILL_A_NORTH_M, synth::HILL_A_EAST_M - 6000.0);
    let (y, n) = passes_and_fails(
        json!({"name": "not_visible_from",
               "params": {"observers": [{"x": ox, "z": oz, "alt_agl": 2}], "max_fraction": 0}}),
        20.0,
        east_of_hill(),
        near,
    );
    assert_eq!((y.value, n.value), (0.0, 1.0));
    passes_and_fails(
        json!({"name": "visible_to",
               "params": {"targets": [{"x": ox, "z": oz, "alt_agl": 2}], "min_fraction": 1}}),
        20.0,
        near,
        east_of_hill(),
    );
}

#[test]
fn a_bad_param_is_named_by_its_path() {
    let c = criterion(json!({"name": "dist_road_band", "params": {"min": 500, "max": 100}}));
    let e = evaluate(
        synth(),
        &[plain()],
        &square(20.0),
        &[Criterion::Valid, c],
        Mode::Every,
    )
    .unwrap_err();
    assert_eq!(e[0].path, "criteria.1.params.min");
}

#[test]
fn valid_is_applied_unasked_and_once() {
    let c = criterion(json!({"name": "slope_max", "params": {"deg": 3}}));
    let e = evaluate(
        synth(),
        &[plain()],
        &square(20.0),
        &[c.clone()],
        Mode::Every,
    )
    .unwrap();
    let names: Vec<_> = e.sites[0].terms.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["valid", "slope_max"]);
    let e = evaluate(
        synth(),
        &[plain()],
        &square(20.0),
        &[c, Criterion::Valid],
        Mode::Every,
    )
    .unwrap();
    let names: Vec<_> = e.sites[0].terms.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["slope_max", "valid"]);
}

/// A lattice `step` meters apart over the middle of the synthetic terrain:
/// the coast, the river, the roads, the hills, the town, the forest and the
/// field.
fn middle_lattice(step: f64) -> Vec<[f64; 2]> {
    let [x0, z0] = at(18_000.0, 18_000.0);
    let [x1, z1] = at(60_000.0, 62_000.0);
    lattice([x0, z0, x1, z1], step, |_, _| true)
}

fn criteria(v: serde_json::Value) -> Vec<Criterion> {
    serde_json::from_value(v).unwrap()
}

/// The pruned evaluation returns exactly the sites the exhaustive one
/// passes, with the same terms, and skips `skipped` tiles or more.
fn assert_pruning_changes_nothing(
    cands: &[[f64; 2]],
    footprint: &Footprint,
    cs: &[Criterion],
    skipped: u64,
) -> Evaluation {
    let every = evaluate(synth(), cands, footprint, cs, Mode::Every).unwrap();
    let pruned = evaluate(synth(), cands, footprint, cs, Mode::Prune).unwrap();
    let passed: Vec<&Site> = every.sites.iter().filter(|s| s.pass).collect();
    let missing: Vec<_> = passed
        .iter()
        .filter(|e| !pruned.sites.contains(e))
        .map(|e| (e.x, e.z, e.orientation_deg))
        .collect();
    assert!(missing.is_empty(), "pruning lost {missing:?}");
    assert_eq!(pruned.sites.len(), passed.len());
    for (p, e) in pruned.sites.iter().zip(&passed) {
        assert_eq!(p, *e);
    }
    assert_eq!(
        pruned.stats.candidates_passed,
        every.stats.candidates_passed
    );
    assert_eq!(every.stats.tiles_skipped, 0);
    assert!(pruned.stats.tiles_skipped >= skipped, "{:?}", pruned.stats);
    pruned
}

#[test]
fn pruning_changes_no_answer() {
    let cands = middle_lattice(1000.0);
    let footprint = Footprint {
        width: 100.0,
        depth: 60.0,
        orientations_deg: vec![0.0, 45.0],
    };
    let all = criteria(json!([
        {"name": "slope_max", "params": {"deg": 2}},
        {"name": "span_max", "params": {"m": 3}},
        {"name": "surface_in", "params": {"classes": ["land"]}},
        {"name": "surface_enum_in", "params": {"values": [1, 4]}},
        {"name": "cover_in", "params": {"classes": ["plain", "field", "forest"]}},
        {"name": "cover_fraction_min", "params": {"class": "plain", "radius": 300, "min": 0.5}},
        {"name": "dist_cover_band", "params": {"class": "forest", "min": 0, "max": 20000}},
        {"name": "dist_road_band", "params": {"min": 0, "max": 8000}},
        {"name": "dist_rail_band", "params": {"min": 0, "max": 20000}},
        {"name": "dist_water_min", "params": {"m": 500}},
        {"name": "dist_airdrome_band", "params": {"min": 2000, "max": 30000}},
        {"name": "scenery_max", "params": {"class": "building", "radius": 1000, "max": 3}},
        {"name": "scenery_in_footprint_max", "params": {"max": 0}},
        {"name": "clearing_max", "params": {"radius": 150, "max": 5}},
        {"name": "tpi_class_in", "params": {"classes": ["flat", "mid"]}}
    ]));
    let e = assert_pruning_changes_nothing(&cands, &footprint, &all, 1);
    assert!(!e.sites.is_empty(), "something on the plane passes");
    // Each alone, so each one's zone map is what skips, on a coarser
    // lattice.
    let coarse = middle_lattice(2000.0);
    for c in &all {
        assert_pruning_changes_nothing(&coarse, &footprint, std::slice::from_ref(c), 0);
    }
    // The field, 3 km square, lies on four tiles at most.
    let field = criteria(json!([{"name": "cover_in", "params": {"classes": ["field"]}}]));
    let e = assert_pruning_changes_nothing(&cands, &footprint, &field, 1);
    assert!(e.stats.tiles_skipped >= e.stats.tiles - 4, "{:?}", e.stats);
}

#[test]
fn pruning_changes_no_answer_with_horizon_and_sightlines() {
    let (n, e) = (synth::HILL_A_NORTH_M, synth::HILL_A_EAST_M);
    let [x0, z0] = at(n - 3000.0, e - 6000.0);
    let [x1, z1] = at(n + 3000.0, e + 6000.0);
    let cands = lattice([x0, z0, x1, z1], 500.0, |_, _| true);
    let [ox, oz] = west_of_hill();
    let cs = criteria(json!([
        {"name": "masked_from",
         "params": {"bearing_deg": 270, "half_width_deg": 20, "min_elev_deg": 3}},
        {"name": "enclosure_min", "params": {"deg": 0}},
        {"name": "not_visible_from",
         "params": {"observers": [{"x": ox, "z": oz, "alt_agl": 2},
                                  {"x": ox - 3000.0, "z": oz, "alt_agl": 50}],
                    "max_fraction": 0.5}},
        {"name": "visible_to",
         "params": {"targets": [{"x": ox, "z": oz + 9000.0, "alt_agl": 10}],
                    "min_fraction": 1}}
    ]));
    let e = assert_pruning_changes_nothing(&cands, &square(40.0), &cs, 0);
    assert!(!e.sites.is_empty());
}

/// A brute-force oracle: each criterion recomputed for one site from the
/// closed form's cells, cell by cell, sharing nothing with the evaluator
/// but the definitions.
struct Oracle<'a> {
    s: &'a Synth,
    g: Grid,
}

impl Oracle<'_> {
    fn cell(&self, l: Layer, r: i64, c: i64) -> i32 {
        self.s.cell(l, r, c)
    }

    fn row_col(&self, x: f64, z: f64) -> (i64, i64) {
        let cs = f64::from(self.g.cell_size);
        (
            ((x - self.g.origin_x) / cs).floor() as i64,
            ((z - self.g.origin_z) / cs).floor() as i64,
        )
    }

    fn centre(&self, r: i64, c: i64) -> [f64; 2] {
        let cs = f64::from(self.g.cell_size);
        [
            self.g.origin_x + (r as f64 + 0.5) * cs,
            self.g.origin_z + (c as f64 + 0.5) * cs,
        ]
    }

    /// Every cell within 2 km whose centre lies in the footprint, the site's
    /// own besides: depth along the bearing, width across it.
    fn footprint(&self, p: [f64; 2], f: &Footprint, deg: f64) -> Vec<(i64, i64)> {
        let (sin, cos) = deg.to_radians().sin_cos();
        let (r0, c0) = self.row_col(p[0], p[1]);
        let mut out = Vec::new();
        for r in r0 - 40..=r0 + 40 {
            for c in c0 - 40..=c0 + 40 {
                let [x, z] = self.centre(r, c);
                let (dx, dz) = (x - p[0], z - p[1]);
                let along = dx * cos + dz * sin;
                let across = -dx * sin + dz * cos;
                let inside =
                    along.abs() <= f.depth / 2.0 + 1e-9 && across.abs() <= f.width / 2.0 + 1e-9;
                if inside || (r, c) == (r0, c0) {
                    out.push((r, c));
                }
            }
        }
        out
    }

    fn slope(&self, r: i64, c: i64) -> f64 {
        let h = |dr: i64, dc: i64| self.cell(Layer::Height, r + dr, c + dc);
        let mut z = [[0.0; 3]; 3];
        for (i, dr) in (-1..=1).enumerate() {
            for (j, dc) in (-1..=1).enumerate() {
                if h(dr, dc) == Layer::Height.nodata() {
                    return f64::NAN;
                }
                z[i][j] = f64::from(h(dr, dc));
            }
        }
        let cs = f64::from(self.g.cell_size);
        let north =
            (z[2][0] + 2.0 * z[2][1] + z[2][2] - z[0][0] - 2.0 * z[0][1] - z[0][2]) / (8.0 * cs);
        let east =
            (z[0][2] + 2.0 * z[1][2] + z[2][2] - z[0][0] - 2.0 * z[1][0] - z[2][0]) / (8.0 * cs);
        north.hypot(east).atan().to_degrees()
    }

    /// Meters from the cell to the nearest cell of `cover`, centre to
    /// centre, or `None` past `cap`.
    fn dist_cover(&self, r: i64, c: i64, cover: i32, cap: f64) -> Option<f64> {
        let n = (cap / f64::from(self.g.cell_size)).ceil() as i64;
        let a = self.centre(r, c);
        let mut best: Option<f64> = None;
        for i in r - n..=r + n {
            for j in c - n..=c + n {
                if self.cell(Layer::Cover, i, j) == cover {
                    let b = self.centre(i, j);
                    let d = (a[0] - b[0]).hypot(a[1] - b[1]);
                    if d <= cap && best.is_none_or(|v| d < v) {
                        best = Some(d);
                    }
                }
            }
        }
        best
    }

    fn share(&self, r: i64, c: i64, cover: i32, radius: f64) -> f64 {
        let n = (radius / f64::from(self.g.cell_size)).floor() as i64;
        let a = self.centre(r, c);
        let (mut of, mut all) = (0, 0);
        for i in r - n..=r + n {
            for j in c - n..=c + n {
                let b = self.centre(i, j);
                let v = self.cell(Layer::Cover, i, j);
                if (a[0] - b[0]).hypot(a[1] - b[1]) <= radius && v != Layer::Cover.nodata() {
                    all += 1;
                    of += i32::from(v == cover);
                }
            }
        }
        f64::from(of) / f64::from(all)
    }
}

#[test]
fn the_evaluator_agrees_with_a_brute_force_oracle() {
    let s = synth();
    let o = Oracle { s, g: s.grid() };
    let footprint = Footprint {
        width: 140.0,
        depth: 70.0,
        orientations_deg: vec![0.0, 30.0, 105.0],
    };
    // Around the town, the forest, the field, the river and the hills, with
    // a step that is no multiple of the cell, so sites fall anywhere in
    // their cells.
    let mut cands = Vec::new();
    for (n, e) in [
        (56_000.0, 30_000.0),
        (42_000.0, 57_000.0),
        (19_800.0, 61_500.0),
        (30_000.0, 45_000.0),
        (46_000.0, 32_000.0),
        (26_000.0, 26_000.0),
    ] {
        let [x, z] = at(n, e);
        cands.extend(lattice(
            [x - 1500.0, z - 1500.0, x + 1500.0, z + 1500.0],
            437.0,
            |_, _| true,
        ));
    }
    let cs = criteria(json!([
        {"name": "slope_max", "params": {"deg": 4}},
        {"name": "surface_in", "params": {"classes": ["land", "river"]}},
        {"name": "cover_in", "params": {"classes": ["plain", "forest", "town"]}},
        {"name": "cover_fraction_min", "params": {"class": "forest", "radius": 400, "min": 0.2}},
        {"name": "dist_cover_band", "params": {"class": "field", "min": 100, "max": 900}},
        {"name": "dist_airdrome_band", "params": {"min": 0, "max": 40000}},
        {"name": "scenery_max", "params": {"class": "building", "radius": 600, "max": 4}},
        {"name": "clearing_max", "params": {"radius": 300, "max": 2}}
    ]));
    let e = evaluate(s, &cands, &footprint, &cs, Mode::Every).unwrap();
    let mut objects = Vec::new();
    s.objects_in([f64::MIN, f64::MIN, f64::MAX, f64::MAX], &mut |ob| {
        objects.push((ob.pos[0], ob.pos[2], ob.class));
    })
    .unwrap();
    let airdrome = at(synth::RUNWAY_NORTH_M, synth::RUNWAY_EAST_M);
    let (mut passed, mut failed) = ([0usize; 9], [0usize; 9]);
    for site in &e.sites {
        let p = [site.x, site.z];
        let (r, c) = o.row_col(p[0], p[1]);
        let cells = o.footprint(p, &footprint, site.orientation_deg);
        let all = |f: &dyn Fn(i64, i64) -> bool| cells.iter().all(|&(r, c)| f(r, c));
        let valid = all(&|r, c| {
            o.cell(Layer::Height, r, c) != Layer::Height.nodata() && o.cell(Layer::Water, r, c) != 2
        });
        let slope = cells
            .iter()
            .map(|&(r, c)| o.slope(r, c))
            .fold(0.0f64, |m, v| {
                if m.is_nan() || v.is_nan() {
                    f64::NAN
                } else {
                    m.max(v)
                }
            });
        let land = all(&|r, c| [0, 3].contains(&o.cell(Layer::Water, r, c)));
        let cover = all(&|r, c| [1, 3, 4].contains(&o.cell(Layer::Cover, r, c)));
        let share = o.share(r, c, 3, 400.0);
        let field = o.dist_cover(r, c, 2, 950.0);
        let air = (p[0] - airdrome[0]).hypot(p[1] - airdrome[1]);
        let count = |class: Option<Class>, radius: f64| {
            objects
                .iter()
                .filter(|(_, _, k)| k.is_structure() && class.is_none_or(|w| *k == w))
                .filter(|(x, z, _)| (x - p[0]).hypot(z - p[1]) <= radius)
                .count() as f64
        };
        let (buildings, any) = (count(Some(Class::Building), 600.0), count(None, 300.0));
        let want = [
            ("valid", f64::NAN, valid),
            ("slope_max", slope, slope <= 4.0),
            ("surface_in", f64::NAN, land),
            ("cover_in", f64::NAN, cover),
            ("cover_fraction_min", share, share >= 0.2),
            (
                "dist_cover_band",
                field.unwrap_or(f64::NAN),
                field.is_some_and(|d| (100.0..=900.0).contains(&d)),
            ),
            ("dist_airdrome_band", air, air <= 40000.0),
            ("scenery_max", buildings, buildings <= 4.0),
            ("clearing_max", any, any <= 2.0),
        ];
        for (k, ((name, value, pass), t)) in want.iter().zip(&site.terms).enumerate() {
            assert_eq!(t.name, *name);
            passed[k] += usize::from(*pass);
            failed[k] += usize::from(!*pass);
            let deg = site.orientation_deg;
            assert_eq!(t.pass, *pass, "{name} at {p:?} {deg}°: {t:?}");
            if !value.is_nan() {
                assert!(
                    (t.value - value).abs() < 1e-3,
                    "{name} at {p:?}: {t:?} against {value}"
                );
            }
        }
        assert_eq!(site.pass, want.iter().all(|w| w.2));
    }
    // No criterion's cases are all one way.
    for k in 0..passed.len() {
        assert!(
            passed[k] >= 10 && failed[k] >= 10,
            "term {k}: {} passed, {} failed",
            passed[k],
            failed[k]
        );
    }
}
