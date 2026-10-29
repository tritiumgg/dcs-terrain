//! `point_info`: what stands at each of a list of points, and where each is
//! from the nearest town, road and airfield.

use serde_json::{Value, json};

use super::geojson::Features;
use super::nearest::{airdromes, edges, towns};
use super::sample::derived_values;
use super::{Terrain, mgrs, refused};
use crate::crs::{Tmerc, grid_bearing};
use crate::decode::rn4::KIND_ROAD;
use crate::layers::Derived;
use crate::types::{
    CoverClass, Issue, Nearby, PointInfo, PointInfoRequest, PointInfoResponse, TownFix, WaterClass,
};

/// The most points one request asks about.
const POINTS_MAX: usize = 10_000;
/// Nearer its town than this, a point is said to be at it.
const AT_TOWN_M: f64 = 50.0;
const COMPASS: [&str; 8] = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];

/// The point of the eight-point compass a bearing in degrees falls in, each
/// point holding the 45° centered on it.
fn compass(bearing: f64) -> &'static str {
    COMPASS[((bearing / 45.0).round() as usize) % COMPASS.len()]
}

/// "6 km NE of Zugdidi": hundreds of meters inside 950 m, whole kilometers
/// from there, where the hundreds round to one.
fn describe(name: &str, distance: f64, compass: &str) -> String {
    if distance < AT_TOWN_M {
        return format!("at {name}");
    }
    let far = if distance < 950.0 {
        format!("{:.0} m", (distance / 100.0).round() * 100.0)
    } else {
        format!("{:.0} km", distance / 1000.0)
    };
    format!("{far} {compass} of {name}")
}

/// The point placed from the town nearest it: bearings from the town.
fn fix(tm: &Tmerc, town: Nearby, p: [f64; 2]) -> TownFix {
    let at = [town.x, town.z];
    let bearing_true_deg = tm.true_bearing(at, p);
    let point = compass(bearing_true_deg);
    let name = town.name.unwrap_or_default();
    TownFix {
        description: describe(&name, town.distance, point),
        compass: point.to_string(),
        bearing_true_deg,
        bearing_grid_deg: grid_bearing(at, p),
        distance: town.distance,
        x: town.x,
        z: town.z,
        name,
    }
}

pub fn point_info(terrain: &Terrain, q: PointInfoRequest) -> Result<PointInfoResponse, Vec<Issue>> {
    let src = terrain.source();
    if q.points.len() > POINTS_MAX {
        return Err(vec![Issue::new(
            "invalid_request",
            &format!("at most {POINTS_MAX} points"),
            "points",
        )]);
    }
    if let Some(k) = (q.points.iter()).position(|p| !(p.x.is_finite() && p.z.is_finite())) {
        return Err(vec![Issue::new(
            "invalid_point",
            "x and z must be finite",
            &format!("points.{k}"),
        )]);
    }
    let pts: Vec<[f64; 2]> = q.points.iter().map(|p| [p.x, p.z]).collect();
    let tables = src.tables().map_err(refused)?;
    let tm = tables.crs.tmerc();
    let ground = src.points(&pts).map_err(refused)?;
    let mut capped = false;
    let slope = derived_values(src, Derived::Slope, &pts, &mut capped)?;
    let aspect = derived_values(src, Derived::Aspect, &pts, &mut capped)?;

    let mut out = Vec::with_capacity(pts.len());
    for (k, &p) in pts.iter().enumerate() {
        let from = (p[0], p[1]);
        let g = ground[k];
        let road = edges(src, KIND_ROAD, from, 1)?.pop();
        let (lat, lon) = tm.to_latlon(p[0], p[1]);
        out.push(PointInfo {
            x: p[0],
            z: p[1],
            height: g.map(|g| g.height),
            water: g.and_then(|g| WaterClass::of_code(g.water)),
            depth: g.filter(|g| g.water != 0).map(|g| g.depth),
            cover: g.map(|g| CoverClass::of(g.cover)),
            slope: slope[k],
            aspect: aspect[k],
            town: towns(tables, from, 1).pop().map(|t| fix(&tm, t, p)),
            road,
            airfield: airdromes(tables, from, 1).pop(),
            lat,
            lon,
            mgrs: mgrs(lat, lon),
        });
    }
    let geojson = if q.geojson {
        let mut f = Features::new(terrain)?;
        for info in &out {
            let Value::Object(mut props) = json!(info) else {
                unreachable!("a point's fields serialize to an object")
            };
            for k in ["x", "z", "lat", "lon"] {
                props.remove(k);
            }
            f.point([info.x, info.z], props);
        }
        Some(f.finish())
    } else {
        None
    };
    Ok(PointInfoResponse {
        provenance: terrain.provenance(),
        points: out,
        geojson,
    })
}

#[cfg(test)]
mod tests {
    use super::super::answer;
    use super::super::geojson::tests::assert_valid;
    use super::super::tests::synth;
    use super::*;
    use crate::crs::tests::{geodesic_azimuth, turn};
    use crate::synth::*;
    use serde_json::json;

    fn form(t: &Terrain) -> Theatre {
        match t {
            Terrain::Synth(s) => *s.form(),
            _ => unreachable!(),
        }
    }

    fn ask(t: &Terrain, pts: &[(f64, f64)]) -> Vec<Value> {
        let points: Vec<_> = pts.iter().map(|&(x, z)| json!({"x": x, "z": z})).collect();
        answer(t, "point_info", &json!({ "points": points })).unwrap()["points"]
            .as_array()
            .unwrap()
            .clone()
    }

    fn close(v: &Value, want: f64, tol: f64) {
        let got = v.as_f64().unwrap_or(f64::NAN);
        assert!((got - want).abs() <= tol, "{got} against {want}");
    }

    #[test]
    fn each_ground_field_is_the_terrains_own() {
        let t = synth();
        let f = form(&t);
        let off = COAST_M - 5000.0 * std::f64::consts::SQRT_2;
        let pts = [
            f.abs(LAKE_NORTH_M, LAKE_EAST_M),
            f.abs(off / 2.0, off / 2.0),
            f.abs(30000.0, RIVER_EAST_M + 10.0),
            // The plane, far from the hills: it falls one percent south.
            f.abs(20025.0, 65000.0),
            f.abs(RUNWAY_NORTH_M, RUNWAY_EAST_M),
            f.abs(100.0, 100.0),
        ];
        let r = ask(&t, &pts);
        assert_eq!(r.len(), pts.len());
        for (row, &(x, z)) in r.iter().zip(&pts) {
            assert_eq!((row["x"].as_f64(), row["z"].as_f64()), (Some(x), Some(z)));
        }
        let [lake, sea, river, plane, runway, outside] = &r[..] else {
            unreachable!()
        };
        assert_eq!(lake["water"], "lake");
        assert_eq!(lake["cover"], "water");
        assert_eq!(lake["height"], LAKE_LEVEL_M);
        close(&lake["depth"], LAKE_DEPTH_M, 1e-9);
        assert_eq!(sea["water"], "sea");
        close(&sea["depth"], 50.0, 1e-9);
        assert_eq!(river["water"], "river");
        close(&river["depth"], RIVER_DEPTH_M, 1e-9);

        assert_eq!(plane["water"], "land");
        assert_eq!(plane["depth"], Value::Null);
        assert_eq!(plane["cover"], "plain");
        close(&plane["height"], PLANE_BASE_M + PLANE_SLOPE * 20025.0, 1e-9);
        close(&plane["slope"], PLANE_SLOPE.atan().to_degrees(), 1e-3);
        close(&plane["aspect"], 180.0, 1e-3);

        assert_eq!(runway["cover"], "runway");
        assert_eq!(runway["airfield"]["id"], AIRDROME_ID.to_string());
        assert_eq!(runway["airfield"]["name"], AIRDROME_DISPLAY_NAME);

        for k in ["height", "water", "depth", "cover", "slope", "aspect"] {
            assert_eq!(outside[k], Value::Null, "{k}");
        }
        // Off the mesh a point is still placed.
        assert!(outside["town"]["name"].is_string() && outside["road"]["class"].is_string());
    }

    #[test]
    fn the_road_airfield_and_position_are_nearest_and_geos_answers() {
        let t = synth();
        let f = form(&t);
        let p = f.abs(ROAD_EW_NORTH_M + 300.0, 6000.0);
        let info = &ask(&t, &[p])[0];
        let road = answer(&t, "nearest", &json!({"kind": "road", "x": p.0, "z": p.1})).unwrap();
        assert_eq!(info["road"], road["results"][0]);
        close(&info["road"]["distance"], 300.0, 1e-3);
        let field = answer(
            &t,
            "nearest",
            &json!({"kind": "airdrome", "x": p.0, "z": p.1}),
        );
        assert_eq!(info["airfield"], field.unwrap()["results"][0]);
        let geo = answer(&t, "geo", &json!({"to_latlon": [{"x": p.0, "z": p.1}]})).unwrap();
        let g = &geo["to_latlon"][0];
        assert_eq!(
            (&info["lat"], &info["lon"], &info["mgrs"]),
            (&g["lat"], &g["lon"], &g["mgrs"])
        );
    }

    #[test]
    fn the_nearest_town_is_the_one_every_town_ranks_first() {
        let t = synth();
        let f = form(&t);
        let size = f.size_m();
        let tables = t.source().tables().unwrap();
        let tm = tables.crs.tmerc();
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut unit = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        let mut pts: Vec<(f64, f64)> = (0..2000)
            .map(|_| f.abs((unit() * 1.4 - 0.2) * size, (unit() * 1.4 - 0.2) * size))
            .collect();
        // Halfway between two towns, where they tie and the first listed
        // wins, and on a town itself.
        let (a, b) = (f.town(0), f.town(1));
        pts.push(((a.0 + b.0) / 2.0, a.1 + 700.0));
        pts.push(b);
        let got = ask(&t, &pts);
        for (row, &(x, z)) in got.iter().zip(&pts) {
            // Every town measured, the first of the nearest kept.
            let (mut best, mut best_d) = (0, f64::INFINITY);
            for (k, town) in tables.towns.iter().enumerate() {
                let d = ((town.x - x).powi(2) + (town.z - z).powi(2)).sqrt();
                if d < best_d {
                    (best, best_d) = (k, d);
                }
            }
            let want = &tables.towns[best];
            let fix = &row["town"];
            let name = want.display_name.as_ref().unwrap_or(&want.name);
            assert_eq!(fix["name"], name.as_str(), "{x} {z}");
            close(&fix["distance"], best_d, 1e-6);
            let grid = (z - want.z)
                .atan2(x - want.x)
                .to_degrees()
                .rem_euclid(360.0);
            assert!(turn(fix["bearing_grid_deg"].as_f64().unwrap(), grid) < 1e-9);
            if best_d > 1.0 {
                let (lat1, lon1) = tm.to_latlon(want.x, want.z);
                let (lat2, lon2) = tm.to_latlon(x, z);
                let az = geodesic_azimuth(lat1, lon1, lat2, lon2);
                assert!(turn(fix["bearing_true_deg"].as_f64().unwrap(), az) < 1e-5);
                assert_eq!(fix["compass"], compass(az));
            }
        }
        let tie = &got[got.len() - 2]["town"];
        assert_eq!(tie["name"], town_name(1));
        assert_eq!(
            got[got.len() - 1]["town"]["description"],
            format!("at {}", town_name(2))
        );
    }

    #[test]
    fn the_synthetic_terrains_grid_north_is_not_true_north() {
        // Its projection's central meridian lies over a degree off it, so
        // a point due grid north of a town is not due true north of it.
        let t = synth();
        let (x, z) = form(&t).town(0);
        let fix = &ask(&t, &[(x + 5000.0, z)])[0]["town"];
        assert_eq!(fix["bearing_grid_deg"], 0.0);
        let conv = fix["bearing_true_deg"].as_f64().unwrap();
        assert!(turn(conv, 0.0) > 0.1 && turn(conv, 0.0) < 5.0, "{conv}");
        assert_eq!(fix["description"], format!("5 km N of {}", town_name(1)));
    }

    #[test]
    fn a_description_rounds_as_a_reader_would() {
        assert_eq!(compass(0.0), "N");
        assert_eq!(compass(22.4), "N");
        assert_eq!(compass(22.6), "NE");
        assert_eq!(compass(337.6), "N");
        assert_eq!(compass(359.99), "N");
        assert_eq!(compass(202.5), "SW");
        assert_eq!(describe("Zugdidi", 49.0, "N"), "at Zugdidi");
        assert_eq!(describe("Zugdidi", 60.0, "E"), "100 m E of Zugdidi");
        assert_eq!(describe("Zugdidi", 940.0, "S"), "900 m S of Zugdidi");
        assert_eq!(describe("Zugdidi", 960.0, "S"), "1 km S of Zugdidi");
        assert_eq!(describe("Zugdidi", 6200.0, "NE"), "6 km NE of Zugdidi");
    }

    #[test]
    fn geojson_holds_a_point_per_request_point() {
        let t = synth();
        let f = form(&t);
        let (a, b) = (f.abs(20025.0, 65000.0), f.abs(100.0, 100.0));
        let v = answer(
            &t,
            "point_info",
            &json!({"points": [{"x": a.0, "z": a.1}, {"x": b.0, "z": b.1}], "geojson": true}),
        )
        .unwrap();
        assert_valid(&v["geojson"]);
        let features = v["geojson"]["features"].as_array().unwrap();
        assert_eq!(features.len(), 2);
        let p = &features[0]["properties"];
        assert_eq!(p["town"], v["points"][0]["town"]);
        assert!(p.get("x").is_none() && p.get("lat").is_none());
    }

    #[test]
    fn too_many_or_bad_points_are_an_issue() {
        let t = synth();
        let issue = |req: Value| answer(&t, "point_info", &req).unwrap_err()[0].clone();
        let many: Vec<_> = (0..=POINTS_MAX).map(|_| json!({"x": 0, "z": 0})).collect();
        assert_eq!(issue(json!({ "points": many })).code, "invalid_request");
        let nan = crate::types::Point {
            x: f64::NAN,
            z: 0.0,
        };
        let ok = crate::types::Point { x: 0.0, z: 0.0 };
        let q = PointInfoRequest {
            points: vec![ok, nan],
            geojson: false,
        };
        let bad = point_info(&t, q).unwrap_err().remove(0);
        assert_eq!(
            (bad.code.as_str(), bad.path.as_str()),
            ("invalid_point", "points.1")
        );
    }
}
