//! `scenery_in`: the scenery objects in a region, or overlapping a placed
//! footprint.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::geojson::{Features, Ring, props};
use super::{Terrain, refused, resolve};
use crate::classes::Class;
use crate::decode::scn5::Pool;
use crate::source::Object;
use crate::types::{
    GeoJson, Issue, ScenePool, SceneryInRequest, SceneryInResponse, SceneryObject, SceneryRow,
};

/// A rectangle in the world: its center, its local x axis as a unit vector
/// in DCS `(x, z)`, and its half extents along that axis and across it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Rect {
    c: [f64; 2],
    u: [f64; 2],
    half: [f64; 2],
}

impl Rect {
    /// A rectangle whose local x axis points `bearing` radians from north
    /// towards east, `along` long on it and `across` wide, centered on `c`.
    pub(super) fn new(c: [f64; 2], bearing: f64, along: f64, across: f64) -> Rect {
        Rect {
            c,
            u: [bearing.cos(), bearing.sin()],
            half: [along / 2.0, across / 2.0],
        }
    }

    /// The local z axis: the x axis turned a quarter clockwise, east at a
    /// bearing of zero.
    fn v(&self) -> [f64; 2] {
        [-self.u[1], self.u[0]]
    }

    /// Half its extent projected on the unit axis `a`.
    fn radius_on(&self, a: [f64; 2]) -> f64 {
        let dot = |p: [f64; 2]| p[0] * a[0] + p[1] * a[1];
        self.half[0] * dot(self.u).abs() + self.half[1] * dot(self.v()).abs()
    }

    /// Whether the two meet, edges touching included: separating axes.
    pub(super) fn meets(&self, o: &Rect) -> bool {
        let d = [o.c[0] - self.c[0], o.c[1] - self.c[1]];
        [self.u, self.v(), o.u, o.v()].iter().all(|&a| {
            let gap = (d[0] * a[0] + d[1] * a[1]).abs();
            gap <= self.radius_on(a) + o.radius_on(a) + 1e-9
        })
    }

    /// The box of the rectangle, min x, min z, max x, max z.
    pub(super) fn bbox(&self) -> [f64; 4] {
        let rx = self.radius_on([1.0, 0.0]);
        let rz = self.radius_on([0.0, 1.0]);
        [
            self.c[0] - rx,
            self.c[1] - rz,
            self.c[0] + rx,
            self.c[1] + rz,
        ]
    }

    /// Whether the point lies in the rectangle, its edge included.
    pub(super) fn contains(&self, p: [f64; 2]) -> bool {
        let d = [p[0] - self.c[0], p[1] - self.c[1]];
        let v = self.v();
        (d[0] * self.u[0] + d[1] * self.u[1]).abs() <= self.half[0] + 1e-9
            && (d[0] * v[0] + d[1] * v[1]).abs() <= self.half[1] + 1e-9
    }

    /// Its four corners, in order around it.
    pub(super) fn corners(&self) -> [[f64; 2]; 4] {
        let v = self.v();
        let at = |a: f64, b: f64| {
            [
                self.c[0] + a * self.half[0] * self.u[0] + b * self.half[1] * v[0],
                self.c[1] + a * self.half[0] * self.u[1] + b * self.half[1] * v[1],
            ]
        };
        [at(1.0, 1.0), at(1.0, -1.0), at(-1.0, -1.0), at(-1.0, 1.0)]
    }
}

/// An object's footprint: its model's box, turned and placed.
pub(super) fn footprint(o: &Object) -> Rect {
    let [x0, _, z0, x1, _, z1] = o.bbox.map(f64::from);
    let (cl, cr) = ((x0 + x1) / 2.0, (z0 + z1) / 2.0);
    let u = [o.rotation.cos(), o.rotation.sin()];
    let v = [-u[1], u[0]];
    let c = [
        o.pos[0] + cl * u[0] + cr * v[0],
        o.pos[2] + cl * u[1] + cr * v[1],
    ];
    Rect::new(c, o.rotation, x1 - x0, z1 - z0)
}

/// How far past a footprint an object may stand and still reach into it:
/// the largest model footprint likely. Objects are found by position, so a
/// search for those meeting a footprint is widened this far.
pub(super) const OBJECT_REACH_M: f64 = 500.0;

/// Whether an object found in the search box is in the answer.
type Test = Box<dyn Fn(&Object) -> bool>;

pub(super) fn row(o: &Object) -> SceneryObject {
    let [x0, _, z0, x1, y1, z1] = o.bbox.map(f64::from);
    let flat = x1 - x0 == 0.0 && z1 - z0 == 0.0;
    SceneryObject {
        id: o.id,
        model: o.model.to_string(),
        class: o.class.name().to_string(),
        x: o.pos[0],
        z: o.pos[2],
        alt: o.pos[1],
        rotation_deg: o.rotation.to_degrees(),
        obb_w: (!flat).then_some(x1 - x0),
        obb_d: (!flat).then_some(z1 - z0),
        height: y1,
    }
}

pub(super) fn scene_pool(p: Pool) -> ScenePool {
    match p {
        Pool::Objects => ScenePool::Objects,
        Pool::Lines => ScenePool::Lines,
        Pool::Trees => ScenePool::Trees,
    }
}

pub fn scenery_in(terrain: &Terrain, q: SceneryInRequest) -> Result<SceneryInResponse, Vec<Issue>> {
    let src = terrain.source();
    let pools: Vec<Pool> = match &q.pools {
        None => vec![Pool::Objects],
        Some(ps) if ps.is_empty() => {
            return Err(vec![Issue::new(
                "invalid_request",
                "name at least one pool, or leave pools out for the objects pool",
                "pools",
            )]);
        }
        Some(ps) => (Pool::ALL.into_iter())
            .filter(|p| ps.contains(&scene_pool(*p)))
            .collect(),
    };
    let classes: Option<Vec<Class>> = match &q.classes {
        None => None,
        Some(names) => Some(
            names
                .iter()
                .enumerate()
                .map(|(k, n)| {
                    Class::ALL
                        .into_iter()
                        .find(|c| c.name() == n)
                        .ok_or_else(|| {
                            let all: Vec<_> = Class::ALL.iter().map(|c| c.name()).collect();
                            vec![Issue::new(
                                "unknown_class",
                                &format!("no class `{n}`; the classes are {}", all.join(", ")),
                                &format!("classes.{k}"),
                            )]
                        })
                })
                .collect::<Result<_, _>>()?,
        ),
    };
    let wanted = |c: Class| match &classes {
        Some(cs) => cs.contains(&c),
        None => c.is_structure(),
    };
    // A box around the region, a test of each object found in it, and the
    // shape asked about, to draw.
    let (bbox, test, asked): ([f64; 4], Test, (&str, Vec<Ring>)) =
        match (&q.region, &q.footprint_at) {
            (Some(r), None) => {
                let area = resolve(src, r, "region")?;
                let outline = area.outline();
                (
                    area.bbox,
                    Box::new(move |o| area.contains(o.pos[0], o.pos[2])),
                    ("region", outline),
                )
            }
            (None, Some(f)) => {
                let fp = &f.footprint;
                let ok = [f.x, f.z, fp.width, fp.depth, f.orientation_deg]
                    .iter()
                    .all(|v| v.is_finite())
                    && fp.width >= 0.0
                    && fp.depth >= 0.0;
                if !ok {
                    return Err(vec![Issue::new(
                        "invalid_request",
                        "a footprint needs a finite position, orientation and size",
                        "footprint_at",
                    )]);
                }
                let rect = Rect::new(
                    [f.x, f.z],
                    f.orientation_deg.to_radians(),
                    fp.depth,
                    fp.width,
                );
                // Objects are found by position; one whose box reaches in from
                // outside is found by widening the search by the largest model
                // footprint likely, then tested exactly.
                let [x0, z0, x1, z1] = rect.bbox();
                (
                    [
                        x0 - OBJECT_REACH_M,
                        z0 - OBJECT_REACH_M,
                        x1 + OBJECT_REACH_M,
                        z1 + OBJECT_REACH_M,
                    ],
                    Box::new(move |o| rect.meets(&footprint(o))),
                    ("footprint", vec![rect.corners().to_vec()]),
                )
            }
            _ => {
                return Err(vec![Issue::new(
                    "invalid_request",
                    "give either region or footprint_at",
                    "",
                )]);
            }
        };
    // Rows in pool and id order. A pool lists each id once, and its index
    // finds each once, so a count needs no row: a forest's trees are
    // counted without holding them.
    let mut found: BTreeMap<(ScenePool, u64), (SceneryObject, Rect)> = BTreeMap::new();
    let mut counts: BTreeMap<ScenePool, BTreeMap<&'static str, u64>> = BTreeMap::new();
    let mut over = false;
    for pool in pools {
        let p = scene_pool(pool);
        let counted = counts.entry(p).or_default();
        src.objects_in(pool, bbox, &mut |o| {
            if over || !wanted(o.class) || !test(o) {
                return;
            }
            *counted.entry(o.class.name()).or_insert(0) += 1;
            if !q.counts_only {
                found.insert((p, o.id), (row(o), footprint(o)));
                over = found.len() > q.max_rows as usize;
            }
        })
        .map_err(refused)?;
    }
    let (objects, boxes): (Vec<SceneryRow>, Vec<Rect>) = (found.into_iter())
        .map(|((pool, _), (object, b))| (SceneryRow { object, pool }, b))
        .unzip();
    if over {
        return Err(vec![Issue::new(
            "too_many_rows",
            &format!(
                "more than {} objects; raise max_rows, name classes, shrink the region, or ask counts_only for counts alone",
                q.max_rows
            ),
            "max_rows",
        )]);
    }
    let mut counts_by_class = BTreeMap::new();
    for (class, n) in counts.values().flatten() {
        *counts_by_class.entry(class.to_string()).or_insert(0) += n;
    }
    let counts_by_pool = (counts.into_iter())
        .map(|(p, cs)| (p, cs.into_iter().map(|(c, n)| (c.to_string(), n)).collect()))
        .collect();
    let geojson = if q.geojson {
        Some(features(terrain, &objects, &boxes, asked)?)
    } else {
        None
    };
    Ok(SceneryInResponse {
        provenance: terrain.provenance(),
        objects,
        counts_by_class,
        counts_by_pool,
        geojson,
    })
}

/// Each object's turned box as a polygon with its fields, or a point where
/// its model has no box or a box too thin to fill, and the region or
/// footprint asked about unless it has no area.
fn features(
    terrain: &Terrain,
    objects: &[SceneryRow],
    boxes: &[Rect],
    (asked, rings): (&str, Vec<Ring>),
) -> Result<GeoJson, Vec<Issue>> {
    let mut f = Features::new(terrain)?;
    for (o, b) in objects.iter().zip(boxes) {
        let Value::Object(mut fields) = json!(o) else {
            unreachable!("an object serializes to an object")
        };
        for k in ["x", "z"] {
            fields.shift_remove(k);
        }
        let ring = vec![b.corners().to_vec()];
        if f.fillable(&ring) {
            f.polygon(&ring, fields);
        } else {
            f.point([o.object.x, o.object.z], fields);
        }
    }
    if f.fillable(&rings) {
        f.polygon(&rings, props([(asked, json!(true))]));
    }
    Ok(f.finish())
}

#[cfg(test)]
mod tests {
    use super::super::answer;
    use super::super::geojson::signed_area;
    use super::super::geojson::tests::assert_valid;
    use super::super::tests::synth;
    use super::*;
    use crate::synth::*;
    use serde_json::{Value, json};

    fn form(t: &Terrain) -> Theatre {
        match t {
            Terrain::Synth(s) => *s.form(),
            _ => unreachable!(),
        }
    }

    #[test]
    fn rectangles_meet_by_their_turned_boxes() {
        let a = Rect::new([0.0, 0.0], 0.0, 10.0, 10.0);
        assert!(a.meets(&Rect::new([10.0, 0.0], 0.0, 10.0, 10.0)));
        assert!(!a.meets(&Rect::new([10.1, 0.0], 0.0, 10.0, 10.0)));
        // Turned 45°, a square's corner reaches 7.07 m from its center.
        let d = Rect::new([12.0, 0.0], std::f64::consts::FRAC_PI_4, 10.0, 10.0);
        assert!(a.meets(&d));
        let far = Rect::new([12.1, 0.0], std::f64::consts::FRAC_PI_4, 10.0, 10.0);
        assert!(!a.meets(&far));
        // Corner to corner diagonally apart: the boxes meet, the shapes not.
        let b = Rect::new([9.0, 9.0], std::f64::consts::FRAC_PI_4, 4.0, 4.0);
        assert!(!a.meets(&b));
    }

    #[test]
    fn a_region_counts_the_generators_objects() {
        let t = synth();
        let f = form(&t);
        let (x, z) = f.abs(SCENERY_TOWN_NORTH_M, SCENERY_TOWN_EAST_M);
        let s = SCENERY_TOWN_SPREAD_M;
        let region =
            json!({"Box": {"min_x": x - s, "min_z": z - s, "max_x": x + s, "max_z": z + s}});
        let v = answer(&t, "scenery_in", &json!({"region": region})).unwrap();
        assert_eq!(v["counts_by_class"]["building"], SCENERY_TOWN_COUNT);
        let objects = v["objects"].as_array().unwrap();
        let o = objects.iter().find(|o| o["class"] == "building").unwrap();
        assert_eq!(o["obb_w"], SCENERY_TOWN_OBB_W_M);
        assert_eq!(o["obb_d"], SCENERY_TOWN_OBB_D_M);
        assert_eq!(o["height"], SCENERY_TOWN_HEIGHT_M);
        let ids: Vec<u64> = objects.iter().map(|o| o["id"].as_u64().unwrap()).collect();
        assert!(ids.windows(2).all(|w| w[0] < w[1]));

        // The whole terrain holds every object, the box-less ones without a box.
        let all = json!({"Box": {"min_x": -1e6, "min_z": -1e6, "max_x": 1e6, "max_z": 1e6}});
        let v = answer(&t, "scenery_in", &json!({"region": all})).unwrap();
        let n: u64 = v["counts_by_class"]
            .as_object()
            .unwrap()
            .values()
            .map(|c| c.as_u64().unwrap())
            .sum();
        assert_eq!(n, u64::from(SCENERY_COUNT));
        let nobox = v["objects"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["model"] == SCENERY_NOBOX_MODEL.to_lowercase())
            .unwrap();
        assert_eq!(nobox["obb_w"], Value::Null);
        let only = json!({"region": all, "classes": ["industrial"]});
        let v = answer(&t, "scenery_in", &only).unwrap();
        assert_eq!(
            v["objects"].as_array().unwrap().len(),
            SCENERY_INDUSTRIAL_COUNT as usize
        );
    }

    #[test]
    fn pools_list_trees_and_poles_and_counts_only_counts_past_the_cap() {
        let t = synth();
        let f = form(&t);
        let all = json!({"Box": {"min_x": -1e6, "min_z": -1e6, "max_x": 1e6, "max_z": 1e6}});
        let sum = |v: &Value| -> u64 {
            let n = v.as_object().unwrap().values();
            n.map(|c| c.as_u64().unwrap()).sum()
        };

        // Every tree counted, none listed, a row cap of one ignored.
        let census = json!({"region": all, "pools": ["trees"], "counts_only": true, "max_rows": 1});
        let v = answer(&t, "scenery_in", &census).unwrap();
        assert_eq!(v["objects"], json!([]));
        assert_eq!(
            sum(&v["counts_by_pool"]["trees"]),
            u64::from(f.tree_count())
        );
        assert_eq!(sum(&v["counts_by_class"]), u64::from(f.tree_count()));
        assert!(v["counts_by_pool"].get("objects").is_none(), "{v}");
        // Drawn, a count is the region alone.
        let drawn =
            json!({"region": all, "pools": ["trees"], "counts_only": true, "geojson": true});
        let v = answer(&t, "scenery_in", &drawn).unwrap();
        assert_valid(&v["geojson"]);
        let features = v["geojson"]["features"].as_array().unwrap();
        assert_eq!(features.len(), 1);
        assert_eq!(features[0]["properties"], json!({"region": true}));
        let listed = json!({"region": all, "pools": ["trees"], "max_rows": 1});
        let e = answer(&t, "scenery_in", &listed).unwrap_err();
        assert_eq!(e[0].code, "too_many_rows");

        // Every pole and every object, in pool order, each row naming its
        // pool; the counts are the rows'.
        let v = answer(
            &t,
            "scenery_in",
            &json!({"region": all, "pools": ["lines", "objects"]}),
        )
        .unwrap();
        let rows = v["objects"].as_array().unwrap();
        let poles = f.pole_norths().count();
        let pools: Vec<&str> = rows.iter().map(|o| o["pool"].as_str().unwrap()).collect();
        assert_eq!(pools.len(), SCENERY_COUNT as usize + poles);
        assert!(
            pools[..SCENERY_COUNT as usize]
                .iter()
                .all(|p| *p == "objects")
        );
        assert!(
            pools[SCENERY_COUNT as usize..]
                .iter()
                .all(|p| *p == "lines")
        );
        assert_eq!(sum(&v["counts_by_pool"]["lines"]), poles as u64);
        assert_eq!(
            sum(&v["counts_by_pool"]["objects"]),
            u64::from(SCENERY_COUNT)
        );
        assert_eq!(sum(&v["counts_by_class"]), rows.len() as u64);
        assert!(rows.iter().all(|o| o["model"] != json!(null)));

        let none = json!({"region": all, "pools": []});
        assert_eq!(
            answer(&t, "scenery_in", &none).unwrap_err()[0].path,
            "pools"
        );
    }

    #[test]
    fn a_footprint_takes_what_its_turned_box_overlaps() {
        let t = synth();
        let f = form(&t);
        let (x, z) = f.abs(SCENERY_INDUSTRIAL_NORTH_M, SCENERY_INDUSTRIAL_EAST_M);
        let at = |dz: f64, width: f64, deg: f64| {
            let req = json!({"footprint_at": {
                "x": x, "z": z + dz, "orientation_deg": deg,
                "footprint": {"width": width, "depth": 10}
            }});
            let v = answer(&t, "scenery_in", &req).unwrap();
            v["objects"]
                .as_array()
                .unwrap()
                .iter()
                .map(|o| o["model"].as_str().unwrap().to_string())
                .collect::<Vec<_>>()
        };
        let a = SCENERY_INDUSTRIAL_MODEL_A.to_lowercase();
        let b = SCENERY_INDUSTRIAL_MODEL_B.to_lowercase();
        // Between the two, a narrow footprint touches neither; widened east
        // and west it reaches both.
        let gap = SCENERY_INDUSTRIAL_SPACING_M / 2.0;
        assert!(at(gap, 2.0, 0.0).is_empty());
        assert_eq!(at(gap, 120.0, 0.0), [a.clone(), b.clone()]);
        // Turned a quarter, the same footprint lies north to south and
        // misses both.
        assert!(at(gap, 120.0, 90.0).is_empty());
        assert_eq!(at(0.0, 2.0, 0.0), [a]);
    }

    #[test]
    fn geojson_draws_each_box_and_the_shape_asked_about() {
        let t = synth();
        let all = json!({"Box": {"min_x": -1e6, "min_z": -1e6, "max_x": 1e6, "max_z": 1e6}});
        let v = answer(&t, "scenery_in", &json!({"region": all, "geojson": true})).unwrap();
        assert_valid(&v["geojson"]);
        let features = v["geojson"]["features"].as_array().unwrap();
        let objects = v["objects"].as_array().unwrap();
        assert_eq!(features.len(), objects.len() + 1);
        let tm = t.source().tables().unwrap().crs.tmerc();
        for (f, o) in features.iter().zip(objects) {
            let p = &f["properties"];
            assert_eq!(p["id"], o["id"]);
            assert_eq!(p["model"], o["model"]);
            assert!(p.get("x").is_none());
            if o["obb_w"].is_null() {
                assert_eq!(f["geometry"]["type"], "Point");
                continue;
            }
            assert_eq!(f["geometry"]["type"], "Polygon");
            // Back in DCS meters, the ring is the turned box: its area the
            // model's.
            let ring: Vec<[f64; 2]> = f["geometry"]["coordinates"][0]
                .as_array()
                .unwrap()
                .iter()
                .map(|q| {
                    let (x, z) = tm.to_xz(q[1].as_f64().unwrap(), q[0].as_f64().unwrap());
                    [x, z]
                })
                .collect();
            let area = signed_area(&ring[..4]).abs() / 2.0;
            let want = o["obb_w"].as_f64().unwrap() * o["obb_d"].as_f64().unwrap();
            assert!(
                (area - want).abs() < 0.01 * want + 0.5,
                "{area} against {want}"
            );
        }
        let nobox = objects.iter().filter(|o| o["obb_w"].is_null()).count();
        assert_eq!(nobox, SCENERY_NOBOX_COUNT as usize);
        let last = &features[objects.len()];
        assert_eq!(last["geometry"]["type"], "Polygon");
        assert_eq!(last["properties"], json!({"region": true}));

        // A footprint is drawn as asked; one with no area is left out.
        let f = form(&t);
        let (x, z) = f.abs(SCENERY_INDUSTRIAL_NORTH_M, SCENERY_INDUSTRIAL_EAST_M);
        let at = |width: f64| {
            let req = json!({"footprint_at": {"x": x, "z": z, "orientation_deg": 30,
                             "footprint": {"width": width, "depth": 10}}, "geojson": true});
            let v = answer(&t, "scenery_in", &req).unwrap();
            assert_valid(&v["geojson"]);
            v
        };
        let v = at(20.0);
        let features = v["geojson"]["features"].as_array().unwrap();
        assert_eq!(features.len(), v["objects"].as_array().unwrap().len() + 1);
        let last = features.last().unwrap();
        assert_eq!(last["properties"], json!({"footprint": true}));
        let v = at(0.0);
        let features = v["geojson"]["features"].as_array().unwrap();
        assert_eq!(features.len(), v["objects"].as_array().unwrap().len());
    }

    #[test]
    fn a_region_with_no_area_once_written_is_not_drawn() {
        let t = synth();
        let drawn = |region: Value| {
            let v = answer(
                &t,
                "scenery_in",
                &json!({"region": region, "geojson": true}),
            )
            .unwrap();
            assert_valid(&v["geojson"]);
            v["geojson"]["features"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f["properties"]["region"] == true)
        };
        // Collinear, its area float noise; a box thinner than the rounding;
        // a band whose hole fills it.
        let line = json!({"Polygon": {"points": [[0.1, 0.7], [1000.4, 334.0], [2000.7, 667.3]]}});
        assert!(!drawn(line));
        let thin =
            json!({"Box": {"min_x": 1000, "min_z": 1000, "max_x": 1000.0001, "max_z": 3000}});
        assert!(!drawn(thin));
        let band = json!({"AirdromeBand": {"airdrome": 1, "min_dist": 900, "max_dist": 900}});
        assert!(!drawn(band));
        // A polygon its caller closed is closed once.
        let closed = json!({"Polygon": {"points": [[0, 0], [1000, 0], [1000, 1000], [0, 0]]}});
        let v = answer(
            &t,
            "scenery_in",
            &json!({"region": closed, "geojson": true}),
        )
        .unwrap();
        let ring =
            v["geojson"]["features"].as_array().unwrap().last().unwrap()["geometry"]["coordinates"]
                [0]
            .clone();
        assert_eq!(ring.as_array().unwrap().len(), 4);
    }

    #[test]
    fn over_the_cap_is_an_error() {
        let t = synth();
        let all = json!({"Box": {"min_x": -1e6, "min_z": -1e6, "max_x": 1e6, "max_z": 1e6}});
        let e = answer(&t, "scenery_in", &json!({"region": all, "max_rows": 5})).unwrap_err();
        assert_eq!(e[0].code, "too_many_rows");
        let e = answer(&t, "scenery_in", &json!({})).unwrap_err();
        assert_eq!(e[0].code, "invalid_request");
        let e = answer(
            &t,
            "scenery_in",
            &json!({"region": all, "classes": ["castle"]}),
        )
        .unwrap_err();
        assert_eq!(
            (e[0].code.as_str(), e[0].path.as_str()),
            ("unknown_class", "classes.0")
        );
    }
}
