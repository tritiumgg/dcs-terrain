//! `route` off the road, on the synthetic terrain.

use super::super::answer;
use super::super::geojson::tests::assert_valid;
use super::super::tests::synth;
use super::*;
use crate::synth::*;
use serde_json::{Value, json};

fn form(t: &Terrain) -> Theatre {
    match t {
        Terrain::Synth(s) => *s.form(),
        _ => unreachable!(),
    }
}

/// A point given north and east of the synthetic terrain's origin.
fn at(t: &Terrain, north: f64, east: f64) -> Value {
    let (x, z) = form(t).abs(north, east);
    json!({"x": x, "z": z})
}

fn ask(t: &Terrain, op: &str, req: Value) -> Result<Value, Vec<Issue>> {
    answer(t, op, &req)
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * b.abs().max(1.0)
}

fn num(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("not a number: {v}"))
}

/// The turns of a way off the road, `[x, z]`.
fn path(leg: &Value) -> Vec<[f64; 2]> {
    leg["path"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| [num(&p["x"]), num(&p["z"])])
        .collect()
}

fn request(t: &Terrain, from: (f64, f64), to: (f64, f64)) -> RouteRequest {
    let point = |(n, e): (f64, f64)| {
        let (x, z) = form(t).abs(n, e);
        Point { x, z }
    };
    RouteRequest {
        network: Network::Roads,
        from: point(from),
        to: point(to),
        avoid: Vec::new(),
        exposure_observers: Vec::new(),
        exposure_weight: 1.0,
        max_detour_factor: 3.0,
        offroad: true,
        max_offroad: 5000.0,
        max_slope_deg: 30.0,
        offroad_weight: 2.0,
        geojson: false,
    }
}

#[test]
fn an_offroad_end_is_a_way_off_the_road_road_legs_and_a_way_off_it() {
    let t = synth();
    // South of the east-west road, west of the north-south one, 10 km apart.
    let req = json!({
        "network": "roads",
        "from": at(&t, ROAD_EW_NORTH_M - 500.0, 10000.0),
        "to": at(&t, ROAD_EW_NORTH_M - 400.0, 20000.0),
        "offroad": true,
        "geojson": true,
    });
    let v = ask(&t, "route", req.clone()).unwrap();
    let (from, to) = (&v["offroad_from"], &v["offroad_to"]);
    // Each way runs from its end to the road, and no shorter than the
    // straight line to it.
    assert!(num(&from["length"]) >= 500.0 - 1e-6, "{from}");
    assert!(num(&to["length"]) >= 400.0 - 1e-6, "{to}");
    let legs = v["legs"].as_array().unwrap();
    assert!(!legs.is_empty());
    assert!(legs.iter().all(|l| l["class"] == ROAD_EW_CLASS));
    let road: f64 = legs.iter().map(|l| num(&l["length"])).sum();
    let total = num(&from["length"]) + road + num(&to["length"]);
    assert!(close(num(&v["length"]), total), "{} {total}", v["length"]);
    // The ways meet the road where the route joins and leaves it.
    let (a, b) = (path(from), path(to));
    let joined = [num(&v["snapped_from"]["x"]), num(&v["snapped_from"]["z"])];
    assert!(dist(a[a.len() - 1], joined) < 1e-2);
    assert!(
        dist(
            b[0],
            [
                num(&legs[legs.len() - 1]["to"]["x"]),
                num(&legs[legs.len() - 1]["to"]["z"])
            ]
        ) < 1e-2
    );
    // The ends as given are the ways' ends and the waypoints'.
    let w = v["waypoints"].as_array().unwrap();
    assert_eq!(w[0], req["from"]);
    assert_eq!(w[w.len() - 1], req["to"]);
    assert_eq!(a[0], [num(&req["from"]["x"]), num(&req["from"]["z"])]);
    // A way off the road costs at least its length, and open ground little
    // more.
    let cost = num(&from["cost"]);
    assert!(cost >= num(&from["length"]) && cost < 1.1 * num(&from["length"]));
    // A diagonal to the road costs less than a square turn, at twice a
    // meter of road a meter: the way leaves toward the far end.
    assert!(joined[1] > num(&req["from"]["z"]) + 100.0, "{joined:?}");
    assert_valid(&v["geojson"]);
    // The totals climb over the whole way: up the plane to the road.
    assert!(num(&v["ascent"]) > 0.0);
    let back: RouteResponse = serde_json::from_value(v).unwrap();
    assert!(back.route.offroad_from.unwrap().is_some());
}

#[test]
fn a_star_equals_dijkstra_over_the_joined_search() {
    let t = synth();
    let mut u = crate::roads::tests::Unit(0x9e37_79b9_7f4a_7c15);
    let mut routed = 0;
    for _ in 0..16 {
        // Within reach of the east-west road and the north-south one, so
        // most pairs join a road.
        let p = |u: &mut crate::roads::tests::Unit| {
            (
                ROAD_EW_NORTH_M - 2500.0 + u.next() * 5000.0,
                10_000.0 + u.next() * 40_000.0,
            )
        };
        let mut q = request(&t, p(&mut u), p(&mut u));
        q.max_offroad = 3000.0;
        let Ok(mut planner) = Planner::new(&t, &q) else {
            continue;
        };
        let a = planner.search(&HashMap::new(), true).unwrap();
        let d = planner.search(&HashMap::new(), false).unwrap();
        assert_eq!(a.is_some(), d.is_some());
        let (Some(a), Some(d)) = (a, d) else { continue };
        assert!(close(a.cost, d.cost), "{} {}", a.cost, d.cost);
        // Every way costs at least its length.
        assert!(a.cost >= a.length(planner.g) * (1.0 - 1e-9));
        routed += 1;
    }
    assert!(routed >= 10, "{routed}");
}

#[test]
fn an_end_whose_nearest_road_joins_nothing_routes_through_another() {
    let t = synth();
    let abs = |n: f64, e: f64| {
        let (x, z) = form(&t).abs(n, e);
        [x, z]
    };
    let road = |a: [f64; 2], b: [f64; 2]| crate::roads::Stretch {
        kind: KIND_ROAD,
        class: 0,
        a: None,
        b: None,
        points: vec![a, b],
    };
    // On the open plane: a long road, and a short piece of road between it
    // and the start, joined to nothing.
    let stretches = vec![
        road(abs(30_000.0, 33_000.0), abs(30_000.0, 43_000.0)),
        road(abs(29_700.0, 34_800.0), abs(29_700.0, 35_600.0)),
    ];
    let stations = stretches
        .iter()
        .map(|s| [0.0, dist(s.points[0], s.points[1])])
        .collect();
    let joins = crate::roads::Joins {
        stations,
        one_way: vec![[false; 2]; 2],
        passes: Vec::new(),
    };
    let dry = &mut |_: f64, _: f64| false;
    let g = crate::roads::build_joined(vec!["road".into()], stretches, Some(joins), dry).unwrap();
    let mut q = request(&t, (29_600.0, 35_200.0), (30_100.0, 42_000.0));
    q.offroad = false;
    // Snapped, the start lies on the piece that joins nothing.
    let mut p = Planner::over(&t, &g, &q).unwrap();
    assert_eq!(p.from.edge, 1);
    assert!(p.search(&HashMap::new(), true).unwrap().is_none());
    let issue = &p.no_route()[0];
    assert_eq!(issue.code, "no_route");
    assert!(issue.message.contains("the from end"), "{}", issue.message);
    assert!(issue.message.contains("set offroad"), "{}", issue.message);
    // Off the road, the way crosses the piece to the long road.
    q.offroad = true;
    let mut p = Planner::over(&t, &g, &q).unwrap();
    let found = p.search(&HashMap::new(), true).unwrap().unwrap();
    assert!(
        found.pieces.iter().all(|piece| piece.edge == 0),
        "{:?}",
        found.pieces
    );
    let route = p.describe(&found, 1.0).unwrap();
    let way = route.offroad_from.unwrap().unwrap();
    // About the 400 m to the long road, a little more for the diagonal.
    assert!((400.0..600.0).contains(&way.length), "{}", way.length);
}

#[test]
fn no_way_off_the_road_crosses_water() {
    let t = synth();
    let src = t.source();
    // Either side of the river, south of the east-west road and its
    // bridge: the way crosses on the bridge.
    let from = (ROAD_EW_NORTH_M - 2000.0, RIVER_EAST_M - 1500.0);
    let to = (ROAD_EW_NORTH_M - 2000.0, RIVER_EAST_M + 1500.0);
    let v = ask(
        &t,
        "route",
        json!({
            "network": "roads",
            "from": at(&t, from.0, from.1),
            "to": at(&t, to.0, to.1),
            "offroad": true,
            "offroad_weight": 1.0,
        }),
    )
    .unwrap();
    let g = src.road_graph().unwrap();
    let bridge = (0..g.edges.len())
        .find(|&i| g.edges[i].kind == KIND_ROAD && g.edges[i].crosses_water())
        .unwrap();
    assert!(
        v["legs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|l| l["edge_id"] == bridge as i64)
    );
    for leg in [&v["offroad_from"], &v["offroad_to"]] {
        let turns = path(leg);
        let mut points = Vec::new();
        for s in turns.windows(2) {
            let n = (dist(s[0], s[1]) / 5.0).ceil() as usize;
            for k in 0..=n {
                let f = k as f64 / n as f64;
                points.push([
                    s[0][0] + f * (s[1][0] - s[0][0]),
                    s[0][1] + f * (s[1][1] - s[0][1]),
                ]);
            }
        }
        for (p, gr) in points.iter().zip(src.points(&points).unwrap()) {
            assert_eq!(gr.unwrap().water, WATER_LAND, "{p:?}");
        }
    }
}

#[test]
fn with_observers_a_hidden_way_is_chosen_over_a_shorter_seen_one() {
    let t = synth();
    // West of the big hill, out of reach of every road: the way runs
    // across country the whole way. The observer stands on the plane north
    // of the hill's western flank, which hides its eastern side.
    let from = (HILL_A_NORTH_M - 1500.0, HILL_A_EAST_M - 1200.0);
    let to = (HILL_A_NORTH_M + 1500.0, HILL_A_EAST_M - 1200.0);
    let (x, z) = form(&t).abs(HILL_A_NORTH_M, HILL_A_EAST_M - 5000.0);
    let req = |weight: f64| {
        json!({
            "network": "roads",
            "from": at(&t, from.0, from.1),
            "to": at(&t, to.0, to.1),
            "offroad": true,
            "max_offroad": 3500.0,
            "exposure_observers": [{"x": x, "z": z, "alt_agl": 2.0}],
            "exposure_weight": weight,
        })
    };
    let seen = ask(&t, "route", req(0.0)).unwrap();
    let hidden = ask(&t, "route", req(20.0)).unwrap();
    for v in [&seen, &hidden] {
        assert!(v["legs"].as_array().unwrap().is_empty(), "{v}");
        assert!(v["snapped_from"].is_null() && v["offroad_to"].is_null());
    }
    let (a, b) = (&seen["offroad_from"], &hidden["offroad_from"]);
    assert!(num(&a["exposure"]) > 0.9, "{a}");
    assert!(num(&b["exposure"]) < 0.5 * num(&a["exposure"]), "{b}");
    assert!(num(&b["length"]) > num(&a["length"]) + 100.0, "{a} {b}");
}

#[test]
fn ends_within_reach_of_each_other_and_no_road_take_one_way() {
    let t = synth();
    // On the plane far from every road.
    let v = ask(
        &t,
        "route",
        json!({
            "network": "roads",
            "from": at(&t, 30_000.0, 38_000.0),
            "to": at(&t, 30_400.0, 38_300.0),
            "offroad": true,
            "geojson": true,
        }),
    )
    .unwrap();
    assert!(v["legs"].as_array().unwrap().is_empty());
    assert!(v["snapped_from"].is_null() && v["snapped_to"].is_null());
    assert!(v["offroad_to"].is_null());
    let way = &v["offroad_from"];
    assert!(close(num(&way["length"]), 500.0), "{way}");
    assert!(close(num(&v["length"]), 500.0));
    assert_eq!(v["waypoints"].as_array().unwrap().len(), 2);
    assert_valid(&v["geojson"]);
    // Out of reach of each other and of any road, there is no route.
    let far = ask(
        &t,
        "route",
        json!({
            "network": "roads",
            "from": at(&t, 30_000.0, 38_000.0),
            "to": at(&t, 30_000.0, 48_000.0),
            "offroad": true,
        }),
    )
    .unwrap_err();
    assert_eq!(far[0].code, "no_route");
    assert_eq!(far[0].path, "from");
    assert!(far[0].message.contains("5000 m"), "{}", far[0].message);
}

#[test]
fn route_alternatives_take_the_same_fields() {
    let t = synth();
    let v = ask(
        &t,
        "route_alternatives",
        json!({
            "network": "roads",
            "from": at(&t, ROAD_NS_SOUTH_NORTH_M - 300.0, ROAD_NS_EAST_M + 200.0),
            "to": at(&t, ROAD_EAST_SOUTH_NORTH_M - 300.0, ROAD_EAST_EAST_M + 200.0),
            "offroad": true,
            "k": 2,
        }),
    )
    .unwrap();
    let routes = v["routes"].as_array().unwrap();
    assert!(!routes.is_empty());
    for r in routes {
        assert!(r["offroad_from"].is_object(), "{r}");
        assert!(r["offroad_to"].is_object(), "{r}");
    }
}

#[test]
fn off_road_fields_are_bounded_and_refused_on_railroads() {
    let t = synth();
    let base = json!({
        "network": "roads",
        "from": at(&t, ROAD_EW_NORTH_M - 500.0, 10000.0),
        "to": at(&t, ROAD_EW_NORTH_M - 400.0, 20000.0),
        "offroad": true,
    });
    for (field, value) in [
        ("max_offroad", json!(0)),
        ("max_offroad", json!(5001)),
        ("max_slope_deg", json!(0)),
        ("max_slope_deg", json!(61)),
        ("offroad_weight", json!(0.99)),
        ("offroad_weight", json!(1001)),
    ] {
        let mut q = base.clone();
        q[field] = value;
        let e = ask(&t, "route", q).unwrap_err();
        assert_eq!(e[0].path, field);
    }
    let mut q = base.clone();
    q["network"] = json!("railroads");
    let e = ask(&t, "route", q).unwrap_err();
    assert_eq!(e[0].path, "offroad");
    // An end on water cannot start.
    let mut q = base;
    q["from"] = at(&t, LAKE_NORTH_M, LAKE_EAST_M);
    let e = ask(&t, "route", q).unwrap_err();
    assert_eq!(
        (e[0].code.as_str(), e[0].path.as_str()),
        ("no_route", "from")
    );
    assert!(e[0].message.contains("a lake"), "{}", e[0].message);
}
