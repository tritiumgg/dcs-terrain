//! `exposure`: the radar floors of many sites at once, over a region.
//!
//! A multiple viewshed, kept as two reductions over the sites' floors: the
//! least floor, the lowest height above the ground at which any site sees a
//! target (GIS calls it the above-ground-level output of a visibility
//! analysis), and the count of sites that see a target at one height (the
//! frequency, or cumulative, viewshed). Each site is one radial sweep, the
//! radar floor's; a site whose radius misses the region is never swept.
//! The sites' rasters share the region's lattice, every one aligned to
//! multiples of its cell from the grid's origin, so a site's cell is the
//! region's cell at a whole offset and the reductions are exact: the least
//! floor is one site's floor bit for bit, and the count is what each
//! site's floor says at the target height. Both are the same in any order
//! of sites or threads.
//!
//! The error is the floors' own. The least of floors each within `e` of
//! the truth is itself within `e`, and a site is counted wrongly only where
//! its floor's error is larger than the floor's distance from the target
//! height.

use rayon::prelude::*;
use serde_json::json;

use super::floor::{Floor, check_bands, floats, site};
use super::geojson::{Features, props};
use super::viewshed::{CELLS_MAX, base64, check_reach, check_resolution, site_eye, too_large};
use super::{Area, Terrain, refused, resolve};
use crate::grid::Window;
use crate::source::Layer;
use crate::types::{
    BoundingBox, ExposureBand, ExposureRaster, ExposureRequest, ExposureResponse, Issue, Region,
};

/// The most sites one request may hold: each is a sweep of its own, and
/// the count of them is kept in a byte.
const SITES_MAX: usize = 64;

/// The least floor and the count over a region's raster.
struct Reduced {
    origin: (f64, f64),
    res: f64,
    rows: i64,
    cols: i64,
    /// Each cell's least floor above its ground, row major, rows running
    /// north: infinite where no site reaches, NaN off the ground or outside
    /// the region.
    agl: Vec<f64>,
    /// Each cell's ground, NaN where it has none or lies outside the
    /// region.
    ground: Vec<f64>,
    /// How many sites see a target at the target height above the cell.
    seen: Vec<u8>,
}

impl Reduced {
    /// The share of the region's cells with ground whose least floor
    /// `keep` holds.
    fn share(&self, keep: impl Fn(f64) -> bool) -> f64 {
        let counted = self.agl.iter().filter(|v| !v.is_nan()).count();
        if counted == 0 {
            return 0.0;
        }
        let n = self.agl.iter().filter(|&&v| !v.is_nan() && keep(v)).count();
        n as f64 / counted as f64
    }
}

pub fn exposure(terrain: &Terrain, q: ExposureRequest) -> Result<ExposureResponse, Vec<Issue>> {
    let src = terrain.source();
    let mut issues: Vec<Issue> = check_bands(&[q.target_alt_agl], "target_alt_agl")
        .into_iter()
        .collect();
    issues.extend(check_bands(&q.bands_agl, "bands_agl"));
    issues.extend(check_resolution(src, q.resolution));
    if q.sites.is_empty() {
        issues.push(Issue::new(
            "invalid_request",
            "name at least one site",
            "sites",
        ));
    }
    if q.sites.len() > SITES_MAX {
        issues.push(Issue::new(
            "too_large",
            &format!(
                "{} sites, over the {SITES_MAX} one request may hold",
                q.sites.len()
            ),
            "sites",
        ));
        return Err(issues);
    }
    // Every site is checked before any is swept, so the request's issues
    // come back together.
    for (i, s) in q.sites.iter().enumerate() {
        let radius = check_reach(src, s.radius, None, &format!("sites.{i}.radius"));
        if let Err(e) = site_eye(terrain, s.observer, &format!("sites.{i}.observer"), radius) {
            issues.extend(e);
        }
    }
    let region = match &q.region {
        Some(r) => resolve(src, r, "region"),
        None if issues.is_empty() => resolve(src, &disks(&q), "region"),
        None => Err(vec![]),
    };
    let area = match region {
        Ok(a) => Some(a),
        Err(e) => {
            issues.extend(e);
            None
        }
    };
    if !issues.is_empty() {
        return Err(issues);
    }
    let r = reduce(terrain, &q, &area.expect("no issues, so a region"))?;

    let mut heights = q.bands_agl.clone();
    heights.sort_unstable_by(f64::total_cmp);
    heights.dedup();
    let bands = heights
        .iter()
        .map(|&h| ExposureBand {
            alt_agl: h,
            unseen_fraction: r.share(|v| v > h),
        })
        .collect();
    let geojson = if q.geojson {
        let mut f = Features::new(terrain)?;
        let corner = |a: f64, b: f64| [r.origin.0 + a * r.res, r.origin.1 + b * r.res];
        // Drawn along the cell edges, unsimplified, so that the area unseen
        // at each band lies inside the area unseen at every lower one.
        for &h in &heights {
            let set = |row: i64, col: i64| {
                (0..r.rows).contains(&row)
                    && (0..r.cols).contains(&col)
                    && r.agl[(row * r.cols + col) as usize] > h
            };
            f.polygons(
                &super::outline::polygons(&set, r.rows, r.cols, &corner, 0.0),
                props([("alt_agl", json!(h)), ("unseen", json!(true))]),
            );
        }
        for s in &q.sites {
            site(&mut f, s.observer);
        }
        Some(f.finish())
    } else {
        None
    };
    let target = q.target_alt_agl;
    Ok(ExposureResponse {
        provenance: terrain.provenance(),
        raster: ExposureRaster {
            origin_x: r.origin.0,
            origin_z: r.origin.1,
            cell: r.res,
            width: r.cols as u32,
            height: r.rows as u32,
            floor_agl: floats(r.agl.iter().copied()),
            floor_msl: floats(r.agl.iter().zip(&r.ground).map(|(a, g)| a + g)),
            seen_count: base64(&r.seen),
        },
        target_alt_agl: target,
        seen_fraction: r.share(|v| v <= target),
        bands,
        geojson,
    })
}

/// The box about every site's radius.
fn disks(q: &ExposureRequest) -> Region {
    let mut b = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for s in &q.sites {
        let (x, z, r) = (s.observer.x, s.observer.z, s.radius);
        b = [
            b[0].min(x - r),
            b[1].min(z - r),
            b[2].max(x + r),
            b[3].max(z + r),
        ];
    }
    Region::Box(BoundingBox {
        min_x: b[0],
        min_z: b[1],
        max_x: b[2],
        max_z: b[3],
    })
}

/// Whether the disk about `(x, z)` of radius `r` meets the box.
fn meets(bbox: [f64; 4], x: f64, z: f64, r: f64) -> bool {
    let dx = (bbox[0] - x).max(x - bbox[2]).max(0.0);
    let dz = (bbox[1] - z).max(z - bbox[3]).max(0.0);
    dx.hypot(dz) <= r
}

/// Sweeps each site whose radius meets the region and folds its floor into
/// the region's raster, in the order the sites were given.
fn reduce(terrain: &Terrain, q: &ExposureRequest, area: &Area) -> Result<Reduced, Vec<Issue>> {
    let src = terrain.source();
    let g = src.grid();
    let cell = f64::from(g.cell_size);
    let res = q.resolution.unwrap_or(cell);
    // The region's raster, aligned as every sweep's is: the cells whose
    // centers lie in its box.
    let [x0, z0, x1, z1] = area.bbox;
    let first = |v: f64, origin: f64| ((v - origin) / res - 0.5).ceil();
    let last = |v: f64, origin: f64| ((v - origin) / res - 0.5).floor();
    let (row0, col0) = (first(x0, g.origin_x), first(z0, g.origin_z));
    let (rows, cols) = (
        (last(x1, g.origin_x) - row0 + 1.0).max(0.0),
        (last(z1, g.origin_z) - col0 + 1.0).max(0.0),
    );
    if rows * cols > CELLS_MAX as f64 {
        return Err(too_large(
            "region",
            (rows * cols) as u64,
            "shrink the region or coarsen the resolution",
            "region",
        ));
    }
    let (row0, col0, rows, cols) = (row0 as i64, col0 as i64, rows as i64, cols as i64);
    let origin = (
        g.origin_x + row0 as f64 * res,
        g.origin_z + col0 as f64 * res,
    );
    let centre = |r: i64, c: i64| {
        (
            origin.0 + (r as f64 + 0.5) * res,
            origin.1 + (c as f64 + 0.5) * res,
        )
    };

    // The heights under the region and a cell beyond, for the bilinear the
    // sweeps take, so a cell no site reaches is told from one with no
    // ground.
    let index = |v: f64, o: f64| ((v - o) / cell).floor() as i64;
    let (r_lo, r_hi) = (
        index(origin.0, g.origin_x) - 1,
        index(origin.0 + rows as f64 * res, g.origin_x) + 1,
    );
    let (c_lo, c_hi) = (
        index(origin.1, g.origin_z) - 1,
        index(origin.1 + cols as f64 * res, g.origin_z) + 1,
    );
    let span = (r_hi - r_lo + 1) as u64 * (c_hi - c_lo + 1) as u64;
    if span > CELLS_MAX {
        return Err(too_large(
            "window of heights",
            span,
            "shrink the region",
            "region",
        ));
    }
    let w = Window::read(
        src,
        Layer::Height,
        r_lo,
        c_lo,
        (r_hi - r_lo + 1) as u32,
        (c_hi - c_lo + 1) as u32,
    )
    .map_err(refused)?;
    let ground: Vec<f64> = (0..rows * cols)
        .into_par_iter()
        .map(|i| {
            let (x, z) = centre(i / cols, i % cols);
            match w.value_at(&g, x, z) {
                Some(h) if area.contains(x, z) => h,
                _ => f64::NAN,
            }
        })
        .collect();
    drop(w);
    let mut agl: Vec<f64> = ground
        .iter()
        .map(|h| if h.is_nan() { f64::NAN } else { f64::INFINITY })
        .collect();
    let mut seen = vec![0u8; (rows * cols) as usize];

    let target = q.target_alt_agl;
    for (i, s) in q.sites.iter().enumerate() {
        if !meets(area.bbox, s.observer.x, s.observer.z, s.radius) {
            continue;
        }
        let at = [format!("sites.{i}.observer"), format!("sites.{i}.radius")];
        let fl = Floor::sweep(
            terrain,
            s.observer,
            [&at[0], &at[1]],
            s.radius,
            q.resolution,
            vec![],
        )?;
        // The site's raster sits at a whole offset in the region's.
        let dr = ((fl.origin.0 - origin.0) / res).round() as i64;
        let dc = ((fl.origin.1 - origin.1) / res).round() as i64;
        let (c_from, c_to) = (dc.max(0), (dc + fl.side).min(cols));
        if c_from >= c_to {
            continue;
        }
        agl.par_chunks_mut(cols as usize)
            .zip(seen.par_chunks_mut(cols as usize))
            .enumerate()
            .for_each(|(r, (agl, seen))| {
                let fr = r as i64 - dr;
                if !(0..fl.side).contains(&fr) {
                    return;
                }
                for c in c_from..c_to {
                    let f = fl.agl[(fr * fl.side + c - dc) as usize];
                    let a = &mut agl[c as usize];
                    // NaN beyond the site's radius, and wherever the region
                    // holds no cell to count.
                    if f.is_nan() || a.is_nan() {
                        continue;
                    }
                    *a = a.min(f);
                    if f <= target {
                        seen[c as usize] += 1;
                    }
                }
            });
    }
    Ok(Reduced {
        origin,
        res,
        rows,
        cols,
        agl,
        ground,
        seen,
    })
}

#[cfg(test)]
mod tests {
    use super::super::floor::tests::{at, floats, site};
    use super::super::geojson::tests::assert_valid;
    use super::super::tests::synth;
    use super::super::viewshed::tests::{crosses, rings_back, unbase64};
    use super::super::{Terrain, answer};
    use crate::synth::{HILL_A_EAST_M, HILL_A_NORTH_M, HILL_B_EAST_M, HILL_B_NORTH_M};
    use serde_json::{Value, json};

    /// West of the big hill, east of the small one, and south of the big
    /// one, each with its own radius, so their disks overlap in part.
    fn sites() -> Vec<Value> {
        let (wx, wz) = site();
        let (ex, ez) = at(HILL_B_NORTH_M, HILL_B_EAST_M + 4000.0);
        let (sx, sz) = at(HILL_A_NORTH_M - 6000.0, HILL_A_EAST_M);
        vec![
            json!({"observer": {"x": wx, "z": wz, "alt_agl": 10}, "radius": 8000}),
            json!({"observer": {"x": ex, "z": ez, "alt_agl": 15}, "radius": 6000}),
            json!({"observer": {"x": sx, "z": sz, "alt_agl": 5}, "radius": 5000}),
        ]
    }

    /// A box about the two hills, some of whose cells no site's radius
    /// reaches.
    fn region() -> Value {
        let (x0, z0) = at(HILL_A_NORTH_M - 6000.0, HILL_A_EAST_M - 8000.0);
        let (x1, z1) = at(HILL_A_NORTH_M + 6000.0, HILL_B_EAST_M + 7000.0);
        json!({"Box": {"min_x": x0, "min_z": z0, "max_x": x1, "max_z": z1}})
    }

    fn ask(t: &Terrain, sites: &[Value], extra: Value) -> Value {
        let mut q = json!({"sites": sites, "region": region(), "target_alt_agl": 30});
        q.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        answer(t, "exposure", &q).unwrap()
    }

    /// A raster's corner, sides and cell, as a response gives them.
    struct Grid {
        x0: f64,
        z0: f64,
        rows: i64,
        cols: i64,
        cell: f64,
    }

    impl Grid {
        fn of(r: &Value) -> Grid {
            Grid {
                x0: r["origin_x"].as_f64().unwrap(),
                z0: r["origin_z"].as_f64().unwrap(),
                rows: r["height"].as_i64().unwrap(),
                cols: r["width"].as_i64().unwrap(),
                cell: r["cell"].as_f64().unwrap(),
            }
        }

        /// The index of the cell holding `(x, z)`, if the raster has one.
        fn index(&self, x: f64, z: f64) -> Option<usize> {
            let r = ((x - self.x0) / self.cell).floor() as i64;
            let c = ((z - self.z0) / self.cell).floor() as i64;
            ((0..self.rows).contains(&r) && (0..self.cols).contains(&c))
                .then_some((r * self.cols + c) as usize)
        }

        fn centre(&self, i: usize) -> (f64, f64) {
            let (r, c) = (i as i64 / self.cols, i as i64 % self.cols);
            (
                self.x0 + (r as f64 + 0.5) * self.cell,
                self.z0 + (c as f64 + 0.5) * self.cell,
            )
        }
    }

    /// Each site's own radar floor, above ground and above sea level, at
    /// every cell of the region's raster `g`; NaN where the site has none.
    fn per_site(t: &Terrain, sites: &[Value], g: &Grid, extra: &Value) -> Vec<[Vec<f32>; 2]> {
        sites
            .iter()
            .map(|s| {
                let mut q = json!({"site": s["observer"], "radius": s["radius"]});
                q.as_object_mut()
                    .unwrap()
                    .extend(extra.as_object().unwrap().clone());
                let v = answer(t, "radar_floor", &q).unwrap();
                let own = Grid::of(&v["raster"]);
                let pick = |layer: &str| -> Vec<f32> {
                    let l = floats(v["raster"][layer].as_str().unwrap());
                    (0..(g.rows * g.cols) as usize)
                        .map(|i| {
                            let (x, z) = g.centre(i);
                            own.index(x, z).map_or(f32::NAN, |j| l[j])
                        })
                        .collect()
                };
                [pick("floor_agl"), pick("floor_msl")]
            })
            .collect()
    }

    /// Checks the least floor and the count against each site's own radar
    /// floor at every cell of the region; returns the cells some site
    /// reaches.
    fn check_against_each_site(t: &Terrain, extra: Value) -> usize {
        let sites = sites();
        let v = ask(t, &sites, extra.clone());
        let r = &v["raster"];
        let g = Grid::of(r);
        let agl = floats(r["floor_agl"].as_str().unwrap());
        let msl = floats(r["floor_msl"].as_str().unwrap());
        let seen = unbase64(r["seen_count"].as_str().unwrap());
        assert_eq!(seen.len(), agl.len());
        let resolution = extra
            .get("resolution")
            .map_or(json!({}), |res| json!({"resolution": res}));
        let each = per_site(t, &sites, &g, &resolution);
        let (mut reached, mut overlapped, mut unreached) = (0, 0, 0);
        for i in 0..agl.len() {
            let floors: Vec<(f32, f32)> = each
                .iter()
                .map(|[a, m]| (a[i], m[i]))
                .filter(|(a, _)| !a.is_nan())
                .collect();
            if agl[i].is_nan() {
                assert!(
                    floors.is_empty() && msl[i].is_nan() && seen[i] == 0,
                    "cell {i}"
                );
                continue;
            }
            match floors.iter().min_by(|a, b| a.0.total_cmp(&b.0)) {
                None => {
                    // Ground no site's radius reaches.
                    assert!(agl[i] == f32::INFINITY && seen[i] == 0, "cell {i}");
                    unreached += 1;
                }
                Some(&(least, least_msl)) => {
                    // The least floor is one site's own, bit for bit.
                    assert_eq!(agl[i].to_bits(), least.to_bits(), "cell {i}");
                    assert_eq!(msl[i].to_bits(), least_msl.to_bits(), "cell {i}");
                    let count = floors.iter().filter(|(a, _)| *a <= 30.0).count();
                    assert_eq!(usize::from(seen[i]), count, "cell {i}");
                    reached += 1;
                    overlapped += usize::from(floors.len() > 1);
                }
            }
        }
        assert!(
            reached > 5_000 && overlapped > 500 && unreached > 500,
            "{reached} {overlapped} {unreached}"
        );
        reached
    }

    #[test]
    fn the_least_floor_and_the_count_are_what_each_sites_floor_says() {
        let t = synth();
        let fine = check_against_each_site(&t, json!({}));
        // At a coarser cell too, the sites' rasters landing on the region's.
        let coarse = check_against_each_site(&t, json!({"resolution": 100}));
        assert!(coarse * 3 < fine, "{coarse} against {fine}");
    }

    #[test]
    fn the_count_is_how_many_sites_visible_finds_see_the_target() {
        let t = synth();
        let sites = sites();
        let v = ask(&t, &sites, json!({}));
        let r = &v["raster"];
        let g = Grid::of(r);
        let agl = floats(r["floor_agl"].as_str().unwrap());
        let seen = unbase64(r["seen_count"].as_str().unwrap());
        let each = per_site(&t, &sites, &g, &json!({}));
        // Every 37th cell with ground some site reaches: the sites within
        // their radius that see a target 30 m above it, line by line.
        let (mut cells, mut agree) = (0, 0);
        for i in (0..agl.len()).step_by(37) {
            if !agl[i].is_finite() {
                continue;
            }
            let (x, z) = g.centre(i);
            let (mut count, mut close) = (0u8, false);
            for (s, [a, _]) in sites.iter().zip(&each) {
                if a[i].is_nan() {
                    continue;
                }
                close |= (a[i] - 30.0).abs() < 5.0;
                let w = answer(
                    &t,
                    "visible",
                    &json!({"a": s["observer"], "b": {"x": x, "z": z, "alt_agl": 30}}),
                )
                .unwrap();
                count += u8::from(w["visible"] == true);
            }
            cells += 1;
            if count == seen[i] {
                agree += 1;
            } else {
                // The raster's floor stands from the line's by the raster's
                // few meters, so the two part only where a site's floor is
                // near the target's height.
                assert!(close, "cell {i} at ({x}, {z}): {} against {count}", seen[i]);
            }
        }
        assert!(cells > 300, "{cells}");
        assert!(agree as f64 >= 0.98 * cells as f64, "{agree} of {cells}");
        let counted = agl.iter().filter(|a| !a.is_nan()).count() as f64;
        let some = (seen.iter().zip(&agl))
            .filter(|(s, a)| !a.is_nan() && **s > 0)
            .count() as f64;
        assert!((v["seen_fraction"].as_f64().unwrap() - some / counted).abs() < 1e-12);
    }

    #[test]
    fn the_order_of_the_sites_changes_nothing_and_a_far_one_is_not_swept() {
        let t = synth();
        let mut sites = sites();
        let v = ask(&t, &sites, json!({}));
        sites.reverse();
        // A site whose radius misses the region: a sweep of it would take
        // seconds and change no cell.
        let (fx, fz) = at(10_000.0, 10_000.0);
        sites.push(json!({"observer": {"x": fx, "z": fz, "alt_agl": 10}, "radius": 20_000}));
        let w = ask(&t, &sites, json!({}));
        for k in ["floor_agl", "floor_msl", "seen_count", "origin_x", "width"] {
            assert_eq!(v["raster"][k], w["raster"][k], "{k}");
        }
        assert!(!super::meets([0.0, 0.0, 10.0, 10.0], 20.0, 20.0, 14.0));
        assert!(super::meets([0.0, 0.0, 10.0, 10.0], 20.0, 20.0, 14.2));
        assert!(super::meets([0.0, 0.0, 10.0, 10.0], 5.0, 5.0, 0.0));
    }

    #[test]
    fn without_a_region_it_covers_every_sites_radius() {
        let t = synth();
        let sites = sites();
        let v = answer(
            &t,
            "exposure",
            &json!({"sites": sites, "target_alt_agl": 0}),
        )
        .unwrap();
        let g = Grid::of(&v["raster"]);
        let agl = floats(v["raster"]["floor_agl"].as_str().unwrap());
        for s in &sites {
            let o = &s["observer"];
            let (x, z, r) = (
                o["x"].as_f64().unwrap(),
                o["z"].as_f64().unwrap(),
                s["radius"].as_f64().unwrap(),
            );
            // The site's own cell sees itself, and its radius's ends are on
            // the raster.
            assert_eq!(agl[g.index(x, z).unwrap()], 0.0);
            for (dx, dz) in [(r, 0.0), (-r, 0.0), (0.0, r), (0.0, -r)] {
                assert!(g.index(x + dx * 0.999, z + dz * 0.999).is_some());
            }
        }
    }

    #[test]
    fn a_circle_region_keeps_the_cells_inside_it() {
        let t = synth();
        let (x, z) = at(HILL_A_NORTH_M, HILL_A_EAST_M);
        let circle = json!({"Circle": {"x": x, "z": z, "radius": 4000}});
        let v = ask(&t, &sites(), json!({"region": circle}));
        let boxed = ask(&t, &sites(), json!({}));
        let (g, b) = (Grid::of(&v["raster"]), Grid::of(&boxed["raster"]));
        let agl = floats(v["raster"]["floor_agl"].as_str().unwrap());
        let all = floats(boxed["raster"]["floor_agl"].as_str().unwrap());
        let mut inside = 0;
        for (i, a) in agl.iter().enumerate() {
            let (cx, cz) = g.centre(i);
            if (cx - x).hypot(cz - z) <= 4000.0 {
                assert_eq!(a.to_bits(), all[b.index(cx, cz).unwrap()].to_bits());
                inside += 1;
            } else {
                assert!(a.is_nan());
            }
        }
        assert!(inside > 15_000, "{inside}");
    }

    #[test]
    fn the_unseen_areas_nest_each_higher_inside_the_lower() {
        let t = synth();
        let v = ask(
            &t,
            &sites(),
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
        assert_eq!(f.len(), 4 + 3);
        let shares: Vec<f64> = v["bands"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b["unseen_fraction"].as_f64().unwrap())
            .collect();
        assert!(shares.windows(2).all(|w| w[0] > w[1]), "{shares:?}");
        let rings: Vec<_> = f[..4]
            .iter()
            .map(|f| rings_back(&t, &f["geometry"]))
            .collect();
        let inside = |rings: &[(bool, Vec<[f64; 2]>)], x: f64, z: f64| {
            rings.iter().filter(|(_, r)| crosses(r, x, z)).count() % 2 == 1
        };
        // A cell's center lies in a band's area just when no site sees it
        // there, so each area lies inside every lower one.
        let r = &v["raster"];
        let grid = Grid::of(r);
        let agl = floats(r["floor_agl"].as_str().unwrap());
        let mut nested = 0;
        for i in (0..agl.len()).step_by(53) {
            let (x, z) = grid.centre(i);
            for k in 0..4 {
                let unseen = f64::from(agl[i]) > heights[k];
                assert_eq!(inside(&rings[k], x, z), unseen, "cell {i} band {k}");
                if unseen && k > 0 {
                    assert!(inside(&rings[k - 1], x, z));
                    nested += 1;
                }
            }
        }
        assert!(nested > 100, "{nested}");
    }

    #[test]
    fn refuses_a_bad_request() {
        let t = synth();
        let issue = |q: Value| answer(&t, "exposure", &q).unwrap_err();
        let mut s = sites();
        s[1]["radius"] = json!(-1);
        s[2]["observer"]["x"] = json!(1e8);
        let e = issue(json!({"sites": s, "target_alt_agl": -3, "resolution": 10}));
        let paths: Vec<_> = e.iter().map(|i| i.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "target_alt_agl",
                "resolution",
                "sites.1.radius",
                "sites.2.observer"
            ]
        );
        let e = issue(json!({"sites": [], "target_alt_agl": 0}));
        assert_eq!(e[0].path, "sites");
        let many: Vec<Value> = (0..65).map(|_| sites()[0].clone()).collect();
        let e = issue(json!({"sites": many, "target_alt_agl": 0}));
        assert_eq!(e[0].code, "too_large");
        let e = issue(json!({"sites": sites(), "target_alt_agl": 0,
            "region": {"Box": {"min_x": 0, "min_z": 0, "max_x": 1e6, "max_z": 1e6}}}));
        assert_eq!(
            (e[0].code.as_str(), e[0].path.as_str()),
            ("too_large", "region")
        );
        let e = issue(json!({"sites": sites(), "target_alt_agl": 0,
            "region": {"Box": {"min_x": 1, "min_z": 0, "max_x": 0, "max_z": 1}}}));
        assert_eq!(e[0].path, "region");
    }
}
