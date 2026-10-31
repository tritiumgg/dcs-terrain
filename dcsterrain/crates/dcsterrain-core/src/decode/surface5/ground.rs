//! The ground of one block, ready to answer: its vertices as the file holds
//! them, a hierarchy of boxes over its triangles, and from those a tile's
//! cells, a single point or the triangles under a segment.
//!
//! Grid units are found from a vertex each time it is wanted, so a block
//! costs its hierarchy and no second copy of its vertices. A box in the
//! block's meters rules out nearly every triangle before one is converted.

use super::bvh::{Bvh, Line, tri_box};
use super::mesh::Block;
use super::sample::{EDGE_TOLERANCE, ROAD_STRIP, cover, rank, round_height, water_class};
use crate::extract::Bounds;
use crate::grid::Grid;
use crate::source::{Cover, Layer};

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
    /// The water's depth over the bed, as interpolated; zero where the
    /// triangle's corners carry none.
    pub depth: f64,
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

/// `x.floor() as i64`, for every `x`, without the call into the C runtime
/// that the default x86-64 target makes for `floor`: preparing a block
/// takes four for each triangle.
fn floor_i64(x: f64) -> i64 {
    let t = x as i64;
    t.saturating_sub(i64::from(t as f64 > x))
}

/// A value over a triangle from its corners', at `u` of the way to the
/// second corner and `v` to the third.
fn lerp(z: [f64; 3], u: f64, v: f64) -> f64 {
    z[0] + u * (z[1] - z[0]) + v * (z[2] - z[0])
}

fn span(corners: &[[f64; 2]], k: usize) -> (f64, f64) {
    corners
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), c| {
            (lo.min(c[k]), hi.max(c[k]))
        })
}

/// One block's ground on one grid. Cell `(row, col)` has its center at
/// `(row, col)` in grid units.
#[derive(Debug, Clone)]
pub struct Prepared {
    pub index: usize,
    block: Block,
    /// The block's place, the cell size and the grid's origin, as f64.
    t: [f64; 3],
    cell: f64,
    origin: [f64; 2],
    /// Whether any vertex has water depth: most land blocks have none.
    wet: bool,
    /// Whether it has no hierarchy and is scanned, for one tile.
    bare: bool,
    bvh: Bvh,
    /// The triangles of the block's runway subsets, searched as `tri` is: an
    /// airfield block holds tens of thousands.
    runway: Vec<[u32; 3]>,
    runway_bvh: Bvh,
}

/// A point in grid units.
pub fn grid_units(g: &Grid, x: f64, z: f64) -> [f64; 2] {
    let cs = f64::from(g.cell_size);
    [(x - g.origin_x) / cs - 0.5, (z - g.origin_z) / cs - 0.5]
}

impl Prepared {
    /// A block read with its `Runway` subset, prepared for `g`.
    pub fn new(mut b: Block, g: &Grid) -> Prepared {
        let all = [b.bbox[0], b.bbox[2], b.bbox[3], b.bbox[5]];
        let runway: Vec<[u32; 3]> = b
            .subsets
            .iter()
            .filter(|s| s.material == RUNWAY_MATERIAL)
            .flat_map(|s| s.tri.iter().copied())
            .collect();
        b.subsets = Vec::new();
        Prepared {
            index: b.index,
            t: b.t.map(f64::from),
            cell: f64::from(g.cell_size),
            origin: [g.origin_x, g.origin_z],
            wet: b.depth.iter().any(|&d| d != 0.0),
            bare: false,
            bvh: Bvh::new(&b.p, &b.tri, all),
            runway_bvh: Bvh::new(&b.p, &runway, all),
            runway,
            block: b,
        }
    }

    /// A block read for one tile and dropped: building a hierarchy costs ten
    /// scans of it, which only a block asked many times repays.
    pub fn bare(mut b: Block, g: &Grid) -> Prepared {
        let runway: Vec<[u32; 3]> = b
            .subsets
            .iter()
            .filter(|s| s.material == RUNWAY_MATERIAL)
            .flat_map(|s| s.tri.iter().copied())
            .collect();
        b.subsets = Vec::new();
        Prepared {
            index: b.index,
            t: b.t.map(f64::from),
            cell: f64::from(g.cell_size),
            origin: [g.origin_x, g.origin_z],
            wet: b.depth.iter().any(|&d| d != 0.0),
            bare: true,
            bvh: Bvh::with_pad(),
            runway_bvh: Bvh::with_pad(),
            runway,
            block: b,
        }
    }

    /// Calls `f` with triangles of `bvh`, of which there are `n`, among them
    /// every one whose box meets `q`.
    fn each_near(&self, bvh: &Bvh, n: usize, q: [f32; 4], mut f: impl FnMut(&[u32])) {
        if !self.bare {
            bvh.search(|node| node.meets(q), f);
            return;
        }
        let mut ids = [0u32; 256];
        for start in (0..n).step_by(ids.len()) {
            let m = ids.len().min(n - start);
            for (k, id) in ids[..m].iter_mut().enumerate() {
                *id = (start + k) as u32;
            }
            f(&ids[..m]);
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
        b.tri = (b.tri.iter().copied())
            .filter(|_| *k.next().unwrap())
            .collect::<Vec<_>>()
            .into();
        b.tritype = (b.tritype.iter().copied())
            .filter(|&t| water_class(t) != 0 && t & ROAD_STRIP == 0)
            .collect::<Vec<_>>()
            .into();
        b.subsets.clear();
        Prepared::new(b, g)
    }

    /// Roughly the bytes it holds, for a cache's budget.
    pub fn bytes(&self) -> usize {
        self.block.p.private_bytes()
            + self.block.tri.private_bytes()
            + self.block.depth.private_bytes()
            + self.block.tritype.private_bytes()
            + self.runway.len() * 12
            + self.bvh.bytes()
            + self.runway_bvh.bytes()
    }

    /// Vertex `k` in grid units, as `grid_units` gives it from the world.
    #[inline]
    fn rc(&self, k: u32) -> [f64; 2] {
        let p = self.block.p[k as usize];
        let x = self.t[0] + f64::from(p[0]);
        let z = self.t[2] + f64::from(p[2]);
        [
            (x - self.origin[0]) / self.cell - 0.5,
            (z - self.origin[1]) / self.cell - 0.5,
        ]
    }

    /// Vertex `k`'s height, as `Block::world` gives it.
    #[inline]
    fn h(&self, k: u32) -> f64 {
        let k = k as usize;
        self.t[1] + f64::from(self.block.p[k][1]) + f64::from(self.block.depth_at(k as u32))
    }

    /// A point in grid units, in the block's own meters.
    #[inline]
    fn local(&self, p: [f64; 2]) -> [f32; 2] {
        [
            ((p[0] + 0.5) * self.cell + self.origin[0] - self.t[0]) as f32,
            ((p[1] + 0.5) * self.cell + self.origin[1] - self.t[2]) as f32,
        ]
    }

    #[inline]
    fn corners(&self, t: [u32; 3]) -> [[f64; 2]; 3] {
        t.map(|k| self.rc(k))
    }

    /// The box of triangle `t` in the block's own meters.
    #[inline]
    fn box_of(&self, t: [u32; 3]) -> [f32; 4] {
        tri_box(t.map(|k| self.block.p[k as usize]))
    }

    fn height(&self, t: [u32; 3], u: f64, v: f64) -> f64 {
        lerp(t.map(|k| self.h(k)), u, v)
    }

    fn depth(&self, t: [u32; 3], u: f64, v: f64) -> f64 {
        if !self.wet {
            return 0.0;
        }
        lerp(t.map(|k| f64::from(self.block.depth_at(k))), u, v)
    }

    /// Whether the box `e`, in the block's meters, holds no cell's center,
    /// a hair more than the slack of `cells` to spare.
    #[inline]
    fn between_cells(&self, e: [f32; 4]) -> bool {
        const SPARE: f64 = 2e-3;
        let inv = 1.0 / self.cell;
        let at =
            |v: f32, axis: usize| (f64::from(v) + self.t[axis * 2] - self.origin[axis]) * inv - 0.5;
        let none = |lo: f64, hi: f64| floor_i64(hi + SPARE) < -floor_i64(SPARE - lo);
        none(at(e[0], 0), at(e[2], 0)) || none(at(e[1], 1), at(e[3], 1))
    }

    /// The question of `p`, in grid units, for `bvh`: a box around it.
    #[inline]
    fn around(&self, p: [f64; 2], bvh: &Bvh) -> [f32; 4] {
        let l = self.local(p);
        let pad = bvh.pad();
        [l[0] - pad, l[1] - pad, l[0] + pad, l[1] + pad]
    }

    /// The question of a box `[r0, c0, r1, c1]` in grid units, for `bvh`.
    #[inline]
    fn around_box(&self, b: [f64; 4], bvh: &Bvh) -> [f32; 4] {
        let (lo, hi) = (self.local([b[0], b[1]]), self.local([b[2], b[3]]));
        let pad = bvh.pad();
        [lo[0] - pad, lo[1] - pad, hi[0] + pad, hi[1] + pad]
    }

    /// Whether a runway subset of this block covers `p`, in grid units.
    pub fn on_runway(&self, p: [f64; 2]) -> bool {
        let q = self.around(p, &self.runway_bvh);
        let mut on = false;
        self.runway_bvh.search(
            |n| n.meets(q),
            |leaf| {
                on = on
                    || leaf.iter().any(|&t| {
                        let t = self.runway[t as usize];
                        meet(self.box_of(t), q) && {
                            let [a, b, c] = self.corners(t);
                            cover(a, b, c, p).is_some()
                        }
                    });
            },
        );
        on
    }

    /// Every triangle of this block's runway subsets whose box meets `b`,
    /// `[r0, c0, r1, c1]` in grid units, its corners in grid units.
    pub fn runway_within(&self, b: [f64; 4], out: &mut Vec<[[f64; 2]; 3]>) {
        let q = self.around_box(b, &self.runway_bvh);
        let mut seen: Vec<u32> = Vec::new();
        self.runway_bvh
            .search(|n| n.meets(q), |leaf| seen.extend_from_slice(leaf));
        seen.sort_unstable();
        for t in seen {
            let p = self.corners(self.runway[t as usize]);
            let (r0, r1) = span(&p, 0);
            let (c0, c1) = span(&p, 1);
            if r0 <= b[2] && r1 >= b[0] && c0 <= b[3] && c1 >= b[1] {
                out.push(p);
            }
        }
    }

    /// Whether a water triangle of this block covers `p`, in grid units,
    /// whatever wins there: a bridge's road strip covers the river it
    /// crosses. A road strip is not water, whatever its other bits say.
    pub fn water_under(&self, p: [f64; 2]) -> bool {
        let q = self.around(p, &self.bvh);
        let mut under = false;
        self.bvh.search(
            |n| n.meets(q),
            |leaf| {
                under = under
                    || leaf.iter().any(|&t| {
                        let ty = self.block.tritype[t as usize];
                        let tri = self.block.tri[t as usize];
                        water_class(ty) != 0 && ty & ROAD_STRIP == 0 && {
                            let [a, b, c] = self.corners(tri);
                            cover(a, b, c, p).is_some()
                        }
                    });
            },
        );
        under
    }

    /// The winning triangle of this block at `p`, in grid units. Its
    /// `runway` is false: a runway subset of any block marks the point, as
    /// it marks a tile's cell, so the caller asks every block `on_runway`.
    pub fn point(&self, p: [f64; 2]) -> Option<Ground> {
        let q = self.around(p, &self.bvh);
        let mut best: Option<Ground> = None;
        self.bvh.search(
            |n| n.meets(q),
            |leaf| {
                for &t in leaf {
                    let tri = self.block.tri[t as usize];
                    if !meet(self.box_of(tri), q) {
                        continue;
                    }
                    let [a, b, c] = self.corners(tri);
                    let Some((u, v)) = cover(a, b, c, p) else {
                        continue;
                    };
                    let g = Ground {
                        h: self.height(tri, u, v),
                        depth: self.depth(tri, u, v),
                        tritype: self.block.tritype[t as usize],
                        runway: false,
                        block: self.index,
                        tri: t as usize,
                    };
                    if best.is_none_or(|cur| g.rank() > cur.rank()) {
                        best = Some(g);
                    }
                }
            },
        );
        best
    }
}

/// Whether two boxes, `[min_x, min_z, max_x, max_z]`, meet.
#[inline]
fn meet(a: [f32; 4], b: [f32; 4]) -> bool {
    a[0] <= b[2] && a[2] >= b[0] && a[1] <= b[3] && a[3] >= b[1]
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
        let d = [b[0] - a[0], b[1] - a[1]];
        let line = Line::new(self.local(a), self.local(b), self.bvh.pad());
        let mut seen: Vec<u32> = Vec::new();
        self.bvh.search(
            |n| n.crosses(&line),
            |leaf| {
                let near = |t: &&u32| line.meets(self.box_of(self.block.tri[**t as usize]));
                seen.extend(leaf.iter().filter(near));
            },
        );
        seen.sort_unstable();
        for k in seen {
            let tri = self.block.tri[k as usize];
            let z = tri.map(|v| self.h(v));
            if let Some((t, h)) = clip(self.corners(tri), z, a, d) {
                out.push(Piece {
                    t,
                    h,
                    rank: rank(self.block.tritype[k as usize], self.index, k as usize),
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
    let mut out = vec![(None, false); points.len()];
    // A corner box widened by the bins' slack holds every point `cover`
    // accepts, as a bin does.
    let near = |c: &[[f64; 2]; 3], p: [f64; 2]| {
        (0..2).all(|k| {
            let (lo, hi) = span(c, k);
            p[k] >= lo - BIN_SLACK && p[k] <= hi + BIN_SLACK
        })
    };
    // Converting every vertex to grid units costs more than testing the
    // triangles, so each is first tested in the block's own meters, against
    // the points taken back there and given a meter more than the slack,
    // far past any rounding: a triangle that misses a point there misses
    // it in grid units. Only the few left are converted.
    let cs = f64::from(g.cell_size);
    let t0 = b.t.map(f64::from);
    let reach = BIN_SLACK * cs + 1.0;
    let local: Vec<[f64; 2]> = points
        .iter()
        .map(|p| {
            [
                (p[0] + 0.5) * cs + g.origin_x - t0[0],
                (p[1] + 0.5) * cs + g.origin_z - t0[2],
            ]
        })
        .collect();
    let all = local.iter().fold(
        [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ],
        |a, p| {
            [
                a[0].min(p[0]),
                a[1].min(p[1]),
                a[2].max(p[0]),
                a[3].max(p[1]),
            ]
        },
    );
    let local_box = |tri: &[u32; 3]| {
        let c = tri.map(|k| b.p[k as usize]);
        let (x0, x1) = (
            c[0][0].min(c[1][0]).min(c[2][0]),
            c[0][0].max(c[1][0]).max(c[2][0]),
        );
        let (z0, z1) = (
            c[0][2].min(c[1][2]).min(c[2][2]),
            c[0][2].max(c[1][2]).max(c[2][2]),
        );
        [
            f64::from(x0) - reach,
            f64::from(z0) - reach,
            f64::from(x1) + reach,
            f64::from(z1) + reach,
        ]
    };
    let misses =
        |m: &[f64; 4], p: [f64; 2]| p[0] < m[0] || p[1] < m[1] || p[0] > m[2] || p[1] > m[3];
    let misses_all =
        |m: &[f64; 4]| all[2] < m[0] || all[3] < m[1] || all[0] > m[2] || all[1] > m[3];
    let corner = |k: u32| {
        let v = b.world(k);
        grid_units(g, v[0], v[2])
    };
    // Which sides of the points' box each vertex lies past, a bit each: a
    // triangle whose three corners share a side misses every point, three
    // bytes read against a box of six floats.
    let near_box = all.map(|v| v as f32);
    let far = reach as f32 + 1.0;
    let sides: Vec<u8> = (b.p.iter())
        .map(|v| {
            u8::from(v[0] < near_box[0] - far)
                | u8::from(v[0] > near_box[2] + far) << 1
                | u8::from(v[2] < near_box[1] - far) << 2
                | u8::from(v[2] > near_box[3] + far) << 3
        })
        .collect();
    let away =
        |t: &[u32; 3]| sides[t[0] as usize] & sides[t[1] as usize] & sides[t[2] as usize] != 0;
    for (t, tri) in b.tri.iter().enumerate() {
        if away(tri) {
            continue;
        }
        let m = local_box(tri);
        if misses_all(&m) {
            continue;
        }
        let mut c = None;
        for ((p, l), o) in points.iter().zip(&local).zip(out.iter_mut()) {
            if misses(&m, *l) {
                continue;
            }
            let c = *c.get_or_insert_with(|| tri.map(corner));
            if !near(&c, *p) {
                continue;
            }
            let Some((u, v)) = cover(c[0], c[1], c[2], *p) else {
                continue;
            };
            let z = tri.map(|k| b.world(k)[1]);
            let g = Ground {
                h: lerp(z, u, v),
                depth: lerp(tri.map(|k| f64::from(b.depth_at(k))), u, v),
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
        if away(tri) {
            continue;
        }
        let m = local_box(tri);
        if misses_all(&m) {
            continue;
        }
        let mut c = None;
        for ((p, l), o) in points.iter().zip(&local).zip(out.iter_mut()) {
            if o.1 || misses(&m, *l) {
                continue;
            }
            let c = *c.get_or_insert_with(|| tri.map(corner));
            if near(&c, *p) && cover(c[0], c[1], c[2], *p).is_some() {
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

/// A cheap test ahead of `cover`, with no division: most of a triangle's box
/// lies outside it, and its cells need no weights.
struct Near {
    a: [f64; 2],
    v0: [f64; 2],
    v1: [f64; 2],
    /// The cross product, made positive by flipping the weights with `sign`.
    d: f64,
    sign: f64,
}

impl Near {
    fn new(p: [[f64; 2]; 3]) -> Near {
        let v0 = [p[1][0] - p[0][0], p[1][1] - p[0][1]];
        let v1 = [p[2][0] - p[0][0], p[2][1] - p[0][1]];
        let d = v0[0] * v1[1] - v1[0] * v0[1];
        Near {
            a: p[0],
            v0,
            v1,
            d: d.abs(),
            sign: if d < 0.0 { -1.0 } else { 1.0 },
        }
    }

    /// Whether `cover` surely refuses `p`: a weight past its edge by more
    /// than twice the tolerance, with the rounding of its products allowed
    /// for. A triangle of no area keeps its cells for `cover`.
    #[inline]
    fn off(&self, p: [f64; 2]) -> bool {
        let (px, pz) = (p[0] - self.a[0], p[1] - self.a[1]);
        let (a, b) = (px * self.v1[1], self.v1[0] * pz);
        let (c, e) = (self.v0[0] * pz, px * self.v0[1]);
        let (u, v) = ((a - b) * self.sign, (c - e) * self.sign);
        let slack = 2.0 * EDGE_TOLERANCE * self.d + 1e-14 * (a.abs() + b.abs() + c.abs() + e.abs());
        u < -slack || v < -slack || u + v > self.d + slack
    }
}

/// Rasterizes tile `(tx, tz)` of `g` from the blocks under it, each cell
/// the winning triangle at its center.
pub fn tile(blocks: &[&Prepared], g: &Grid, tx: u32, tz: u32) -> Tile {
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
    let around = [
        rows.0 as f64,
        cols.0 as f64,
        (rows.1 - 1) as f64,
        (cols.1 - 1) as f64,
    ];

    for b in blocks {
        let q = b.around_box(around, &b.bvh);
        b.each_near(&b.bvh, b.block.tri.len(), q, |leaf| {
            for &t in leaf {
                let tri = b.block.tri[t as usize];
                let e = b.box_of(tri);
                if !meet(e, q) || b.between_cells(e) {
                    continue;
                }
                let p = b.corners(tri);
                let Some((rr, cc)) = cells(p, rows, cols) else {
                    continue;
                };
                let k = rank(b.block.tritype[t as usize], b.index, t as usize);
                let near = Near::new(p);
                for r in rr {
                    for c in cc.clone() {
                        let i = at(r, c);
                        let cell = [r as f64, c as f64];
                        if (best[i] != NONE && k <= best[i]) || near.off(cell) {
                            continue;
                        }
                        let Some((u, v)) = cover(p[0], p[1], p[2], cell) else {
                            continue;
                        };
                        best[i] = k;
                        h[i] = b.height(tri, u, v);
                        ty[i] = b.block.tritype[t as usize];
                    }
                }
            }
        });
        let q = b.around_box(around, &b.runway_bvh);
        b.each_near(&b.runway_bvh, b.runway.len(), q, |leaf| {
            for &t in leaf {
                if !meet(b.box_of(b.runway[t as usize]), q) {
                    continue;
                }
                let p = b.corners(b.runway[t as usize]);
                let Some((rr, cc)) = cells(p, rows, cols) else {
                    continue;
                };
                let near = Near::new(p);
                for r in rr {
                    for c in cc.clone() {
                        let cell = [r as f64, c as f64];
                        if !near.off(cell) && cover(p[0], p[1], p[2], cell).is_some() {
                            runway[at(r, c)] = true;
                        }
                    }
                }
            }
        });
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
        assert_eq!((p.tri, p.runway, p.depth), (0, false, 0.0));
        // In the lake, 0.8 of the way to the corner 50 m deep.
        let w = at(&b, &g, 900.0, 900.0).unwrap();
        assert_eq!(w.tri, 1);
        assert!((w.depth - 40.0).abs() < 1e-9, "{}", w.depth);
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
        for v in &b.vertices() {
            assert_eq!(b.on_runway(*v), scan(*v), "{v:?}");
        }
        // A box takes every triangle whose box meets it, as a scan of their
        // boxes finds, boxes touching included.
        let boxed = |q: [f64; 4]| -> Vec<[[f64; 2]; 3]> {
            let mut all: Vec<_> = (b.runway.iter())
                .map(|&t| b.corners(t))
                .filter(|p| {
                    let ((r0, r1), (c0, c1)) = (span(p, 0), span(p, 1));
                    r0 <= q[2] && r1 >= q[0] && c0 <= q[3] && c1 >= q[1]
                })
                .collect();
            all.sort_by(|a, b| a.partial_cmp(b).unwrap());
            all
        };
        for k in 0..2000 {
            let (r, c) = (f64::from(k % 40) * 10.3, f64::from(k / 40) * 8.1);
            let w = f64::from(k % 7) * 9.0;
            let q = [r, c, r + w, c + w * 0.5];
            let mut got = Vec::new();
            b.runway_within(q, &mut got);
            got.sort_by(|a, b| a.partial_cmp(b).unwrap());
            assert_eq!(got, boxed(q), "{q:?}");
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

    impl Prepared {
        /// Every vertex in grid units.
        fn vertices(&self) -> Vec<[f64; 2]> {
            (0..self.block.p.len() as u32).map(|k| self.rc(k)).collect()
        }
    }

    /// A 40 km block of every size of triangle: a fine patch of 70 m
    /// triangles, a coarse one of 900 m, two triangles over the whole
    /// block, a sliver, and a runway subset of fine and large triangles.
    fn mixed() -> build::Block {
        let s = 40000.0f32;
        let mut p: Vec<[f32; 3]> = vec![
            [0.0, -20.0, 0.0],
            [s, -20.0, 0.0],
            [0.0, -20.0, s],
            [s, -20.0, s],
        ];
        let mut tris: Vec<[u32; 3]> = vec![[0, 1, 2], [1, 3, 2]];
        let mut tritype: Vec<u32> = vec![0, 0];
        let mut lattice = |p: &mut Vec<[f32; 3]>, at: [f32; 2], step: f32, n: u32, ty: u32| {
            let base = p.len() as u32;
            for i in 0..n {
                for j in 0..n {
                    // A deterministic wobble, so edges run every way.
                    let w = ((i * 7 + j * 13) % 11) as f32 / 11.0 - 0.5;
                    let x = at[0] + i as f32 * step + w * step * 0.3;
                    let z = at[1] + j as f32 * step - w * step * 0.2;
                    p.push([x, 100.0 + (i + 2 * j) as f32, z]);
                }
            }
            for i in 0..n - 1 {
                for j in 0..n - 1 {
                    let k = base + i * n + j;
                    tris.push([k, k + n, k + 1]);
                    tris.push([k + 1, k + n, k + n + 1]);
                    tritype.push(ty);
                    tritype.push(if (i + j) % 5 == 0 { ROAD_STRIP } else { ty });
                }
            }
        };
        lattice(&mut p, [5000.0, 5000.0], 70.0, 30, 1024);
        lattice(&mut p, [12000.0, 3000.0], 900.0, 9, LAKE);
        let k = p.len() as u32;
        p.extend([
            [20000.0, 50.0, 20000.0],
            [30000.0, 50.0, 20001.0],
            [25000.0, 50.0, 20000.5],
        ]);
        tris.push([k, k + 1, k + 2]);
        tritype.push(0);
        // The runway: three fine triangles of the patch and a large one of
        // its own vertices, through a base index.
        let r = p.len() as u32;
        p.extend([
            [6000.0, 120.0, 6000.0],
            [9000.0, 120.0, 6200.0],
            [6100.0, 120.0, 9000.0],
        ]);
        let base = [0, 1, 30, 31, 32, r, r + 1, r + 2];
        let depth: Vec<f32> = (0..p.len())
            .map(|i| if i < 4 { 30.0 } else { 0.0 })
            .collect();
        build::Block {
            t: [100.0, 0.0, -50.0],
            bbox: [0.0, -20.0, 0.0, s, 300.0, s],
            arrays: vec![
                u32s("TRITYPE", &tritype),
                tri(&tris, Some(("Surface5.0", "Land"))),
                u32s("baseIndex", &base.map(|v| if v < 33 { v + 4 } else { v })),
                build::tri(
                    &[[0, 1, 2], [2, 1, 3], [1, 4, 3], [5, 6, 7]],
                    Some(("Runway5.0", "Runway")),
                ),
                vec3("Pbase", &p),
                f32s("depth", &depth),
            ],
        }
    }

    #[test]
    fn the_hierarchy_answers_as_a_search_of_every_triangle_does() {
        let g = grid(810);
        let block = &prepared(&[mixed()], &g)[0];
        // Points everywhere, densest over the fine patch, and every vertex.
        let mut pts: Vec<[f64; 2]> = block.vertices();
        for k in 0..40000 {
            let (a, b) = (f64::from(k % 200), f64::from(k / 200));
            pts.push([a * 4.07 - 3.0, b * 4.11 - 3.0]);
            pts.push([100.0 + a * 0.23, 100.0 + b * 0.21]);
        }
        // What any binning must answer, from every triangle and no bin: the
        // covering triangle of highest rank, and any runway triangle at all.
        let brute = |p: [f64; 2]| {
            let mut best: Option<Ground> = None;
            for (t, &tri) in block.block.tri.iter().enumerate() {
                let [a, b, c] = block.corners(tri);
                let Some((u, v)) = cover(a, b, c, p) else {
                    continue;
                };
                let g = Ground {
                    h: block.height(tri, u, v),
                    depth: block.depth(tri, u, v),
                    tritype: block.block.tritype[t],
                    runway: false,
                    block: block.index,
                    tri: t,
                };
                if best.is_none_or(|cur| g.rank() > cur.rank()) {
                    best = Some(g);
                }
            }
            let runway = block.runway.iter().any(|&tri| {
                let [a, b, c] = block.corners(tri);
                cover(a, b, c, p).is_some()
            });
            (best, runway)
        };
        let mut found = 0;
        for &p in &pts {
            let got = block.point(p);
            assert_eq!((got, block.on_runway(p)), brute(p), "{p:?}");
            found += usize::from(got.is_some_and(|g| g.tri >= 2));
        }
        assert!(
            found > 10000,
            "{found} points off the two largest triangles"
        );
        // Segments of every length and direction, some along a grid line.
        let mut a = Vec::new();
        for k in 0..400 {
            let (s, e) = (pts[k * 97 % pts.len()], pts[(k * 389 + 1000) % pts.len()]);
            let e = if k % 9 == 0 { [s[0], e[1]] } else { e };
            a.clear();
            block.along(s, e, &mut a);
            // Every triangle clipped, in triangle order, as no bin decides.
            let d = [e[0] - s[0], e[1] - s[1]];
            let every: Vec<Piece> = (block.block.tri.iter().enumerate())
                .filter_map(|(t, &tri)| {
                    let z = tri.map(|v| block.h(v));
                    clip(block.corners(tri), z, s, d).map(|(t2, h)| Piece {
                        t: t2,
                        h,
                        rank: rank(block.block.tritype[t], block.index, t),
                    })
                })
                .collect();
            assert_eq!(a, every, "{s:?} to {e:?}");
        }
    }

    #[test]
    fn a_tile_from_a_scan_is_the_tile_from_the_hierarchy() {
        let g = grid(810);
        let f = file(&[mixed()], 1);
        let s = Surface5::open(&f[..], "t").unwrap();
        let read = || s.read_block(&f[..], 0, &[RUNWAY_MATERIAL]).unwrap();
        let (held, bare) = (Prepared::new(read(), &g), Prepared::bare(read(), &g));
        let (ntx, ntz) = g.tiles();
        for tx in 0..ntx {
            for tz in 0..ntz {
                let t = tile(&[&held], &g, tx, tz);
                assert_eq!(t, tile(&[&bare], &g, tx, tz), "({tx}, {tz})");
            }
        }
    }

    #[test]
    fn a_few_points_answer_as_the_prepared_block_does() {
        let g = grid(810);
        let f = file(&[mixed()], 1);
        let raw = Surface5::open(&f[..], "t")
            .unwrap()
            .read_block(&f[..], 0, &[RUNWAY_MATERIAL])
            .unwrap();
        let b = Prepared::new(raw.clone(), &g);
        // Every vertex, points over the fine patch and the runway, and
        // points across the block, a hair either side of grid lines too.
        let mut pts: Vec<[f64; 2]> = b.vertices();
        for k in 0..6000 {
            let (a, c) = (f64::from(k % 80), f64::from(k / 80));
            pts.push([100.0 + a * 0.6, 95.0 + c * 0.7]);
            pts.push([a * 10.3 - 2.0, c * 10.9 - 2.0]);
            pts.push([a.round() + 1e-12, c.round() - 1e-12]);
        }
        let (mut ground, mut runway) = (0, 0);
        for n in [1, 8, 32] {
            for chunk in pts.chunks(n) {
                for (p, (g, r)) in chunk.iter().zip(few(&raw, &g, chunk)) {
                    assert_eq!(g, b.point(*p), "{p:?}");
                    assert_eq!(r, b.on_runway(*p), "{p:?}");
                    ground += usize::from(g.is_some());
                    runway += usize::from(r);
                }
            }
        }
        assert!(
            ground > 10000 && runway > 1000,
            "{ground} on ground, {runway} on a runway"
        );
    }

    #[test]
    fn floor_matches_the_runtime_s() {
        for x in [
            0.0,
            -0.0,
            0.5,
            -0.5,
            1.0,
            -1.0,
            -1.0000001,
            7.999999999,
            -8.0,
            1e17,
            -1e17,
            1e300,
            -1e300,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NAN,
            f64::MIN_POSITIVE,
            -f64::MIN_POSITIVE,
        ] {
            assert_eq!(floor_i64(x), x.floor() as i64, "{x}");
        }
    }
}
