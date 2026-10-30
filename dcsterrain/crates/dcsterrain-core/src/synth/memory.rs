//! The synthetic terrain as a `Source`, every tile computed from the closed
//! form when it is asked for.

use super::*;
use crate::grid::Grid;
use std::sync::OnceLock;

use crate::classes::{Class, Classes};
use crate::decode::scn5::{Model, Pool};
use crate::roads::Graph;
use crate::source::{Ground, Identity, Layer, Object, Piece, Refusal, Source, TILE_SIZE, Tables};

/// The terrain `--terrain synth` names.
pub const TERRAIN_ID: &str = "synth";
/// The first id of the power-line poles, and of the trees, as objects.
pub const LINES_FIRST_ID: u64 = 80_000_001;
pub const TREES_FIRST_ID: u64 = 90_000_001;

#[derive(Debug, Clone)]
pub struct Synth {
    form: Theatre,
    identity: Identity,
    roads: OnceLock<Graph>,
    betweenness: OnceLock<Vec<f32>>,
    tables: OnceLock<Tables>,
}

impl Synth {
    pub fn new(seed: u32, size_km: u32, cell_size: u32) -> Result<Synth, Refusal> {
        let form = Theatre::new(seed, size_km, cell_size).map_err(|e| Refusal(e.0))?;
        Ok(Synth {
            form,
            identity: Identity {
                terrain: TERRAIN_ID.to_string(),
                dcs_build: DCS_BUILD.to_string(),
                // No files answered, so the settings that made the terrain
                // stand in for their sizes.
                fingerprint: format!("synth-{seed}-{size_km}-{cell_size}"),
            },
            roads: OnceLock::new(),
            betweenness: OnceLock::new(),
            tables: OnceLock::new(),
        })
    }

    /// The closed form behind the tiles, for a test's expected values.
    pub fn form(&self) -> &Theatre {
        &self.form
    }

    /// A cell's value in one layer by the closed form, the layer's `nodata`
    /// past the grid or outside the mesh.
    pub fn cell(&self, layer: Layer, row: i64, col: i64) -> i32 {
        let n = i64::from(self.form.cells());
        if !(0..n).contains(&row) || !(0..n).contains(&col) {
            return layer.nodata();
        }
        let c = self.form.cell(row as u32, col as u32);
        match layer {
            Layer::Height => i32::from(c.height),
            Layer::Water => i32::from(c.water),
            Layer::Surface => i32::from(c.surface),
            Layer::Cover => i32::from(c.cover),
        }
    }
}

impl Source for Synth {
    fn identity(&self) -> &Identity {
        &self.identity
    }

    fn grid(&self) -> Grid {
        let t = &self.form;
        Grid {
            cell_size: t.cell_size,
            origin_x: f64::from(ORIGIN_X_M),
            origin_z: f64::from(ORIGIN_Z_M),
            height: t.cells(),
            width: t.cells(),
            tile_size: TILE_SIZE,
        }
    }

    fn tile(&self, layer: Layer, tx: u32, tz: u32) -> Result<Vec<i32>, Refusal> {
        let ts = i64::from(TILE_SIZE);
        let (r0, c0) = (i64::from(tx) * ts, i64::from(tz) * ts);
        Ok((0..ts * ts)
            .map(|i| self.cell(layer, r0 + i / ts, c0 + i % ts))
            .collect())
    }

    fn point(&self, x: f64, z: f64) -> Result<Option<Ground>, Refusal> {
        let t = &self.form;
        let (n, e) = (x - f64::from(ORIGIN_X_M), z - f64::from(ORIGIN_Z_M));
        Ok(t.covered(n, e).then(|| Ground {
            height: t.height(n, e),
            depth: t.depth(n, e),
            water: t.water(n, e),
            surface: t.surface(n, e),
            cover: t.cover(n, e),
        }))
    }

    /// The closed form has no triangles, so the segment is cut every eighth
    /// of a cell into pieces straight between the heights at their ends: the
    /// ends are then the heights `point` gives, as a mesh's are, where cell
    /// triangles from corner heights would stand a valley's end below its
    /// piece. A piece with either end off the mesh is left out.
    fn along(&self, a: [f64; 2], b: [f64; 2]) -> Result<Vec<Piece>, Refusal> {
        let t = &self.form;
        let step = f64::from(t.cell_size) / 8.0;
        let n = ((b[0] - a[0]).hypot(b[1] - a[1]) / step).ceil().max(1.0) as u32;
        let at = |k: u32| {
            let f = f64::from(k) / f64::from(n);
            let (x, z) = (a[0] + f * (b[0] - a[0]), a[1] + f * (b[1] - a[1]));
            let (north, east) = (x - f64::from(ORIGIN_X_M), z - f64::from(ORIGIN_Z_M));
            (f, t.covered(north, east).then(|| t.height(north, east)))
        };
        let mut out = Vec::new();
        let mut last = at(0);
        for k in 1..=n {
            let next = at(k);
            if let ((s, Some(hs)), (e, Some(he))) = (last, next) {
                out.push(Piece {
                    t: [s, e],
                    h: [hs, he],
                    rank: 0,
                });
            }
            last = next;
        }
        Ok(out)
    }

    fn road_graph(&self) -> Result<&Graph, Refusal> {
        Ok(self.roads.get_or_init(|| self.form.road_graph()))
    }

    fn road_betweenness(&self) -> Result<Option<&[f32]>, Refusal> {
        let g = self.road_graph()?;
        let b = self
            .betweenness
            .get_or_init(|| crate::roads::betweenness(g, crate::roads::SOURCES, 2));
        Ok(Some(b))
    }

    /// The runway's rectangle as two triangles.
    fn runway_triangles(&self, b: [f64; 4]) -> Result<Vec<[[f64; 2]; 3]>, Refusal> {
        let (hw, hl) = (RUNWAY_WIDTH_M / 2.0, RUNWAY_LENGTH_M / 2.0);
        let at = |n: f64, e: f64| {
            let (x, z) = self.form.abs(RUNWAY_NORTH_M + n, RUNWAY_EAST_M + e);
            [x, z]
        };
        let (sw, ne) = (at(-hw, -hl), at(hw, hl));
        if sw[0] > b[2] || ne[0] < b[0] || sw[1] > b[3] || ne[1] < b[1] {
            return Ok(Vec::new());
        }
        let (se, nw) = (at(-hw, hl), at(hw, -hl));
        Ok(vec![[sw, se, ne], [sw, ne, nw]])
    }

    fn tables(&self) -> Result<&Tables, Refusal> {
        Ok(self.tables.get_or_init(|| self.form.tables()))
    }

    fn objects_in(
        &self,
        pool: Pool,
        b: [f64; 4],
        f: &mut dyn FnMut(&Object),
    ) -> Result<(), Refusal> {
        let (names, models) = models();
        let classes = Classes::new(TERRAIN_ID, &models);
        let mut put = |id: u64, model: &str, x: f64, alt: f64, z: f64, rotation: f64| {
            let i = names.iter().position(|n| *n == model).unwrap();
            f(&Object {
                id,
                model: &models[i].name,
                class: classes.of(i as u32).class,
                pos: [x, alt, z],
                rotation,
                bbox: models[i].bbox,
            });
        };
        match pool {
            Pool::Objects => {
                for o in self.form.scenery() {
                    if inside(b, o.x, o.z) {
                        put(o.id, o.model, o.x, o.alt, o.z, o.rotation);
                    }
                }
            }
            // The instances as `instances_in` places them, each id from
            // where it stands, so a box finds an instance under one id: a
            // pole by its place along the line, a tree by its cell and its
            // place among the cell's trees.
            Pool::Lines | Pool::Trees => {
                let mut at = Vec::new();
                self.instances_in(pool, b, &mut |x, z| at.push((x, z)))?;
                let g = self.grid();
                let c = f64::from(g.cell_size);
                let mut last = None;
                let mut nth = 0;
                for (x, z) in at {
                    nth = if last == Some((x, z)) { nth + 1 } else { 0 };
                    last = Some((x, z));
                    let (model, id) = match pool {
                        Pool::Lines => {
                            let k = ((x - g.origin_x - LINE_SOUTH_M) / LINE_POLE_SPACING_M).round();
                            (LINE_MODEL, LINES_FIRST_ID + k as u64)
                        }
                        _ => {
                            let row = ((x - g.origin_x) / c).floor() as u64;
                            let col = ((z - g.origin_z) / c).floor() as u64;
                            let cell = row * u64::from(self.form.cells()) + col;
                            (
                                TREE_MODEL,
                                TREES_FIRST_ID + cell * u64::from(TREES_PER_CELL) + nth,
                            )
                        }
                    };
                    let alt = self.point(x, z)?.map_or(0.0, |g| g.height);
                    put(id, model, x, alt, z, 0.0);
                }
            }
        }
        Ok(())
    }

    fn structures_in(
        &self,
        b: [f64; 4],
        f: &mut dyn FnMut(f64, f64, Class),
    ) -> Result<(), Refusal> {
        let (names, models) = models();
        let classes = Classes::new(TERRAIN_ID, &models);
        for o in self.form.scenery() {
            if inside(b, o.x, o.z) {
                let i = names.iter().position(|n| *n == o.model).unwrap();
                f(o.x, o.z, classes.of(i as u32).class);
            }
        }
        Ok(())
    }

    fn instances_in(
        &self,
        pool: Pool,
        b: [f64; 4],
        f: &mut dyn FnMut(f64, f64),
    ) -> Result<(), Refusal> {
        let t = &self.form;
        match pool {
            Pool::Objects => {
                return Err(Refusal(
                    "objects are read through `structures_in`".to_string(),
                ));
            }
            Pool::Lines => {
                for n in t.pole_norths() {
                    let (x, z) = t.abs(n, LINE_EAST_M);
                    if inside(b, x, z) {
                        f(x, z);
                    }
                }
            }
            // Each forest cell's trees stand at its center.
            Pool::Trees => {
                let g = self.grid();
                let c = f64::from(g.cell_size);
                // The cells whose centers lie in `lo..=hi`.
                let span = |lo: f64, hi: f64, origin: f64| {
                    let first = ((lo - origin) / c - 0.5).ceil().max(0.0);
                    let last = ((hi - origin) / c - 0.5)
                        .floor()
                        .min(f64::from(t.cells()) - 1.0);
                    first as u32..(last + 1.0).max(first) as u32
                };
                for row in span(b[0], b[2], g.origin_x) {
                    for col in span(b[1], b[3], g.origin_z) {
                        let (n, e) = t.centre(row, col);
                        let (x, z) = t.abs(n, e);
                        for _ in 0..t.trees(n, e) {
                            f(x, z);
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

fn inside(b: [f64; 4], x: f64, z: f64) -> bool {
    (b[0]..=b[2]).contains(&x) && (b[1]..=b[3]).contains(&z)
}

/// The synthetic scenery's models, as the constants spell them and as a
/// model table would hold them, so the same rules class them.
fn models() -> (Vec<&'static str>, Vec<Model>) {
    let boxed = [
        (
            SCENERY_TOWN_MODEL,
            SCENERY_TOWN_OBB_W_M,
            SCENERY_TOWN_OBB_D_M,
            SCENERY_TOWN_HEIGHT_M,
        ),
        (
            SCENERY_INDUSTRIAL_MODEL_A,
            SCENERY_INDUSTRIAL_A_OBB_W_M,
            SCENERY_INDUSTRIAL_A_OBB_D_M,
            SCENERY_INDUSTRIAL_A_HEIGHT_M,
        ),
        (
            SCENERY_INDUSTRIAL_MODEL_B,
            SCENERY_INDUSTRIAL_B_OBB_W_M,
            SCENERY_INDUSTRIAL_B_OBB_D_M,
            SCENERY_INDUSTRIAL_B_HEIGHT_M,
        ),
        (
            SCENERY_POLE_MODEL,
            SCENERY_POLE_OBB_W_M,
            SCENERY_POLE_OBB_D_M,
            SCENERY_POLE_HEIGHT_M,
        ),
        (SCENERY_NOBOX_MODEL, 0.0, 0.0, 0.0),
        (LINE_MODEL, LINE_OBB_W_M, LINE_OBB_D_M, LINE_HEIGHT_M),
        (TREE_MODEL, TREE_OBB_W_M, TREE_OBB_D_M, TREE_HEIGHT_M),
    ];
    let names = boxed.iter().map(|m| m.0).collect();
    let models = boxed
        .iter()
        .map(|&(name, w, d, h)| {
            let (w, d, h) = ((w / 2.0) as f32, (d / 2.0) as f32, h as f32);
            Model {
                name: name.to_lowercase(),
                bbox: [-w, 0.0, -d, w, h, d],
            }
        })
        .collect();
    (names, models)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::Cover;

    fn synth() -> Synth {
        Synth::new(DEFAULT_SEED, DEFAULT_SIZE_KM, CELL_SIZE_M).unwrap()
    }

    #[test]
    fn a_segment_is_pieces_meeting_at_the_heights_points_give() {
        let s = synth();
        // From the mesh margin, across the coast and the lake, to the hills.
        let (a, b) = ([-29_000.0, -44_000.0], [20_000.0, -10_000.0]);
        let pieces = s.along(a, b).unwrap();
        // The first eighth-cell lies in the margin, outside the mesh.
        assert!(pieces[0].t[0] > 0.0);
        for p in &pieces {
            for (t, h) in p.t.iter().zip(p.h) {
                let (x, z) = (a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1]));
                assert_eq!(s.point(x, z).unwrap().unwrap().height, h, "t {t}");
            }
        }
        assert_eq!(pieces.last().unwrap().t[1], 1.0);
    }

    /// The cover of the cell whose centre is nearest an offset.
    fn cover_at(north: f64, east: f64) -> i32 {
        let c = f64::from(CELL_SIZE_M);
        synth().cell(Layer::Cover, (north / c) as i64, (east / c) as i64)
    }

    #[test]
    fn the_tables_and_objects_are_the_constants() {
        let s = synth();
        let t = s.tables().unwrap();
        let counts = [
            t.airdromes.len(),
            t.runways.len(),
            t.stands.len(),
            t.beacons.len(),
            t.radio.len(),
            t.towns.len(),
            t.nodes.len(),
        ];
        let want = [
            1,
            1,
            STAND_COUNT,
            BEACON_COUNT,
            RADIO_COUNT,
            TOWNS_COUNT,
            NODES_COUNT,
        ];
        assert_eq!(counts, want.map(|n| n as usize));
        let a = &t.airdromes[0];
        let (x, z) = s.form().abs(RUNWAY_NORTH_M, RUNWAY_EAST_M);
        assert_eq!((a.x, a.z), (Some(x), Some(z)));
        // The table's projection puts the airdrome where its lat/lon says.
        let (bx, bz) = t.crs.tmerc().to_xz(a.lat.unwrap(), a.lon.unwrap());
        assert!((bx - x).abs() < 1e-3 && (bz - z).abs() < 1e-3, "{bx} {bz}");

        let mut all = Vec::new();
        s.objects_in(
            Pool::Objects,
            [f64::MIN, f64::MIN, f64::MAX, f64::MAX],
            &mut |o| all.push((o.id, o.model.to_string(), o.class)),
        )
        .unwrap();
        assert_eq!(all.len(), SCENERY_COUNT as usize);
        assert_eq!(all[0].1, SCENERY_TOWN_MODEL.to_lowercase());
        assert_eq!(all[0].0, SCENERY_FIRST_ID);
        let mut standing = 0;
        s.structures_in([f64::MIN, f64::MIN, f64::MAX, f64::MAX], &mut |_, _, _| {
            standing += 1
        })
        .unwrap();
        assert_eq!(standing, all.len());
    }

    #[test]
    fn has_a_patch_of_each_cover_class() {
        let s = synth();
        let n = i64::from(s.form().cells());
        let mut seen = std::collections::BTreeSet::new();
        for r in (0..n).step_by(5) {
            for c in (0..n).step_by(5) {
                seen.insert(s.cell(Layer::Cover, r, c));
            }
        }
        let mut want: Vec<i32> = Cover::ALL.iter().map(|c| c.code()).collect();
        want.push(Layer::Cover.nodata());
        want.sort();
        assert_eq!(seen.into_iter().collect::<Vec<_>>(), want);
    }

    #[test]
    fn each_cover_patch_is_where_the_constants_put_it() {
        let code = |c: Cover| c.code();
        assert_eq!(cover_at(56000.0, 30000.0), code(Cover::Town));
        assert_eq!(cover_at(40025.0, 55025.0), code(Cover::Forest));
        // The forest's hole is plain ground.
        assert_eq!(cover_at(42000.0, 57000.0), code(Cover::Plain));
        assert_eq!(cover_at(21000.0, 61000.0), code(Cover::Field));
        assert_eq!(cover_at(FIELD_NORTH_M + 25.0, 61000.0), code(Cover::Plain));
        assert_eq!(cover_at(31000.0, 23000.0), code(Cover::Beach));
        // The beach runs into the sea, which stays water.
        assert_eq!(cover_at(30025.0, 20025.0), code(Cover::Water));
        assert_eq!(cover_at(RUNWAY_NORTH_M, RUNWAY_EAST_M), code(Cover::Runway));
        assert_eq!(cover_at(LAKE_NORTH_M, LAKE_EAST_M), code(Cover::Water));
        assert_eq!(cover_at(30000.0, RIVER_EAST_M + 10.0), code(Cover::Water));
        // A road keeps the ground's cover, and the bridge is not water.
        assert_eq!(cover_at(ROAD_EW_NORTH_M, 20000.0), code(Cover::Plain));
        assert_eq!(cover_at(ROAD_EW_NORTH_M, RIVER_EAST_M), code(Cover::Plain));
        assert_eq!(cover_at(1000.0, 30000.0), Layer::Cover.nodata());
    }

    #[test]
    fn a_tile_is_its_cells_and_nodata_past_the_grid() {
        let s = synth();
        let ts = i64::from(TILE_SIZE);
        // 1400 cells is 21 full tiles and 56 cells of a 22nd.
        let last = u32::try_from(i64::from(s.form().cells()) / ts).unwrap();
        for layer in Layer::ALL {
            let t = s.tile(layer, 3, last).unwrap();
            assert_eq!(t.len(), (ts * ts) as usize);
            for (i, &v) in t.iter().enumerate() {
                let (r, c) = (3 * ts + i as i64 / ts, i64::from(last) * ts + i as i64 % ts);
                assert_eq!(v, s.cell(layer, r, c), "{} ({r}, {c})", layer.name());
            }
            assert_eq!(*t.last().unwrap(), layer.nodata());
        }
    }

    #[test]
    fn a_point_is_the_closed_form_there_and_a_cell_its_center() {
        let s = synth();
        let (x, z) = s.form().abs(LAKE_NORTH_M + 10.0, LAKE_EAST_M);
        let p = s.point(x, z).unwrap().unwrap();
        assert_eq!(p.height, LAKE_LEVEL_M);
        assert_eq!((p.water, p.cover), (WATER_LAKE, Cover::Water));
        // Off a cell center the height is the hill's own, not the cell's.
        let (n, e) = (HILL_A_NORTH_M + 333.0, HILL_A_EAST_M + 17.0);
        let (x, z) = s.form().abs(n, e);
        assert_eq!(
            s.point(x, z).unwrap().unwrap().height,
            s.form().ground(n, e)
        );
        let g = s.grid();
        let (r, c) = g.cell_at(x, z).unwrap();
        let (cx, cz) = g.centre(r, c);
        let p = s.point(cx, cz).unwrap().unwrap();
        assert_eq!(
            (p.height + 0.5).floor() as i32,
            s.cell(Layer::Height, r.into(), c.into())
        );
        assert_eq!(
            i32::from(p.surface),
            s.cell(Layer::Surface, r.into(), c.into())
        );
        // The margin is covered by nothing.
        let (x, z) = s.form().abs(10.0, 30000.0);
        assert_eq!(s.point(x, z).unwrap(), None);
    }

    #[test]
    fn scenery_and_roads_are_the_closed_forms() {
        let s = synth();
        let all = [-1e9, -1e9, 1e9, 1e9];
        let mut by_class = std::collections::BTreeMap::new();
        s.structures_in(all, &mut |_, _, c| *by_class.entry(c).or_insert(0) += 1)
            .unwrap();
        let want = [
            (Class::Building, SCENERY_TOWN_COUNT),
            (Class::Industrial, SCENERY_INDUSTRIAL_COUNT),
            (Class::Misc, SCENERY_POLE_COUNT + SCENERY_NOBOX_COUNT),
        ];
        assert_eq!(by_class, want.into_iter().collect());
        let count = |pool, b| {
            let mut n = 0;
            s.instances_in(pool, b, &mut |_, _| n += 1).unwrap();
            n
        };
        assert_eq!(count(Pool::Trees, all), s.form().tree_count());
        assert_eq!(count(Pool::Lines, all), s.form().pole_count());
        // One forest cell, its center on the box's edge.
        let (x, z) = s.form().abs(FOREST_SOUTH_M + 25.0, FOREST_WEST_M + 25.0);
        assert_eq!(
            count(Pool::Trees, [x, z, x + 10.0, z + 10.0]),
            TREES_PER_CELL
        );
        assert_eq!(count(Pool::Trees, [x + 1.0, z, x + 10.0, z + 10.0]), 0);
        assert!(s.instances_in(Pool::Objects, all, &mut |_, _| ()).is_err());
        // As objects, the same instances with their models' boxes.
        let mut trees = Vec::new();
        s.objects_in(Pool::Trees, all, &mut |o| {
            trees.push((o.id, o.model.to_string(), o.bbox))
        })
        .unwrap();
        assert_eq!(trees.len() as u32, s.form().tree_count());
        assert_eq!(trees[0].1, TREE_MODEL.to_lowercase());
        // Each id once, and the same id from a box holding only its cell.
        let ids: std::collections::BTreeSet<u64> = trees.iter().map(|t| t.0).collect();
        assert_eq!(ids.len(), trees.len());
        let (x, z) = s.form().abs(FOREST_SOUTH_M + 525.0, FOREST_WEST_M + 75.0);
        let mut one = Vec::new();
        s.objects_in(Pool::Trees, [x, z, x, z], &mut |o| one.push(o.id))
            .unwrap();
        assert_eq!(one.len() as u32, TREES_PER_CELL);
        assert!(one.iter().all(|id| ids.contains(id)), "{one:?}");
        let pole = |b| {
            let mut v = Vec::new();
            s.objects_in(Pool::Lines, b, &mut |o| v.push(o.id)).unwrap();
            v
        };
        let (px, pz) = s
            .form()
            .abs(LINE_SOUTH_M + LINE_POLE_SPACING_M, LINE_EAST_M);
        assert_eq!(pole([px, pz, px, pz]), [crate::synth::LINES_FIRST_ID + 1]);
        assert_eq!(trees[0].2[3] - trees[0].2[0], TREE_OBB_W_M as f32);
        let mut poles = 0;
        s.objects_in(Pool::Lines, all, &mut |o| {
            assert_eq!(o.model, LINE_MODEL.to_lowercase());
            poles += 1;
        })
        .unwrap();
        assert_eq!(poles, s.form().pole_count());
        // The runway, two triangles covering its rectangle, from a box that
        // meets it; none from one beside it.
        let (x, z) = s.form().abs(RUNWAY_NORTH_M, RUNWAY_EAST_M);
        let tris = s.runway_triangles([x, z, x + 1.0, z + 1.0]).unwrap();
        let area: f64 = (tris.iter())
            .map(|[a, b, c]| {
                ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])).abs() / 2.0
            })
            .sum();
        assert_eq!(area, RUNWAY_WIDTH_M * RUNWAY_LENGTH_M);
        let beside = x + RUNWAY_WIDTH_M / 2.0 + 1.0;
        assert!(
            s.runway_triangles([beside, z, beside + 1.0, z])
                .unwrap()
                .is_empty()
        );
        assert_eq!(*s.road_graph().unwrap(), s.form().road_graph());
    }

    #[test]
    fn refuses_another_cell_size() {
        assert!(Synth::new(1, 70, 100).is_err());
        assert_eq!(synth().identity().terrain, "synth");
    }
}
