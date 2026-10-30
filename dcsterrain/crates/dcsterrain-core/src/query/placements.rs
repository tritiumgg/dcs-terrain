//! `validate_placements`: whether each placement's shape stands on the
//! ground it asks for, no steeper than it allows, and clear of scenery and
//! the airfields' pavement.
//!
//! The ground is read cell by cell over every cell the shape covers part
//! of: conservative coverage, so a shape reaching a hair into a cell reads
//! it, and one only touching the cell's edge does not. Each cell is judged
//! by its center, as the raster is, so water and slope are found to within
//! a cell of the shape and never missed in a cell the shape reaches. A
//! point reads its own ground, and the slope of its own cell.
//!
//! Objects and pavement are a broad phase and a narrow one: the scene's
//! own index and the mesh's runway index give what lies in a box around
//! the shape, and each is then tested exactly against it, touching
//! counted, as `shape` decides.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::criteria::FOOTPRINT_MAX_M;
use super::geojson::{Features, props, signed_area};
use super::sample::derived_values;
use super::scenery::{OBJECT_REACH_M, Rect, footprint, row};
use super::shape::{Shape, TOUCH_M};
use super::{Terrain, invalid, refused};
use crate::decode::scn5::Pool;
use crate::grid::Grid;
use crate::layers::Derived;
use crate::source::{Ground, Object, Source, Tables};
use crate::types::{
    CoverClass, Issue, Placement, PlacementCheck, PlacementIssue, PlacementShape, ScenePool,
    SceneryObject, Underfoot, ValidatePlacementsRequest, ValidatePlacementsResponse, WaterClass,
};

/// The most placements one request checks.
const PLACEMENTS_MAX: usize = 10_000;
/// The most vertices a polygon may have.
const POLYGON_POINTS_MAX: usize = 1_000;
/// The most objects a placement lists, the nearest its position first; it
/// counts them all.
pub const OBJECTS_LISTED_MAX: usize = 100;

/// The issues with placement `k`'s request.
fn check_request(p: &Placement, k: usize) -> Vec<Issue> {
    let at = |field: &str| format!("placements.{k}.{field}");
    let mut issues = Vec::new();
    if !(p.x.is_finite() && p.z.is_finite() && p.heading.is_finite()) {
        issues.extend(invalid("x, z and heading must be finite", &at("x")));
    }
    let size = |v: f64| (0.0..=FOOTPRINT_MAX_M).contains(&v);
    let ok = match &p.shape {
        PlacementShape::Point => true,
        PlacementShape::Rect { width, depth } => size(*width) && size(*depth),
        PlacementShape::Circle { radius } => size(2.0 * radius),
        PlacementShape::Polygon { points } => {
            let finite = points.iter().all(|q| q[0].is_finite() && q[1].is_finite());
            let shape = Shape::polygon(points.clone());
            let b = if finite && points.len() >= 3 {
                shape.bbox()
            } else {
                [0.0; 4]
            };
            (3..=POLYGON_POINTS_MAX).contains(&points.len())
                && finite
                && size(b[2] - b[0])
                && size(b[3] - b[1])
        }
    };
    if !ok {
        issues.extend(invalid(
            &format!(
                "a shape needs finite sizes from 0 to {FOOTPRINT_MAX_M} m across, and a polygon \
                 3 to {POLYGON_POINTS_MAX} points"
            ),
            &at("shape"),
        ));
    }
    if p.max_slope_deg.is_some_and(|d| !(0.0..=90.0).contains(&d)) {
        issues.extend(invalid(
            "must be an angle from 0 to 90 degrees",
            &at("max_slope_deg"),
        ));
    }
    if let Some(d) = p.min_depth {
        if !(d.is_finite() && d >= 0.0) {
            issues.extend(invalid(
                "must be a finite depth, 0 or more",
                &at("min_depth"),
            ));
        } else if p.on == Underfoot::Land {
            issues.extend(invalid("applies to `on: water` only", &at("min_depth")));
        }
    }
    issues
}

/// The placement's shape in the world.
fn shape_of(p: &Placement) -> Shape {
    match &p.shape {
        PlacementShape::Point => Shape::point([p.x, p.z]),
        &PlacementShape::Rect { width, depth } => {
            let r = Rect::new([p.x, p.z], p.heading.to_radians(), depth, width);
            Shape::polygon(r.corners().to_vec())
        }
        &PlacementShape::Circle { radius } => Shape::Disc {
            c: [p.x, p.z],
            r: radius,
        },
        PlacementShape::Polygon { points } => Shape::polygon(points.clone()),
    }
}

/// The middle of a box, the origin of the frame a shape is tested in.
fn middle(b: [f64; 4]) -> [f64; 2] {
    [(b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0]
}

/// The cells, `(row, col)`, whose inside the shape reaches by more than
/// `TOUCH_M`: every cell it covers part of, and none it only touches at the
/// cell's edge. A shape that reaches no cell's inside, a point or a line
/// along cell edges, takes the cell holding its middle.
pub(super) fn covered(g: &Grid, s: &Shape) -> Vec<(i64, i64)> {
    let cs = f64::from(g.cell_size);
    let b = s.bbox();
    let o = middle(b);
    let local = s.translated(o);
    let row = |x: f64| ((x - g.origin_x) / cs).floor() as i64;
    let col = |z: f64| ((z - g.origin_z) / cs).floor() as i64;
    // Shrunk by twice the tolerance, so meeting it within the tolerance
    // means reaching past the cell's edge.
    let e = 2.0 * TOUCH_M;
    let mut out = Vec::new();
    for r in row(b[0])..=row(b[2]) {
        for c in col(b[1])..=col(b[3]) {
            let x0 = g.origin_x + r as f64 * cs - o[0];
            let z0 = g.origin_z + c as f64 * cs - o[1];
            let cell = Shape::polygon(vec![
                [x0 + e, z0 + e],
                [x0 + cs - e, z0 + e],
                [x0 + cs - e, z0 + cs - e],
                [x0 + e, z0 + cs - e],
            ]);
            if local.meets(&cell) {
                out.push((r, c));
            }
        }
    }
    if out.is_empty() {
        out.push((row(o[0]), col(o[1])));
    }
    out
}

/// An object's shape: its box turned and placed, or for a tree the circle
/// its box bounds. A model with no box is its position alone.
fn object_shape(pool: Pool, o: &Object) -> Shape {
    let r = footprint(o);
    let c = r.corners();
    match pool {
        Pool::Trees => {
            let [x0, _, z0, x1, _, z1] = o.bbox.map(f64::from);
            Shape::Disc {
                c: [(c[0][0] + c[2][0]) / 2.0, (c[0][1] + c[2][1]) / 2.0],
                r: (x1 - x0).min(z1 - z0).max(0.0) / 2.0,
            }
        }
        _ => Shape::polygon(c.to_vec()),
    }
}

fn scene_pool(p: Pool) -> ScenePool {
    match p {
        Pool::Objects => ScenePool::Objects,
        Pool::Lines => ScenePool::Lines,
        Pool::Trees => ScenePool::Trees,
    }
}

/// An object a shape meets: its pool, its row and its own shape.
type Met = (Pool, SceneryObject, Shape);

/// Every object the shape meets, the nearest `at` first, and how many
/// there are.
fn objects_met(src: &dyn Source, s: &Shape, at: [f64; 2]) -> Result<(Vec<Met>, u32), Vec<Issue>> {
    let b = s.bbox();
    let o = middle(b);
    let local = s.translated(o);
    // Objects are found by position, so the search is widened by the
    // largest model footprint likely, then each is tested exactly.
    let wide = [
        b[0] - OBJECT_REACH_M,
        b[1] - OBJECT_REACH_M,
        b[2] + OBJECT_REACH_M,
        b[3] + OBJECT_REACH_M,
    ];
    let mut found: BTreeMap<(u8, u64), Met> = BTreeMap::new();
    for (k, pool) in Pool::ALL.into_iter().enumerate() {
        src.objects_in(pool, wide, &mut |ob| {
            if !ob.class.is_structure() {
                return;
            }
            let shape = object_shape(pool, ob);
            let ob_b = shape.bbox();
            let apart = ob_b[0] > b[2] + TOUCH_M
                || ob_b[2] < b[0] - TOUCH_M
                || ob_b[1] > b[3] + TOUCH_M
                || ob_b[3] < b[1] - TOUCH_M;
            if !apart && local.meets(&shape.translated(o)) {
                found.insert((k as u8, ob.id), (pool, row(ob), shape));
            }
        })
        .map_err(refused)?;
    }
    let n = found.len() as u32;
    let mut all: Vec<_> = found.into_values().collect();
    let d = |r: &SceneryObject| (r.x - at[0]).hypot(r.z - at[1]);
    all.sort_by(|a, b| d(&a.1).total_cmp(&d(&b.1)));
    all.truncate(OBJECTS_LISTED_MAX);
    Ok((all, n))
}

/// The pavement issue, if the shape meets any of `tris`, the runway
/// surface's triangles under its box: the runway whose centerline is
/// nearest the shape, and how near.
fn pavement(tris: &[[[f64; 2]; 3]], tables: Option<&Tables>, s: &Shape) -> Option<PlacementIssue> {
    let b = s.bbox();
    let o = middle(b);
    let local = s.translated(o);
    let on = tris.iter().any(|t| {
        local.meets(&Shape::polygon(
            t.iter().map(|&p| [p[0] - o[0], p[1] - o[1]]).collect(),
        ))
    });
    if !on {
        return None;
    }
    let nearest = tables.into_iter().flat_map(|t| &t.runways).map(|r| {
        let a = [r.edge1_x - o[0], r.edge1_z - o[1]];
        let e = [r.edge2_x - o[0], r.edge2_z - o[1]];
        (local.distance(&Shape::polygon(vec![a, e, e, a])), r)
    });
    let best = nearest.min_by(|a, b| a.0.total_cmp(&b.0));
    Some(PlacementIssue::Pavement {
        airdrome_id: best.and_then(|(_, r)| r.airdrome_id),
        airdrome: best.map(|(_, r)| r.airdrome_name_id.clone()),
        runway: best.map(|(_, r)| r.name.clone()),
        centerline_m: best.map(|(d, _)| d),
    })
}

/// One sample of the ground: a cell's center or a point's own position,
/// with its ground and slope.
struct Sample {
    at: [f64; 2],
    ground: Option<Ground>,
    slope: Option<f64>,
}

/// The ground issues and figures of one placement from its samples.
fn ground_check(
    p: &Placement,
    samples: &[&Sample],
    issues: &mut Vec<PlacementIssue>,
) -> (Option<f64>, Option<f64>, Vec<CoverClass>) {
    let d = |s: &&Sample| (s.at[0] - p.x).hypot(s.at[1] - p.z);
    // The nearest of those `keep` keeps, and how many it keeps.
    let nearest = |keep: &dyn Fn(&Sample) -> bool| {
        let kept: Vec<&&Sample> = samples.iter().filter(|s| keep(s)).collect();
        let n = kept.len() as u32;
        kept.into_iter()
            .min_by(|a, b| d(a).total_cmp(&d(b)))
            .map(|s| (n, s.at))
    };
    if let Some((cells, [x, z])) = nearest(&|s| s.ground.is_none()) {
        issues.push(PlacementIssue::NoGround { cells, x, z });
    }
    let water = |s: &Sample| s.ground.map(|g| g.water);
    match p.on {
        Underfoot::Land => {
            for w in WaterClass::ALL.into_iter().skip(1) {
                if let Some((cells, [x, z])) = nearest(&|s| water(s) == Some(w.code())) {
                    issues.push(PlacementIssue::Water {
                        water: w,
                        cells,
                        x,
                        z,
                    });
                }
            }
        }
        Underfoot::Water => {
            if let Some((cells, [x, z])) = nearest(&|s| water(s) == Some(0)) {
                issues.push(PlacementIssue::Land { cells, x, z });
            }
        }
    }
    let wet: Vec<(f64, [f64; 2])> = samples
        .iter()
        .filter_map(|s| s.ground.filter(|g| g.water != 0).map(|g| (g.depth, s.at)))
        .collect();
    let min_depth = wet.iter().map(|w| w.0).reduce(f64::min);
    if let (Some(min), Some(depth)) = (p.min_depth, min_depth) {
        let shallow: Vec<&(f64, [f64; 2])> = wet.iter().filter(|w| w.0 < min).collect();
        if let Some(&&(_, [x, z])) = shallow.iter().min_by(|a, b| {
            (a.1[0] - p.x)
                .hypot(a.1[1] - p.z)
                .total_cmp(&(b.1[0] - p.x).hypot(b.1[1] - p.z))
        }) {
            issues.push(PlacementIssue::Shallow {
                depth,
                min_depth: min,
                cells: shallow.len() as u32,
                x,
                z,
            });
        }
    }
    let steepest = samples
        .iter()
        .filter_map(|s| s.slope.map(|v| (v, s.at)))
        .max_by(|a, b| a.0.total_cmp(&b.0));
    if let (Some(max), Some((slope, [x, z]))) = (p.max_slope_deg, steepest)
        && slope > max
    {
        issues.push(PlacementIssue::Slope {
            slope_deg: slope,
            max_slope_deg: max,
            x,
            z,
        });
    }
    let mut cover: Vec<CoverClass> = Vec::new();
    for g in samples.iter().filter_map(|s| s.ground) {
        let c = CoverClass::of(g.cover);
        if !cover.contains(&c) {
            cover.push(c);
        }
    }
    let order = |c: &CoverClass| {
        crate::source::Cover::ALL
            .iter()
            .position(|k| CoverClass::of(*k) == *c)
    };
    cover.sort_by_key(order);
    (steepest.map(|s| s.0), min_depth, cover)
}

pub fn validate_placements(
    terrain: &Terrain,
    q: ValidatePlacementsRequest,
) -> Result<ValidatePlacementsResponse, Vec<Issue>> {
    let src = terrain.source();
    if q.placements.len() > PLACEMENTS_MAX {
        return Err(invalid(
            &format!("at most {PLACEMENTS_MAX} placements"),
            "placements",
        ));
    }
    let issues: Vec<Issue> = (q.placements.iter().enumerate())
        .flat_map(|(k, p)| check_request(p, k))
        .collect();
    if !issues.is_empty() {
        return Err(issues);
    }
    let g = src.grid();
    let cs = f64::from(g.cell_size);
    let shapes: Vec<Shape> = q.placements.iter().map(shape_of).collect();

    // Each placement's samples, a cell read once however many placements
    // cover it.
    let mut at: Vec<[f64; 2]> = Vec::new();
    let mut by_cell: BTreeMap<(i64, i64), usize> = BTreeMap::new();
    let mut taken: Vec<Vec<usize>> = Vec::with_capacity(shapes.len());
    for (p, s) in q.placements.iter().zip(&shapes) {
        if p.shape == PlacementShape::Point || matches!(s, Shape::Disc { r, .. } if *r == 0.0) {
            at.push([p.x, p.z]);
            taken.push(vec![at.len() - 1]);
            continue;
        }
        let cells = covered(&g, s);
        let mut ks = Vec::with_capacity(cells.len());
        for (r, c) in cells {
            let k = *by_cell.entry((r, c)).or_insert_with(|| {
                at.push([
                    g.origin_x + (r as f64 + 0.5) * cs,
                    g.origin_z + (c as f64 + 0.5) * cs,
                ]);
                at.len() - 1
            });
            ks.push(k);
        }
        taken.push(ks);
    }
    let ground = src.points(&at).map_err(refused)?;
    let mut capped = false;
    let slope = derived_values(src, Derived::Slope, &at, &mut capped)?;
    let samples: Vec<Sample> = (0..at.len())
        .map(|k| Sample {
            at: at[k],
            ground: ground[k],
            slope: slope[k],
        })
        .collect();

    let tables = src.tables().ok();
    let boxes: Vec<[f64; 4]> = shapes.iter().map(Shape::bbox).collect();
    let tris = src.runway_triangles_each(&boxes).map_err(refused)?;
    let mut out = Vec::with_capacity(shapes.len());
    let mut met_shapes = Vec::with_capacity(shapes.len());
    for (((p, s), ks), tris) in (q.placements.iter().zip(&shapes).zip(&taken)).zip(&tris) {
        let mut issues = Vec::new();
        let mine: Vec<&Sample> = ks.iter().map(|&k| &samples[k]).collect();
        let (max_slope_deg, min_depth, cover) = ground_check(p, &mine, &mut issues);
        let (objects, n) = objects_met(src, s, [p.x, p.z])?;
        if let Some(i) = pavement(tris, tables, s) {
            issues.push(i);
        }
        for (pool, object, _) in &objects {
            issues.push(PlacementIssue::Scenery {
                pool: scene_pool(*pool),
                object: object.clone(),
            });
        }
        out.push(PlacementCheck {
            id: p.id.clone(),
            ok: issues.is_empty(),
            cells: ks.len() as u32,
            max_slope_deg,
            min_depth,
            cover,
            objects_met: n,
            issues,
        });
        met_shapes.push(objects);
    }

    let geojson = if q.geojson {
        let mut f = Features::new(terrain)?;
        for ((p, s), (check, objects)) in
            (q.placements.iter().zip(&shapes)).zip(out.iter().zip(&met_shapes))
        {
            let kinds: Vec<Value> = (check.issues.iter())
                .map(|i| json!(i)["kind"].clone())
                .collect();
            f.point(
                [p.x, p.z],
                props([
                    ("id", json!(p.id)),
                    ("ok", json!(check.ok)),
                    ("issues", json!(kinds)),
                ]),
            );
            let ring = s.outline();
            if ring.len() >= 3 && signed_area(&ring) != 0.0 {
                f.polygon(
                    &[ring],
                    props([("id", json!(p.id)), ("shape", json!(true))]),
                );
            }
            for (pool, o, shape) in objects {
                let ring = shape.outline();
                if ring.len() >= 3 && signed_area(&ring) != 0.0 {
                    f.polygon(
                        &[ring],
                        props([
                            ("id", json!(p.id)),
                            ("object", json!(o.id)),
                            ("model", json!(o.model)),
                            ("class", json!(o.class)),
                            ("pool", json!(scene_pool(*pool))),
                        ]),
                    );
                }
            }
        }
        Some(f.finish())
    } else {
        None
    };
    Ok(ValidatePlacementsResponse {
        provenance: terrain.provenance(),
        placements: out,
        geojson,
    })
}

#[cfg(test)]
mod tests {
    use super::super::geojson::tests::assert_valid;
    use super::super::shape;
    use super::super::tests::synth;
    use super::*;
    use crate::roads::tests::Unit;
    use crate::synth::*;

    fn form(t: &Terrain) -> Theatre {
        match t {
            Terrain::Synth(s) => *s.form(),
            _ => unreachable!(),
        }
    }

    /// Each placement of `v` checked on `t`.
    fn checked(t: &Terrain, v: Value) -> Vec<PlacementCheck> {
        let q: ValidatePlacementsRequest =
            serde_json::from_value(json!({ "placements": v })).unwrap();
        validate_placements(t, q).unwrap().placements
    }

    fn kinds(c: &PlacementCheck) -> Vec<String> {
        (c.issues.iter())
            .map(|i| json!(i)["kind"].as_str().unwrap().to_string())
            .collect()
    }

    /// A plain, gently sloping, far from water, scenery and roads.
    const PLAIN: (f64, f64) = (40000.0, 25000.0);

    #[test]
    fn every_shape_on_open_ground_has_no_issue() {
        let t = synth();
        let (x, z) = form(&t).abs(PLAIN.0, PLAIN.1);
        let tri = json!([[x, z], [x + 300.0, z + 40.0], [x + 120.0, z + 260.0]]);
        let c = checked(
            &t,
            json!([
                {"id": "p", "x": x, "z": z, "max_slope_deg": 2},
                {"id": "r", "x": x, "z": z, "heading": 30,
                 "shape": {"Rect": {"width": 40, "depth": 120}}, "max_slope_deg": 2},
                {"id": "c", "x": x, "z": z, "shape": {"Circle": {"radius": 200}}, "max_slope_deg": 2},
                {"id": "g", "x": x, "z": z, "shape": {"Polygon": {"points": tri}}, "max_slope_deg": 2},
            ]),
        );
        for k in &c {
            assert!(k.ok, "{k:?}");
            assert!(
                k.max_slope_deg.unwrap() < 1.0 && k.min_depth.is_none(),
                "{k:?}"
            );
        }
        assert_eq!(c[0].cells, 1);
        // 40 × 120 m turned covers several cells; a 200 m circle about 50.
        assert!(c[1].cells >= 4 && (45..=60).contains(&c[2].cells), "{c:?}");
    }

    #[test]
    fn water_land_and_depth_are_found_under_each_shape() {
        let t = synth();
        let f = form(&t);
        let (lx, lz) = f.abs(LAKE_NORTH_M, LAKE_EAST_M);
        let (px, pz) = f.abs(PLAIN.0, PLAIN.1);
        let square = |half: f64| {
            json!([
                [lx - half, lz - half],
                [lx + half, lz - half],
                [lx + half, lz + half],
                [lx - half, lz + half]
            ])
        };
        let c = checked(
            &t,
            json!([
                {"id": "in the lake", "x": lx, "z": lz},
                {"id": "over its shore", "x": lx, "z": lz + LAKE_RADIUS_M,
                 "shape": {"Circle": {"radius": 300}}},
                {"id": "a boat ashore", "x": px, "z": pz, "on": "water"},
                {"id": "too shallow", "x": lx, "z": lz, "on": "water", "min_depth": 13,
                 "shape": {"Polygon": {"points": square(100.0)}}},
                {"id": "deep enough", "x": lx, "z": lz, "on": "water", "min_depth": 11,
                 "shape": {"Polygon": {"points": square(100.0)}}},
                {"id": "a barge on the shore", "x": lx, "z": lz + LAKE_RADIUS_M, "on": "water",
                 "shape": {"Rect": {"width": 200, "depth": 50}}},
            ]),
        );
        let PlacementIssue::Water {
            water,
            cells: 1,
            x,
            z,
        } = c[0].issues[0]
        else {
            panic!("{:?}", c[0]);
        };
        assert_eq!((water, x, z), (WaterClass::Lake, lx, lz));
        assert!((c[0].min_depth.unwrap() - LAKE_DEPTH_M).abs() < 1e-9);
        let PlacementIssue::Water { cells, .. } = c[1].issues[0] else {
            panic!("{:?}", c[1]);
        };
        assert!(cells > 10 && cells < c[1].cells, "{:?}", c[1]);
        assert_eq!(kinds(&c[2]), ["land"]);
        let PlacementIssue::Shallow {
            depth, min_depth, ..
        } = c[3].issues[0]
        else {
            panic!("{:?}", c[3]);
        };
        assert!(
            depth < LAKE_DEPTH_M && depth > 11.5 && min_depth == 13.0,
            "{depth}"
        );
        assert!(c[4].ok, "{:?}", c[4]);
        assert_eq!(kinds(&c[5]), ["land"]);
        assert!(c[5].cover.contains(&CoverClass::Water), "{:?}", c[5]);
    }

    #[test]
    fn a_hill_flank_is_too_steep_and_no_mesh_is_no_ground() {
        let t = synth();
        let f = form(&t);
        // The steepest of a Gaussian hill is a sigma out, about 13.6°.
        let (x, z) = f.abs(HILL_A_NORTH_M, HILL_A_EAST_M + HILL_SIGMA_M);
        let (ox, oz) = f.abs(MESH_MARGIN_M / 2.0, 30000.0);
        let rect = json!({"Rect": {"width": 100, "depth": 100}});
        let c = checked(
            &t,
            json!([
                {"id": "steep", "x": x, "z": z, "shape": rect, "max_slope_deg": 10},
                {"id": "allowed", "x": x, "z": z, "shape": rect, "max_slope_deg": 20},
                {"id": "off", "x": ox, "z": oz},
                {"id": "half off", "x": ox, "z": oz, "shape": {"Circle": {"radius": MESH_MARGIN_M}}},
            ]),
        );
        let PlacementIssue::Slope {
            slope_deg,
            max_slope_deg,
            ..
        } = c[0].issues[0]
        else {
            panic!("{:?}", c[0]);
        };
        assert!(
            (12.0..15.0).contains(&slope_deg) && max_slope_deg == 10.0,
            "{slope_deg}"
        );
        assert!(
            c[1].ok && c[1].max_slope_deg == Some(slope_deg),
            "{:?}",
            c[1]
        );
        assert_eq!(kinds(&c[2]), ["no_ground"]);
        assert!(c[2].max_slope_deg.is_none() && c[2].cover.is_empty());
        let PlacementIssue::NoGround { cells, .. } = c[3].issues[0] else {
            panic!("{:?}", c[3]);
        };
        assert!(cells > 0 && cells < c[3].cells, "{:?}", c[3]);
    }

    /// The first object of `model`, as `scenery_in` reports it.
    fn object(t: &Terrain, model: &str) -> SceneryObject {
        let mut found = None;
        let all = [-1e9, -1e9, 1e9, 1e9];
        t.source()
            .objects_in(Pool::Objects, all, &mut |o| {
                if o.model == model.to_lowercase() && found.is_none() {
                    found = Some(row(o));
                }
            })
            .unwrap();
        found.unwrap()
    }

    #[test]
    fn a_building_is_met_inside_and_edge_to_edge_and_missed_a_millimeter_off() {
        let t = synth();
        let a = object(&t, SCENERY_INDUSTRIAL_MODEL_A);
        let (w, d) = (a.obb_w.unwrap(), a.obb_d.unwrap());
        // Turned 0, the box's local x is north: `obb_w` runs north–south.
        let east = a.z + d / 2.0;
        let rect = |dz: f64| {
            json!({"id": "", "x": a.x, "z": east + 5.0 + dz,
                   "shape": {"Rect": {"width": 10, "depth": 10}}})
        };
        let corner = [a.x + w / 2.0, east];
        let north = a.x + w / 2.0;
        let c = checked(
            &t,
            json!([
                {"id": "over it", "x": a.x, "z": a.z, "heading": 45,
                 "shape": {"Rect": {"width": 4, "depth": 4}}},
                rect(0.0),
                rect(1e-3),
                // A circle whose rim reaches the corner, and one just short.
                {"id": "", "x": corner[0] + 30.0, "z": corner[1] + 40.0,
                 "shape": {"Circle": {"radius": 50.0}}},
                {"id": "", "x": corner[0] + 30.0, "z": corner[1] + 40.0,
                 "shape": {"Circle": {"radius": 49.999}}},
                // A polygon whose side lies along the box's north side.
                {"id": "", "x": a.x, "z": a.z, "shape": {"Polygon": {"points": [
                    [north, a.z - 5.0], [north + 8.0, a.z], [north, a.z + 5.0]
                ]}}},
            ]),
        );
        for (k, want) in [
            (0, true),
            (1, true),
            (2, false),
            (3, true),
            (4, false),
            (5, true),
        ] {
            let met = c[k].issues.iter().any(|i| {
                matches!(i, PlacementIssue::Scenery { pool: ScenePool::Objects, object }
                    if object.id == a.id)
            });
            assert_eq!(met, want, "{k}: {:?}", c[k]);
            assert_eq!(c[k].objects_met, u32::from(want), "{k}");
        }
    }

    #[test]
    fn trees_and_poles_are_met_by_their_own_shapes() {
        let t = synth();
        let f = form(&t);
        // The hole in the forest is 1 km square, its middle on a cell's
        // corner; the trees nearest it stand at the centers of two cells
        // beyond each side, 525 m out and 25 m across, each a 3 m circle.
        let mid = f.abs(
            (FOREST_HOLE_SOUTH_M + FOREST_HOLE_NORTH_M) / 2.0,
            (FOREST_HOLE_WEST_M + FOREST_HOLE_EAST_M) / 2.0,
        );
        let reach = 525.0f64.hypot(25.0) - TREE_OBB_W_M / 2.0;
        let (lx, lz) = f.abs(LINE_SOUTH_M + 2.0 * LINE_POLE_SPACING_M, LINE_EAST_M);
        let c = checked(
            &t,
            json!([
                {"id": "", "x": mid.0, "z": mid.1, "shape": {"Circle": {"radius": reach - 0.01}}},
                {"id": "", "x": mid.0, "z": mid.1, "shape": {"Circle": {"radius": reach}}},
                {"id": "", "x": lx, "z": lz},
                {"id": "", "x": lx + 1.0, "z": lz},
            ]),
        );
        assert!(
            c[0].issues.is_empty() && c[0].cover.contains(&CoverClass::Forest),
            "{:?}",
            c[0]
        );
        // Eight cells, two beyond each side, three trees each.
        assert_eq!(c[1].objects_met, 8 * TREES_PER_CELL, "{:?}", c[1]);
        assert!(c[1].issues.iter().all(|i| matches!(
            i,
            PlacementIssue::Scenery {
                pool: ScenePool::Trees,
                ..
            }
        )));
        assert!(
            matches!(
                &c[2].issues[..],
                [PlacementIssue::Scenery {
                    pool: ScenePool::Lines,
                    ..
                }]
            ),
            "{:?}",
            c[2]
        );
        assert!(c[3].ok, "{:?}", c[3]);
    }

    #[test]
    fn the_runway_is_pavement_up_to_its_edge() {
        let t = synth();
        let f = form(&t);
        let (x, z) = f.abs(RUNWAY_NORTH_M, RUNWAY_EAST_M);
        let edge = x + RUNWAY_WIDTH_M / 2.0;
        let rect = json!({"Rect": {"width": 10, "depth": 10}});
        let c = checked(
            &t,
            json!([
                {"id": "", "x": x, "z": z, "shape": {"Circle": {"radius": 10}}},
                {"id": "", "x": edge + 5.0, "z": z, "shape": rect},
                {"id": "", "x": edge + 5.001, "z": z, "shape": rect},
                {"id": "", "x": x, "z": z},
            ]),
        );
        let name = format!("{RUNWAY_EDGE1_NAME}-{RUNWAY_EDGE2_NAME}");
        for (k, centerline) in [(0, 0.0), (1, RUNWAY_WIDTH_M / 2.0), (3, 0.0)] {
            let i = c[k].issues.iter().find_map(|i| match i {
                PlacementIssue::Pavement {
                    airdrome,
                    runway,
                    centerline_m,
                    ..
                } => Some((airdrome.clone(), runway.clone(), *centerline_m)),
                _ => None,
            });
            let (airdrome, runway, d) = i.unwrap_or_else(|| panic!("{k}: {:?}", c[k]));
            assert_eq!(airdrome.as_deref(), Some(AIRDROME_NAME_ID));
            assert_eq!(runway, Some(name.clone()));
            assert!((d.unwrap() - centerline).abs() < 1e-9, "{k}: {d:?}");
        }
        assert!(
            !kinds(&c[2]).contains(&"pavement".to_string()),
            "{:?}",
            c[2]
        );
    }

    #[test]
    fn refuses_what_cannot_be_placed() {
        let t = synth();
        let e = |v: Value| {
            let q: ValidatePlacementsRequest =
                serde_json::from_value(json!({ "placements": [v] })).unwrap();
            let e = validate_placements(&t, q).unwrap_err();
            (e[0].code.clone(), e[0].path.clone())
        };
        let bad = |path: &str| {
            (
                "invalid_request".to_string(),
                format!("placements.0.{path}"),
            )
        };
        assert_eq!(
            e(json!({"id": "", "x": 0, "z": 0, "shape": {"Rect": {"width": 10001, "depth": 1}}})),
            bad("shape")
        );
        assert_eq!(
            e(json!({"id": "", "x": 0, "z": 0, "shape": {"Circle": {"radius": -1}}})),
            bad("shape")
        );
        assert_eq!(
            e(
                json!({"id": "", "x": 0, "z": 0, "shape": {"Polygon": {"points": [[0, 0], [1, 1]]}}})
            ),
            bad("shape")
        );
        assert_eq!(
            e(json!({"id": "", "x": 0, "z": 0, "max_slope_deg": 91})),
            bad("max_slope_deg")
        );
        assert_eq!(
            e(json!({"id": "", "x": 0, "z": 0, "min_depth": 3})),
            bad("min_depth")
        );
    }

    #[test]
    fn geojson_holds_each_placement_its_shape_and_what_it_meets() {
        let t = synth();
        let a = object(&t, SCENERY_INDUSTRIAL_MODEL_A);
        let q: ValidatePlacementsRequest = serde_json::from_value(json!({
            "geojson": true,
            "placements": [
                {"id": "a", "x": a.x, "z": a.z, "shape": {"Circle": {"radius": 5}}},
                {"id": "b", "x": a.x, "z": a.z},
            ]
        }))
        .unwrap();
        let v = validate_placements(&t, q).unwrap().geojson.unwrap();
        assert_valid(&v);
        let f = v["features"].as_array().unwrap();
        let types: Vec<&str> = (f.iter())
            .map(|f| f["geometry"]["type"].as_str().unwrap())
            .collect();
        // a's point, its circle, the building; b's point, the building.
        assert_eq!(types, ["Point", "Polygon", "Polygon", "Point", "Polygon"]);
        assert_eq!(f[0]["properties"]["issues"], json!(["scenery"]));
        let ring = f[1]["geometry"]["coordinates"][0].as_array().unwrap();
        assert_eq!(ring.len(), shape::DISC_SIDES + 1);
        assert_eq!(f[2]["properties"]["model"], json!(a.model));
    }

    /// The area of the polygon inside the cell from `(x0, z0)`, `cs`
    /// square: the polygon clipped to each side of the cell in turn.
    fn clipped_area(points: &[[f64; 2]], x0: f64, z0: f64, cs: f64) -> f64 {
        let mut p = points.to_vec();
        let sides = [
            (0, x0, true),
            (0, x0 + cs, false),
            (1, z0, true),
            (1, z0 + cs, false),
        ];
        for (k, v, above) in sides {
            let keep = |q: &[f64; 2]| if above { q[k] >= v } else { q[k] <= v };
            let mut out = Vec::new();
            for i in 0..p.len() {
                let (a, b) = (p[i], p[(i + 1) % p.len()]);
                if keep(&a) {
                    out.push(a);
                }
                if keep(&a) != keep(&b) {
                    let t = (v - a[k]) / (b[k] - a[k]);
                    out.push([a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])]);
                }
            }
            p = out;
            if p.is_empty() {
                return 0.0;
            }
        }
        signed_area(&p).abs() / 2.0
    }

    #[test]
    fn coverage_is_every_cell_whose_inside_the_shape_reaches() {
        // Against each cell's area clipped by the shape for boxes, and for
        // discs the distance from the center to the cell; a cell the shape
        // reaches by under a millimeter either way is not judged.
        let t = synth();
        let g = t.source().grid();
        let cs = f64::from(g.cell_size);
        let mut u = Unit(0x5851_f42d_4c95_7f2d);
        let (mut judged, mut taken) = (0, 0);
        for k in 0..400 {
            let c = [
                g.origin_x + 30000.0 + u.next() * 500.0,
                g.origin_z + 30000.0 + u.next() * 500.0,
            ];
            let s = if k % 2 == 0 {
                let along = 1.0 + u.next() * 200.0;
                let r = Rect::new(c, u.next() * 6.3, along, 1.0 + u.next() * 200.0);
                Shape::polygon(r.corners().to_vec())
            } else {
                Shape::Disc {
                    c,
                    r: 1.0 + u.next() * 150.0,
                }
            };
            let got = covered(&g, &s);
            let b = s.bbox();
            let row = |x: f64| ((x - g.origin_x) / cs).floor() as i64;
            let col = |z: f64| ((z - g.origin_z) / cs).floor() as i64;
            for r in row(b[0]) - 1..=row(b[2]) + 1 {
                for q in col(b[1]) - 1..=col(b[3]) + 1 {
                    let (x0, z0) = (g.origin_x + r as f64 * cs, g.origin_z + q as f64 * cs);
                    let reach = match &s {
                        // The area over a sliver as thin as the cell is wide.
                        Shape::Polygon { points, .. } => clipped_area(points, x0, z0, cs) / cs,
                        Shape::Disc { c, r } => {
                            let dx = c[0] - c[0].clamp(x0, x0 + cs);
                            let dz = c[1] - c[1].clamp(z0, z0 + cs);
                            r - dx.hypot(dz)
                        }
                    };
                    if reach.abs() < 1e-3 {
                        continue;
                    }
                    judged += 1;
                    taken += usize::from(reach > 0.0);
                    assert_eq!(
                        got.contains(&(r, q)),
                        reach > 0.0,
                        "{k}: cell {r} {q} of {s:?}"
                    );
                }
            }
        }
        assert!(
            judged > 8_000 && taken > 4_000,
            "{judged} judged, {taken} taken"
        );
        // Edge-aligned, a cell's square covers that cell alone.
        let s = Shape::polygon(vec![
            [g.origin_x + 1000.0, g.origin_z + 1000.0],
            [g.origin_x + 1050.0, g.origin_z + 1000.0],
            [g.origin_x + 1050.0, g.origin_z + 1050.0],
            [g.origin_x + 1000.0, g.origin_z + 1050.0],
        ]);
        assert_eq!(covered(&g, &s), [(20, 20)]);
    }
}
