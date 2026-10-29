//! `viewshed`: the cells around an observer at which a target is seen.
//!
//! Radials leave the observer one per mask cell on the outer ring and step
//! the raster every half cell, keeping the steepest angle to the ground so
//! far. A cell takes the radial nearest its center in bearing, and is seen
//! when its target stands at or above that radial's steepest angle short of
//! it: the test `visible` makes of a sightline, swept once for every cell.

use std::f64::consts::TAU;

use serde_json::json;

use super::geojson::{Features, props};
use super::{Terrain, outline, refused};
use crate::grid::Window;
use crate::source::Layer;
use crate::types::{Issue, Polygon, ViewshedRequest, ViewshedResponse};

/// The most cells the mask, or the window of heights under it, may hold:
/// a radius of about 50 km at 50 m, a few hundred milliseconds of sweep.
const CELLS_MAX: u64 = 4 << 20;

fn invalid(code: &str, message: &str, path: &str) -> Issue {
    Issue::new(code, message, path)
}

fn too_large(cells: u64) -> Vec<Issue> {
    vec![Issue::new(
        "too_large",
        &format!(
            "this viewshed reads {cells} cells, over the {CELLS_MAX} one may; shrink the \
             radius or coarsen the resolution"
        ),
        "radius",
    )]
}

/// Standard base64 with padding.
fn base64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = u32::from(c[0]) << 16
            | u32::from(*c.get(1).unwrap_or(&0)) << 8
            | u32::from(*c.get(2).unwrap_or(&0));
        for k in 0..4 {
            if k <= c.len() {
                s.push(char::from(A[(n >> (18 - 6 * k) & 63) as usize]));
            } else {
                s.push('=');
            }
        }
    }
    s
}

pub fn viewshed(terrain: &Terrain, q: ViewshedRequest) -> Result<ViewshedResponse, Vec<Issue>> {
    let src = terrain.source();
    let g = src.grid();
    let cell = f64::from(g.cell_size);
    let o = q.observer;
    let res = q.resolution.unwrap_or(cell);
    let mut issues = Vec::new();
    if ![o.x, o.z, o.alt_agl].iter().all(|v| v.is_finite()) {
        issues.push(invalid(
            "invalid_point",
            "every coordinate must be finite",
            "observer",
        ));
    }
    if !(q.radius.is_finite() && q.radius > 0.0) {
        issues.push(invalid(
            "invalid_request",
            "the radius must be positive",
            "radius",
        ));
    }
    if !q.target_alt_agl.is_finite() {
        issues.push(invalid(
            "invalid_request",
            "the target height must be finite",
            "target_alt_agl",
        ));
    }
    if !(res.is_finite() && res >= cell) {
        issues.push(invalid(
            "invalid_request",
            &format!("the resolution must be at least the cell size, {cell} m"),
            "resolution",
        ));
    }
    if !issues.is_empty() {
        return Err(issues);
    }

    // The mask, aligned to multiples of its cell from the grid's origin, so
    // at the cell size its cells are the grid's.
    let n = (q.radius / res).ceil();
    let side = 2.0 * n + 1.0;
    if side * side > CELLS_MAX as f64 {
        return Err(too_large((side * side) as u64));
    }
    let (n, side) = (n as i64, side as i64);
    let row0 = ((o.x - g.origin_x) / res).floor() as i64 - n;
    let col0 = ((o.z - g.origin_z) / res).floor() as i64 - n;
    let (origin_x, origin_z) = (
        g.origin_x + row0 as f64 * res,
        g.origin_z + col0 as f64 * res,
    );

    // The heights out to the radius and a cell beyond, for the bilinear.
    let reach = q.radius + 2.0 * cell;
    let index = |v: f64, origin: f64| ((v - origin) / cell).floor() as i64;
    let (r_lo, r_hi) = (
        index(o.x - reach, g.origin_x),
        index(o.x + reach, g.origin_x),
    );
    let (c_lo, c_hi) = (
        index(o.z - reach, g.origin_z),
        index(o.z + reach, g.origin_z),
    );
    let (rows, cols) = ((r_hi - r_lo + 1) as u64, (c_hi - c_lo + 1) as u64);
    if rows * cols > CELLS_MAX {
        return Err(too_large(rows * cols));
    }

    let Some(ground) = src.point(o.x, o.z).map_err(refused)? else {
        return Err(vec![Issue::new(
            "off_terrain",
            "no mesh covers this point, so it has no ground to stand on",
            "observer",
        )]);
    };
    let eye = ground.height + o.alt_agl;
    let w =
        Window::read(src, Layer::Height, r_lo, c_lo, rows as u32, cols as u32).map_err(refused)?;
    let height = |x: f64, z: f64| w.value_at(&g, x, z);

    let step = cell / 2.0;
    let radials = ((TAU * q.radius / res).ceil() as u64).max(8);
    let bearing = TAU / radials as f64;
    let mut bits = vec![0u8; ((side * side) as usize).div_ceil(8)];
    let mut set = |i: i64| bits[(i / 8) as usize] |= 0x80 >> (i % 8);
    let centre = |r: i64, c: i64| {
        (
            origin_x + (r as f64 + 0.5) * res,
            origin_z + (c as f64 + 0.5) * res,
        )
    };

    // Each cell within the radius that has ground, keyed by its radial and
    // the last step short of it, so each radial is swept once in order.
    let (mut cells, mut seen) = (Vec::new(), 0u64);
    for r in 0..side {
        for c in 0..side {
            let (x, z) = centre(r, c);
            let (dx, dz) = (x - o.x, z - o.z);
            let d = dx.hypot(dz);
            if d > q.radius || height(x, z).is_none() {
                continue;
            }
            let i = r * side + c;
            // Too close for a step between: nothing stands in the way.
            let last = (d / step).ceil() as u64 - 1;
            if last == 0 {
                set(i);
                seen += 1;
                continue;
            }
            let k = (dz.atan2(dx).rem_euclid(TAU) / bearing).round() as u64 % radials;
            cells.push((k << 44) | (last << 24) | i as u64);
        }
    }
    cells.sort_unstable();
    let total = cells.len() as u64 + seen;

    let mut at = 0;
    while at < cells.len() {
        let k = cells[at] >> 44;
        let (sin, cos) = (k as f64 * bearing).sin_cos();
        let (mut steepest, mut done) = (f64::NEG_INFINITY, 0u64);
        while at < cells.len() && cells[at] >> 44 == k {
            let (last, i) = ((cells[at] >> 24) & 0xFFFFF, (cells[at] & 0xFFFFFF) as i64);
            while done < last {
                done += 1;
                let d = done as f64 * step;
                if let Some(h) = height(o.x + d * cos, o.z + d * sin) {
                    steepest = steepest.max((h - eye) / d);
                }
            }
            let (x, z) = centre(i / side, i % side);
            let d = (x - o.x).hypot(z - o.z);
            let target = height(x, z).expect("kept for its ground") + q.target_alt_agl;
            if (target - eye) / d >= steepest {
                set(i);
                seen += 1;
            }
            at += 1;
        }
    }

    let polygons = (q.as_polygons || q.geojson).then(|| {
        let lit = |r: i64, c: i64| {
            let i = r * side + c;
            (0..side).contains(&r)
                && (0..side).contains(&c)
                && bits[(i / 8) as usize] & (0x80 >> (i % 8)) != 0
        };
        let corner = |r: f64, c: f64| [origin_x + r * res, origin_z + c * res];
        outline::polygons(&lit, side, side, &corner, res / 2.0)
    });
    let rings = q.as_polygons.then(|| {
        polygons
            .iter()
            .flatten()
            .flatten()
            .map(|r| Polygon { points: r.clone() })
            .collect()
    });
    let geojson = match &polygons {
        Some(p) if q.geojson => {
            let mut f = Features::new(terrain)?;
            f.polygons(
                p,
                props([
                    ("observer", json!(o)),
                    ("radius", json!(q.radius)),
                    ("target_alt_agl", json!(q.target_alt_agl)),
                ]),
            );
            f.point([o.x, o.z], props([("alt_agl", json!(o.alt_agl))]));
            Some(f.finish())
        }
        _ => None,
    };
    Ok(ViewshedResponse {
        provenance: terrain.provenance(),
        origin_x,
        origin_z,
        cell: res,
        width: side as u32,
        height: side as u32,
        bits: base64(&bits),
        visible_fraction: if total == 0 {
            0.0
        } else {
            seen as f64 / total as f64
        },
        rings,
        geojson,
    })
}

#[cfg(test)]
mod tests {
    use super::super::geojson::signed_area;
    use super::super::geojson::tests::assert_valid;
    use super::super::sightline::{Heights, walk};
    use super::super::tests::synth;
    use super::super::{Terrain, answer};
    use crate::synth::{HILL_A_EAST_M, HILL_A_NORTH_M, ORIGIN_X_M, ORIGIN_Z_M};
    use serde_json::{Value, json};

    /// North of the big hill, on its flank, so the hill shades the south.
    fn observer() -> (f64, f64) {
        (
            HILL_A_NORTH_M + 2500.0 + f64::from(ORIGIN_X_M),
            HILL_A_EAST_M + f64::from(ORIGIN_Z_M),
        )
    }

    fn ask(t: &Terrain, radius: f64, extra: Value) -> Value {
        let (x, z) = observer();
        let mut q = json!({
            "observer": {"x": x, "z": z, "alt_agl": 2},
            "radius": radius,
            "target_alt_agl": 2,
        });
        q.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        answer(t, "viewshed", &q).unwrap()
    }

    fn unbase64(s: &str) -> Vec<u8> {
        let a = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = Vec::new();
        for c in s.as_bytes().chunks(4) {
            let v: Vec<u32> = c
                .iter()
                .filter(|&&b| b != b'=')
                .map(|&b| a.find(char::from(b)).unwrap() as u32)
                .collect();
            let n = v.iter().fold(0, |n, &d| n << 6 | d) << (6 * (4 - v.len()));
            for k in 0..v.len() - 1 {
                out.push((n >> (16 - 8 * k)) as u8);
            }
        }
        out
    }

    /// The mask a response carries.
    struct Mask {
        side: i64,
        cell: f64,
        origin: (f64, f64),
        bits: Vec<u8>,
    }

    impl Mask {
        fn of(v: &Value) -> Mask {
            assert_eq!(v["width"], v["height"]);
            Mask {
                side: v["width"].as_i64().unwrap(),
                cell: v["cell"].as_f64().unwrap(),
                origin: (
                    v["origin_x"].as_f64().unwrap(),
                    v["origin_z"].as_f64().unwrap(),
                ),
                bits: unbase64(v["bits"].as_str().unwrap()),
            }
        }

        fn get(&self, r: i64, c: i64) -> bool {
            let i = r * self.side + c;
            (0..self.side).contains(&r)
                && (0..self.side).contains(&c)
                && self.bits[(i / 8) as usize] & (0x80 >> (i % 8)) != 0
        }

        fn centre(&self, r: i64, c: i64) -> (f64, f64) {
            (
                self.origin.0 + (r as f64 + 0.5) * self.cell,
                self.origin.1 + (c as f64 + 0.5) * self.cell,
            )
        }
    }

    #[test]
    fn the_visible_share_is_what_a_sightline_to_every_cell_finds() {
        let t = synth();
        let radius = 4000.0;
        let v = ask(&t, radius, json!({}));
        let m = Mask::of(&v);
        let src = t.source();
        let (ox, oz) = observer();
        let eye = src.point(ox, oz).unwrap().unwrap().height + 2.0;
        let mut h = Heights::new(src);
        let (mut total, mut seen, mut differ) = (0, 0, 0);
        for r in 0..m.side {
            for c in 0..m.side {
                let (x, z) = m.centre(r, c);
                if (x - ox).hypot(z - oz) > radius {
                    assert!(!m.get(r, c));
                    continue;
                }
                let g = h.at(x, z).unwrap().unwrap();
                let s = walk(&mut h, [ox, oz, eye], [x, z, g + 2.0], 25.0).unwrap();
                let brute = s.blocking.is_none();
                total += 1;
                seen += usize::from(brute);
                differ += usize::from(brute != m.get(r, c));
            }
        }
        let brute = seen as f64 / total as f64;
        let got = v["visible_fraction"].as_f64().unwrap();
        // The hill hides a real share, and the sweep finds the same share.
        assert!((0.2..0.9).contains(&brute), "{brute}");
        assert!((got - brute).abs() < 0.005, "{got} against {brute}");
        assert!((differ as f64) < 0.01 * total as f64, "{differ} of {total}");
    }

    /// Each ring of a `MultiPolygon` back in DCS x and z, open, with
    /// whether it is a hole.
    fn rings_back(t: &Terrain, poly: &Value) -> Vec<(bool, Vec<[f64; 2]>)> {
        let tm = t.source().tables().unwrap().crs.tmerc();
        let mut rings = Vec::new();
        for p in poly["coordinates"].as_array().unwrap() {
            for (i, ring) in p.as_array().unwrap().iter().enumerate() {
                let mut pts: Vec<[f64; 2]> = ring
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|q| {
                        let (x, z) = tm.to_xz(q[1].as_f64().unwrap(), q[0].as_f64().unwrap());
                        [x, z]
                    })
                    .collect();
                pts.pop();
                rings.push((i > 0, pts));
            }
        }
        rings
    }

    fn crosses(r: &[[f64; 2]], x: f64, z: f64) -> bool {
        let mut odd = false;
        for i in 0..r.len() {
            let (a, b) = (r[i], r[(i + 1) % r.len()]);
            if (a[1] > z) != (b[1] > z) && x < a[0] + (z - a[1]) / (b[1] - a[1]) * (b[0] - a[0]) {
                odd = !odd;
            }
        }
        odd
    }

    #[test]
    fn the_geojson_encloses_the_visible_cells() {
        let t = synth();
        let v = ask(&t, 3000.0, json!({"geojson": true, "as_polygons": true}));
        let g = &v["geojson"];
        assert_valid(g);
        let m = Mask::of(&v);
        let f = &g["features"][0];
        assert_eq!(f["geometry"]["type"], "MultiPolygon");
        assert_eq!(f["properties"]["radius"], 3000.0);
        assert_eq!(g["features"][1]["geometry"]["type"], "Point");
        let rings = rings_back(&t, &f["geometry"]);
        assert_eq!(v["rings"].as_array().unwrap().len(), rings.len());
        let area: f64 = rings
            .iter()
            .map(|(hole, r)| {
                let a = signed_area(r).abs() / 2.0;
                if *hole { -a } else { a }
            })
            .sum();
        let inside = |x, z| rings.iter().filter(|(_, r)| crosses(r, x, z)).count() % 2 == 1;
        let (mut cells, mut edges) = (0.0, 0.0);
        for r in -1..=m.side {
            for c in -1..=m.side {
                let lit = m.get(r, c);
                cells += f64::from(u8::from(lit));
                for (dr, dc) in [(1, 0), (0, 1)] {
                    edges += f64::from(u8::from(lit != m.get(r + dr, c + dc)));
                }
                // Away from the outline, a cell's center is inside it
                // exactly when the cell is set.
                let near = (-1..=1)
                    .flat_map(|dr| (-1..=1).map(move |dc| (dr, dc)))
                    .any(|(dr, dc)| m.get(r + dr, c + dc) != lit);
                if !near {
                    let (x, z) = m.centre(r, c);
                    assert_eq!(inside(x, z), lit, "cell ({r}, {c})");
                }
            }
        }
        // The area projected back is the set cells' to within one cell
        // along every edge of the outline.
        let cell_area = m.cell * m.cell;
        assert!(cells > 100.0 && edges > 0.0);
        assert!(
            (area - cells * cell_area).abs() <= edges * cell_area,
            "{area} against {} with {edges} edges",
            cells * cell_area
        );
    }

    #[test]
    fn a_plane_is_seen_whole_and_a_coarser_mask_agrees() {
        let t = synth();
        let (x, z) = (
            f64::from(ORIGIN_X_M) + 30000.0,
            f64::from(ORIGIN_Z_M) + 60000.0,
        );
        let q = |res: f64| {
            json!({
                "observer": {"x": x, "z": z, "alt_agl": 2},
                "radius": 2000, "target_alt_agl": 2, "resolution": res,
            })
        };
        assert_eq!(
            answer(&t, "viewshed", &q(50.0)).unwrap()["visible_fraction"],
            1.0
        );
        let coarse = answer(&t, "viewshed", &q(200.0)).unwrap();
        assert_eq!(coarse["cell"], 200.0);
        assert_eq!(coarse["visible_fraction"], 1.0);
        let share = |v: Value| v["visible_fraction"].as_f64().unwrap();
        let fine = share(ask(&t, 4000.0, json!({})));
        let coarse = share(ask(&t, 4000.0, json!({"resolution": 100})));
        assert!((fine - coarse).abs() < 0.02, "{fine} against {coarse}");
    }

    #[test]
    fn refuses_a_viewshed_too_large_and_a_bad_request() {
        let t = synth();
        let observer = |x: f64| json!({"x": x, "z": 0, "alt_agl": 2});
        let e = answer(
            &t,
            "viewshed",
            &json!({"observer": observer(0.0), "radius": 200000, "target_alt_agl": 0}),
        )
        .unwrap_err();
        assert_eq!(
            (e[0].code.as_str(), e[0].path.as_str()),
            ("too_large", "radius")
        );
        let e = answer(
            &t,
            "viewshed",
            &json!({"observer": observer(0.0), "radius": -1, "target_alt_agl": 0,
                    "resolution": 10}),
        )
        .unwrap_err();
        let paths: Vec<_> = e.iter().map(|i| i.path.as_str()).collect();
        assert_eq!(paths, ["radius", "resolution"]);
        let e = answer(
            &t,
            "viewshed",
            &json!({"observer": observer(1e8), "radius": 1000, "target_alt_agl": 0}),
        )
        .unwrap_err();
        assert_eq!(e[0].code, "off_terrain");
    }

    #[test]
    fn base64_is_the_standard_alphabet_padded() {
        assert_eq!(super::base64(b""), "");
        assert_eq!(super::base64(b"f"), "Zg==");
        assert_eq!(super::base64(b"fo"), "Zm8=");
        assert_eq!(super::base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(unbase64("Zm9vYg=="), b"foob");
    }
}
