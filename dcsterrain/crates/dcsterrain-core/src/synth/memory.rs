//! The synthetic terrain as a `Source`, every tile computed from the closed
//! form when it is asked for.

use super::*;
use crate::grid::Grid;
use crate::source::{Ground, Identity, Layer, Refusal, Source, TILE_SIZE};

/// The terrain `--terrain synth` names.
pub const TERRAIN_ID: &str = "synth";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Synth {
    form: Theatre,
    identity: Identity,
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
            water: t.water(n, e),
            surface: t.surface(n, e),
            cover: t.cover(n, e),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::Cover;

    fn synth() -> Synth {
        Synth::new(DEFAULT_SEED, DEFAULT_SIZE_KM, CELL_SIZE_M).unwrap()
    }

    /// The cover of the cell whose centre is nearest an offset.
    fn cover_at(north: f64, east: f64) -> i32 {
        let c = f64::from(CELL_SIZE_M);
        synth().cell(Layer::Cover, (north / c) as i64, (east / c) as i64)
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
    fn refuses_another_cell_size() {
        assert!(Synth::new(1, 70, 100).is_err());
        assert_eq!(synth().identity().terrain, "synth");
    }
}
