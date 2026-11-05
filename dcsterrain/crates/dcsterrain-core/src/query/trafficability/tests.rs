use super::super::answer;
use super::super::geojson::tests::assert_valid;
use super::super::tests::synth;
use super::{Searched, search_way};
use crate::decode::rn4::KIND_ROAD;
use crate::source::Source;
use crate::synth::*;
use serde_json::{Value, json};

fn at(t: &super::Terrain, north: f64, east: f64) -> (f64, f64) {
    match t {
        super::Terrain::Synth(s) => s.form().abs(north, east),
        _ => unreachable!(),
    }
}

fn ask_with(t: &super::Terrain, north: f64, east: f64, extra: Value) -> Result<Value, String> {
    let (x, z) = at(t, north, east);
    let mut q = json!({"x": x, "z": z, "network": "roads", "geojson": true});
    for (k, v) in extra.as_object().unwrap() {
        q[k] = v.clone();
    }
    answer(t, "trafficability", &q).map_err(|e| format!("{}: {}", e[0].code, e[0].message))
}

fn ask(t: &super::Terrain, north: f64, east: f64, max: f64) -> Value {
    ask_with(t, north, east, json!({"max_slope_deg": max})).unwrap()
}

#[test]
fn open_ground_reaches_the_road_at_about_its_length() {
    let t = synth();
    // 500 m south of the east-west road, on the plane.
    let v = ask(&t, ROAD_EW_NORTH_M - 500.0, 10000.0, 15.0);
    assert_eq!(v["reachable"], true, "{v}");
    assert!((v["distance"].as_f64().unwrap() - 500.0).abs() < 1e-6);
    let cost = v["straight"]["cost"].as_f64().unwrap();
    assert!((500.0..502.0).contains(&cost), "{cost}");
    // On open ground the way is the straight line.
    let way = &v["way"];
    assert_eq!(way["path"].as_array().unwrap().len(), 2, "{way}");
    assert_eq!(way["meets"], v["nearest_road"]);
    assert!((way["length"].as_f64().unwrap() - 500.0).abs() < 1e-6);
    assert!(way["edge_id"].is_i64());
    assert_valid(&v["geojson"]);
}

#[test]
fn a_way_goes_round_the_big_hill_where_the_straight_line_climbs_it() {
    let t = synth();
    // West of the big hill: the line east to the north-south road runs
    // over its summit, steeper than 8° on its flanks.
    let v = ask(&t, HILL_A_NORTH_M, HILL_A_EAST_M - 4000.0, 8.0);
    assert!(v["straight"]["cost"].is_null(), "{v}");
    assert!(v["straight"]["blocking"].is_object());
    assert!(
        v["straight"]["reason"].as_str().unwrap().contains("slope"),
        "{v}"
    );
    assert_eq!(v["reachable"], true, "{v}");
    let way = &v["way"];
    let length = way["length"].as_f64().unwrap();
    assert!(length > v["distance"].as_f64().unwrap(), "{way}");
    assert!(
        v["reason"].as_str().unwrap().contains("round what blocks"),
        "{}",
        v["reason"]
    );
    // It keeps off the hill's steep middle: no turn and no point between
    // two within a sigma of the summit.
    let (hx, hz) = at(&t, HILL_A_NORTH_M, HILL_A_EAST_M);
    let path: Vec<[f64; 2]> = way["path"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| [p["x"].as_f64().unwrap(), p["z"].as_f64().unwrap()])
        .collect();
    for s in path.windows(2) {
        for k in 0..=100 {
            let f = f64::from(k) / 100.0;
            let q = [
                s[0][0] + f * (s[1][0] - s[0][0]),
                s[0][1] + f * (s[1][1] - s[0][1]),
            ];
            assert!((q[0] - hx).hypot(q[1] - hz) > HILL_SIGMA_M, "{q:?}");
        }
    }
    // It meets the road where the way's last turn is.
    assert_eq!(
        &way["meets"],
        way["path"].as_array().unwrap().last().unwrap()
    );
    assert!(way["ascent"].as_f64().unwrap() >= 0.0);
    assert_valid(&v["geojson"]);
}

#[test]
fn wherever_the_straight_line_passes_the_way_costs_no_more() {
    let t = synth();
    let mut passed = 0;
    for (north, east, max) in [
        (ROAD_EW_NORTH_M - 500.0, 10000.0, 15.0),
        (ROAD_EW_NORTH_M - 3000.0, RIVER_EAST_M - 100.0, 30.0),
        (HILL_A_NORTH_M + HILL_SIGMA_M, HILL_A_EAST_M, 30.0),
        (HILL_A_NORTH_M, HILL_A_EAST_M - 4000.0, 30.0),
        (FOREST_SOUTH_M + 100.0, FOREST_WEST_M + 100.0, 20.0),
        (ROAD_EW_NORTH_M - 2000.0, 60000.0, 20.0),
    ] {
        let v = ask(&t, north, east, max);
        let Some(line) = v["straight"]["cost"].as_f64() else {
            continue;
        };
        passed += 1;
        let way = v["way"]["cost"].as_f64().unwrap();
        assert!(way <= line, "{north}, {east}: {way} over {line}");
    }
    assert!(passed >= 5, "{passed}");
}

#[test]
fn across_the_river_with_no_bridge_in_reach_is_unreachable() {
    let t = synth();
    // East of the river, its nearest road the north-south one to the west;
    // the bridge and the roads east of the river lie past 13 km.
    let v = ask_with(
        &t,
        40000.0,
        RIVER_EAST_M + 100.0,
        json!({"max_slope_deg": 30, "max_distance": 13000}),
    )
    .unwrap();
    assert_eq!(v["reachable"], false, "{v}");
    assert!(v["way"].is_null());
    let reason = v["reason"].as_str().unwrap();
    assert!(reason.contains("a river"), "{reason}");
    assert!(reason.contains("13000 m"), "{reason}");
    // The straight line west crosses the river.
    assert!(v["straight"]["reason"].as_str().unwrap().contains("river"));
    assert_valid(&v["geojson"]);
}

#[test]
fn a_near_road_searches_no_cell_past_its_cost() {
    let t = synth();
    let super::Terrain::Synth(s) = &t else {
        unreachable!()
    };
    let g = s.road_graph().unwrap();
    let (x, z) = at(&t, ROAD_EW_NORTH_M - 500.0, 10000.0);
    let found = search_way(s.as_ref(), g, KIND_ROAD, [x, z], 500.0, 20.0, 20_000.0).unwrap();
    let Searched::Found(way, _) = &found.outcome else {
        panic!("no way found")
    };
    let goal = found.goal.unwrap();
    assert!(found.most <= goal, "{} past {goal}", found.most);
    // The point lies on the edge of its cell, 25 m from the center the
    // search starts at.
    assert!(goal < 530.0, "{goal}");
    assert!(way.cost <= goal + 1e-9);
    // One round, over the first reach.
    assert_eq!(found.reach, 2000.0);
}

#[test]
fn a_point_on_water_or_too_steep_cannot_start() {
    let t = synth();
    let river = ask(&t, ROAD_EW_NORTH_M - 3000.0, RIVER_EAST_M, 30.0);
    assert_eq!(river["reachable"], false);
    assert!(river["reason"].as_str().unwrap().contains("river"));
    assert!(river["straight"]["cost"].is_null());
    // On the big hill's flank, steeper than 5° but not than 30°.
    let flank = HILL_A_NORTH_M + HILL_SIGMA_M;
    let gentle = ask(&t, flank, HILL_A_EAST_M, 30.0);
    assert_eq!(gentle["reachable"], true, "{gentle}");
    let steep = ask(&t, flank, HILL_A_EAST_M, 5.0);
    assert_eq!(steep["reachable"], false, "{steep}");
    assert!(steep["reason"].as_str().unwrap().contains("slope"));
    assert!(steep["straight"]["blocking"].is_object());
}

#[test]
fn past_max_distance_is_too_far_and_the_fields_are_bounded() {
    let t = synth();
    let e = ask_with(&t, 0.0, 0.0, json!({"max_slope_deg": 90})).unwrap_err();
    assert!(e.starts_with("invalid_request"), "{e}");
    for bad in [0.0, -1.0, 20_001.0] {
        let e = ask_with(
            &t,
            0.0,
            0.0,
            json!({"max_slope_deg": 20, "max_distance": bad}),
        )
        .unwrap_err();
        assert!(e.starts_with("invalid_request"), "{e}");
    }
    let far = ask_with(&t, 5000.0, 5000.0, json!({"max_slope_deg": 20})).unwrap_err();
    assert!(far.starts_with("too_far"), "{far}");
    let near = ask_with(
        &t,
        ROAD_EW_NORTH_M - 500.0,
        10000.0,
        json!({"max_slope_deg": 20, "max_distance": 400}),
    )
    .unwrap_err();
    assert!(
        near.starts_with("too_far") && near.contains("max_distance"),
        "{near}"
    );
}
