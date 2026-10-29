//! The ground of one block, ready to answer: its corners in grid units, its
//! triangles binned by the cells they can cover, and from those a tile's
//! cells or a single point.
//!
//! A block spans many tiles, and a tile or point needs only the triangles
//! near it, so each triangle is listed in every bin of `BIN_CELLS` square
//! cells its box touches. A tile visits its bins, a point the one it falls
//! in, and a triangle listed in several bins is tested in each only against
//! that bin's cells.

use super::mesh::Block;
use super::sample::{EDGE_TOLERANCE, ROAD_STRIP, cover, rank, round_height, water_class};
use crate::extract::Bounds;
use crate::grid::Grid;
use crate::source::{Cover, Layer};

/// Cells along each side of a bin of the triangle index. A tile's side is a
/// whole number of bins.
pub const BIN_CELLS: u32 = 8;

/// The material whose subset marks a runway.
pub const RUNWAY_MATERIAL: &str = "Runway";

/// The grid of `cell_size` cells over bounds `b`: its origin and extent
/// snapped outward to whole cells.
pub fn grid_for(b: &Bounds, cell_size: u32, tile_size: u32) -> Grid {
    let cs = f64::from(cell_size);
    let (x0, z0) = ((b.min_x / cs).floor() * cs, (b.min_z / cs).floor() * cs);
    let (x1, z1) = ((b.max_x / cs).ceil() * cs, (b.max_z / cs).ceil() * cs);
    Grid {
        cell_size,
        origin_x: x0,
        origin_z: z0,
        height: ((x1 - x0) / cs).round() as u32,
        width: ((z1 - z0) / cs).round() as u32,
        tile_size,
    }
}

/// How far past its box a triangle is listed and rasterized, in cells: a
/// point on its edge is covered within the edge tolerance, which grows with
/// the triangle's size, and must find it. Five centimeters is far more than
/// that tolerance reaches on any triangle a block holds, and costs nothing:
/// the cover test decides.
const BIN_SLACK: f64 = 1e-3;

/// `surface` codes. 2, shallow water, is never written: nothing in the file
/// is known to mark it.
pub const SURFACE_LAND: u8 = 1;
pub const SURFACE_WATER: u8 = 3;
pub const SURFACE_ROAD: u8 = 4;
pub const SURFACE_RUNWAY: u8 = 5;

/// Triangle type bits of the ground cover, in the order they are tested.
const COVER_BITS: [(u32, Cover); 5] = [
    (32, Cover::Runway),
    (64, Cover::Town),
    (1024, Cover::Forest),
    (128, Cover::Field),
    (256, Cover::Beach),
];

/// The ground cover a triangle type gives. Road and rail strips carry the
/// bit of the ground under them, so they keep its cover.
pub fn cover_of(tritype: u32) -> Cover {
    if water_class(tritype) != 0 {
        return Cover::Water;
    }
    COVER_BITS
        .iter()
        .find(|(bit, _)| tritype & bit != 0)
        .map_or(Cover::Plain, |&(_, c)| c)
}

/// The `surface` code of a point on a triangle of type `tritype`, inside a
/// runway subset or not.
pub fn surface_of(tritype: u32, runway: bool) -> u8 {
    if runway {
        SURFACE_RUNWAY
    } else if tritype & ROAD_STRIP != 0 {
        SURFACE_ROAD
    } else if water_class(tritype) != 0 {
        SURFACE_WATER
    } else {
        SURFACE_LAND
    }
}

/// The winning triangle at a point, and what it gives there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ground {
    /// The water surface over water, the ground elsewhere, as interpolated.
    pub h: f64,
    pub tritype: u32,
    /// Whether a runway subset of any block covers the point.
    pub runway: bool,
    pub block: usize,
    pub tri: usize,
}

impl Ground {
    fn rank(&self) -> u64 {
        rank(self.tritype, self.block, self.tri)
    }

    /// A layer's value here, as a tile cell holds it.
    pub fn value(&self, layer: Layer) -> i32 {
        match layer {
            Layer::Height => i32::from(round_height(self.h)),
            Layer::Water => i32::from(water_class(self.tritype)),
            Layer::Surface => i32::from(surface_of(self.tritype, self.runway)),
            Layer::Cover => cover_of(self.tritype).code(),
        }
    }
}

/// Of two answers for one point, the one reported: the winning triangle,
/// over a runway if either saw one.
pub fn merge(a: Option<Ground>, b: Option<Ground>) -> Option<Ground> {
    match (a, b) {
        (Some(a), Some(b)) => {
            let runway = a.runway || b.runway;
            let mut g = if b.rank() > a.rank() { b } else { a };
            g.runway = runway;
            Some(g)
        }
        (a, b) => a.or(b),
    }
}

/// Triangles listed by bin, over a rectangle of bins.
#[derive(Debug, Clone, Default)]
struct Bins {
    row0: i64,
    col0: i64,
    rows: i64,
    cols: i64,
    /// Where each bin's list starts in `items`, and one past the last.
    start: Vec<u32>,
    items: Vec<u32>,
}

/// The bins a span of grid units touches.
fn bin_span(lo: f64, hi: f64) -> (i64, i64) {
    let b = f64::from(BIN_CELLS);
    (
        ((lo - BIN_SLACK) / b).floor() as i64,
        ((hi + BIN_SLACK) / b).floor() as i64,
    )
}

fn span(corners: &[[f64; 2]], k: usize) -> (f64, f64) {
    corners
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), c| {
            (lo.min(c[k]), hi.max(c[k]))
        })
}

impl Bins {
    fn new(rc: &[[f64; 2]], tri: &[[u32; 3]]) -> Bins {
        if tri.is_empty() {
            return Bins::default();
        }
        let ranges: Vec<((i64, i64), (i64, i64))> = tri
            .iter()
            .map(|t| {
                let p = t.map(|k| rc[k as usize]);
                let (r0, r1) = span(&p, 0);
                let (c0, c1) = span(&p, 1);
                (bin_span(r0, r1), bin_span(c0, c1))
            })
            .collect();
        let row0 = ranges.iter().map(|r| r.0.0).min().unwrap();
        let col0 = ranges.iter().map(|r| r.1.0).min().unwrap();
        let rows = ranges.iter().map(|r| r.0.1).max().unwrap() - row0 + 1;
        let cols = ranges.iter().map(|r| r.1.1).max().unwrap() - col0 + 1;
        let at = |r: i64, c: i64| ((r - row0) * cols + (c - col0)) as usize;
        let mut count = vec![0u32; (rows * cols) as usize + 1];
        for ((r0, r1), (c0, c1)) in &ranges {
            for r in *r0..=*r1 {
                for c in *c0..=*c1 {
                    count[at(r, c) + 1] += 1;
                }
            }
        }
        for i in 1..count.len() {
            count[i] += count[i - 1];
        }
        let mut fill = count.clone();
        let mut items = vec![0u32; *count.last().unwrap() as usize];
        for (t, ((r0, r1), (c0, c1))) in ranges.iter().enumerate() {
            for r in *r0..=*r1 {
                for c in *c0..=*c1 {
                    let k = &mut fill[at(r, c)];
                    items[*k as usize] = t as u32;
                    *k += 1;
                }
            }
        }
        Bins {
            row0,
            col0,
            rows,
            cols,
            start: count,
            items,
        }
    }

    /// The triangles listed in bin `(r, c)`.
    fn get(&self, r: i64, c: i64) -> &[u32] {
        let (r, c) = (r - self.row0, c - self.col0);
        if !(0..self.rows).contains(&r) || !(0..self.cols).contains(&c) {
            return &[];
        }
        let i = (r * self.cols + c) as usize;
        &self.items[self.start[i] as usize..self.start[i + 1] as usize]
    }

    /// The triangles listed in the bin `p`, in grid units, falls in.
    fn at(&self, p: [f64; 2]) -> &[u32] {
        let b = f64::from(BIN_CELLS);
        self.get((p[0] / b).floor() as i64, (p[1] / b).floor() as i64)
    }

    fn bytes(&self) -> usize {
        (self.start.len() + self.items.len()) * 4
    }
}

/// One block's ground on one grid. Cell `(row, col)` has its center at
/// `(row, col)` in grid units.
#[derive(Debug, Clone)]
pub struct Prepared {
    pub index: usize,
    rc: Vec<[f64; 2]>,
    h: Vec<f64>,
    tri: Vec<[u32; 3]>,
    tritype: Vec<u32>,
    bins: Bins,
    /// The triangles of the block's runway subsets, binned as `tri` is: an
    /// airfield block holds tens of thousands.
    runway: Vec<[u32; 3]>,
    runway_bins: Bins,
}

/// A point in grid units.
pub fn grid_units(g: &Grid, x: f64, z: f64) -> [f64; 2] {
    let cs = f64::from(g.cell_size);
    [(x - g.origin_x) / cs - 0.5, (z - g.origin_z) / cs - 0.5]
}

impl Prepared {
    /// A block read with its `Runway` subset, prepared for `g`.
    pub fn new(b: Block, g: &Grid) -> Prepared {
        let n = b.p.len() as u32;
        let world: Vec<[f64; 3]> = (0..n).map(|i| b.world(i)).collect();
        let rc: Vec<[f64; 2]> = world.iter().map(|v| grid_units(g, v[0], v[2])).collect();
        let runway: Vec<[u32; 3]> = b
            .subsets
            .iter()
            .filter(|s| s.material == RUNWAY_MATERIAL)
            .flat_map(|s| s.tri.iter().copied())
            .collect();
        Prepared {
            index: b.index,
            bins: Bins::new(&rc, &b.tri),
            runway_bins: Bins::new(&rc, &runway),
            h: world.iter().map(|v| v[1]).collect(),
            rc,
            tri: b.tri,
            tritype: b.tritype,
            runway,
        }
    }

    /// Only the water triangles of a block that are not road strips, for
    /// `water_under`: a sample then tests the few near it, not the land.
    pub fn water(mut b: Block, g: &Grid) -> Prepared {
        let keep: Vec<bool> = b
            .tritype
            .iter()
            .map(|&t| water_class(t) != 0 && t & ROAD_STRIP == 0)
            .collect();
        let mut k = keep.iter();
        b.tri.retain(|_| *k.next().unwrap());
        b.tritype
            .retain(|&t| water_class(t) != 0 && t & ROAD_STRIP == 0);
        b.subsets.clear();
        Prepared::new(b, g)
    }

    /// Roughly the bytes it holds, for a cache's budget.
    pub fn bytes(&self) -> usize {
        self.rc.len() * 24
            + (self.tri.len() + self.runway.len()) * 12
            + self.tritype.len() * 4
            + self.bins.bytes()
            + self.runway_bins.bytes()
    }

    fn corners(&self, t: [u32; 3]) -> [[f64; 2]; 3] {
        t.map(|k| self.rc[k as usize])
    }

    fn height(&self, t: [u32; 3], u: f64, v: f64) -> f64 {
        let z = t.map(|k| self.h[k as usize]);
        z[0] + u * (z[1] - z[0]) + v * (z[2] - z[0])
    }

    /// Whether a runway subset of this block covers `p`, in grid units.
    pub fn on_runway(&self, p: [f64; 2]) -> bool {
        self.runway_bins.at(p).iter().any(|&t| {
            let [a, b, c] = self.corners(self.runway[t as usize]);
            cover(a, b, c, p).is_some()
        })
    }

    /// Whether a water triangle of this block covers `p`, in grid units,
    /// whatever wins there: a bridge's road strip covers the river it
    /// crosses. A road strip is not water, whatever its other bits say.
    pub fn water_under(&self, p: [f64; 2]) -> bool {
        self.bins.at(p).iter().any(|&t| {
            let ty = self.tritype[t as usize];
            let [a, b, c] = self.corners(self.tri[t as usize]);
            water_class(ty) != 0 && ty & ROAD_STRIP == 0 && cover(a, b, c, p).is_some()
        })
    }

    /// The winning triangle of this block at `p`, in grid units. Its
    /// `runway` is false: a runway subset of any block marks the point, as
    /// it marks a tile's cell, so the caller asks every block `on_runway`.
    pub fn point(&self, p: [f64; 2]) -> Option<Ground> {
        let mut best: Option<Ground> = None;
        for &t in self.bins.at(p) {
            let tri = self.tri[t as usize];
            let [a, b, c] = self.corners(tri);
            let Some((u, v)) = cover(a, b, c, p) else {
                continue;
            };
            let g = Ground {
                h: self.height(tri, u, v),
                tritype: self.tritype[t as usize],
                runway: false,
                block: self.index,
                tri: t as usize,
            };
            if best.is_none_or(|cur| g.rank() > cur.rank()) {
                best = Some(g);
            }
        }
        best
    }
}

/// The part of a segment one triangle covers: from `t[0]` to `t[1]` of the
/// way along it, the triangle's height rising linearly from `h[0]` to `h[1]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Piece {
    pub t: [f64; 2],
    pub h: [f64; 2],
    /// Where pieces overlap, the one of higher rank is the ground.
    pub rank: u64,
}

impl Prepared {
    /// Every triangle of this block over the segment from `a` to `b`, in
    /// grid units, clipped to the part of it the triangle covers.
    pub fn along(&self, a: [f64; 2], b: [f64; 2], out: &mut Vec<Piece>) {
        let bins = &self.bins;
        if bins.rows == 0 {
            return;
        }
        let w = f64::from(BIN_CELLS);
        let d = [b[0] - a[0], b[1] - a[1]];
        // The rows of bins the segment crosses, and in each the columns of
        // the stretch of it within that row, widened by the bins' slack: a
        // triangle that covers any point of the segment is listed in the
        // bin that point falls in.
        let (r0, r1) = bin_span(a[0].min(b[0]), a[0].max(b[0]));
        let mut seen: Vec<u32> = Vec::new();
        for r in r0.max(bins.row0)..=r1.min(bins.row0 + bins.rows - 1) {
            let (lo, hi) = (r as f64 * w - BIN_SLACK, (r + 1) as f64 * w + BIN_SLACK);
            let (t0, t1) = if d[0] == 0.0 {
                (0.0, 1.0)
            } else {
                let (u, v) = ((lo - a[0]) / d[0], (hi - a[0]) / d[0]);
                (u.min(v).max(0.0), u.max(v).min(1.0))
            };
            let (z0, z1) = (a[1] + t0 * d[1], a[1] + t1 * d[1]);
            let (c0, c1) = bin_span(z0.min(z1), z0.max(z1));
            for c in c0..=c1 {
                seen.extend_from_slice(bins.get(r, c));
            }
        }
        seen.sort_unstable();
        seen.dedup();
        for k in seen {
            let tri = self.tri[k as usize];
            let z = tri.map(|v| self.h[v as usize]);
            if let Some((t, h)) = clip(self.corners(tri), z, a, d) {
                out.push(Piece {
                    t,
                    h,
                    rank: rank(self.tritype[k as usize], self.index, k as usize),
                });
            }
        }
    }
}

/// The stretch of the segment `a + t d`, `t` in 0..1, that the triangle
/// with corners `c` and heights `z` covers, as `cover` decides it, and the
/// triangle's height at each end of it; `None` where it covers none of it
/// or only a point.
fn clip(c: [[f64; 2]; 3], z: [f64; 3], a: [f64; 2], d: [f64; 2]) -> Option<([f64; 2], [f64; 2])> {
    let (v0x, v0z) = (c[1][0] - c[0][0], c[1][1] - c[0][1]);
    let (v1x, v1z) = (c[2][0] - c[0][0], c[2][1] - c[0][1]);
    let den = v0x * v1z - v1x * v0z;
    if den == 0.0 {
        return None;
    }
    // The weights towards the second and third corners, each `w + t dw`.
    let (px, pz) = (a[0] - c[0][0], a[1] - c[0][1]);
    let u = ((px * v1z - v1x * pz) / den, (d[0] * v1z - v1x * d[1]) / den);
    let v = ((v0x * pz - px * v0z) / den, (v0x * d[1] - d[0] * v0z) / den);
    let e = EDGE_TOLERANCE;
    let (mut lo, mut hi) = (0.0f64, 1.0f64);
    // Each of u, v and 1 - u - v at least -e.
    for (f0, fd) in [
        (u.0 + e, u.1),
        (v.0 + e, v.1),
        (1.0 + e - u.0 - v.0, -u.1 - v.1),
    ] {
        if fd == 0.0 {
            if f0 < 0.0 {
                return None;
            }
        } else if fd > 0.0 {
            lo = lo.max(-f0 / fd);
        } else {
            hi = hi.min(-f0 / fd);
        }
    }
    if lo >= hi {
        return None;
    }
    let h = |t: f64| z[0] + (u.0 + t * u.1) * (z[1] - z[0]) + (v.0 + t * v.1) * (z[2] - z[0]);
    Some(([lo, hi], [h(lo), h(hi)]))
}

/// For each of `points`, in grid units, block `b`'s winning triangle there
/// and whether its runway subset covers it, testing every triangle rather
/// than binning them: for a handful of points, binning a block costs more
/// than it saves. The answers are `Prepared::point` and `on_runway`'s.
pub fn few(b: &Block, g: &Grid, points: &[[f64; 2]]) -> Vec<(Option<Ground>, bool)> {
    let world: Vec<[f64; 3]> = (0..b.p.len() as u32).map(|i| b.world(i)).collect();
    let rc: Vec<[f64; 2]> = world.iter().map(|v| grid_units(g, v[0], v[2])).collect();
    let mut out = vec![(None, false); points.len()];
    // A corner box widened by the bins' slack holds every point `cover`
    // accepts, as a bin does.
    let near = |c: &[[f64; 2]; 3], p: [f64; 2]| {
        (0..2).all(|k| {
            let (lo, hi) = span(c, k);
            p[k] >= lo - BIN_SLACK && p[k] <= hi + BIN_SLACK
        })
    };
    for (t, tri) in b.tri.iter().enumerate() {
        let c = tri.map(|k| rc[k as usize]);
        for (p, o) in points.iter().zip(out.iter_mut()) {
            if !near(&c, *p) {
                continue;
            }
            let Some((u, v)) = cover(c[0], c[1], c[2], *p) else {
                continue;
            };
            let z = tri.map(|k| world[k as usize][1]);
            let g = Ground {
                h: z[0] + u * (z[1] - z[0]) + v * (z[2] - z[0]),
                tritype: b.tritype[t],
                runway: false,
                block: b.index,
                tri: t,
            };
            if o.0.is_none_or(|cur: Ground| g.rank() > cur.rank()) {
                o.0 = Some(g);
            }
        }
    }
    let runway = b.subsets.iter().filter(|s| s.material == RUNWAY_MATERIAL);
    for tri in runway.flat_map(|s| s.tri.iter()) {
        let c = tri.map(|k| rc[k as usize]);
        for (p, o) in points.iter().zip(out.iter_mut()) {
            if !o.1 && near(&c, *p) && cover(c[0], c[1], c[2], *p).is_some() {
                o.1 = true;
            }
        }
    }
    out
}

/// The ground at `p`, in grid units, from every block that may cover it.
pub fn point<'a>(blocks: impl IntoIterator<Item = &'a Prepared>, p: [f64; 2]) -> Option<Ground> {
    let mut best: Option<Ground> = None;
    let mut runway = false;
    for b in blocks {
        best = merge(best, b.point(p));
        runway |= b.on_runway(p);
    }
    best.map(|mut g| {
        g.runway = runway;
        g
    })
}

/// One tile of every layer, row-major with columns fastest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tile {
    pub height: Vec<i16>,
    pub water: Vec<u8>,
    pub surface: Vec<u8>,
    pub cover: Vec<u8>,
}

impl Tile {
    /// One layer's cells, as `Source` hands them out.
    pub fn layer(&self, layer: Layer) -> Vec<i32> {
        match layer {
            Layer::Height => self.height.iter().map(|&v| i32::from(v)).collect(),
            Layer::Water => self.water.iter().map(|&v| i32::from(v)).collect(),
            Layer::Surface => self.surface.iter().map(|&v| i32::from(v)).collect(),
            Layer::Cover => self.cover.iter().map(|&v| i32::from(v)).collect(),
        }
    }
}

/// The cells of a triangle with corners `p` inside the half-open cell
/// rectangle `[r0, r1) × [c0, c1)`, as inclusive row and column ranges.
fn cells(
    p: [[f64; 2]; 3],
    (r0, r1): (i64, i64),
    (c0, c1): (i64, i64),
) -> Option<(std::ops::RangeInclusive<i64>, std::ops::RangeInclusive<i64>)> {
    // A center a hair outside the corners' box is still covered within the
    // edge tolerance, as a point there is, so the box is widened by the
    // bins' slack.
    let clip = |k: usize, start: i64, end: i64| {
        let (lo, hi) = span(&p, k);
        let lo = ((lo - BIN_SLACK).ceil() as i64).max(start);
        let hi = ((hi + BIN_SLACK).floor() as i64).min(end - 1);
        (lo <= hi).then_some(lo..=hi)
    };
    Some((clip(0, r0, r1)?, clip(1, c0, c1)?))
}

/// Rasterizes tile `(tx, tz)` of `g` from the blocks under it, each cell
/// the winning triangle at its center. `g.tile_size` must be a whole number
/// of bins.
pub fn tile(blocks: &[&Prepared], g: &Grid, tx: u32, tz: u32) -> Tile {
    assert!(
        g.tile_size.is_multiple_of(BIN_CELLS),
        "a tile is a whole number of bins"
    );
    let ts = i64::from(g.tile_size);
    let n = (ts * ts) as usize;
    const NONE: u64 = u64::MAX;
    let mut best = vec![NONE; n];
    let mut h = vec![0f64; n];
    let mut ty = vec![0u32; n];
    let mut runway = vec![false; n];
    let (tr0, tc0) = (i64::from(tx) * ts, i64::from(tz) * ts);
    // The tile's cells on the grid.
    let rows = (tr0, (tr0 + ts).min(i64::from(g.height)));
    let cols = (tc0, (tc0 + ts).min(i64::from(g.width)));
    let at = |r: i64, c: i64| ((r - tr0) * ts + (c - tc0)) as usize;
    let bb = i64::from(BIN_CELLS);
    let per = ts / bb;

    for b in blocks {
        for br in tr0 / bb..tr0 / bb + per {
            for bc in tc0 / bb..tc0 / bb + per {
                let bin_rows = (rows.0.max(br * bb), rows.1.min((br + 1) * bb));
                let bin_cols = (cols.0.max(bc * bb), cols.1.min((bc + 1) * bb));
                for &t in b.bins.get(br, bc) {
                    let tri = b.tri[t as usize];
                    let p = b.corners(tri);
                    let Some((rr, cc)) = cells(p, bin_rows, bin_cols) else {
                        continue;
                    };
                    let k = rank(b.tritype[t as usize], b.index, t as usize);
                    for r in rr {
                        for c in cc.clone() {
                            let Some((u, v)) = cover(p[0], p[1], p[2], [r as f64, c as f64]) else {
                                continue;
                            };
                            let i = at(r, c);
                            if best[i] == NONE || k > best[i] {
                                best[i] = k;
                                h[i] = b.height(tri, u, v);
                                ty[i] = b.tritype[t as usize];
                            }
                        }
                    }
                }
                for &t in b.runway_bins.get(br, bc) {
                    let p = b.corners(b.runway[t as usize]);
                    let Some((rr, cc)) = cells(p, bin_rows, bin_cols) else {
                        continue;
                    };
                    for r in rr {
                        for c in cc.clone() {
                            if cover(p[0], p[1], p[2], [r as f64, c as f64]).is_some() {
                                runway[at(r, c)] = true;
                            }
                        }
                    }
                }
            }
        }
    }

    let mut out = Tile {
        height: vec![Layer::Height.nodata() as i16; n],
        water: vec![Layer::Water.nodata() as u8; n],
        surface: vec![Layer::Surface.nodata() as u8; n],
        cover: vec![Layer::Cover.nodata() as u8; n],
    };
    for i in 0..n {
        if best[i] == NONE {
            continue;
        }
        out.height[i] = round_height(h[i]);
        out.water[i] = water_class(ty[i]);
        out.surface[i] = surface_of(ty[i], runway[i]);
        out.cover[i] = cover_of(ty[i]).code() as u8;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::build::{self, f32s, file, tri, u32s, vec3};
    use super::super::mesh::Surface5;
    use super::super::sample::{LAKE, SEA};
    use super::*;

    /// A kilometer square at the origin in two triangles: the south-west
    /// half rising from 100 m in the south to 200 m in the north, the
    /// north-east half a lake. A runway subset covers a triangle near the
    /// south-west corner, over three vertices the full list does not use.
    fn square(tritype: [u32; 2]) -> build::Block {
        build::Block {
            t: [0.0, 0.0, 0.0],
            bbox: [0.0, 0.0, 0.0, 1000.0, 300.0, 1000.0],
            arrays: vec![
                u32s("TRITYPE", &tritype),
                tri(&[[0, 1, 2], [1, 3, 2]], Some(("Surface5.0", "Land"))),
                u32s("baseIndex", &[4, 5, 6]),
                tri(&[[0, 1, 2]], Some(("Runway5.0", "Runway"))),
                vec3(
                    "Pbase",
                    &[
                        [0.0, 100.0, 0.0],
                        [1000.0, 200.0, 0.0],
                        [0.0, 100.0, 1000.0],
                        [1000.0, 50.0, 1000.0],
                        [100.0, 110.0, 100.0],
                        [300.0, 130.0, 100.0],
                        [100.0, 110.0, 300.0],
                    ],
                ),
                f32s("depth", &[0.0, 0.0, 0.0, 50.0, 0.0, 0.0, 0.0]),
            ],
        }
    }

    /// A 64-cell tile grid over `cells` square cells at the origin.
    fn grid(cells: u32) -> Grid {
        Grid {
            cell_size: 50,
            origin_x: 0.0,
            origin_z: 0.0,
            height: cells,
            width: cells,
            tile_size: 64,
        }
    }

    fn prepared(blocks: &[build::Block], g: &Grid) -> Vec<Prepared> {
        let f = file(blocks, blocks.len() as u32);
        let s = Surface5::open(&f[..], "t").unwrap();
        (0..blocks.len())
            .map(|i| Prepared::new(s.read_block(&f[..], i, &[RUNWAY_MATERIAL]).unwrap(), g))
            .collect()
    }

    fn at(blocks: &[Prepared], g: &Grid, x: f64, z: f64) -> Option<Ground> {
        let p = grid_units(g, x, z);
        point(blocks, p)
    }

    fn cell(t: &Tile, r: usize, c: usize) -> [i32; 4] {
        let i = r * 64 + c;
        [
            i32::from(t.height[i]),
            i32::from(t.water[i]),
            i32::from(t.surface[i]),
            i32::from(t.cover[i]),
        ]
    }

    #[test]
    fn a_tile_cell_is_the_point_at_its_center() {
        let g = grid(20);
        let b = prepared(&[square([0, LAKE])], &g);
        let refs: Vec<&Prepared> = b.iter().collect();
        let t = tile(&refs, &g, 0, 0);
        // (25, 25): 102.5 m rounds up; plain land.
        let plain = Cover::Plain.code();
        assert_eq!(cell(&t, 0, 0), [103, 0, 1, plain]);
        // (125, 125) lies inside the runway subset, on ground with no runway bit.
        assert_eq!(cell(&t, 2, 2), [113, 0, 5, plain]);
        // (975, 975), the lake: its surface, not its bed.
        assert_eq!(cell(&t, 19, 19)[1..], [1, 3, Cover::Water.code()]);
        // Past the grid's edge is nodata.
        assert_eq!(cell(&t, 20, 0), Layer::ALL.map(Layer::nodata));
        for r in 0..20 {
            for c in 0..20 {
                let (x, z) = g.centre(r, c);
                let p = at(&b, &g, x, z).unwrap();
                let want = Layer::ALL.map(|l| p.value(l));
                assert_eq!(cell(&t, r as usize, c as usize), want, "({r}, {c})");
            }
        }
    }

    #[test]
    fn the_grid_is_the_bounds_snapped_outward() {
        let g = grid_for(
            &Bounds {
                min_x: -490048.7,
                min_z: -570000.0,
                max_x: 270000.0,
                max_z: 960039.4,
            },
            50,
            64,
        );
        assert_eq!((g.origin_x, g.origin_z), (-490050.0, -570000.0));
        assert_eq!((g.height, g.width), (15201, 30601));
        assert_eq!((g.cell_size, g.tile_size), (50, 64));
    }

    #[test]
    fn a_point_interpolates_within_its_triangle() {
        let g = grid(20);
        let b = prepared(&[square([0, LAKE])], &g);
        let p = at(&b, &g, 500.0, 250.0).unwrap();
        assert!((p.h - 150.0).abs() < 1e-9, "{}", p.h);
        assert_eq!((p.tri, p.runway), (0, false));
        assert!(at(&b, &g, 200.0, 200.0).unwrap().runway);
        assert!(at(&b, &g, -1.0, 500.0).is_none());
        assert!(at(&b, &g, 500.0, 1000.5).is_none());
    }

    #[test]
    fn a_segment_is_cut_where_it_crosses_triangle_edges() {
        let g = grid(24);
        let b = prepared(&[square([0, LAKE])], &g);
        // From south of the square, across the diagonal, to past its east edge.
        let (a, e) = ([100.0, -50.0], [900.0, 1050.0]);
        let mut pieces = Vec::new();
        b[0].along(
            grid_units(&g, a[0], a[1]),
            grid_units(&g, e[0], e[1]),
            &mut pieces,
        );
        assert_eq!(pieces.len(), 2, "{pieces:?}");
        for k in 0..=200 {
            let t = f64::from(k) / 200.0;
            let (x, z) = (a[0] + t * (e[0] - a[0]), a[1] + t * (e[1] - a[1]));
            let want = at(&b, &g, x, z);
            let on: Vec<&Piece> = pieces
                .iter()
                .filter(|p| p.t[0] <= t && t <= p.t[1])
                .collect();
            assert_eq!(want.is_some(), !on.is_empty(), "t {t}");
            if let Some(w) = want {
                let p = on.iter().max_by_key(|p| p.rank).unwrap();
                let h = p.h[0] + (t - p.t[0]) / (p.t[1] - p.t[0]) * (p.h[1] - p.h[0]);
                assert!((h - w.h).abs() < 1e-6, "t {t}: {h} against {}", w.h);
                assert_eq!(p.rank, w.rank());
            }
        }
        // Both ends land on the square's edges, within the edge tolerance.
        let lo = pieces.iter().map(|p| p.t[0]).fold(1.0, f64::min);
        let hi = pieces.iter().map(|p| p.t[1]).fold(0.0, f64::max);
        assert!((lo - 50.0 / 1100.0).abs() < 1e-6 && (hi - 1050.0 / 1100.0).abs() < 1e-6);
    }

    #[test]
    fn a_road_strip_wins_over_a_later_block_and_water_over_land() {
        let g = grid(20);
        let mut low = square([0, 0]);
        low.t[1] = -50.0;
        let b = prepared(&[square([ROAD_STRIP, ROAD_STRIP]), low], &g);
        let p = at(&b, &g, 25.0, 25.0).unwrap();
        assert_eq!((p.block, p.value(Layer::Surface)), (0, SURFACE_ROAD.into()));

        let mut land = square([0, 0]);
        land.t[1] = 50.0;
        let b = prepared(&[square([LAKE, LAKE]), land], &g);
        assert_eq!(at(&b, &g, 25.0, 25.0).unwrap().block, 0);
    }

    #[test]
    fn a_runway_subset_marks_a_point_another_block_wins() {
        // The first block's own triangle covers only its south-west half,
        // and its runway subset lies in the north-east half, where only the
        // second block's ground is.
        let mut a = square([0, 0]);
        a.arrays[0] = u32s("TRITYPE", &[0]);
        a.arrays[1] = tri(&[[0, 1, 2]], Some(("Surface5.0", "Land")));
        a.arrays[2] = u32s("baseIndex", &[3, 5, 6]);
        a.arrays[4] = vec3(
            "Pbase",
            &[
                [0.0, 100.0, 0.0],
                [1000.0, 200.0, 0.0],
                [0.0, 100.0, 1000.0],
                [1000.0, 100.0, 1000.0],
                [0.0; 3],
                [600.0, 100.0, 1000.0],
                [1000.0, 100.0, 600.0],
            ],
        );
        a.arrays[5] = f32s("depth", &[0.0; 7]);
        let g = grid(20);
        let b = prepared(&[a, square([0, 0])], &g);
        let p = at(&b, &g, 900.0, 900.0).unwrap();
        assert_eq!(
            (p.block, p.value(Layer::Surface)),
            (1, SURFACE_RUNWAY.into())
        );
        let refs: Vec<&Prepared> = b.iter().collect();
        let t = tile(&refs, &g, 0, 0);
        assert_eq!(t.surface[18 * 64 + 18], SURFACE_RUNWAY);
    }

    #[test]
    fn the_runway_index_finds_what_a_search_of_every_runway_triangle_finds() {
        // A 20 km block whose runway subset spans many bins, two of its
        // triangles sharing an edge and one a sliver.
        let s = 20000.0;
        let block = build::Block {
            t: [0.0; 3],
            bbox: [0.0, 0.0, 0.0, s, 1000.0, s],
            arrays: vec![
                u32s("TRITYPE", &[0, 0]),
                tri(&[[0, 1, 2], [1, 3, 2]], Some(("Surface5.0", "Land"))),
                u32s("baseIndex", &[4, 5, 6, 7, 8]),
                tri(
                    &[[0, 1, 2], [1, 3, 2], [0, 4, 1]],
                    Some(("Runway5.0", "Runway")),
                ),
                vec3(
                    "Pbase",
                    &[
                        [0.0, 0.0, 0.0],
                        [s, 1000.0, 0.0],
                        [0.0, 500.0, s],
                        [s, 200.0, s],
                        [3000.0, 0.0, 2000.0],
                        [9000.0, 0.0, 2400.0],
                        [3100.0, 0.0, 6000.0],
                        [9200.0, 0.0, 6100.0],
                        [6000.0, 0.0, 2150.0],
                    ],
                ),
            ],
        };
        let g = grid(400);
        let b = &prepared(&[block], &g)[0];
        assert_eq!(b.runway.len(), 3);
        let scan = |p: [f64; 2]| {
            b.runway.iter().any(|&t| {
                let [x, y, z] = b.corners(t);
                cover(x, y, z, p).is_some()
            })
        };
        let mut on = 0;
        for k in 0..20000 {
            let p = [f64::from(k % 200) * 1.03 + 0.37, f64::from(k / 200) * 1.31];
            let want = scan(p);
            assert_eq!(b.on_runway(p), want, "{p:?}");
            on += usize::from(want);
        }
        assert!(on > 1000, "{on} points on the runway");
        // Corners and edges, where the tolerance reaches past a bin.
        for v in &b.rc {
            assert_eq!(b.on_runway(*v), scan(*v), "{v:?}");
        }
        // A tile marks the cells the scan finds, across bins and tile edges.
        let (ntx, ntz) = g.tiles();
        for tx in 0..ntx {
            for tz in 0..ntz {
                let t = tile(&[b], &g, tx, tz);
                for i in 0..64 * 64 {
                    let (r, c) = (tx * 64 + i / 64, tz * 64 + i % 64);
                    if r >= 400 || c >= 400 {
                        continue;
                    }
                    let want = scan([f64::from(r), f64::from(c)]);
                    assert_eq!(t.surface[i as usize] == SURFACE_RUNWAY, want, "({r}, {c})");
                }
            }
        }
    }

    #[test]
    fn cover_takes_water_then_the_first_bit_in_order() {
        assert_eq!(cover_of(SEA | 1024), Cover::Water);
        assert_eq!(cover_of(160), Cover::Runway);
        assert_eq!(cover_of(192), Cover::Town);
        assert_eq!(cover_of(1024 | 128), Cover::Forest);
        assert_eq!(cover_of(128 | 256), Cover::Field);
        assert_eq!(cover_of(256), Cover::Beach);
        // A road strip keeps the ground's bit.
        assert_eq!(cover_of(ROAD_STRIP | 1024), Cover::Forest);
        assert_eq!(cover_of(ROAD_STRIP), Cover::Plain);
    }

    #[test]
    fn tiles_across_blocks_match_points_everywhere() {
        // Two 20 km blocks, the second offset 10 km north and east, so its
        // triangles span many bins and tiles, the last tile partial.
        let block = |t: [f32; 3], tritype: [u32; 2]| {
            let s = 20000.0;
            build::Block {
                t,
                bbox: [0.0, 0.0, 0.0, s, 1000.0, s],
                arrays: vec![
                    u32s("TRITYPE", &tritype),
                    tri(&[[0, 1, 2], [1, 3, 2]], None),
                    vec3(
                        "Pbase",
                        &[
                            [0.0, 0.0, 0.0],
                            [s, 1000.0, 0.0],
                            [0.0, 500.0, s],
                            [s, 200.0, s],
                        ],
                    ),
                ],
            }
        };
        let g = grid(600);
        let blocks = [
            block([0.0; 3], [0, LAKE]),
            block([10000.0, 0.0, 10000.0], [ROAD_STRIP | 1024, 64]),
        ];
        let b = prepared(&blocks, &g);
        // The index finds what a search of every triangle finds.
        let f = file(&blocks, 2);
        let mesh = Surface5::open(&f[..], "t")
            .unwrap()
            .read_all(&f[..], &[])
            .unwrap();
        for k in 0..2000 {
            let (x, z) = (f64::from(k) * 15.1 - 100.0, f64::from(k * 7 % 2000) * 15.3);
            let got = at(&b, &g, x, z);
            let want = mesh.sample(x, z);
            let key = |h: f64, tt, b, t| ((h * 1e6).round(), tt, b, t);
            assert_eq!(
                got.map(|p| key(p.h, p.tritype, p.block, p.tri)),
                want.map(|h| key(h.h, h.tritype, h.block, h.tri)),
                "({x}, {z})"
            );
        }
        let refs: Vec<&Prepared> = b.iter().collect();
        let (ntx, ntz) = g.tiles();
        assert_eq!((ntx, ntz), (10, 10));
        for tx in 0..ntx {
            for tz in 0..ntz {
                let t = tile(&refs, &g, tx, tz);
                for i in 0..64 * 64 {
                    let (r, c) = (tx * 64 + i / 64, tz * 64 + i % 64);
                    let got = cell(&t, (i / 64) as usize, (i % 64) as usize);
                    let want = if r >= 600 || c >= 600 {
                        Layer::ALL.map(Layer::nodata)
                    } else {
                        let (x, z) = g.centre(r, c);
                        at(&b, &g, x, z).map_or(Layer::ALL.map(Layer::nodata), |p| {
                            Layer::ALL.map(|l| p.value(l))
                        })
                    };
                    assert_eq!(got, want, "({r}, {c})");
                }
            }
        }
    }
}
