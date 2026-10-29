//! `coverage`: where a site sees a target at each of several heights above
//! the ground, and where it does not.
//!
//! Read off the radar floor: a cell is covered at a band when its floor is
//! at or below it, so one sweep answers every band. A blind approach is a
//! radial of the sweep's own spacing, one per cell on the outer ring, that
//! stays masked from the radius in to the least masked range.

use std::f64::consts::TAU;

use serde_json::json;

use super::floor::{Floor, check_bands};
use super::geojson::{Features, props};
use super::spectrum::radial;
use super::{Terrain, resolve};
use crate::types::{CoverageBand, CoverageRequest, CoverageResponse, Issue, Polygon};

pub fn coverage(terrain: &Terrain, q: CoverageRequest) -> Result<CoverageResponse, Vec<Issue>> {
    let mut issues: Vec<Issue> = check_bands(&q.envelope.alt_bands_agl, "envelope.alt_bands_agl")
        .into_iter()
        .collect();
    if !(q.min_masked_range.is_finite() && q.min_masked_range >= 0.0) {
        issues.push(Issue::new(
            "invalid_request",
            "the least masked range must be finite and at least 0",
            "min_masked_range",
        ));
    }
    let asset = match &q.asset {
        Some(r) => match resolve(terrain.source(), r, "asset") {
            Ok(a) => Some(a),
            Err(e) => {
                issues.extend(e);
                None
            }
        },
        None => None,
    };
    let fl = Floor::sweep(
        terrain,
        q.site,
        ["site", "envelope.radius"],
        q.envelope.radius,
        q.resolution,
        issues,
    )?;
    // The asset's cells within the radius that have ground.
    let asset_cells: Option<Vec<f64>> = asset.as_ref().map(|a| {
        let mut v = Vec::new();
        for r in 0..fl.side {
            for c in 0..fl.side {
                let x = fl.origin.0 + (r as f64 + 0.5) * fl.res;
                let z = fl.origin.1 + (c as f64 + 0.5) * fl.res;
                if let Some(f) = fl.get(r, c)
                    && a.contains(x, z)
                {
                    v.push(f);
                }
            }
        }
        v
    });
    let radials = ((TAU * fl.radius / fl.res).ceil() as u64).max(8);
    let polygons: Vec<_> = q
        .envelope
        .alt_bands_agl
        .iter()
        .map(|&h| fl.polygons(|v| v > h, fl.res / 2.0))
        .collect();
    let bands = q
        .envelope
        .alt_bands_agl
        .iter()
        .zip(&polygons)
        .map(|(&h, p)| CoverageBand {
            alt_agl: h,
            covered_fraction: fl.share(|v| v <= h),
            dead_zones: p
                .iter()
                .flatten()
                .map(|r| Polygon { points: r.clone() })
                .collect(),
            asset_covered_fraction: asset_cells.as_ref().map(|v| {
                if v.is_empty() {
                    0.0
                } else {
                    v.iter().filter(|&&f| f <= h).count() as f64 / v.len() as f64
                }
            }),
            blind_approach_exists: (0..radials).any(|k| {
                let b = k as f64 * 360.0 / radials as f64;
                radial(&fl, b, h, q.min_masked_range).blind
            }),
        })
        .collect();
    let geojson = if q.geojson {
        let mut f = Features::new(terrain)?;
        for (&h, p) in q.envelope.alt_bands_agl.iter().zip(&polygons) {
            f.polygons(
                p,
                props([("alt_agl", json!(h)), ("dead_zone", json!(true))]),
            );
        }
        f.point(
            [q.site.x, q.site.z],
            props([("site", json!(true)), ("alt_agl", json!(q.site.alt_agl))]),
        );
        if let Some(a) = &asset {
            f.polygon(&a.outline(), props([("asset", json!(true))]));
        }
        Some(f.finish())
    } else {
        None
    };
    Ok(CoverageResponse {
        provenance: terrain.provenance(),
        bands,
        geojson,
    })
}

#[cfg(test)]
mod tests {
    use super::super::floor::tests::{at, profile, site};
    use super::super::geojson::tests::assert_valid;
    use super::super::tests::synth;
    use super::super::viewshed::tests::crosses;
    use super::super::{Terrain, answer};
    use crate::synth::{HILL_A_EAST_M, HILL_A_NORTH_M};
    use serde_json::{Value, json};

    fn ask(t: &Terrain, extra: Value) -> Value {
        let (x, z) = site();
        let mut q = json!({
            "site": {"x": x, "z": z, "alt_agl": 10},
            "envelope": {"radius": 12000, "alt_bands_agl": [30, 3000]},
            "min_masked_range": 7000,
        });
        q.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        answer(t, "coverage", &q).unwrap()
    }

    #[test]
    fn the_dead_zone_lies_behind_the_hill_as_wide_as_its_geometry() {
        let t = synth();
        let v = ask(&t, json!({"geojson": true}));
        let low = &v["bands"][0];
        assert_eq!(low["alt_agl"], 30.0);
        let rings: Vec<Vec<[f64; 2]>> = low["dead_zones"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| serde_json::from_value(p["points"].clone()).unwrap())
            .collect();
        let dead = |x: f64, z: f64| rings.iter().filter(|r| crosses(r, x, z)).count() % 2 == 1;
        let (sx, sz) = site();
        // Around the arc 10 km out, every quarter degree: where the
        // closed-form ground hides a target 30 m up, and where the dead
        // zones say it is hidden.
        let (mut want, mut got, mut agree, mut n) = (0, 0, 0, 0);
        for k in 0..1440 {
            let b = f64::from(k) / 4.0;
            let p = profile(&t, b, 10000.0);
            let hidden = p.last().unwrap().1 > 30.0;
            let (sin, cos) = b.to_radians().sin_cos();
            let drawn = dead(sx + 10000.0 * cos, sz + 10000.0 * sin);
            if hidden {
                assert!((45.0..135.0).contains(&b), "{b}");
            }
            want += usize::from(hidden);
            got += usize::from(drawn);
            agree += usize::from(hidden == drawn);
            n += 1;
        }
        // Behind the hill, and as wide, to a degree.
        assert!(want > 40, "{want}");
        assert!(
            want.abs_diff(got) <= 4,
            "{got} against {want} quarter degrees"
        );
        assert!(agree >= n - 6, "{agree} of {n}");
        let covered = low["covered_fraction"].as_f64().unwrap();
        assert!((0.5..0.98).contains(&covered), "{covered}");
        assert_eq!(low["blind_approach_exists"], true);
        // High enough, everything is seen and no approach is blind.
        let high = &v["bands"][1];
        assert_eq!(high["covered_fraction"], 1.0);
        assert_eq!(high["dead_zones"], json!([]));
        assert_eq!(high["blind_approach_exists"], false);
        let g = &v["geojson"];
        assert_valid(g);
        let f = g["features"].as_array().unwrap();
        assert_eq!(f.len(), 3);
        assert_eq!(f[0]["properties"]["alt_agl"], 30.0);
        assert_eq!(f[2]["geometry"]["type"], "Point");
    }

    #[test]
    fn an_asset_behind_the_hill_is_dead_and_one_in_the_open_covered() {
        let t = synth();
        let (bx, bz) = at(HILL_A_NORTH_M, HILL_A_EAST_M + 3000.0);
        let (sx, sz) = site();
        for (x, z, want) in [(bx, bz, 0.0), (sx + 1000.0, sz - 3000.0, 1.0)] {
            let v = ask(
                &t,
                json!({"asset": {"Circle": {"x": x, "z": z, "radius": 400}}, "geojson": true}),
            );
            assert_eq!(v["bands"][0]["asset_covered_fraction"], want, "{x} {z}");
            assert_eq!(v["bands"][1]["asset_covered_fraction"], 1.0);
            let f = v["geojson"]["features"].as_array().unwrap();
            assert_eq!(f.last().unwrap()["properties"]["asset"], true);
            assert_valid(&v["geojson"]);
        }
        // Out past the radius, the asset has no cell to cover.
        let v = ask(
            &t,
            json!({"asset": {"Circle": {"x": sx + 50000.0, "z": sz, "radius": 400}}}),
        );
        assert_eq!(v["bands"][0]["asset_covered_fraction"], 0.0);
        let v = ask(&t, json!({}));
        assert!(v["bands"][0].get("asset_covered_fraction").is_none());
    }

    #[test]
    fn refuses_a_bad_band_range_or_asset() {
        let t = synth();
        let (x, z) = site();
        let e = answer(
            &t,
            "coverage",
            &json!({"site": {"x": x, "z": z, "alt_agl": 10},
                    "envelope": {"radius": 0, "alt_bands_agl": [-1]},
                    "min_masked_range": -5,
                    "asset": {"Circle": {"x": 0, "z": 0, "radius": -1}}}),
        )
        .unwrap_err();
        let paths: Vec<_> = e.iter().map(|i| i.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "envelope.radius",
                "envelope.alt_bands_agl",
                "min_masked_range",
                "asset"
            ]
        );
    }
}
