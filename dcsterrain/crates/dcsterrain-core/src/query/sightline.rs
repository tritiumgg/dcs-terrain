//! The terrain under a straight segment: `exact` over the mesh's own
//! triangles, which `visible` judges by, and `walk` over the 50 m raster,
//! which steps a line the way `viewshed`'s radial sweep does and so is what
//! that sweep is checked against. `floor` finds over the same triangles
//! the lowest a target at the far end may stand and be seen.

use std::collections::{BinaryHeap, HashMap};

use crate::grid::{Grid, Window};
use crate::source::{Layer, Piece, Refusal, Source};

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

/// How far below the line the ground must reach before it blocks: the
/// ends' heights and the pieces' are interpolated in different frames, and
/// a line ending on the ground would otherwise be blocked by rounding.
const BLOCK_TOLERANCE_M: f64 = 1e-4;

/// The sightline from `a` to `b`, each x, z and an absolute altitude, over
/// the mesh triangles `pieces` that `Source::along` found under it: exact,
/// since between two edge crossings the ground is one triangle's plane and
/// the gap to the line is linear. Where pieces overlap the one of higher
/// rank is the ground; stretches no piece covers do not block. The
/// clearance is the least gap anywhere between the ends, which tends to
/// each end's height above its ground. The world is flat.
pub fn exact(pieces: &[Piece], a: [f64; 3], b: [f64; 3]) -> Sight {
    let mut sight = Sight {
        clearance: None,
        blocking: None,
    };
    let line = |t: f64| a[2] + t * (b[2] - a[2]);
    spans(pieces, |s, e, ground| {
        let (gs, ge) = (line(s) - ground(s), line(e) - ground(e));
        let least = gs.min(ge);
        sight.clearance = Some(sight.clearance.map_or(least, |c: f64| c.min(least)));
        if sight.blocking.is_none() && least < -BLOCK_TOLERANCE_M {
            // Where the gap first falls past the tolerance.
            let t = if gs < -BLOCK_TOLERANCE_M {
                s
            } else {
                s + (e - s) * (gs + BLOCK_TOLERANCE_M) / (gs - ge)
            };
            let (x, z) = (a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1]));
            sight.blocking = Some([x, z, ground(t)]);
        }
    });
    sight
}

/// The lowest altitude at the far end of a segment at which a target is
/// seen from an eye at altitude `eye` at its near end, over the mesh
/// triangles `pieces` that `Source::along` found under it, or infinity
/// when the ground at the eye rises over it. Exact, as `exact` is: the
/// line from the eye clears the ground at `t` when the target stands at or
/// above `eye + (ground(t) − eye) / t`, which over one triangle's plane is
/// monotonic in `t`, so its greatest is at a crossing. The ends' own
/// ground is left to the caller. The world is flat.
pub fn floor(pieces: &[Piece], eye: f64) -> f64 {
    let mut floor = f64::NEG_INFINITY;
    spans(pieces, |s, e, ground| {
        if s <= 0.0 && ground(0.0) - eye > BLOCK_TOLERANCE_M {
            floor = f64::INFINITY;
        }
        // Next to the eye the bound tends to the slope or falls away, and
        // the far end of the span bounds it.
        for t in [s, e] {
            if t > 0.0 {
                floor = floor.max(eye + (ground(t) - eye) / t);
            }
        }
    });
    floor
}

/// Calls `f` with each stretch `[s, e]` between consecutive crossings of
/// the pieces, and the ground over it, the plane of the piece of highest
/// rank that covers it; stretches no piece covers are skipped.
fn spans(pieces: &[Piece], mut f: impl FnMut(f64, f64, &dyn Fn(f64) -> f64)) {
    let mut ts: Vec<f64> = pieces.iter().flat_map(|p| p.t).collect();
    ts.sort_unstable_by(f64::total_cmp);
    ts.dedup();
    let mut order: Vec<usize> = (0..pieces.len()).collect();
    order.sort_unstable_by(|&i, &j| pieces[i].t[0].total_cmp(&pieces[j].t[0]));
    let mut next = order.into_iter().peekable();
    // The pieces begun so far, highest rank on top; one that has ended is
    // dropped when it reaches the top, since the sweep never goes back.
    let mut live: BinaryHeap<(u64, usize)> = BinaryHeap::new();
    for w in ts.windows(2) {
        let (s, e) = (w[0], w[1]);
        let m = 0.5 * (s + e);
        while let Some(i) = next.next_if(|&i| pieces[i].t[0] <= m) {
            live.push((pieces[i].rank, i));
        }
        while live.peek().is_some_and(|&(_, i)| pieces[i].t[1] < m) {
            live.pop();
        }
        let Some(&(_, i)) = live.peek() else {
            continue;
        };
        let p = &pieces[i];
        let ground = |t: f64| p.h[0] + (t - p.t[0]) / (p.t[1] - p.t[0]) * (p.h[1] - p.h[0]);
        f(s, e, &ground);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn piece(t: [f64; 2], h: [f64; 2], rank: u64) -> Piece {
        Piece { t, h, rank }
    }

    /// A ridge 100 m high halfway along a kilometer, on flat ground at 0 m.
    fn ridge() -> Vec<Piece> {
        vec![
            piece([0.0, 0.4], [0.0, 0.0], 1),
            piece([0.4, 0.5], [0.0, 100.0], 2),
            piece([0.5, 0.6], [100.0, 0.0], 3),
            piece([0.6, 1.0], [0.0, 0.0], 4),
        ]
    }

    #[test]
    fn a_line_over_the_ridge_clears_it_by_the_gap_at_its_crest() {
        let s = exact(&ridge(), [0.0, 0.0, 101.0], [1000.0, 0.0, 101.0]);
        assert_eq!(s.blocking, None);
        assert!((s.clearance.unwrap() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_line_into_the_ridge_is_blocked_where_it_meets_the_slope() {
        let s = exact(&ridge(), [0.0, 0.0, 50.0], [1000.0, 0.0, 50.0]);
        let [x, z, g] = s.blocking.unwrap();
        // The slope reaches 50 m a quarter of the way up from 400 m to 500 m.
        assert!(
            (x - 450.0).abs() < 0.01 && z == 0.0 && (g - 50.0).abs() < 0.01,
            "{x} {g}"
        );
        assert!((s.clearance.unwrap() + 50.0).abs() < 1e-9);
    }

    #[test]
    fn the_floor_behind_the_ridge_is_where_a_line_grazes_its_crest() {
        // From 101 m, a line grazing the 100 m crest halfway drops 1 m to
        // it and 2 m in all by the far end.
        let f = floor(&ridge(), 101.0);
        assert!((f - 99.0).abs() < 1e-9, "{f}");
        let to = |alt: f64| exact(&ridge(), [0.0, 0.0, 101.0], [1000.0, 0.0, alt]);
        assert_eq!(to(f).blocking, None);
        assert!(to(f - 0.01).blocking.is_some());
        // An eye under its own ground sees nothing; one on it sees the slope.
        assert_eq!(floor(&ridge(), -1.0), f64::INFINITY);
        let rising = [piece([0.0, 1.0], [0.0, 10.0], 1)];
        assert!((floor(&rising, 0.0) - 10.0).abs() < 1e-9);
        assert_eq!(floor(&[], 5.0), f64::NEG_INFINITY);
    }

    #[test]
    fn the_higher_rank_is_the_ground_where_pieces_overlap() {
        // A road strip laid at 0 m through a mountain: the strip wins.
        let mut p = vec![piece([0.0, 1.0], [500.0, 500.0], 1)];
        p.push(piece([0.0, 1.0], [0.0, 0.0], 1 << 63));
        let s = exact(&p, [0.0, 0.0, 2.0], [1000.0, 0.0, 2.0]);
        assert_eq!(s.blocking, None);
        assert!((s.clearance.unwrap() - 2.0).abs() < 1e-9);
    }

    #[test]
    fn a_stretch_no_triangle_covers_does_not_block() {
        let p = [
            piece([0.0, 0.3], [0.0, 0.0], 1),
            piece([0.7, 1.0], [0.0, 0.0], 2),
        ];
        let s = exact(&p, [0.0, 0.0, 1.0], [1000.0, 0.0, 1.0]);
        assert_eq!((s.blocking, s.clearance), (None, Some(1.0)));
        assert_eq!(exact(&[], [0.0; 3], [1.0, 0.0, 0.0]).clearance, None);
    }

    #[test]
    fn a_line_ending_on_the_ground_is_not_blocked_by_rounding() {
        // Down a slope to a point on it, a hair below the plane.
        let p = [piece([0.0, 1.0], [100.0, 0.0], 1)];
        let s = exact(&p, [0.0, 0.0, 110.0], [1000.0, 0.0, -1e-6]);
        assert_eq!(s.blocking, None);
    }
}
