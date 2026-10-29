//! `radar_floor`: the lowest height above the ground at which a site sees a
//! target, over a raster about it or at given points.
//!
//! The raster is `viewshed`'s radial sweep, keeping what that sweep already
//! finds: the steepest slope `m` from the site's eye to the ground short of
//! a cell. A target at the cell is seen when the line to it is at least
//! that steep, so the floor is `eye + m × range − ground`, or the ground
//! itself where nothing rises over the line. One sweep, the cost of a
//! viewshed, and `viewshed` at any target height is, but for rounding, the
//! cells whose floor is at or below it. A band is the cells at or below its
//! height, so each band holds every lower one.
//!
//! A point is judged over the mesh's triangles instead, as `visible` judges
//! a sightline, so a point's floor is the least altitude `visible` passes.

use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::json;

use super::geojson::{Features, Ring, props};
use super::sightline::floor as exact_floor;
use super::viewshed::{Sweep, base64, prepare};
use super::{Terrain, outline, refused};
use crate::types::{
    FloorBand, FloorRaster, Issue, Observer, PointFloor, RadarFloorRequest, RadarFloorResponse,
};

/// The most points one request may ask after: each is its own sightline.
const POINTS_MAX: usize = 10_000;

/// The floor over a raster about a site, as one sweep finds it.
pub(super) struct Floor {
    pub site: (f64, f64),
    pub radius: f64,
    pub res: f64,
    pub side: i64,
    pub origin: (f64, f64),
    /// Each cell's floor above its ground, row major, rows running north;
    /// NaN beyond the radius or off the ground.
    pub agl: Vec<f64>,
    /// Each cell's ground under it, NaN where `agl` is.
    pub ground: Vec<f64>,
    /// The cells within the radius that have ground.
    pub cells: u64,
}

impl Floor {
    /// The floor about the site `o` out to `radius`, the two found at `at`
    /// in the request, with the caller's own issues `more`.
    pub fn sweep(
        terrain: &Terrain,
        o: Observer,
        at: [&str; 2],
        radius: f64,
        resolution: Option<f64>,
        more: Vec<Issue>,
    ) -> Result<Floor, Vec<Issue>> {
        let s: Sweep = prepare(terrain, o, at, radius, resolution, more)?;
        let n = (s.side * s.side) as usize;
        let nan = f64::NAN.to_bits();
        let agl: Vec<AtomicU64> = (0..n).map(|_| AtomicU64::new(nan)).collect();
        let ground: Vec<AtomicU64> = (0..n).map(|_| AtomicU64::new(nan)).collect();
        let put = |i: i64, floor: f64, g: f64| {
            agl[i as usize].store(floor.to_bits(), Ordering::Relaxed);
            ground[i as usize].store(g.to_bits(), Ordering::Relaxed);
        };
        let eye = s.eye;
        let (_, cells) = s.sweep(&|i, g| put(i, 0.0, g), &|i, d, g, steepest| {
            put(i, (eye + steepest * d - g).max(0.0), g);
            false
        });
        let load = |v: Vec<AtomicU64>| -> Vec<f64> {
            v.into_iter()
                .map(|a| f64::from_bits(a.into_inner()))
                .collect()
        };
        Ok(Floor {
            site: s.o,
            radius: s.radius,
            res: s.res,
            side: s.side,
            origin: s.origin,
            agl: load(agl),
            ground: load(ground),
            cells,
        })
    }

    /// The floor of cell `(r, c)`, `None` off the raster, beyond the radius
    /// or off the ground.
    pub fn get(&self, r: i64, c: i64) -> Option<f64> {
        if !((0..self.side).contains(&r) && (0..self.side).contains(&c)) {
            return None;
        }
        let v = self.agl[(r * self.side + c) as usize];
        (!v.is_nan()).then_some(v)
    }

    /// The floor of the cell holding `(x, z)`.
    pub fn at(&self, x: f64, z: f64) -> Option<f64> {
        self.get(
            ((x - self.origin.0) / self.res).floor() as i64,
            ((z - self.origin.1) / self.res).floor() as i64,
        )
    }

    /// The share of the counted cells `keep` holds.
    pub fn share(&self, keep: impl Fn(f64) -> bool) -> f64 {
        if self.cells == 0 {
            return 0.0;
        }
        let n = self.agl.iter().filter(|&&v| !v.is_nan() && keep(v)).count();
        n as f64 / self.cells as f64
    }

    /// The outlines of the counted cells whose floor `keep` holds, along
    /// cell edges, simplified to `tolerance`.
    pub fn polygons(&self, keep: impl Fn(f64) -> bool, tolerance: f64) -> Vec<Vec<Ring>> {
        let set = |r: i64, c: i64| self.get(r, c).is_some_and(&keep);
        let corner = |r: f64, c: f64| [self.origin.0 + r * self.res, self.origin.1 + c * self.res];
        outline::polygons(&set, self.side, self.side, &corner, tolerance)
    }
}

/// The issue with heights above ground to judge at, found at `path`, if
/// any is not a finite height of 0 or more.
pub(super) fn check_bands(bands: &[f64], path: &str) -> Option<Issue> {
    (!bands.iter().all(|h| h.is_finite() && *h >= 0.0)).then(|| {
        Issue::new(
            "invalid_request",
            "every band's height must be finite and at least 0",
            path,
        )
    })
}

/// Little-endian `f32`s, base64 encoded.
fn floats(v: impl Iterator<Item = f64>) -> String {
    let bytes: Vec<u8> = v.flat_map(|f| (f as f32).to_le_bytes()).collect();
    base64(&bytes)
}

pub fn radar_floor(
    terrain: &Terrain,
    q: RadarFloorRequest,
) -> Result<RadarFloorResponse, Vec<Issue>> {
    let mut issues = Vec::new();
    issues.extend(check_bands(&q.bands_agl, "bands_agl"));
    if let Some(p) = &q.points {
        if p.len() > POINTS_MAX {
            issues.push(Issue::new(
                "too_large",
                &format!(
                    "{} points, over the {POINTS_MAX} one request may ask after",
                    p.len()
                ),
                "points",
            ));
        }
        if !p.iter().all(|p| p.x.is_finite() && p.z.is_finite()) {
            issues.push(Issue::new(
                "invalid_point",
                "every coordinate must be finite",
                "points",
            ));
        }
    }
    match &q.points {
        Some(points) => {
            let rows = at_points(terrain, &q, points, issues)?;
            let geojson = if q.geojson {
                let mut f = Features::new(terrain)?;
                for p in &rows {
                    f.point(
                        [p.x, p.z],
                        props([
                            ("floor_agl", json!(p.floor_agl)),
                            ("floor_msl", json!(p.floor_msl)),
                        ]),
                    );
                }
                site(&mut f, q.site);
                Some(f.finish())
            } else {
                None
            };
            Ok(RadarFloorResponse {
                provenance: terrain.provenance(),
                raster: None,
                bands: Vec::new(),
                points: Some(rows),
                geojson,
            })
        }
        None => {
            let fl = Floor::sweep(
                terrain,
                q.site,
                ["site", "radius"],
                q.radius,
                q.resolution,
                issues,
            )?;
            let mut heights = q.bands_agl.clone();
            heights.sort_unstable_by(f64::total_cmp);
            heights.dedup();
            let bands = heights
                .iter()
                .map(|&h| FloorBand {
                    alt_agl: h,
                    seen_fraction: fl.share(|v| v <= h),
                })
                .collect();
            let geojson = if q.geojson {
                let mut f = Features::new(terrain)?;
                // Drawn along the cell edges, unsimplified, so that each band
                // lies inside the next as its cells do.
                for &h in &heights {
                    f.polygons(
                        &fl.polygons(|v| v <= h, 0.0),
                        props([("alt_agl", json!(h))]),
                    );
                }
                site(&mut f, q.site);
                Some(f.finish())
            } else {
                None
            };
            let msl = fl.agl.iter().zip(&fl.ground).map(|(a, g)| a + g);
            Ok(RadarFloorResponse {
                provenance: terrain.provenance(),
                raster: Some(FloorRaster {
                    origin_x: fl.origin.0,
                    origin_z: fl.origin.1,
                    cell: fl.res,
                    width: fl.side as u32,
                    height: fl.side as u32,
                    floor_agl: floats(fl.agl.iter().copied()),
                    floor_msl: floats(msl),
                }),
                bands,
                points: None,
                geojson,
            })
        }
    }
}

fn site(f: &mut Features, o: Observer) {
    f.point(
        [o.x, o.z],
        props([("site", json!(true)), ("alt_agl", json!(o.alt_agl))]),
    );
}

/// The site's eye, after the checks a sweep would make of it.
pub(super) fn eye(
    terrain: &Terrain,
    o: Observer,
    path: &str,
    mut issues: Vec<Issue>,
) -> Result<f64, Vec<Issue>> {
    if ![o.x, o.z, o.alt_agl].iter().all(|v| v.is_finite()) {
        issues.insert(
            0,
            Issue::new("invalid_point", "every coordinate must be finite", path),
        );
    }
    if !issues.is_empty() {
        return Err(issues);
    }
    match terrain.source().point(o.x, o.z).map_err(refused)? {
        Some(g) => Ok(g.height + o.alt_agl),
        None => Err(vec![Issue::new(
            "off_terrain",
            "no mesh covers this point, so it has no ground to stand on",
            path,
        )]),
    }
}

/// The floor at a point `p` from the eye `[x, z, alt]`, as an altitude,
/// over the mesh's triangles; infinite when the eye is under its ground.
pub(super) fn point_floor(
    terrain: &Terrain,
    eye: [f64; 3],
    p: [f64; 2],
    ground: f64,
) -> Result<f64, Vec<Issue>> {
    let pieces = terrain
        .source()
        .along([eye[0], eye[1]], p)
        .map_err(refused)?;
    Ok(exact_floor(&pieces, eye[2]).max(ground))
}

fn at_points(
    terrain: &Terrain,
    q: &RadarFloorRequest,
    points: &[crate::types::Point],
    mut issues: Vec<Issue>,
) -> Result<Vec<PointFloor>, Vec<Issue>> {
    if !(q.radius.is_finite() && q.radius > 0.0) {
        issues.push(Issue::new(
            "invalid_request",
            "the radius must be positive",
            "radius",
        ));
    }
    let eye = eye(terrain, q.site, "site", issues)?;
    let src = terrain.source();
    let xz: Vec<[f64; 2]> = points.iter().map(|p| [p.x, p.z]).collect();
    let grounds = src.points(&xz).map_err(refused)?;
    let mut rows = Vec::with_capacity(points.len());
    for (p, g) in xz.iter().zip(grounds) {
        let ground = g.map(|g| g.height);
        let within = (p[0] - q.site.x).hypot(p[1] - q.site.z) <= q.radius;
        let floor = match ground {
            Some(g) if within => Some(point_floor(terrain, [q.site.x, q.site.z, eye], *p, g)?)
                .filter(|f| f.is_finite()),
            _ => None,
        };
        rows.push(PointFloor {
            x: p[0],
            z: p[1],
            ground,
            floor_agl: floor.zip(ground).map(|(f, g)| f - g),
            floor_msl: floor,
        });
    }
    Ok(rows)
}

#[cfg(test)]
pub(super) mod tests {
    use super::super::geojson::tests::assert_valid;
    use super::super::tests::synth;
    use super::super::viewshed::tests::{crosses, rings_back, unbase64};
    use super::super::{Terrain, answer};
    use crate::synth::{HILL_A_EAST_M, HILL_A_NORTH_M, ORIGIN_X_M, ORIGIN_Z_M};
    use serde_json::{Value, json};

    /// DCS x and z of a point given north and east of the synthetic grid's
    /// origin.
    pub fn at(north: f64, east: f64) -> (f64, f64) {
        (north + f64::from(ORIGIN_X_M), east + f64::from(ORIGIN_Z_M))
    }

    /// Six kilometers west of the big hill's top, so the hill shades the
    /// east.
    pub fn site() -> (f64, f64) {
        at(HILL_A_NORTH_M, HILL_A_EAST_M - 6000.0)
    }

    fn ask(t: &Terrain, radius: f64, extra: Value) -> Value {
        let (x, z) = site();
        let mut q = json!({"site": {"x": x, "z": z, "alt_agl": 10}, "radius": radius});
        q.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        answer(t, "radar_floor", &q).unwrap()
    }

    fn floats(s: &str) -> Vec<f32> {
        unbase64(s)
            .chunks(4)
            .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
            .collect()
    }

    /// The floor at `(x, z)` from the closed-form ground, the steepest
    /// slope to it found every 5 m: the geometry the floor must match.
    fn closed_form(t: &Terrain, eye: f64, x: f64, z: f64) -> f64 {
        let Terrain::Synth(s) = t else { panic!() };
        let f = s.form();
        let h = |x: f64, z: f64| f.height(x - f64::from(ORIGIN_X_M), z - f64::from(ORIGIN_Z_M));
        let (ox, oz) = site();
        let d = (x - ox).hypot(z - oz);
        let n = (d / 5.0).ceil() as u64;
        let mut steepest = f64::NEG_INFINITY;
        for k in 1..n {
            let u = k as f64 / n as f64;
            steepest = steepest.max((h(ox + u * (x - ox), oz + u * (z - oz)) - eye) / (u * d));
        }
        (eye + steepest * d - h(x, z)).max(0.0)
    }

    /// The closed-form floor along the radial at `bearing_deg` from the
    /// site, the steepest slope kept every 5 m, at each range a multiple of
    /// 25 m out to `radius`.
    pub fn profile(t: &Terrain, bearing_deg: f64, radius: f64) -> Vec<(f64, f64)> {
        let Terrain::Synth(s) = t else { panic!() };
        let f = s.form();
        let h = |x: f64, z: f64| f.height(x - f64::from(ORIGIN_X_M), z - f64::from(ORIGIN_Z_M));
        let (ox, oz) = site();
        let eye = t.source().point(ox, oz).unwrap().unwrap().height + 10.0;
        let (sin, cos) = bearing_deg.to_radians().sin_cos();
        let (mut steepest, mut out) = (f64::NEG_INFINITY, Vec::new());
        for k in 1..=(radius / 5.0) as u64 {
            let d = k as f64 * 5.0;
            let g = h(ox + d * cos, oz + d * sin);
            if k % 5 == 0 {
                out.push((d, (eye + steepest * d - g).max(0.0)));
            }
            steepest = steepest.max((g - eye) / d);
        }
        out
    }

    #[test]
    fn the_floor_behind_the_hill_matches_its_geometry() {
        let t = synth();
        let v = ask(&t, 12000.0, json!({}));
        let r = &v["raster"];
        let side = r["width"].as_i64().unwrap();
        let cell = r["cell"].as_f64().unwrap();
        let (x0, z0) = (
            r["origin_x"].as_f64().unwrap(),
            r["origin_z"].as_f64().unwrap(),
        );
        let agl = floats(r["floor_agl"].as_str().unwrap());
        let msl = floats(r["floor_msl"].as_str().unwrap());
        assert_eq!(agl.len() as i64, side * side);
        let (ox, oz) = site();
        let eye = t.source().point(ox, oz).unwrap().unwrap().height + 10.0;
        let (mut behind, mut errors) = (0, Vec::new());
        for row in (0..side).step_by(6) {
            for col in (0..side).step_by(6) {
                let i = (row * side + col) as usize;
                let (x, z) = (
                    x0 + (row as f64 + 0.5) * cell,
                    z0 + (col as f64 + 0.5) * cell,
                );
                if (x - ox).hypot(z - oz) > 12000.0 {
                    assert!(agl[i].is_nan() && msl[i].is_nan());
                    continue;
                }
                let want = closed_form(&t, eye, x, z);
                behind += usize::from(want > 50.0);
                errors.push((f64::from(agl[i]) - want).abs());
                assert!(agl[i] >= 0.0 && msl[i] >= agl[i] - 1000.0);
            }
        }
        errors.sort_unstable_by(f64::total_cmp);
        let p = |q: f64| errors[((errors.len() - 1) as f64 * q) as usize];
        // The hill hides a broad shadow, hundreds of meters deep behind its
        // top, and the floor follows the geometry to a few meters, the
        // bilinear raster's error on the hill's flanks.
        assert!(behind > 200, "{behind}");
        assert!(
            p(0.5) < 0.5 && p(0.95) < 2.5 && p(1.0) < 8.0,
            "{} {} {}",
            p(0.5),
            p(0.95),
            p(1.0)
        );
    }

    #[test]
    fn a_viewshed_at_any_height_is_the_cells_at_or_below_the_floor() {
        let t = synth();
        let v = ask(&t, 8000.0, json!({}));
        let agl = floats(v["raster"]["floor_agl"].as_str().unwrap());
        let (x, z) = site();
        for target in [0.0, 2.0, 30.0, 150.0] {
            let w = answer(
                &t,
                "viewshed",
                &json!({"observer": {"x": x, "z": z, "alt_agl": 10}, "radius": 8000,
                        "target_alt_agl": target}),
            )
            .unwrap();
            assert_eq!(w["width"], v["raster"]["width"]);
            let bits = unbase64(w["bits"].as_str().unwrap());
            let mut differ = 0;
            for (i, f) in agl.iter().enumerate() {
                let set = bits[i / 8] & (0x80 >> (i % 8)) != 0;
                // Rounding apart, the two say the same of every cell.
                if set != (!f.is_nan() && f64::from(*f) <= target) {
                    differ += 1;
                }
            }
            assert!(differ <= 2, "{differ} at {target}");
        }
    }

    /// Whether `(x, z)` lies inside the polygons, holes apart, or within
    /// `tol` of an edge of theirs.
    fn inside_or_on(rings: &[(bool, Vec<[f64; 2]>)], x: f64, z: f64, tol: f64) -> bool {
        let near = rings.iter().any(|(_, r)| {
            (0..r.len()).any(|i| {
                let (a, b) = (r[i], r[(i + 1) % r.len()]);
                let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
                let u = (((x - a[0]) * dx + (z - a[1]) * dz) / (dx * dx + dz * dz)).clamp(0.0, 1.0);
                (x - a[0] - u * dx).hypot(z - a[1] - u * dz) <= tol
            })
        });
        near || rings.iter().filter(|(_, r)| crosses(r, x, z)).count() % 2 == 1
    }

    #[test]
    fn the_geojson_bands_nest_each_lower_inside_the_next() {
        let t = synth();
        let v = ask(
            &t,
            9000.0,
            json!({"geojson": true, "bands_agl": [300, 30, 100, 1000, 100]}),
        );
        let g = &v["geojson"];
        assert_valid(g);
        let f = g["features"].as_array().unwrap();
        let heights: Vec<f64> = f[..4]
            .iter()
            .map(|f| f["properties"]["alt_agl"].as_f64().unwrap())
            .collect();
        assert_eq!(heights, [30.0, 100.0, 300.0, 1000.0]);
        assert_eq!(f[4]["geometry"]["type"], "Point");
        let bands = v["bands"].as_array().unwrap();
        let shares: Vec<f64> = bands
            .iter()
            .map(|b| b["seen_fraction"].as_f64().unwrap())
            .collect();
        assert!(shares.windows(2).all(|w| w[0] < w[1]), "{shares:?}");
        let rings: Vec<_> = f[..4]
            .iter()
            .map(|f| rings_back(&t, &f["geometry"]))
            .collect();
        let mut vertices = 0;
        for k in 0..3 {
            let (lower, upper) = (&rings[k], &rings[k + 1]);
            assert!(!lower.is_empty());
            for (_, r) in lower {
                for p in r {
                    // A vertex of a lower band lies in the next band up, or
                    // on its edge to within the centimeter GeoJSON keeps.
                    assert!(inside_or_on(upper, p[0], p[1], 0.05), "band {k} at {p:?}");
                    vertices += 1;
                }
            }
        }
        assert!(vertices > 100, "{vertices}");
    }

    #[test]
    fn a_point_floor_is_the_least_altitude_visible_passes() {
        let t = synth();
        let (sx, sz) = site();
        // Behind the hill, on its flank, beside it and in the open.
        let points: Vec<(f64, f64)> = [
            (0.0, 3000.0),
            (0.0, 5000.0),
            (300.0, 7000.0),
            (-2500.0, 6500.0),
            (4000.0, 8000.0),
            (-1000.0, -3000.0),
        ]
        .iter()
        .map(|&(n, e)| at(HILL_A_NORTH_M + n, HILL_A_EAST_M + e))
        .collect();
        let pts: Vec<Value> = points
            .iter()
            .map(|&(x, z)| json!({"x": x, "z": z}))
            .collect();
        let v = ask(&t, 20000.0, json!({"points": pts, "geojson": true}));
        assert!(v.get("raster").is_none());
        assert_valid(&v["geojson"]);
        let rows = v["points"].as_array().unwrap();
        let mut masked = 0;
        for (row, &(x, z)) in rows.iter().zip(&points) {
            let floor = row["floor_msl"].as_f64().unwrap();
            let agl = row["floor_agl"].as_f64().unwrap();
            assert!(agl >= 0.0 && (floor - agl - row["ground"].as_f64().unwrap()).abs() < 1e-9);
            let seen = |alt: f64| {
                answer(
                    &t,
                    "visible",
                    &json!({"a": {"x": sx, "z": sz, "alt_agl": 10}, "b": {"x": x, "z": z, "alt": alt}}),
                )
                .unwrap()["visible"]
                    == true
            };
            assert!(seen(floor + 0.001), "{x} {z}");
            if agl > 0.1 {
                masked += 1;
                assert!(!seen(floor - 0.01), "{x} {z}");
            }
        }
        assert!(masked >= 3, "{masked}");
        // The raster agrees with the points to the raster's own error.
        let r = &ask(&t, 20000.0, json!({}))["raster"];
        let agl = floats(r["floor_agl"].as_str().unwrap());
        let (side, cell) = (r["width"].as_i64().unwrap(), r["cell"].as_f64().unwrap());
        for (row, &(x, z)) in rows.iter().zip(&points) {
            let i = ((x - r["origin_x"].as_f64().unwrap()) / cell).floor() as i64 * side
                + ((z - r["origin_z"].as_f64().unwrap()) / cell).floor() as i64;
            let want = row["floor_agl"].as_f64().unwrap();
            let got = f64::from(agl[i as usize]);
            assert!((got - want).abs() < 10.0, "{x} {z}: {got} against {want}");
        }
    }

    #[test]
    fn a_point_beyond_the_radius_or_off_the_mesh_has_no_floor() {
        let t = synth();
        let (sx, sz) = site();
        let v = ask(
            &t,
            1000.0,
            json!({"points": [{"x": sx + 5000.0, "z": sz}, {"x": 1e8, "z": 0}, {"x": sx, "z": sz}]}),
        );
        let p = &v["points"];
        assert!(p[0]["floor_agl"].is_null() && p[0]["ground"].is_number());
        assert!(p[1]["floor_agl"].is_null() && p[1]["ground"].is_null());
        assert_eq!(p[2]["floor_agl"], 0.0);
    }

    #[test]
    fn refuses_a_bad_request() {
        let t = synth();
        let (sx, sz) = site();
        let issue = |q: Value| answer(&t, "radar_floor", &q).unwrap_err();
        let e = issue(
            json!({"site": {"x": sx, "z": sz, "alt_agl": 10}, "radius": -1,
                             "bands_agl": [30, -5]}),
        );
        let paths: Vec<_> = e.iter().map(|i| i.path.as_str()).collect();
        assert_eq!(paths, ["radius", "bands_agl"]);
        let many: Vec<Value> = (0..10_001).map(|_| json!({"x": sx, "z": sz})).collect();
        let e = issue(
            json!({"site": {"x": sx, "z": sz, "alt_agl": 10}, "radius": 100,
                             "points": many}),
        );
        assert_eq!(e[0].code, "too_large");
        let e = issue(json!({"site": {"x": 1e8, "z": 0, "alt_agl": 10}, "radius": 100}));
        assert_eq!(
            (e[0].code.as_str(), e[0].path.as_str()),
            ("off_terrain", "site")
        );
    }
}
