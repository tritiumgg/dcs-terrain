//! The terrain under a straight segment, read from the 50 m raster.
//!
//! `walk` is the only place a sightline is stepped, so an exact walk over the
//! mesh's own triangles can replace it without touching what calls it.

use std::collections::HashMap;

use crate::grid::{Grid, Window};
use crate::source::{Layer, Refusal, Source};

/// The ground's height at any point, read a tile at a time as a walk
/// reaches it, so a long sightline reads only the tiles it crosses.
pub struct Heights<'a> {
    src: &'a dyn Source,
    grid: Grid,
    windows: HashMap<(i64, i64), Window>,
}

impl<'a> Heights<'a> {
    pub fn new(src: &'a dyn Source) -> Heights<'a> {
        Heights {
            src,
            grid: src.grid(),
            windows: HashMap::new(),
        }
    }

    /// The raster height at `(x, z)`, bilinear between the cell centers
    /// around it; `None` off the grid or where no mesh covers a cell that
    /// carries weight.
    pub fn at(&mut self, x: f64, z: f64) -> Result<Option<f64>, Refusal> {
        let g = &self.grid;
        if g.cell_at(x, z).is_none() {
            return Ok(None);
        }
        // The first of the four cells the height is drawn from, as
        // `Window::value_at` finds it; its tile's window holds one row and
        // one column more, so the other three lie in it too.
        let c = f64::from(g.cell_size);
        let first = |v: f64, origin: f64, n: u32| {
            ((v - origin) / c - 0.5)
                .clamp(0.0, f64::from(n - 1))
                .floor() as i64
        };
        let ts = i64::from(g.tile_size);
        let key = (
            first(x, g.origin_x, g.height) / ts,
            first(z, g.origin_z, g.width) / ts,
        );
        if !self.windows.contains_key(&key) {
            let size = g.tile_size + 1;
            let w = Window::read(self.src, Layer::Height, key.0 * ts, key.1 * ts, size, size)?;
            self.windows.insert(key, w);
        }
        Ok(self.windows[&key].value_at(g, x, z))
    }
}

/// What a sightline found between its ends.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sight {
    /// The least height of the line above the ground at any step between
    /// the ends, negative where the ground rises above it; `None` when the
    /// ends are too close for a step between them.
    pub clearance: Option<f64>,
    /// The first step from `a` where the ground rises above the line: x, z
    /// and the ground's height.
    pub blocking: Option<[f64; 3]>,
}

/// Steps from `a` to `b`, each x, z and an absolute altitude, at most
/// `step` apart, reading the ground at every step strictly between them. A
/// step no mesh covers does not block. The world is flat.
pub fn walk(h: &mut Heights, a: [f64; 3], b: [f64; 3], step: f64) -> Result<Sight, Refusal> {
    let d = (b[0] - a[0]).hypot(b[1] - a[1]);
    let n = (d / step).ceil() as u64;
    let mut sight = Sight {
        clearance: None,
        blocking: None,
    };
    for k in 1..n {
        let t = k as f64 / n as f64;
        let (x, z) = (a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1]));
        let Some(g) = h.at(x, z)? else {
            continue;
        };
        let gap = a[2] + t * (b[2] - a[2]) - g;
        sight.clearance = Some(sight.clearance.map_or(gap, |c: f64| c.min(gap)));
        if gap < 0.0 && sight.blocking.is_none() {
            sight.blocking = Some([x, z, g]);
        }
    }
    Ok(sight)
}
