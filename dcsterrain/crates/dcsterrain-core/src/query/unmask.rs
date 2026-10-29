//! `unmask_profile`: where along a route a site first sees what flies it,
//! and the stretches it does not.
//!
//! The route is sampled every `step` from its first point, and its last
//! point too. Each sample is judged against its radar floor, found over the
//! mesh's triangles as `radar_floor` finds a point's, so a sample is seen
//! exactly when `visible` would see it from the site.

use serde_json::json;

use super::floor::{eye, point_floor};
use super::geojson::{Features, props};
use super::{Terrain, refused};
use crate::types::{Issue, Point3, RouteSpan, UnmaskProfileRequest, UnmaskProfileResponse};

/// The most samples one route may take: each is its own sightline.
const SAMPLES_MAX: f64 = 100_000.0;

/// How far under its floor a sample may be and still be seen, as `visible`
/// lets a line pass that close over the ground.
const TOLERANCE_M: f64 = 1e-4;

/// A route's point, its altitude as it was given.
#[derive(Clone, Copy)]
enum Alt {
    Msl(f64),
    Agl(f64),
}

pub fn unmask_profile(
    terrain: &Terrain,
    q: UnmaskProfileRequest,
) -> Result<UnmaskProfileResponse, Vec<Issue>> {
    let src = terrain.source();
    let mut issues = Vec::new();
    if q.route.len() < 2 {
        issues.push(Issue::new(
            "invalid_request",
            "a route needs 2 or more points",
            "route",
        ));
    }
    for (i, p) in q.route.iter().enumerate() {
        let path = format!("route[{i}]");
        if let Some(e) = p.check(&path) {
            issues.push(e);
        } else if ![Some(p.x), Some(p.z), p.alt, p.alt_agl]
            .into_iter()
            .flatten()
            .all(f64::is_finite)
        {
            issues.push(Issue::new(
                "invalid_point",
                "every coordinate must be finite",
                &path,
            ));
        }
    }
    let length: f64 = q
        .route
        .windows(2)
        .map(|w| (w[1].x - w[0].x).hypot(w[1].z - w[0].z))
        .sum();
    if !(q.step.is_finite() && q.step > 0.0) {
        issues.push(Issue::new(
            "invalid_request",
            "the step must be positive",
            "step",
        ));
    } else if length / q.step > SAMPLES_MAX {
        issues.push(Issue::new(
            "too_large",
            &format!(
                "a route of {length:.0} m at {} m takes over {SAMPLES_MAX} samples; lengthen the step",
                q.step
            ),
            "step",
        ));
    }
    let o = q.site;
    let eye = eye(terrain, o, "site", issues)?;

    // Every sample: where it is, how far along, and its altitude between
    // the two route points it lies between.
    // Each sample: where it is, how far along, the route point it follows
    // and how far towards the next.
    let mut samples: Vec<([f64; 2], f64, usize, f64)> = Vec::new();
    let mut along = 0.0;
    let mut next = 0.0;
    for (i, w) in q.route.windows(2).enumerate() {
        let (a, b) = (&w[0], &w[1]);
        let len = (b.x - a.x).hypot(b.z - a.z);
        while next < along + len || (next == 0.0 && len == 0.0) {
            let u = if len > 0.0 { (next - along) / len } else { 0.0 };
            let at = [a.x + u * (b.x - a.x), a.z + u * (b.z - a.z)];
            samples.push((at, next, i, u));
            next += q.step;
        }
        along += len;
    }
    let last = &q.route[q.route.len() - 1];
    samples.push(([last.x, last.z], length, q.route.len() - 2, 1.0));

    // The ground under every route point and every sample, read block by
    // block at once.
    let ends: Vec<[f64; 2]> = q.route.iter().map(|p| [p.x, p.z]).collect();
    let mut wanted = ends.clone();
    wanted.extend(samples.iter().map(|s| s.0));
    let ground: Vec<Option<f64>> = src
        .points(&wanted)
        .map_err(refused)?
        .into_iter()
        .map(|g| g.map(|g| g.height))
        .collect();
    let mut off = Vec::new();
    for (i, p) in q.route.iter().enumerate() {
        if p.alt_agl.is_some() && ground[i].is_none() {
            off.push(Issue::new(
                "off_terrain",
                "no mesh covers this point, so a height above ground means nothing",
                &format!("route[{i}]"),
            ));
        }
    }
    if !off.is_empty() {
        return Err(off);
    }

    // Each judged sample: how far along, where, its altitude, and whether
    // the site sees it.
    let mut judged: Vec<(f64, [f64; 2], f64, bool)> = Vec::new();
    let given = |i: usize| {
        let p = &q.route[i];
        p.alt.map_or(Alt::Agl(p.alt_agl.unwrap_or(0.0)), Alt::Msl)
    };
    for (k, &(at, s, i, u)) in samples.iter().enumerate() {
        let g = ground[ends.len() + k];
        // Between two heights above ground, the height above the ground
        // under the sample; else the altitude between the two ends'.
        let alt = match (given(i), given(i + 1)) {
            (Alt::Agl(a), Alt::Agl(b)) => match g {
                Some(g) => g + a + u * (b - a),
                // Off the mesh a height above ground cannot be placed.
                None => continue,
            },
            (a, b) => {
                let msl = |v: Alt, gv: Option<f64>| match v {
                    Alt::Msl(m) => m,
                    Alt::Agl(h) => gv.expect("an end above ground has ground") + h,
                };
                let (a, b) = (msl(a, ground[i]), msl(b, ground[i + 1]));
                a + u * (b - a)
            }
        };
        let (at, s) = (&at, &s);
        let floor = point_floor(
            terrain,
            [o.x, o.z, eye],
            *at,
            g.unwrap_or(f64::NEG_INFINITY),
        )?;
        judged.push((*s, *at, alt, alt + TOLERANCE_M >= floor));
    }

    let first = judged.iter().find(|j| j.3);
    let mut masked_segments: Vec<RouteSpan> = Vec::new();
    let mut runs: Vec<Vec<[f64; 2]>> = Vec::new();
    for (i, j) in judged.iter().enumerate() {
        if j.3 {
            continue;
        }
        if i > 0 && !judged[i - 1].3 {
            masked_segments.last_mut().expect("a run is open").to = j.0;
            runs.last_mut().expect("a run is open").push(j.1);
        } else {
            masked_segments.push(RouteSpan { from: j.0, to: j.0 });
            runs.push(vec![j.1]);
        }
    }
    let seen = judged.iter().filter(|j| j.3).count();
    let first_seen = first.map(|j| Point3 {
        x: j.1[0],
        z: j.1[1],
        alt: Some(j.2),
        alt_agl: None,
    });
    let geojson = if q.geojson {
        let mut f = Features::new(terrain)?;
        f.line(&ends, props([("route", json!(true))]));
        for (span, run) in masked_segments.iter().zip(&runs) {
            let p = props([
                ("masked", json!(true)),
                ("from", json!(span.from)),
                ("to", json!(span.to)),
            ]);
            if run.len() > 1 {
                f.line(run, p);
            } else {
                f.point(run[0], p);
            }
        }
        if let Some(j) = first {
            f.point(
                j.1,
                props([("first_seen", json!(true)), ("alt", json!(j.2))]),
            );
        }
        f.point(
            [o.x, o.z],
            props([("site", json!(true)), ("alt_agl", json!(o.alt_agl))]),
        );
        Some(f.finish())
    } else {
        None
    };
    Ok(UnmaskProfileResponse {
        provenance: terrain.provenance(),
        first_seen_dist_from_site: first.map(|j| (j.1[0] - o.x).hypot(j.1[1] - o.z)),
        first_seen_dist_along_route: first.map(|j| j.0),
        first_seen,
        masked_segments,
        visible_fraction: if judged.is_empty() {
            0.0
        } else {
            seen as f64 / judged.len() as f64
        },
        geojson,
    })
}

#[cfg(test)]
mod tests {
    use super::super::floor::tests::{at, profile, site};
    use super::super::geojson::tests::assert_valid;
    use super::super::tests::synth;
    use super::super::{Terrain, answer};
    use crate::synth::{HILL_A_EAST_M, HILL_A_NORTH_M};
    use serde_json::{Value, json};

    fn ask(t: &Terrain, route: Value, extra: Value) -> Result<Value, Vec<crate::types::Issue>> {
        let (x, z) = site();
        let mut q = json!({"site": {"x": x, "z": z, "alt_agl": 10}, "route": route});
        q.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        answer(t, "unmask_profile", &q)
    }

    #[test]
    fn a_route_over_the_hill_is_first_seen_where_its_geometry_says() {
        let t = synth();
        let (sx, sz) = site();
        // From 12 km east of the site, behind the hill, straight in to a
        // kilometer short of it, 30 m above the ground.
        let (fx, fz) = at(HILL_A_NORTH_M, HILL_A_EAST_M + 6000.0);
        let route = json!([
            {"x": fx, "z": fz, "alt_agl": 30},
            {"x": sx, "z": sz + 1000.0, "alt_agl": 30},
        ]);
        let v = ask(&t, route, json!({"step": 50, "geojson": true})).unwrap();
        // The geometry: coming in along the radial due east, the farthest
        // range at which the closed-form floor is at or below 30 m.
        let p = profile(&t, 90.0, 12000.0);
        let want = p
            .iter()
            .rev()
            .find(|(d, f)| *d >= 1000.0 && *f <= 30.0)
            .unwrap()
            .0;
        let got = v["first_seen_dist_from_site"].as_f64().unwrap();
        assert!((got - want).abs() <= 50.0, "{got} against {want}");
        // Just past the top of the hill, 6 km out, the target clears it.
        assert!((6000.0..6500.0).contains(&want), "{want}");
        let along = v["first_seen_dist_along_route"].as_f64().unwrap();
        assert!((along + got - 12000.0).abs() < 1e-6, "{along} {got}");
        // Masked from the start to where it is first seen.
        let spans = v["masked_segments"].as_array().unwrap();
        assert_eq!(spans[0]["from"], 0.0);
        assert!((spans[0]["to"].as_f64().unwrap() - (along - 50.0)).abs() < 1e-6);
        let alt = v["first_seen"]["alt"].as_f64().unwrap();
        let ground = t.source().point(sx, sz + got).unwrap().unwrap().height;
        assert!((alt - ground - 30.0).abs() < 1.0, "{alt} {ground}");
        let fraction = v["visible_fraction"].as_f64().unwrap();
        assert!((0.2..0.8).contains(&fraction), "{fraction}");
        let g = &v["geojson"];
        assert_valid(g);
        assert_eq!(g["features"][0]["geometry"]["type"], "LineString");
    }

    #[test]
    fn a_route_high_above_the_hill_is_seen_all_along() {
        let t = synth();
        let (fx, fz) = at(HILL_A_NORTH_M, HILL_A_EAST_M + 6000.0);
        let (mx, mz) = at(HILL_A_NORTH_M + 2000.0, HILL_A_EAST_M);
        let v = ask(
            &t,
            json!([{"x": fx, "z": fz, "alt": 3000}, {"x": mx, "z": mz, "alt": 2500},
                   {"x": mx - 3000.0, "z": mz, "alt_agl": 1500}]),
            json!({}),
        )
        .unwrap();
        assert_eq!(v["visible_fraction"], 1.0);
        assert_eq!(v["masked_segments"], json!([]));
        assert_eq!(v["first_seen_dist_along_route"], 0.0);
        assert_eq!(v["first_seen"]["alt"], 3000.0);
    }

    #[test]
    fn refuses_a_short_route_a_bad_step_and_two_heights() {
        let t = synth();
        let (sx, sz) = site();
        let e = ask(
            &t,
            json!([{"x": sx, "z": sz, "alt": 3, "alt_agl": 3}]),
            json!({"step": 0}),
        )
        .unwrap_err();
        let paths: Vec<_> = e.iter().map(|i| i.path.as_str()).collect();
        assert_eq!(paths, ["route", "route[0]", "step"]);
        let e = ask(
            &t,
            json!([{"x": sx, "z": sz, "alt": 3}, {"x": 1e8, "z": 0, "alt_agl": 3}]),
            json!({"step": 1e9}),
        )
        .unwrap_err();
        assert_eq!(
            (e[0].code.as_str(), e[0].path.as_str()),
            ("off_terrain", "route[1]")
        );
        let e = ask(
            &t,
            json!([{"x": sx, "z": sz, "alt": 3}, {"x": sx + 1e6, "z": sz, "alt": 3}]),
            json!({"step": 1}),
        )
        .unwrap_err();
        assert_eq!(e[0].code, "too_large");
    }
}
