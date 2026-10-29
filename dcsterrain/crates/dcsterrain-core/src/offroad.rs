//! How hard the ground is to cross off the road: a cost per meter for each
//! cell, from its slope and its cover, over a rectangle of cells.
//!
//! The model is a coarse cross-country movement map, the go, slow-go and
//! no-go of a ground planner: open flat ground costs 1 per meter, and each
//! cover and slope slows a vehicle by a factor; water, and ground steeper
//! than the limit, cannot be crossed. The factors are this tool's own, not
//! measured against the game's ground units, which path by rules of their
//! own.

use crate::grid::Window;
use crate::layers::{Cells, Derived, derive};
use crate::source::{Cover, Layer, Refusal, Source};

/// The share of open ground's speed each cover leaves a vehicle.
pub fn cover_factor(cover: i32) -> Option<f64> {
    Some(match cover {
        c if c == Cover::Plain.code() || c == Cover::Runway.code() => 1.0,
        c if c == Cover::Field.code() => 0.8,
        c if c == Cover::Beach.code() => 0.6,
        c if c == Cover::Town.code() => 0.5,
        c if c == Cover::Forest.code() => 0.25,
        _ => return None,
    })
}

/// The share of flat ground's speed a slope leaves: one at flat, falling
/// with the square of the slope to a quarter at the limit, and none past
/// it.
pub fn slope_factor(slope_deg: f64, max_slope_deg: f64) -> Option<f64> {
    (slope_deg <= max_slope_deg).then(|| 1.0 - 0.75 * (slope_deg / max_slope_deg).powi(2))
}

/// The cost per meter of a cell, 1 on open flat ground, or `None` where it
/// cannot be crossed: water, no ground, or a slope past `max_slope_deg`.
pub fn cost(slope_deg: f64, water: i32, cover: i32, max_slope_deg: f64) -> Option<f64> {
    if water != 0 || slope_deg.is_nan() {
        return None;
    }
    Some(1.0 / (cover_factor(cover)? * slope_factor(slope_deg, max_slope_deg)?))
}

/// Why a cell cannot be crossed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Block {
    /// No mesh covers it.
    NoGround,
    /// Its water class, as the `water` layer codes it.
    Water(i32),
    /// Its slope in degrees.
    Steep(f64),
    /// A cover no vehicle crosses, as the `cover` layer codes it.
    Cover(i32),
}

/// Each cell's cost per meter over a rectangle, row-major with columns
/// fastest, or why it cannot be crossed.
pub struct CostField {
    pub cells: Cells,
    pub values: Vec<Result<f64, Block>>,
}

impl CostField {
    /// The cost of the cells `at`, slopes past `max_slope_deg` blocking.
    pub fn new(
        src: &(impl Source + ?Sized),
        at: Cells,
        max_slope_deg: f64,
    ) -> Result<CostField, Refusal> {
        let slope = derive(src, Derived::Slope, at)?;
        let water = Window::read(src, Layer::Water, at.row0, at.col0, at.rows, at.cols)?;
        let cover = Window::read(src, Layer::Cover, at.row0, at.col0, at.rows, at.cols)?;
        let values = (0..slope.values.len())
            .map(|i| {
                let (s, w, c) = (f64::from(slope.values[i]), water.data[i], cover.data[i]);
                if w == Layer::Water.nodata() || c == Layer::Cover.nodata() || s.is_nan() {
                    Err(Block::NoGround)
                } else if w != 0 {
                    Err(Block::Water(w))
                } else if cover_factor(c).is_none() {
                    Err(Block::Cover(c))
                } else {
                    cost(s, w, c, max_slope_deg).ok_or(Block::Steep(s))
                }
            })
            .collect();
        Ok(CostField { cells: at, values })
    }

    /// The cell at a grid row and column, or `None` outside the field.
    pub fn get(&self, row: i64, col: i64) -> Option<Result<f64, Block>> {
        let (r, c) = (row - self.cells.row0, col - self.cells.col0);
        let inside = (0..i64::from(self.cells.rows)).contains(&r)
            && (0..i64::from(self.cells.cols)).contains(&c);
        inside.then(|| self.values[(r * i64::from(self.cells.cols) + c) as usize])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_flat_ground_costs_one_and_the_rest_more_or_nothing() {
        let plain = Cover::Plain.code();
        assert_eq!(cost(0.0, 0, plain, 20.0), Some(1.0));
        assert_eq!(cost(0.0, 0, Cover::Forest.code(), 20.0), Some(4.0));
        assert_eq!(cost(20.0, 0, plain, 20.0), Some(4.0));
        assert_eq!(cost(20.1, 0, plain, 20.0), None);
        assert_eq!(cost(0.0, 3, plain, 20.0), None);
        assert_eq!(cost(0.0, 0, Cover::Water.code(), 20.0), None);
        assert_eq!(cost(f64::NAN, 0, plain, 20.0), None);
        // Steeper always costs more.
        let mut last = 0.0;
        for d in 0..=20 {
            let c = cost(f64::from(d), 0, plain, 20.0).unwrap();
            assert!(c >= last);
            last = c;
        }
    }

    #[test]
    fn the_synthetic_forest_costs_four_and_its_lake_blocks() {
        use crate::synth::{self, *};
        let s = Synth::new(synth::DEFAULT_SEED, synth::DEFAULT_SIZE_KM, CELL_SIZE_M).unwrap();
        let g = s.grid();
        let cell = |north: f64, east: f64| {
            let (x, z) = s.form().abs(north, east);
            let (r, c) = g.cell_at(x, z).unwrap();
            (i64::from(r), i64::from(c))
        };
        let (r0, c0) = cell(FOREST_SOUTH_M + 100.0, FOREST_WEST_M + 100.0);
        let (r1, c1) = cell(LAKE_NORTH_M, LAKE_EAST_M);
        let at = Cells {
            row0: r1.min(r0),
            col0: c1.min(c0),
            rows: (r1.max(r0) - r1.min(r0) + 1) as u32,
            cols: (c1.max(c0) - c1.min(c0) + 1) as u32,
        };
        let f = CostField::new(&s, at, 20.0).unwrap();
        let forest = f.get(r0, c0).unwrap().unwrap();
        // The plane's one percent slows it a little past the forest's four.
        assert!((4.0..4.01).contains(&forest), "{forest}");
        assert_eq!(f.get(r1, c1).unwrap(), Err(Block::Water(1)));
        assert!(f.get(r1 + 10_000, c1).is_none());
    }
}
