//! `sample`: layer values on a lattice inside a region, or at given points.

use std::collections::{BTreeMap, HashMap};

use super::{Terrain, refused, resolve};
use crate::classes::Class;
use crate::decode::scn5::Pool;
use crate::grid::{self, Window};
use crate::layers::{self, Cells, DISTANCE_CAP_M, Derived, TPI_300, TPI_2000, Target};
use crate::source::{Cover, Ground, Layer, Source};
use crate::types::{Issue, SampleRequest, SampleResponse, SampleRow};

// The radii the radius layers are measured within: a neighborhood's
// buildings, the trees about a position, and the ground a site sits among.
const COUNT_RADIUS_M: f64 = 1000.0;
const DENSITY_RADIUS_M: f64 = 300.0;
const SHARE_RADIUS_M: f64 = 300.0;

/// Up to this many cells, a derived layer over scattered cells is computed
/// over the box around them all, about 50 km square; past it, over the box
/// around each tile's cells.
const ONE_BOX_CELLS: u64 = 1 << 20;
/// The most lattice points a region's box may hold, inside the region or
/// not: well under a second to walk.
const LATTICE_CANDIDATES_MAX: f64 = 1e8;
/// Up to this many cells, a region's ground layers are read as one window
/// around its lattice; past it, at each lattice point.
const ONE_WINDOW_CELLS: u64 = 4 << 20;

/// A layer a request names.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Want {
    Ground(Layer),
    Derived(Derived),
}

fn want(name: &str) -> Option<Want> {
    if let Some(l) = Layer::named(name) {
        return Some(Want::Ground(l));
    }
    let d = |d| Some(Want::Derived(d));
    let dist = |to| {
        d(Derived::Distance {
            to,
            cap: DISTANCE_CAP_M,
        })
    };
    match name {
        "valid" => d(Derived::Valid),
        "slope" => d(Derived::Slope),
        "aspect" => d(Derived::Aspect),
        "roughness" => d(Derived::Roughness),
        "tpi_300" => d(TPI_300),
        "tpi_2000" => d(TPI_2000),
        "dist_road" => dist(Target::Road),
        "dist_rail" => dist(Target::Rail),
        "dist_water" => dist(Target::Water),
        "dist_builtup" => dist(Target::Builtup),
        "count_building" => d(Derived::Count {
            class: Class::Building,
            radius: COUNT_RADIUS_M,
        }),
        "count_industrial" => d(Derived::Count {
            class: Class::Industrial,
            radius: COUNT_RADIUS_M,
        }),
        "tree_density" => d(Derived::Density {
            pool: Pool::Trees,
            radius: DENSITY_RADIUS_M,
        }),
        "enclosure" => d(Derived::Enclosure),
        _ => {
            if let Some(c) = name.strip_prefix("share_") {
                let cover = Cover::ALL.into_iter().find(|v| v.name() == c)?;
                return d(Derived::CoverFraction {
                    cover,
                    radius: SHARE_RADIUS_M,
                });
            }
            let bin: u8 = name.strip_prefix("horizon_")?.parse().ok()?;
            (usize::from(bin) < crate::horizon::BINS)
                .then_some(Want::Derived(Derived::Horizon { bin }))
        }
    }
}

fn invalid(message: &str, path: &str) -> Vec<Issue> {
    vec![Issue::new("invalid_request", message, path)]
}

fn too_many(n: usize, max: u32) -> Vec<Issue> {
    vec![Issue::new(
        "too_many_rows",
        &format!(
            "this answer has more than {max} rows ({n} or more); raise max_rows, widen the \
             step or shrink the region"
        ),
        "max_rows",
    )]
}

/// The lattice points inside the region, `step` apart from its box's
/// minimum corner, or too many rows.
fn lattice(area: &super::Area, step: f64, max_rows: u32) -> Result<Vec<[f64; 2]>, Vec<Issue>> {
    let [x0, z0, x1, z1] = area.bbox;
    let (fx, fz) = (
        ((x1 - x0) / step).floor() + 1.0,
        ((z1 - z0) / step).floor() + 1.0,
    );
    // The cap counts the points inside the region, so a sparse region in a
    // vast box would walk the whole box before reaching it.
    if fx * fz > LATTICE_CANDIDATES_MAX {
        return Err(vec![Issue::new(
            "too_many_rows",
            &format!(
                "the region's box holds {:.0} lattice points, over the {LATTICE_CANDIDATES_MAX:.0} \
                 a region may span; widen the step or shrink the region",
                fx * fz
            ),
            "step",
        )]);
    }
    let (nx, nz) = (fx as u64, fz as u64);
    let mut out = Vec::new();
    for i in 0..nx {
        let x = x0 + i as f64 * step;
        for k in 0..nz {
            let z = z0 + k as f64 * step;
            if area.contains(x, z) {
                if out.len() == max_rows as usize {
                    return Err(too_many(out.len() + 1, max_rows));
                }
                out.push([x, z]);
            }
        }
    }
    Ok(out)
}

/// A point's ground layer value as a number, null where nothing covers it.
fn ground_value(g: Option<&Ground>, l: Layer) -> Option<f64> {
    let g = g?;
    Some(match l {
        Layer::Height => g.height,
        Layer::Water => f64::from(g.water),
        Layer::Surface => f64::from(g.surface),
        Layer::Cover => f64::from(g.cover.code()),
    })
}

/// Each lattice point's value of a ground layer from the 50 m raster:
/// `height` bilinear, the rest the cell holding the point.
fn raster_values(
    src: &dyn Source,
    l: Layer,
    pts: &[[f64; 2]],
) -> Result<Vec<Option<f64>>, Vec<Issue>> {
    let g = src.grid();
    // Only points on the grid have a value, and only they bound the window.
    let (mut r0, mut c0, mut r1, mut c1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
    for &[x, z] in pts {
        if let Some((r, k)) = g.cell_at(x, z) {
            let (r, k) = (i64::from(r), i64::from(k));
            (r0, c0, r1, c1) = (r0.min(r - 1), c0.min(k - 1), r1.max(r + 1), c1.max(k + 1));
        }
    }
    if r0 > r1 {
        return Ok(vec![None; pts.len()]);
    }
    let cells = (r1 - r0 + 1) as u64 * (c1 - c0 + 1) as u64;
    if cells > ONE_WINDOW_CELLS {
        return pts
            .iter()
            .map(|&[x, z]| grid::sample(src, l, x, z).map_err(refused))
            .collect();
    }
    let w = Window::read(src, l, r0, c0, (r1 - r0 + 1) as u32, (c1 - c0 + 1) as u32)
        .map_err(refused)?;
    Ok(pts.iter().map(|&[x, z]| w.value_at(&g, x, z)).collect())
}

/// Each point's value of a derived layer at the cell holding it, computed
/// over the box around all the cells, or around each tile's when that box
/// is too large, so scattered points do not derive a whole terrain.
fn derived_values(
    src: &dyn Source,
    d: Derived,
    pts: &[[f64; 2]],
    capped: &mut bool,
) -> Result<Vec<Option<f64>>, Vec<Issue>> {
    let g = src.grid();
    let cell: Vec<Option<(i64, i64)>> = pts
        .iter()
        .map(|&[x, z]| g.cell_at(x, z).map(|(r, c)| (i64::from(r), i64::from(c))))
        .collect();
    let bound = |ks: &mut dyn Iterator<Item = (i64, i64)>| {
        let (mut r0, mut c0, mut r1, mut c1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
        for (r, c) in ks {
            (r0, c0, r1, c1) = (r0.min(r), c0.min(c), r1.max(r), c1.max(c));
        }
        (r0 <= r1).then(|| Cells {
            row0: r0,
            col0: c0,
            rows: (r1 - r0 + 1) as u32,
            cols: (c1 - c0 + 1) as u32,
        })
    };
    let mut groups: Vec<(Cells, Vec<usize>)> = Vec::new();
    match bound(&mut cell.iter().flatten().copied()) {
        None => {}
        Some(all) if u64::from(all.rows) * u64::from(all.cols) <= ONE_BOX_CELLS => {
            groups.push((all, (0..pts.len()).collect()));
        }
        Some(_) => {
            let ts = i64::from(g.tile_size);
            let mut by_tile: BTreeMap<(i64, i64), Vec<usize>> = BTreeMap::new();
            for (k, rc) in cell.iter().enumerate() {
                if let Some((r, c)) = rc {
                    by_tile.entry((r / ts, c / ts)).or_default().push(k);
                }
            }
            for ks in by_tile.into_values() {
                let at = bound(&mut ks.iter().map(|&k| cell[k].unwrap())).unwrap();
                groups.push((at, ks));
            }
        }
    }
    let mut out = vec![None; pts.len()];
    for (at, ks) in groups {
        let r = layers::derive(src, d, at).map_err(refused)?;
        *capped |= r.capped;
        for k in ks {
            if let Some((row, col)) = cell[k] {
                out[k] = r.get(row, col).filter(|v| !v.is_nan()).map(f64::from);
            }
        }
    }
    Ok(out)
}

pub fn sample(terrain: &Terrain, q: SampleRequest) -> Result<SampleResponse, Vec<Issue>> {
    let src = terrain.source();
    if q.layers.is_empty() {
        return Err(invalid("name at least one layer", "layers"));
    }
    let mut wants = Vec::with_capacity(q.layers.len());
    for (k, name) in q.layers.iter().enumerate() {
        match want(name) {
            Some(w) => wants.push(w),
            None => {
                return Err(vec![Issue::new(
                    "unknown_layer",
                    &format!("no layer `{name}`"),
                    &format!("layers.{k}"),
                )]);
            }
        }
    }
    let regional = match (&q.region, &q.points) {
        (Some(_), None) => true,
        (None, Some(_)) => false,
        _ => return Err(invalid("give either region with step, or points", "")),
    };
    let pts: Vec<[f64; 2]> = if let Some(region) = &q.region {
        let cell = f64::from(src.grid().cell_size);
        let step = q
            .step
            .ok_or_else(|| invalid("a region needs a step", "step"))?;
        if !(step >= cell && step.is_finite()) {
            return Err(invalid(
                &format!("step must be at least the cell size, {cell} m"),
                "step",
            ));
        }
        lattice(&resolve(src, region, "region")?, step, q.max_rows)?
    } else {
        let points = q.points.as_deref().unwrap_or_default();
        if points.len() > q.max_rows as usize {
            return Err(too_many(points.len(), q.max_rows));
        }
        if let Some(k) = points
            .iter()
            .position(|p| !(p.x.is_finite() && p.z.is_finite()))
        {
            return Err(vec![Issue::new(
                "invalid_point",
                "x and z must be finite",
                &format!("points.{k}"),
            )]);
        }
        points.iter().map(|p| [p.x, p.z]).collect()
    };

    let needs_ground = wants.iter().any(|w| matches!(w, Want::Ground(_)));
    let ground = if !regional && needs_ground {
        src.points(&pts).map_err(refused)?
    } else {
        Vec::new()
    };
    let mut capped = false;
    let mut columns: HashMap<usize, Vec<Option<f64>>> = HashMap::new();
    for (k, w) in wants.iter().enumerate() {
        if let Some(first) = wants[..k].iter().position(|v| v == w) {
            let same = columns[&first].clone();
            columns.insert(k, same);
            continue;
        }
        let col = match *w {
            Want::Ground(l) if regional => raster_values(src, l, &pts)?,
            Want::Ground(l) => ground.iter().map(|g| ground_value(g.as_ref(), l)).collect(),
            Want::Derived(d) => derived_values(src, d, &pts, &mut capped)?,
        };
        columns.insert(k, col);
    }
    let rows = pts
        .iter()
        .enumerate()
        .map(|(i, &[x, z])| SampleRow {
            x,
            z,
            values: q
                .layers
                .iter()
                .enumerate()
                .map(|(k, name)| (name.clone(), columns[&k][i]))
                .collect(),
        })
        .collect();
    Ok(SampleResponse {
        provenance: terrain.provenance(),
        rows,
        distance_capped: capped,
    })
}

#[cfg(test)]
mod tests {
    use super::super::answer;
    use super::super::tests::synth;
    use super::*;
    use crate::synth::*;
    use serde_json::{Value, json};

    fn rows(v: &Value) -> &Vec<Value> {
        v["rows"].as_array().unwrap()
    }

    #[test]
    fn every_named_layer_is_known() {
        let mut names = vec![
            "height",
            "water",
            "surface",
            "cover",
            "valid",
            "slope",
            "aspect",
            "roughness",
            "tpi_300",
            "tpi_2000",
            "dist_road",
            "dist_rail",
            "dist_water",
            "dist_builtup",
            "count_building",
            "count_industrial",
            "tree_density",
            "enclosure",
            "horizon_0",
            "horizon_31",
        ]
        .into_iter()
        .map(String::from)
        .collect::<Vec<_>>();
        names.extend(Cover::ALL.iter().map(|c| format!("share_{}", c.name())));
        for n in &names {
            assert!(want(n).is_some(), "{n}");
        }
        for n in ["horizon_32", "share_moon", "dist_moon", "Height"] {
            assert!(want(n).is_none(), "{n}");
        }
    }

    #[test]
    fn points_take_the_mesh_and_come_back_in_order() {
        let t = synth();
        let s = t.source();
        let form = match &t {
            Terrain::Synth(s) => *s.form(),
            _ => unreachable!(),
        };
        // The lake, the hill top, outside the mesh, and the runway.
        let pts = [
            form.abs(LAKE_NORTH_M, LAKE_EAST_M),
            form.abs(HILL_A_NORTH_M, HILL_A_EAST_M),
            form.abs(100.0, 100.0),
            form.abs(RUNWAY_NORTH_M, RUNWAY_EAST_M),
        ];
        let v = answer(
            &t,
            "sample",
            &json!({
                "points": pts.iter().map(|&(x, z)| json!({"x": x, "z": z})).collect::<Vec<_>>(),
                "layers": ["height", "water", "surface", "slope", "height"]
            }),
        )
        .unwrap();
        let r = rows(&v);
        assert_eq!(r.len(), 4);
        for (row, &(x, z)) in r.iter().zip(&pts) {
            assert_eq!((row["x"].as_f64(), row["z"].as_f64()), (Some(x), Some(z)));
            let g = s.point(x, z).unwrap();
            assert_eq!(row["values"]["height"].as_f64(), g.map(|g| g.height));
        }
        assert_eq!(r[0]["values"]["water"].as_f64(), Some(1.0));
        assert_eq!(r[0]["values"]["height"], LAKE_LEVEL_M);
        assert_eq!(r[2]["values"]["height"], Value::Null);
        assert_eq!(r[2]["values"]["slope"], Value::Null);
        assert_eq!(r[3]["values"]["surface"].as_f64(), Some(5.0));
        assert!(r[1]["values"]["slope"].as_f64().unwrap() < 2.0);
        assert_eq!(v["distance_capped"], false);
    }

    #[test]
    fn a_region_samples_its_lattice_bilinear_in_height() {
        let t = synth();
        let s = t.source();
        let form = match &t {
            Terrain::Synth(s) => *s.form(),
            _ => unreachable!(),
        };
        let (x, z) = form.abs(HILL_A_NORTH_M, HILL_A_EAST_M);
        let v = answer(
            &t,
            "sample",
            &json!({
                "region": {"Circle": {"x": x, "z": z, "radius": 1000}},
                "step": 125,
                "layers": ["height", "cover", "tpi_300"]
            }),
        )
        .unwrap();
        let r = rows(&v);
        // The lattice points of a 125 m grid within 1 km of its center.
        let n = (0..17)
            .flat_map(|i| (0..17).map(move |k| (i, k)))
            .filter(|&(i, k)| {
                let (dx, dz) = (
                    -1000.0 + 125.0 * f64::from(i),
                    -1000.0 + 125.0 * f64::from(k),
                );
                dx.hypot(dz) <= 1000.0
            })
            .count();
        assert_eq!(r.len(), n);
        for row in r {
            let (px, pz) = (row["x"].as_f64().unwrap(), row["z"].as_f64().unwrap());
            let want = grid::sample(s, Layer::Height, px, pz).unwrap();
            assert_eq!(row["values"]["height"].as_f64(), want);
            let cover = grid::sample(s, Layer::Cover, px, pz).unwrap();
            assert_eq!(row["values"]["cover"].as_f64(), cover);
        }
        // A hill top stands above its surroundings.
        let top = r
            .iter()
            .find(|row| {
                (row["x"].as_f64().unwrap() - x).abs() < 1.0
                    && (row["z"].as_f64().unwrap() - z).abs() < 1.0
            })
            .unwrap();
        assert!(top["values"]["tpi_300"].as_f64().unwrap() > 5.0);
    }

    #[test]
    fn scattered_points_derive_each_tile_alone_and_agree() {
        let t = synth();
        let form = match &t {
            Terrain::Synth(s) => *s.form(),
            _ => unreachable!(),
        };
        // Far enough apart that their box is over the one-box limit.
        let a = form.abs(5000.0, 5000.0);
        let b = form.abs(64000.0, 64000.0);
        let req = |pts: &[(f64, f64)]| {
            json!({
                "points": pts.iter().map(|&(x, z)| json!({"x": x, "z": z})).collect::<Vec<_>>(),
                "layers": ["slope", "dist_road"]
            })
        };
        let both = answer(&t, "sample", &req(&[a, b])).unwrap();
        let one = answer(&t, "sample", &req(&[a])).unwrap();
        let other = answer(&t, "sample", &req(&[b])).unwrap();
        assert_eq!(rows(&both)[0]["values"], rows(&one)[0]["values"]);
        assert_eq!(rows(&both)[1]["values"], rows(&other)[0]["values"]);
    }

    #[test]
    fn over_the_cap_is_an_error_never_a_truncation() {
        let t = synth();
        let code = |req: Value| answer(&t, "sample", &req).unwrap_err()[0].code.clone();
        let region = json!({"Box": {"min_x": 0, "min_z": 0, "max_x": 990, "max_z": 990}});
        // 10 × 10 lattice points at 110 m.
        let ok = json!({"region": region, "step": 110, "layers": ["height"], "max_rows": 100});
        assert_eq!(rows(&answer(&t, "sample", &ok).unwrap()).len(), 100);
        let over = json!({"region": region, "step": 110, "layers": ["height"], "max_rows": 99});
        assert_eq!(code(over), "too_many_rows");
        let pts = vec![json!({"x": 0, "z": 0}); 3];
        assert_eq!(
            code(json!({"points": pts, "layers": ["height"], "max_rows": 2})),
            "too_many_rows"
        );
        // A whole terrain at the cell size is far over the default.
        let all =
            json!({"Box": {"min_x": -30000, "min_z": -45000, "max_x": 40000, "max_z": 25000}});
        assert_eq!(
            code(json!({"region": all, "step": 50, "layers": ["height"]})),
            "too_many_rows"
        );
    }

    #[test]
    fn an_empty_far_or_sparse_region_answers_or_refuses_at_once() {
        let t = synth();
        let ask = |region: Value| {
            answer(
                &t,
                "sample",
                &json!({"region": region, "step": 50, "layers": ["height", "slope"]}),
            )
        };
        // A triangle no lattice point falls in.
        let thin = json!({"Polygon": {"points": [[0, 0], [1, 1], [0, 2]]}});
        assert!(rows(&ask(thin).unwrap()).is_empty());
        // Far off the grid, every value is null.
        for at in [1e30, -1e30, 1e300] {
            let far = json!({"Box": {"min_x": at, "min_z": at, "max_x": at, "max_z": at}});
            let v = ask(far).unwrap();
            assert_eq!(rows(&v)[0]["values"]["height"], Value::Null, "{at}");
        }
        // A sliver in a box of 1e11 lattice points is refused before walking it.
        let sliver = json!({"Polygon": {"points": [[0, 0], [1, 1e10], [2, 0]]}});
        let started = std::time::Instant::now();
        assert_eq!(ask(sliver).unwrap_err()[0].code, "too_many_rows");
        assert!(started.elapsed().as_secs_f64() < 1.0);
    }

    #[test]
    fn a_malformed_request_names_what_is_wrong() {
        let t = synth();
        let issue = |req: Value| {
            let e = answer(&t, "sample", &req).unwrap_err();
            (e[0].code.clone(), e[0].path.clone())
        };
        let region = json!({"Circle": {"x": 0, "z": 0, "radius": 100}});
        let pt = json!([{"x": 0, "z": 0}]);
        let i = |c: &str, p: &str| (c.to_string(), p.to_string());
        assert_eq!(
            issue(json!({"region": region, "layers": ["height"]})),
            i("invalid_request", "step")
        );
        assert_eq!(
            issue(json!({"region": region, "step": 10, "layers": ["height"]})),
            i("invalid_request", "step")
        );
        assert_eq!(
            issue(json!({"region": region, "points": pt, "step": 50, "layers": ["height"]})),
            i("invalid_request", "")
        );
        assert_eq!(
            issue(json!({"points": pt, "layers": ["height", "moon"]})),
            i("unknown_layer", "layers.1")
        );
        assert_eq!(
            issue(json!({"points": pt, "layers": []})),
            i("invalid_request", "layers")
        );
    }
}
