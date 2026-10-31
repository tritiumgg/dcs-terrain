//! A bounding volume hierarchy over one block's triangles, in the block's
//! own meters.
//!
//! A block runs from a handful of triangles to 800 thousand, some a hundred
//! kilometers across and most a few meters, so no one grid of bins suits it.
//! Triangles are sorted along a Morton curve through their centers, which a
//! radix sort does in linear time, cut into leaves of `LEAF`, and boxes built
//! from the leaves up, four children to a node: a node is a cache line, its
//! four boxes tested together, and the whole costs about 7 bytes a triangle
//! where the file's own arrays cost 16, and a vertex 16 more.
//!
//! A search takes a test of a node's four boxes and is handed each leaf whose
//! box passes. It may hand over triangles a question excludes, and never
//! leaves one out: boxes are exact in the file's f32, and a question widens
//! itself by `Bvh::pad`.

/// Triangles to a leaf.
const LEAF: usize = 8;
/// Children to a node.
const WIDE: usize = 4;
/// Bits of a center along each axis in its sort key.
const KEY_BITS: u32 = 12;

/// `[min_x, min_z, max_x, max_z]` holding nothing, which any box widens.
const NO_BOX: [f32; 4] = [
    f32::INFINITY,
    f32::INFINITY,
    f32::NEG_INFINITY,
    f32::NEG_INFINITY,
];

fn grow(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[0].min(b[0]),
        a[1].min(b[1]),
        a[2].max(b[2]),
        a[3].max(b[3]),
    ]
}

/// The box of a triangle's corners, `[x, y, z]` each.
#[inline]
pub fn tri_box(c: [[f32; 3]; 3]) -> [f32; 4] {
    let lo = |k: usize| c[0][k].min(c[1][k]).min(c[2][k]);
    let hi = |k: usize| c[0][k].max(c[1][k]).max(c[2][k]);
    [lo(0), lo(2), hi(0), hi(2)]
}

/// Four boxes, by side. An unused one is inverted, so every test fails it.
#[derive(Debug, Clone, Copy)]
#[repr(C, align(64))]
pub struct Node {
    min_x: [f32; WIDE],
    min_z: [f32; WIDE],
    max_x: [f32; WIDE],
    max_z: [f32; WIDE],
}

const EMPTY: Node = Node {
    min_x: [NO_BOX[0]; WIDE],
    min_z: [NO_BOX[1]; WIDE],
    max_x: [NO_BOX[2]; WIDE],
    max_z: [NO_BOX[3]; WIDE],
};

impl Node {
    fn set(&mut self, k: usize, b: [f32; 4]) {
        (self.min_x[k], self.min_z[k]) = (b[0], b[1]);
        (self.max_x[k], self.max_z[k]) = (b[2], b[3]);
    }

    fn union(&self) -> [f32; 4] {
        (0..WIDE).fold(NO_BOX, |a, k| {
            grow(
                a,
                [self.min_x[k], self.min_z[k], self.max_x[k], self.max_z[k]],
            )
        })
    }

    /// Which of the boxes meet `[min_x, min_z, max_x, max_z]`, a bit each.
    #[inline]
    pub fn meets(&self, b: [f32; 4]) -> u32 {
        let mut m = 0;
        for k in 0..WIDE {
            let hit = (self.min_x[k] <= b[2])
                & (self.max_x[k] >= b[0])
                & (self.min_z[k] <= b[3])
                & (self.max_z[k] >= b[1]);
            m |= u32::from(hit) << k;
        }
        m
    }

    /// Which of the boxes meet `line`.
    #[inline]
    pub fn crosses(&self, line: &Line) -> u32 {
        let mut m = 0;
        for k in 0..WIDE {
            let (x0, x1) = (self.min_x[k], self.max_x[k]);
            let (z0, z1) = (self.min_z[k], self.max_z[k]);
            // A box misses a segment only if an axis of the box or the
            // segment's normal parts them.
            let (cx, cz) = ((x0 + x1) * 0.5 - line.a[0], (z0 + z1) * 0.5 - line.a[1]);
            let (hx, hz) = ((x1 - x0) * 0.5, (z1 - z0) * 0.5);
            let off = (line.n[0] * cx + line.n[1] * cz).abs();
            let reach = line.n[0].abs() * hx + line.n[1].abs() * hz + line.pad;
            let hit = (x0 <= line.hull[2])
                & (x1 >= line.hull[0])
                & (z0 <= line.hull[3])
                & (z1 >= line.hull[1])
                & (off <= reach);
            m |= u32::from(hit) << k;
        }
        m
    }
}

/// A segment as a search asks about it, in the block's meters.
#[derive(Debug, Clone, Copy)]
pub struct Line {
    a: [f32; 2],
    /// A unit normal.
    n: [f32; 2],
    /// Its box, widened by the pad.
    hull: [f32; 4],
    pad: f32,
}

impl Line {
    pub fn new(a: [f32; 2], b: [f32; 2], pad: f32) -> Line {
        let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
        let len = (dx * dx + dz * dz).sqrt();
        let n = if len > 0.0 {
            [-dz / len, dx / len]
        } else {
            [0.0, 0.0]
        };
        let hull = [
            a[0].min(b[0]) - pad,
            a[1].min(b[1]) - pad,
            a[0].max(b[0]) + pad,
            a[1].max(b[1]) + pad,
        ];
        Line { a, n, hull, pad }
    }

    /// Whether the box `[min_x, min_z, max_x, max_z]` meets this segment.
    #[inline]
    pub fn meets(&self, b: [f32; 4]) -> bool {
        let mut n = EMPTY;
        n.set(0, b);
        n.crosses(self) != 0
    }
}

/// Spreads the low 12 bits of `v` to the even bits.
#[inline]
fn spread(v: u32) -> u32 {
    let v = (v | (v << 8)) & 0x00ff_00ff;
    let v = (v | (v << 4)) & 0x0f0f_0f0f;
    let v = (v | (v << 2)) & 0x3333_3333;
    (v | (v << 1)) & 0x5555_5555
}

/// Sorts `v` by its high word, a key of 24 bits, in two passes of twelve,
/// `tmp` being as long.
fn radix(v: &mut Vec<u64>, tmp: &mut Vec<u64>) {
    let mut counts = [[0u32; 1 << KEY_BITS]; 2];
    let digit = |e: u64, pass: usize| (e >> (32 + KEY_BITS as usize * pass)) as usize & 4095;
    for &e in v.iter() {
        for (pass, c) in counts.iter_mut().enumerate() {
            c[digit(e, pass)] += 1;
        }
    }
    for (pass, c) in counts.iter_mut().enumerate() {
        let mut at = 0;
        for n in c.iter_mut() {
            let here = *n;
            *n = at;
            at += here;
        }
        for &e in v.iter() {
            let n = &mut c[digit(e, pass)];
            tmp[*n as usize] = e;
            *n += 1;
        }
        std::mem::swap(v, tmp);
    }
}

/// See the module's head.
#[derive(Debug, Clone, Default)]
pub struct Bvh {
    /// Triangles in leaf order: leaf k's are the k-th `LEAF` of them.
    order: Vec<u32>,
    /// Root first, then each level whole, a node's children `WIDE` to a node
    /// in the level below.
    nodes: Vec<Node>,
    /// Where each level starts in `nodes`, and past the last.
    levels: Vec<u32>,
    pad: f32,
}

impl Bvh {
    /// The hierarchy over the triangles `tri` of vertices `p`, `[x, y, z]`,
    /// which lie in `all`, `[min_x, min_z, max_x, max_z]`, to within a key's
    /// width: a key only orders triangles.
    pub fn new(p: &[[f32; 3]], tri: &[[u32; 3]], all: [f32; 4]) -> Bvh {
        if tri.is_empty() {
            return Bvh::default();
        }
        let boxed = |t: &[u32; 3]| tri_box(t.map(|v| p[v as usize]));
        let side = (all[2] - all[0]).max(all[3] - all[1]);
        // By a triangle's first corner: one vertex to read, and triangles
        // near one another have corners near one another.
        let top = (1u32 << KEY_BITS) - 1;
        let scale = top as f32 / side.max(f32::MIN_POSITIVE);
        let key = |t: &[u32; 3]| {
            let v = p[t[0] as usize];
            let q = |at: f32, lo: f32| (((at - lo) * scale) as u32).min(top);
            spread(q(v[0], all[0])) | (spread(q(v[2], all[1])) << 1)
        };
        let mut sorted: Vec<u64> = tri
            .iter()
            .enumerate()
            .map(|(i, t)| (u64::from(key(t)) << 32) | i as u64)
            .collect();
        let mut tmp = vec![0u64; sorted.len()];
        radix(&mut sorted, &mut tmp);
        drop(tmp);
        let order: Vec<u32> = sorted.iter().map(|&e| e as u32).collect();
        drop(sorted);

        // From the leaves up, a level of nodes at a time.
        let mut level: Vec<Node> = Vec::with_capacity(order.len() / (LEAF * WIDE) + 1);
        for (k, leaf) in order.chunks(LEAF).enumerate() {
            if k % WIDE == 0 {
                level.push(EMPTY);
            }
            let b = (leaf.iter()).fold(NO_BOX, |a, &t| grow(a, boxed(&tri[t as usize])));
            level.last_mut().unwrap().set(k % WIDE, b);
        }
        let mut stack = vec![level];
        while stack.last().unwrap().len() > 1 {
            let below = stack.last().unwrap();
            let mut up = vec![EMPTY; below.len().div_ceil(WIDE)];
            for (k, n) in below.iter().enumerate() {
                up[k / WIDE].set(k % WIDE, n.union());
            }
            stack.push(up);
        }
        let mut nodes = Vec::with_capacity(stack.iter().map(Vec::len).sum());
        let mut levels = vec![0];
        for l in stack.iter().rev() {
            nodes.extend_from_slice(l);
            levels.push(nodes.len() as u32);
        }
        // Five centimeters, more than a point on an edge lies past a
        // triangle, and the rounding of a question to f32 anywhere near.
        let reach = side.max(all[0].abs()).max(all[1].abs()).max(all[2].abs());
        let pad = 0.1 + 8.0 * reach.max(all[3].abs()) * f32::EPSILON;
        Bvh {
            order,
            nodes,
            levels,
            pad,
        }
    }

    /// No triangles, for a block that is scanned, and a question widened as
    /// one is widened for a hierarchy over the largest block.
    pub fn with_pad() -> Bvh {
        Bvh {
            pad: 1.0,
            ..Bvh::default()
        }
    }

    /// How far a question must widen itself, in meters.
    pub fn pad(&self) -> f32 {
        self.pad
    }

    pub fn bytes(&self) -> usize {
        self.order.len() * 4 + self.nodes.len() * size_of::<Node>() + self.levels.len() * 4
    }

    /// Calls `leaf` with the triangles of each leaf whose box `test` passes.
    #[inline]
    pub fn search(&self, test: impl Fn(&Node) -> u32, mut leaf: impl FnMut(&[u32])) {
        if self.nodes.is_empty() {
            return;
        }
        let last = self.levels.len() - 2;
        // A level adds at most `WIDE - 1` to the stack, and 32 bits of
        // triangles make under 16 levels.
        let mut stack = [(0u32, 0u32); 64];
        let mut top = 1;
        while top > 0 {
            top -= 1;
            let (depth, i) = stack[top];
            let at = self.levels[depth as usize] + i;
            let mut m = test(&self.nodes[at as usize]);
            while m != 0 {
                let k = m.trailing_zeros();
                m &= m - 1;
                let child = i * WIDE as u32 + k;
                if depth as usize == last {
                    let s = child as usize * LEAF;
                    leaf(&self.order[s..(s + LEAF).min(self.order.len())]);
                } else {
                    stack[top] = (depth + 1, child);
                    top += 1;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl Bvh {
        fn over(p: &[[f32; 3]], tri: &[[u32; 3]]) -> Bvh {
            let box_of = |t: &[u32; 3]| tri_box(t.map(|v| p[v as usize]));
            Bvh::new(p, tri, tri.iter().fold(NO_BOX, |a, t| grow(a, box_of(t))))
        }
    }

    /// A lattice of `n` by `n` unit squares in two triangles each, and two
    /// triangles over all of it.
    fn lattice(n: u32) -> (Vec<[f32; 3]>, Vec<[u32; 3]>) {
        let mut p = Vec::new();
        for i in 0..=n {
            for j in 0..=n {
                p.push([i as f32, 0.0, j as f32]);
            }
        }
        let at = |i: u32, j: u32| i * (n + 1) + j;
        let mut t = Vec::new();
        for i in 0..n {
            for j in 0..n {
                t.push([at(i, j), at(i + 1, j), at(i, j + 1)]);
                t.push([at(i + 1, j), at(i + 1, j + 1), at(i, j + 1)]);
            }
        }
        t.push([at(0, 0), at(n, 0), at(0, n)]);
        t.push([at(n, 0), at(n, n), at(0, n)]);
        (p, t)
    }

    fn box_of(p: &[[f32; 3]], t: [u32; 3]) -> [f32; 4] {
        tri_box(t.map(|v| p[v as usize]))
    }

    #[test]
    fn a_search_finds_every_triangle_whose_box_meets_the_question() {
        for n in [1, 2, 3, 9, 40] {
            let (p, t) = lattice(n);
            let b = Bvh::over(&p, &t);
            let mut seen = vec![0u32; t.len()];
            let all = [-1e9, -1e9, 1e9, 1e9];
            b.search(
                |n| n.meets(all),
                |l| l.iter().for_each(|&i| seen[i as usize] += 1),
            );
            assert!(seen.iter().all(|&s| s == 1), "{n}: each leaf once");
            for k in 0..500 {
                let (x, z) = ((k * 7 % 41) as f32 * 0.9, (k * 13 % 41) as f32 * 0.9);
                let q = [x, z, x + (k % 5) as f32, z + (k % 3) as f32];
                let mut got = vec![false; t.len()];
                let mark = |l: &[u32]| l.iter().for_each(|&i| got[i as usize] = true);
                b.search(|n| n.meets(q), mark);
                for (i, &tri) in t.iter().enumerate() {
                    let e = box_of(&p, tri);
                    let meets = e[0] <= q[2] && e[2] >= q[0] && e[1] <= q[3] && e[3] >= q[1];
                    assert!(got[i] || !meets, "{n} {q:?} {i}");
                }
            }
        }
    }

    #[test]
    fn a_search_along_a_line_finds_every_box_it_crosses() {
        let (p, t) = lattice(40);
        let b = Bvh::over(&p, &t);
        for k in 0..300 {
            let a = [(k * 7 % 41) as f32, (k * 11 % 41) as f32];
            let e = [(k * 13 % 41) as f32, (k * 3 % 41) as f32 + 0.5];
            let line = Line::new(a, e, b.pad());
            let mut got = vec![false; t.len()];
            let mark = |l: &[u32]| l.iter().for_each(|&i| got[i as usize] = true);
            b.search(|n| n.crosses(&line), mark);
            for s in 0..=64 {
                let u = s as f32 / 64.0;
                let q = [a[0] + u * (e[0] - a[0]), a[1] + u * (e[1] - a[1])];
                for (i, &tri) in t.iter().enumerate() {
                    let e = box_of(&p, tri);
                    if e[0] <= q[0] && e[2] >= q[0] && e[1] <= q[1] && e[3] >= q[1] {
                        assert!(got[i], "{a:?}: {i} at {q:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn it_costs_under_eight_bytes_a_triangle() {
        let (p, t) = lattice(100);
        assert!(Bvh::over(&p, &t).bytes() < 8 * t.len());
    }

    #[test]
    fn no_triangles_find_nothing() {
        let mut n = 0;
        Bvh::over(&[], &[]).search(|_| 15, |_| n += 1);
        assert_eq!(n, 0);
    }
}
