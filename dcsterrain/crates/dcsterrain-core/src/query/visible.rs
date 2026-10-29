//! `visible`: whether the terrain hides one point from another.

use serde_json::json;

use super::geojson::{Features, props};
use super::sightline::{Heights, walk};
use super::{Terrain, refused};
use crate::types::{Issue, Point3, VisibleRequest, VisibleResponse};

/// A point's absolute altitude over the ground under it, or the issue with
/// it. `path` names it in the request.
fn altitude(p: &Point3, ground: Option<f64>, path: &str) -> Result<(f64, f64), Issue> {
    if let Some(i) = p.check(path) {
        return Err(i);
    }
    let finite = [Some(p.x), Some(p.z), p.alt, p.alt_agl]
        .into_iter()
        .flatten()
        .all(f64::is_finite);
    if !finite {
        return Err(Issue::new(
            "invalid_point",
            "every coordinate must be finite",
            path,
        ));
    }
    let Some(g) = ground else {
        return Err(Issue::new(
            "off_terrain",
            "no mesh covers this point, so it has no ground to stand on",
            path,
        ));
    };
    Ok((p.alt.unwrap_or_else(|| g + p.alt_agl.unwrap_or(0.0)), g))
}

pub fn visible(terrain: &Terrain, q: VisibleRequest) -> Result<VisibleResponse, Vec<Issue>> {
    let src = terrain.source();
    // The ends stand on the mesh triangle's own height, as the game's do.
    let ground = if [q.a.x, q.a.z, q.b.x, q.b.z].iter().all(|v| v.is_finite()) {
        src.points(&[[q.a.x, q.a.z], [q.b.x, q.b.z]])
            .map_err(refused)?
            .into_iter()
            .map(|g| g.map(|g| g.height))
            .collect()
    } else {
        vec![None, None]
    };
    let (a, b) = (
        altitude(&q.a, ground[0], "a"),
        altitude(&q.b, ground[1], "b"),
    );
    let ((alt_a, ground_a), (alt_b, ground_b)) = match (a, b) {
        (Ok(a), Ok(b)) => (a, b),
        (a, b) => return Err([a.err(), b.err()].into_iter().flatten().collect()),
    };
    let step = f64::from(src.grid().cell_size) / 2.0;
    let sight = walk(
        &mut Heights::new(src),
        [q.a.x, q.a.z, alt_a],
        [q.b.x, q.b.z, alt_b],
        step,
    )
    .map_err(refused)?;
    // Ends too close for a step between them see each other over nothing
    // but the ground they stand on.
    let clearance_m = sight
        .clearance
        .unwrap_or((alt_a - ground_a).min(alt_b - ground_b));
    let blocking = sight.blocking.map(|[x, z, alt]| Point3 {
        x,
        z,
        alt: Some(alt),
        alt_agl: None,
    });
    let visible = blocking.is_none();
    let geojson = if q.geojson {
        let mut f = Features::new(terrain)?;
        f.line(
            &[[q.a.x, q.a.z], [q.b.x, q.b.z]],
            props([
                ("visible", json!(visible)),
                ("clearance_m", json!(clearance_m)),
            ]),
        );
        f.point(
            [q.a.x, q.a.z],
            props([("end", json!("a")), ("alt", json!(alt_a))]),
        );
        f.point(
            [q.b.x, q.b.z],
            props([("end", json!("b")), ("alt", json!(alt_b))]),
        );
        if let Some(p) = &blocking {
            f.point(
                [p.x, p.z],
                props([("blocking", json!(true)), ("alt", json!(p.alt))]),
            );
        }
        Some(f.finish())
    } else {
        None
    };
    Ok(VisibleResponse {
        provenance: terrain.provenance(),
        visible,
        blocking,
        clearance_m,
        geojson,
    })
}

#[cfg(test)]
mod tests {
    use super::super::geojson::tests::assert_valid;
    use super::super::tests::synth;
    use super::super::{Terrain, answer};
    use crate::synth::{HILL_A_EAST_M, HILL_A_NORTH_M, ORIGIN_X_M, ORIGIN_Z_M};
    use serde_json::{Value, json};

    /// DCS x and z of a point given north and east of the synthetic grid's
    /// origin.
    fn at(north: f64, east: f64) -> (f64, f64) {
        (north + f64::from(ORIGIN_X_M), east + f64::from(ORIGIN_Z_M))
    }

    /// The least gap between the segment and the closed-form ground, stepped
    /// every meter between the ends, and where it first goes negative.
    fn exact(t: &Terrain, a: (f64, f64, f64), b: (f64, f64, f64)) -> (f64, Option<(f64, f64)>) {
        let Terrain::Synth(s) = t else { panic!() };
        let f = s.form();
        let n = (b.0 - a.0).hypot(b.1 - a.1).ceil() as u64;
        let (mut least, mut first) = (f64::INFINITY, None);
        for k in 1..n {
            let u = k as f64 / n as f64;
            let (x, z) = (a.0 + u * (b.0 - a.0), a.1 + u * (b.1 - a.1));
            let g = f.height(x - f64::from(ORIGIN_X_M), z - f64::from(ORIGIN_Z_M));
            let gap = a.2 + u * (b.2 - a.2) - g;
            least = least.min(gap);
            if gap < 0.0 && first.is_none() {
                first = Some((x, z));
            }
        }
        (least, first)
    }

    fn ask(t: &Terrain, a: (f64, f64), b: (f64, f64), alt: Value) -> Value {
        let end = |p: (f64, f64)| {
            let mut v = json!({"x": p.0, "z": p.1});
            v.as_object_mut()
                .unwrap()
                .extend(alt.as_object().unwrap().clone());
            v
        };
        answer(
            t,
            "visible",
            &json!({"a": end(a), "b": end(b), "geojson": true}),
        )
        .unwrap()
    }

    fn ground(t: &Terrain, (x, z): (f64, f64)) -> f64 {
        t.source().point(x, z).unwrap().unwrap().height
    }

    #[test]
    fn the_hill_hides_one_side_from_the_other() {
        let t = synth();
        let (a, b) = (
            at(HILL_A_NORTH_M - 6000.0, HILL_A_EAST_M),
            at(HILL_A_NORTH_M + 6000.0, HILL_A_EAST_M),
        );
        let v = ask(&t, a, b, json!({"alt_agl": 2}));
        let (least, first) = exact(
            &t,
            (a.0, a.1, ground(&t, a) + 2.0),
            (b.0, b.1, ground(&t, b) + 2.0),
        );
        assert_eq!(v["visible"], false);
        let c = v["clearance_m"].as_f64().unwrap();
        assert!(c < -100.0 && (c - least).abs() < 1.0, "{c} against {least}");
        // The first step in the way is where the line meets the closed-form
        // ground, to within the raster's error: at the foot of the near
        // slope the ground rises so gently that half a meter is a hundred
        // meters along the line, so it is compared in height, not place.
        assert!(first.is_some());
        let p = &v["blocking"];
        let (px, pz) = (p["x"].as_f64().unwrap(), p["z"].as_f64().unwrap());
        assert!(px < at(HILL_A_NORTH_M, 0.0).0 && (pz - a.1).abs() < 1e-6);
        let (alt_a, alt_b) = (ground(&t, a) + 2.0, ground(&t, b) + 2.0);
        let line = alt_a + (px - a.0) / (b.0 - a.0) * (alt_b - alt_a);
        assert!((line - ground(&t, (px, pz))).abs() < 1.0);
        assert!((p["alt"].as_f64().unwrap() - ground(&t, (px, pz))).abs() < 1.0);
        let g = &v["geojson"];
        assert_valid(g);
        assert_eq!(g["features"].as_array().unwrap().len(), 4);
        assert_eq!(g["features"][0]["properties"]["visible"], false);
    }

    #[test]
    fn the_same_line_beside_the_hill_is_clear() {
        let t = synth();
        let east = HILL_A_EAST_M - 6000.0;
        let (a, b) = (
            at(HILL_A_NORTH_M - 6000.0, east),
            at(HILL_A_NORTH_M + 6000.0, east),
        );
        let v = ask(&t, a, b, json!({"alt_agl": 2}));
        let (least, _) = exact(
            &t,
            (a.0, a.1, ground(&t, a) + 2.0),
            (b.0, b.1, ground(&t, b) + 2.0),
        );
        assert_eq!(v["visible"], true);
        assert_eq!(v["blocking"], Value::Null);
        let c = v["clearance_m"].as_f64().unwrap();
        assert!(c > 0.0 && (c - least).abs() < 1.0, "{c} against {least}");
        assert_valid(&v["geojson"]);
        assert_eq!(v["geojson"]["features"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn a_line_grazing_the_top_turns_on_its_clearance() {
        let t = synth();
        let (a, b) = (
            at(HILL_A_NORTH_M - 6000.0, HILL_A_EAST_M),
            at(HILL_A_NORTH_M + 6000.0, HILL_A_EAST_M),
        );
        let top = ground(&t, at(HILL_A_NORTH_M, HILL_A_EAST_M));
        for margin in [-5.0, 5.0] {
            // A level line `margin` above the closed-form top.
            let alt = top + margin;
            let (least, _) = exact(&t, (a.0, a.1, alt), (b.0, b.1, alt));
            let v = ask(&t, a, b, json!({"alt": alt}));
            assert_eq!(v["visible"], margin > 0.0, "{margin}");
            let c = v["clearance_m"].as_f64().unwrap();
            assert!((c - least).abs() < 1.0, "{c} against {least}");
        }
    }

    #[test]
    fn each_bad_end_is_an_issue_naming_it() {
        let t = synth();
        let e = answer(
            &t,
            "visible",
            &json!({"a": {"x": 0, "z": 0}, "b": {"x": 1e9, "z": 0, "alt_agl": 2}}),
        )
        .unwrap_err();
        let got: Vec<_> = e
            .iter()
            .map(|i| (i.code.as_str(), i.path.as_str()))
            .collect();
        assert_eq!(got, [("point3_no_height", "a"), ("off_terrain", "b")]);
    }

    #[test]
    fn ends_a_step_apart_report_the_lower_ends_height() {
        let t = synth();
        let a = at(HILL_A_NORTH_M - 6000.0, HILL_A_EAST_M);
        let b = (a.0 + 10.0, a.1);
        let v = ask(&t, a, b, json!({"alt_agl": 3}));
        assert_eq!(v["visible"], true);
        assert!((v["clearance_m"].as_f64().unwrap() - 3.0).abs() < 1e-9);
    }
}
