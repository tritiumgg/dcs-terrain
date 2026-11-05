//! How hard the ground is to cross off the road: a cost per meter for each
//! cell, from its slope and its cover, over a rectangle of cells.
//!
//! The model is a coarse cross-country movement map, the go, slow-go and
//! no-go of a ground planner: open flat ground costs 1 per meter, and each
//! cover and slope slows a vehicle by a factor; water, and ground steeper
//! than the limit, cannot be crossed. The factors are this tool's own, not
//! measured against the game's ground units, which path by rules of their
//! own.
//!
//! `way` searches the least-cost way over these cells from a point to a
//! network.

pub mod way;

use crate::grid::{Grid, Window};
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
    Steep(f32),
    /// A cover no vehicle crosses, as the `cover` layer codes it.
    Cover(i32),
}

/// Each cell's cost per meter over a rectangle, row-major with columns
/// fastest, or why it cannot be crossed: eight bytes a cell, since a search
/// holds one over every cell within its reach.
pub struct CostField {
    pub cells: Cells,
    pub values: Vec<Result<f32, Block>>,
}

impl CostField {
    /// The cost of the cells `at`, slopes past `max_slope_deg` blocking.
    /// It is read a row of tiles at a time, so the layers it is made from
    /// are never held whole beside it.
    pub fn new(
        src: &(impl Source + ?Sized),
        at: Cells,
        max_slope_deg: f64,
    ) -> Result<CostField, Refusal> {
        let tile = i64::from(src.grid().tile_size.max(1));
        let mut values = Vec::with_capacity(at.rows as usize * at.cols as usize);
        let end = at.row0 + i64::from(at.rows);
        let mut row = at.row0;
        while row < end {
            let next = ((row.div_euclid(tile) + 1) * tile).min(end);
            let strip = Cells {
                row0: row,
                col0: at.col0,
                rows: (next - row) as u32,
                cols: at.cols,
            };
            let slope = derive(src, Derived::Slope, strip)?;
            let water = Window::read(src, Layer::Water, row, at.col0, strip.rows, at.cols)?;
            let cover = Window::read(src, Layer::Cover, row, at.col0, strip.rows, at.cols)?;
            values.extend((0..slope.values.len()).map(|i| {
                let (s, w, c) = (slope.values[i], water.data[i], cover.data[i]);
                if w == Layer::Water.nodata() || c == Layer::Cover.nodata() || s.is_nan() {
                    Err(Block::NoGround)
                } else if w != 0 {
                    Err(Block::Water(w))
                } else if cover_factor(c).is_none() {
                    Err(Block::Cover(c))
                } else {
                    cost(f64::from(s), w, c, max_slope_deg)
                        .map(|v| v as f32)
                        .ok_or(Block::Steep(s))
                }
            }));
            row = next;
        }
        Ok(CostField { cells: at, values })
    }

    /// The index of the cell at a grid row and column, or `None` outside
    /// the field.
    pub fn index(&self, row: i64, col: i64) -> Option<usize> {
        let (r, c) = (row - self.cells.row0, col - self.cells.col0);
        let inside = (0..i64::from(self.cells.rows)).contains(&r)
            && (0..i64::from(self.cells.cols)).contains(&c);
        inside.then(|| (r * i64::from(self.cells.cols) + c) as usize)
    }

    /// The grid row and column of the cell at `index`.
    pub fn row_col(&self, index: usize) -> (i64, i64) {
        let cols = self.cells.cols as usize;
        (
            self.cells.row0 + (index / cols) as i64,
            self.cells.col0 + (index % cols) as i64,
        )
    }

    /// The cell at a grid row and column, or `None` outside the field.
    pub fn get(&self, row: i64, col: i64) -> Option<Result<f64, Block>> {
        self.index(row, col).map(|i| self.values[i].map(f64::from))
    }
}

/// Walks the segment from `a` to `b`, each `[x, z]`, through the grid's
/// cells in order, calling `f(row, col, t0, t1)` with the share of the way
/// along it each cell holds, until `f` returns false; returns whether it
/// walked to the end. Where it passes exactly through a corner, the two
/// cells beside the corner are called too, holding none of it, so a way
/// judged over the cells cannot slip diagonally between two that block.
pub fn cross(
    g: &Grid,
    a: [f64; 2],
    b: [f64; 2],
    f: &mut dyn FnMut(i64, i64, f64, f64) -> bool,
) -> bool {
    let cs = f64::from(g.cell_size);
    let (ua, va) = ((a[0] - g.origin_x) / cs, (a[1] - g.origin_z) / cs);
    let (du, dv) = ((b[0] - g.origin_x) / cs - ua, (b[1] - g.origin_z) / cs - va);
    let (mut r, mut c) = (ua.floor() as i64, va.floor() as i64);
    let step = |d: f64| {
        if d > 0.0 {
            1
        } else if d < 0.0 {
            -1
        } else {
            0
        }
    };
    let (sr, sc) = (step(du), step(dv));
    // The share of the way at which the walk first leaves the cell's row or
    // column, and the share each further row or column takes.
    let first = |p: f64, d: f64, i: i64| {
        if d > 0.0 {
            ((i + 1) as f64 - p) / d
        } else if d < 0.0 {
            (i as f64 - p) / d
        } else {
            f64::INFINITY
        }
    };
    let (mut tr, mut tc) = (first(ua, du, r), first(va, dv, c));
    let (dtr, dtc) = (1.0 / du.abs(), 1.0 / dv.abs());
    let mut t = 0.0;
    loop {
        let next = tr.min(tc).min(1.0);
        if !f(r, c, t, next) {
            return false;
        }
        if next >= 1.0 {
            return true;
        }
        if (tr - tc).abs() < 1e-12 {
            if !f(r + sr, c, next, next) || !f(r, c + sc, next, next) {
                return false;
            }
            (r, c, tr, tc) = (r + sr, c + sc, tr + dtr, tc + dtc);
        } else if tr < tc {
            (r, tr) = (r + sr, tr + dtr);
        } else {
            (c, tc) = (c + sc, tc + dtc);
        }
        t = next;
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cell_of_the_field_holds_in_eight_bytes() {
        assert_eq!(std::mem::size_of::<Result<f32, Block>>(), 8);
    }

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
