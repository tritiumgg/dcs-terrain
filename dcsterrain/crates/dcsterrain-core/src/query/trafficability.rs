//! `trafficability`: whether a vehicle can drive from a point straight to
//! the nearest road, judged every 25 m along the line by the off-road cost
//! of the cell under it.

use serde_json::json;

use super::geojson::{Features, props};
use super::route::{kind_of, samples_along};
use super::{Terrain, refused};
use crate::layers::Cells;
use crate::offroad::{Block, CostField};
use crate::types::{Issue, Point, TrafficabilityRequest, TrafficabilityResponse};

/// The steepest limit a request may set: past it the cost model's slope
/// factor says nothing a vehicle could use.
const MAX_SLOPE_LIMIT_DEG: f64 = 60.0;
/// The longest line judged: past it the nearest road is no way out on
/// foot or wheels, and the window of cells under it grows past a query.
pub const MAX_DISTANCE_M: f64 = 20_000.0;

fn water_name(w: i32) -> &'static str {
    match w {
        1 => "a lake",
        2 => "the sea",
        3 => "a river",
        _ => "water",
    }
}

pub fn trafficability(
    terrain: &Terrain,
    q: TrafficabilityRequest,
) -> Result<TrafficabilityResponse, Vec<Issue>> {
    if !(q.x.is_finite() && q.z.is_finite()) {
        let path = if q.x.is_finite() { "z" } else { "x" };
        return Err(vec![Issue::new("invalid_point", "must be finite", path)]);
    }
    if !(q.max_slope_deg > 0.0 && q.max_slope_deg <= MAX_SLOPE_LIMIT_DEG) {
        return Err(vec![Issue::new(
            "invalid_request",
            &format!("must be more than 0 and at most {MAX_SLOPE_LIMIT_DEG}"),
            "max_slope_deg",
        )]);
    }
    let src = terrain.source();
    let g = src.road_graph().map_err(refused)?;
    let p = [q.x, q.z];
    let Some(&(_, road)) = g.nearest(kind_of(q.network), p, 1).first() else {
        return Err(vec![Issue::new(
            "no_network",
            "this terrain has no edge of that network",
            "network",
        )]);
    };
    let distance = (road[0] - p[0]).hypot(road[1] - p[1]);
    if distance > MAX_DISTANCE_M {
        return Err(vec![Issue::new(
            "too_far",
            &format!(
                "the nearest road is {distance:.0} m away, past the {MAX_DISTANCE_M} m judged"
            ),
            "",
        )]);
    }
    let line = [p, road];
    let samples = samples_along(&line);
    let grid = src.grid();
    let bbox = [
        p[0].min(road[0]),
        p[1].min(road[1]),
        p[0].max(road[0]),
        p[1].max(road[1]),
    ];
    let field =
        CostField::new(src, Cells::covering(&grid, bbox), q.max_slope_deg).map_err(refused)?;
    let mut cost = 0.0;
    let mut blocking = None;
    let mut reason = format!(
        "passable: no slope over {}° and no water on the {distance:.0} m to the road",
        q.max_slope_deg
    );
    // The last sample is the road itself, which may be a bridge over water.
    for (i, s) in samples.iter().enumerate().take(samples.len() - 1) {
        let cell = grid
            .cell_at(s[0], s[1])
            .and_then(|(r, c)| field.get(r.into(), c.into()))
            .unwrap_or(Err(Block::NoGround));
        match cell {
            Ok(c) => {
                // Each sample's cell stands for the stretch to the next.
                if let Some(next) = samples.get(i + 1) {
                    cost += c * (next[0] - s[0]).hypot(next[1] - s[1]);
                }
            }
            Err(b) => {
                let along = (s[0] - p[0]).hypot(s[1] - p[1]);
                reason = match b {
                    Block::NoGround => format!("no ground {along:.0} m along the line"),
                    Block::Water(w) => format!("{} {along:.0} m along the line", water_name(w)),
                    Block::Cover(_) => {
                        format!("cover no vehicle crosses {along:.0} m along the line")
                    }
                    Block::Steep(d) => format!(
                        "a slope of {d:.1}°, over the {}° allowed, {along:.0} m along the line",
                        q.max_slope_deg
                    ),
                };
                blocking = Some(Point { x: s[0], z: s[1] });
                break;
            }
        }
    }
    let reachable = blocking.is_none();
    let geojson = if q.geojson {
        let mut f = Features::new(terrain)?;
        f.line(
            &line,
            props([
                ("reachable", json!(reachable)),
                ("distance", json!(distance)),
            ]),
        );
        f.point(p, props([("end", json!("from"))]));
        f.point(road, props([("end", json!("road"))]));
        if let Some(b) = &blocking {
            f.point([b.x, b.z], props([("blocking", json!(true))]));
        }
        Some(f.finish())
    } else {
        None
    };
    Ok(TrafficabilityResponse {
        provenance: terrain.provenance(),
        reachable,
        nearest_road: Point {
            x: road[0],
            z: road[1],
        },
        distance,
        cost: reachable.then_some(cost),
        blocking,
        reason,
        geojson,
    })
}

#[cfg(test)]
mod tests {
    use super::super::answer;
    use super::super::geojson::tests::assert_valid;
    use super::super::tests::synth;
    use crate::synth::*;
    use serde_json::{Value, json};

    fn at(t: &super::Terrain, north: f64, east: f64) -> (f64, f64) {
        match t {
            super::Terrain::Synth(s) => s.form().abs(north, east),
            _ => unreachable!(),
        }
    }

    fn ask(t: &super::Terrain, north: f64, east: f64, max: f64) -> Value {
        let (x, z) = at(t, north, east);
        answer(
            t,
            "trafficability",
            &json!({"x": x, "z": z, "max_slope_deg": max, "network": "roads", "geojson": true}),
        )
        .unwrap()
    }

    #[test]
    fn open_ground_reaches_the_road_at_about_its_length() {
        let t = synth();
        // 500 m south of the east-west road, on the plane.
        let v = ask(&t, ROAD_EW_NORTH_M - 500.0, 10000.0, 15.0);
        assert_eq!(v["reachable"], true, "{v}");
        assert!((v["distance"].as_f64().unwrap() - 500.0).abs() < 1e-6);
        let cost = v["cost"].as_f64().unwrap();
        assert!((500.0..502.0).contains(&cost), "{cost}");
        assert_valid(&v["geojson"]);
    }

    #[test]
    fn the_river_and_a_steep_hillside_block() {
        let t = synth();
        // Beside the river the line north to the road runs along its bank.
        let bank = ask(&t, ROAD_EW_NORTH_M - 3000.0, RIVER_EAST_M - 100.0, 30.0);
        assert_eq!(bank["reachable"], true, "{bank}");
        // From the river itself.
        let river = ask(&t, ROAD_EW_NORTH_M - 3000.0, RIVER_EAST_M, 30.0);
        assert_eq!(river["reachable"], false);
        assert!(river["reason"].as_str().unwrap().contains("river"));
        assert!(river["cost"].is_null());
        // On the big hill's flank, steeper than 5° but not than 30°.
        let flank = HILL_A_NORTH_M + HILL_SIGMA_M;
        let gentle = ask(&t, flank, HILL_A_EAST_M, 30.0);
        assert_eq!(gentle["reachable"], true, "{gentle}");
        let steep = ask(&t, flank, HILL_A_EAST_M, 5.0);
        assert_eq!(steep["reachable"], false, "{steep}");
        assert!(steep["reason"].as_str().unwrap().contains("slope"));
        assert!(steep["blocking"].is_object());
        let e = answer(
            &t,
            "trafficability",
            &json!({"x": 0, "z": 0, "max_slope_deg": 90, "network": "roads"}),
        )
        .unwrap_err();
        assert_eq!(e[0].code, "invalid_request");
        let (x, z) = at(&t, 5000.0, 5000.0);
        let far = answer(
            &t,
            "trafficability",
            &json!({"x": x, "z": z, "max_slope_deg": 20, "network": "roads"}),
        )
        .unwrap_err();
        assert_eq!(far[0].code, "too_far");
    }
}
