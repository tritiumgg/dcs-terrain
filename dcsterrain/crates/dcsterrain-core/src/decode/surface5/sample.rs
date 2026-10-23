//! The ground at a point: which finest-mesh triangle covers it, and the
//! height there.
//!
//! Triangles overlap where a strip is laid over the ground, and the game
//! reports the strip. Through the Roki tunnel a road runs under a mountain
//! triangle, and taking the mountain puts the road 729 m too high; where a
//! lake is laid over land, the game reports the lake. So a road strip wins,
//! then water, then the later block, then the later triangle (ADR 0041).

use super::mesh::{Block, Mesh};

/// Triangle type bits.
pub const SEA: u32 = 1;
pub const LAKE: u32 = 2;
pub const RIVER: u32 = 4;
pub const ROAD_STRIP: u32 = 8;
const WATER: u32 = SEA | LAKE | RIVER;

/// How far outside a triangle, in barycentric terms, a point may lie and
/// still be covered, so a point on a shared edge is covered by both.
const EDGE_TOLERANCE: f64 = 1e-7;

/// The barycentric weights of `p` towards `b` and `c` in the triangle
/// `a, b, c`, or `None` when the triangle does not cover it or has no area.
/// Coordinates are any frame's x and z.
pub fn cover(a: [f64; 2], b: [f64; 2], c: [f64; 2], p: [f64; 2]) -> Option<(f64, f64)> {
    let (v0x, v0z) = (b[0] - a[0], b[1] - a[1]);
    let (v1x, v1z) = (c[0] - a[0], c[1] - a[1]);
    let (px, pz) = (p[0] - a[0], p[1] - a[1]);
    let d = v0x * v1z - v1x * v0z;
    if d == 0.0 {
        return None;
    }
    let u = (px * v1z - v1x * pz) / d;
    let v = (v0x * pz - px * v0z) / d;
    let e = EDGE_TOLERANCE;
    (u >= -e && v >= -e && u + v <= 1.0 + e).then_some((u, v))
}

/// The `water` layer's class for a triangle type: 0 land, 1 lake, 2 sea,
/// 3 river. A river bit wins over a lake or sea bit beside it (ADR 0041).
pub fn water_class(tritype: u32) -> u8 {
    if tritype & RIVER != 0 {
        3
    } else if tritype & SEA != 0 {
        2
    } else if tritype & LAKE != 0 {
        1
    } else {
        0
    }
}

/// Where two triangles cover a point, the one of higher rank is reported.
pub fn rank(tritype: u32, block: usize, tri: usize) -> u64 {
    let road = u64::from(tritype & ROAD_STRIP != 0);
    let water = u64::from(tritype & WATER != 0);
    (road << 63) | (water << 62) | ((block as u64) << 32) | tri as u64
}

/// The `height` layer's metres: rounded at the game's own precision, f32,
/// which matches it on more cells than rounding the f64 (ADR 0041).
pub fn round_height(h: f64) -> i16 {
    // Clamped short of nodata, which no ground reaches.
    ((h as f32) + 0.5).floor().clamp(-32767.0, 32767.0) as i16
}

/// A triangle covering a point, and the height it gives there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit {
    /// The water surface over water, the ground elsewhere.
    pub h: f64,
    pub tritype: u32,
    pub block: usize,
    pub tri: usize,
}

impl Hit {
    /// Whether this triangle is the one reported where both cover a point.
    pub fn beats(&self, other: &Hit) -> bool {
        rank(self.tritype, self.block, self.tri) > rank(other.tritype, other.block, other.tri)
    }
}

/// The heights of a triangle's corners, and the corners in the x–z plane.
pub fn corners(b: &Block, t: usize) -> ([f64; 3], [[f64; 2]; 3]) {
    let w = b.tri[t].map(|i| b.world(i));
    (w.map(|v| v[1]), w.map(|v| [v[0], v[2]]))
}

impl Mesh {
    /// The winning triangle at world `(x, z)`, or `None` outside the mesh.
    pub fn sample(&self, x: f64, z: f64) -> Option<Hit> {
        let mut best: Option<Hit> = None;
        for b in &self.blocks {
            let [x0, z0, x1, z1] = b.world_box();
            if x < x0 || x > x1 || z < z0 || z > z1 {
                continue;
            }
            for t in 0..b.tri.len() {
                let (h, [pa, pb, pc]) = corners(b, t);
                let Some((u, v)) = cover(pa, pb, pc, [x, z]) else {
                    continue;
                };
                let hit = Hit {
                    h: h[0] + u * (h[1] - h[0]) + v * (h[2] - h[0]),
                    tritype: b.tritype[t],
                    block: b.index,
                    tri: t,
                };
                if best.is_none_or(|cur| hit.beats(&cur)) {
                    best = Some(hit);
                }
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 100 m square at (1000, 2000) in two triangles: the south-west half
    /// land, 15 m at its west corners and 25 m at the south-east one; the
    /// north-east half a lake, 10 m deep over a 5 m bed at its north-east corner.
    fn square(index: usize, tritype: [u32; 2]) -> Block {
        Block {
            index,
            t: [1000.0, 5.0, 2000.0],
            bbox: [0.0, 0.0, 0.0, 100.0, 30.0, 100.0],
            p: vec![
                [0.0, 10.0, 0.0],
                [100.0, 10.0, 0.0],
                [0.0, 20.0, 100.0],
                [100.0, 0.0, 100.0],
            ],
            depth: vec![0.0, 0.0, 0.0, 10.0],
            tri: vec![[0, 1, 2], [1, 3, 2]],
            tritype: tritype.to_vec(),
            subsets: Vec::new(),
        }
    }

    #[test]
    fn interpolates_within_a_triangle() {
        let mesh = Mesh {
            blocks: vec![square(0, [0, LAKE])],
        };
        let hit = mesh.sample(1010.0, 2050.0).unwrap();
        assert_eq!((hit.tri, hit.tritype), (0, 0));
        assert!((hit.h - 20.0).abs() < 1e-9, "{}", hit.h);
        let lake = mesh.sample(1090.0, 2090.0).unwrap();
        assert_eq!(water_class(lake.tritype), 1);
        assert!((lake.h - 16.0).abs() < 1e-9, "{}", lake.h);
    }

    #[test]
    fn is_none_outside_the_mesh() {
        let mesh = Mesh {
            blocks: vec![square(0, [0, 0])],
        };
        assert!(mesh.sample(999.0, 2050.0).is_none());
        assert!(mesh.sample(1050.0, 2100.5).is_none());
    }

    #[test]
    fn a_shared_edge_is_covered_and_the_later_triangle_wins() {
        let mesh = Mesh {
            blocks: vec![square(0, [0, 0])],
        };
        assert_eq!(mesh.sample(1050.0, 2050.0).unwrap().tri, 1);
    }

    #[test]
    fn a_road_strip_wins_over_a_later_block() {
        let mesh = Mesh {
            blocks: vec![square(0, [ROAD_STRIP, 0]), square(1, [0, 0])],
        };
        let hit = mesh.sample(1010.0, 2010.0).unwrap();
        assert_eq!((hit.block, hit.tritype), (0, ROAD_STRIP));
        // Without a strip the later block wins.
        let hit = mesh.sample(1090.0, 2090.0).unwrap();
        assert_eq!(hit.block, 1);
    }

    #[test]
    fn a_degenerate_triangle_covers_nothing() {
        assert_eq!(cover([0.0, 0.0], [1.0, 1.0], [2.0, 2.0], [1.0, 1.0]), None);
    }

    #[test]
    fn water_bits_rank_river_then_sea_then_lake() {
        assert_eq!(water_class(SEA | LAKE | RIVER), 3);
        assert_eq!(water_class(LAKE | RIVER), 3);
        assert_eq!(water_class(SEA | LAKE), 2);
        assert_eq!(water_class(LAKE | 64), 1);
        assert_eq!(water_class(ROAD_STRIP | 64), 0);
    }

    #[test]
    fn water_wins_over_a_later_land_triangle_and_a_road_over_both() {
        let mesh = Mesh {
            blocks: vec![square(0, [LAKE, LAKE]), square(1, [0, 0])],
        };
        assert_eq!(mesh.sample(1010.0, 2010.0).unwrap().block, 0);
        let mesh = Mesh {
            blocks: vec![square(0, [LAKE, LAKE]), square(1, [ROAD_STRIP, 0])],
        };
        assert_eq!(mesh.sample(1010.0, 2010.0).unwrap().block, 1);
    }

    #[test]
    fn heights_round_at_f32() {
        // 2.4999999999 is 2.5 in f32, so it rounds up as the game rounds it.
        assert_eq!(round_height(2.499_999_999_9), 3);
        assert_eq!(round_height(-0.5), 0);
        assert_eq!(round_height(1e9), 32767);
    }
}
