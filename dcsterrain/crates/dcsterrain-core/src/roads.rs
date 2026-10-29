//! The road and rail networks as graphs, built from the stretches the
//! map-wide road file holds.
//!
//! A node is a junction: every stretch end naming one id meets at the mean of
//! those ends, and an end naming none is a node of its own. Roads and rails
//! are two networks that share no node. A node where exactly two edges of one
//! class meet and run on within 1° is contracted, its two edges joined into
//! one. Each edge keeps its polyline, class, length and whether it crosses
//! water; betweenness, computed apart because it takes minutes on the largest
//! terrains, ranks the edges each network's shortest paths lean on.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::ops::Range;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use crate::decode::rn4::{KIND_RAIL, KIND_ROAD};
use crate::source::Refusal;

/// Where along an edge its ground is sampled for water.
pub const WATER_STEP_M: f64 = 25.0;
/// How far from straight on two edges may turn at a node that is contracted.
pub const COLLINEAR_DEG: f64 = 1.0;
/// Shortest-path trees betweenness grows, at most, per network: a sample of
/// the sources on a large graph, every node on a small one.
pub const SOURCES: usize = 2000;
/// The shortest an edge is taken to be by betweenness.
const MIN_LENGTH_M: f64 = 1e-3;

/// One stretch of road or rail as the road file holds it, between the
/// junctions its ends name.
#[derive(Debug, Clone, PartialEq)]
pub struct Stretch {
    pub kind: u8,
    pub class: u8,
    pub a: Option<u32>,
    pub b: Option<u32>,
    /// `[x, z]`, from the end at `a` to the end at `b`. It stops short of
    /// each junction.
    pub points: Vec<[f64; 2]>,
}

/// One edge of the graph, between two nodes.
#[derive(Debug, Clone, PartialEq)]
pub struct Edge {
    pub kind: u8,
    pub class: u8,
    pub a: u32,
    pub b: u32,
    /// Meters along the polyline and the gaps from its ends to its nodes, so
    /// the straight line between two nodes is never longer than a path.
    pub length: f64,
    pub crosses_water: bool,
    /// Where its polyline lies in the graph's `points`: `[x, z]`, from `a`'s
    /// end to `b`'s, through each contracted node.
    pub points: Range<u32>,
}

/// The graph. Its edges are not changed once a search has run over it, since
/// the index the search builds would no longer match them.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Graph {
    pub classes: Vec<String>,
    /// `[x, z]` of each node.
    pub nodes: Vec<[f64; 2]>,
    pub edges: Vec<Edge>,
    /// Every edge's polyline, one after another in edge order: one buffer
    /// rather than one per edge, so a graph of half a million edges decodes
    /// and frees in a few allocations.
    pub points: Vec<[f32; 2]>,
    index: Indexes,
}

/// Tells which points lie on water.
pub trait Water {
    /// Whether each of `points`, `[x, z]`, lies on water: under any water
    /// triangle, even one a bridge's road strip covers.
    fn on_water(&mut self, points: &[[f32; 2]]) -> Result<Vec<bool>, Refusal>;
}

impl<F: FnMut(f64, f64) -> bool> Water for F {
    fn on_water(&mut self, points: &[[f32; 2]]) -> Result<Vec<bool>, Refusal> {
        Ok(points
            .iter()
            .map(|p| self(f64::from(p[0]), f64::from(p[1])))
            .collect())
    }
}

fn dist(p: [f64; 2], q: [f64; 2]) -> f64 {
    (p[0] - q[0]).hypot(p[1] - q[1])
}

fn polyline_length(points: &[[f64; 2]]) -> f64 {
    points.windows(2).map(|s| dist(s[0], s[1])).sum()
}

/// An edge while the graph is built, in full precision.
struct Building {
    kind: u8,
    class: u8,
    a: usize,
    b: usize,
    length: f64,
    points: Vec<[f64; 2]>,
}

impl Building {
    fn reverse(&mut self) {
        std::mem::swap(&mut self.a, &mut self.b);
        self.points.reverse();
    }
}

/// The graph of `stretches`, whose classes index `classes`. Stretches of
/// any kind but road and rail are left out. `water` tells which samples of
/// each edge lie on water.
pub fn build(
    classes: Vec<String>,
    stretches: Vec<Stretch>,
    water: &mut dyn Water,
) -> Result<Graph, Refusal> {
    let stretches: Vec<Stretch> = stretches
        .into_iter()
        .filter(|s| (s.kind == KIND_ROAD || s.kind == KIND_RAIL) && s.points.len() >= 2)
        .collect();
    // A node per junction id, per network, at the mean of its ends.
    let mut junction: HashMap<(u8, u32), usize> = HashMap::new();
    let mut sums: Vec<[f64; 3]> = Vec::new();
    let mut ends = Vec::with_capacity(stretches.len());
    for s in &stretches {
        let mut end = |id: Option<u32>, p: [f64; 2]| {
            let i = match id {
                Some(id) => *junction.entry((s.kind, id)).or_insert_with(|| {
                    sums.push([0.0; 3]);
                    sums.len() - 1
                }),
                None => {
                    sums.push([0.0; 3]);
                    sums.len() - 1
                }
            };
            sums[i][0] += p[0];
            sums[i][1] += p[1];
            sums[i][2] += 1.0;
            i
        };
        let a = end(s.a, s.points[0]);
        let b = end(s.b, s.points[s.points.len() - 1]);
        ends.push((a, b));
    }
    let nodes: Vec<[f64; 2]> = sums.iter().map(|s| [s[0] / s[2], s[1] / s[2]]).collect();
    let mut edges: Vec<Option<Building>> = stretches
        .into_iter()
        .zip(ends)
        .map(|(s, (a, b))| {
            let length = dist(nodes[a], s.points[0])
                + polyline_length(&s.points)
                + dist(s.points[s.points.len() - 1], nodes[b]);
            Some(Building {
                kind: s.kind,
                class: s.class,
                a,
                b,
                length,
                points: s.points,
            })
        })
        .collect();
    contract(&nodes, &mut edges);

    // Renumber what is left: the edges in file order, and the nodes they
    // reach in order of first reach.
    let mut renumber = vec![u32::MAX; nodes.len()];
    let mut kept_nodes = Vec::new();
    let mut number = |n: usize| {
        if renumber[n] == u32::MAX {
            renumber[n] = kept_nodes.len() as u32;
            kept_nodes.push(nodes[n]);
        }
        renumber[n]
    };
    let built: Vec<Building> = edges.into_iter().flatten().collect();
    let ends: Vec<(u32, u32)> = built.iter().map(|e| (number(e.a), number(e.b))).collect();

    let (samples, owner) = water_samples(&built);
    let wet = water.on_water(&samples)?;
    if wet.len() != samples.len() {
        return Err(Refusal(format!(
            "the water test answered {} of {} samples",
            wet.len(),
            samples.len()
        )));
    }
    let mut crosses = vec![false; built.len()];
    for (k, w) in wet.into_iter().enumerate() {
        crosses[owner[k] as usize] |= w;
    }
    let mut points = Vec::new();
    let mut edges = Vec::with_capacity(built.len());
    for ((e, (a, b)), crosses_water) in built.iter().zip(ends).zip(crosses) {
        let start = run_start(points.len())?;
        points.extend(e.points.iter().map(|p| [p[0] as f32, p[1] as f32]));
        edges.push(Edge {
            kind: e.kind,
            class: e.class,
            a,
            b,
            length: e.length,
            crosses_water,
            points: start..run_start(points.len())?,
        });
    }
    Ok(Graph {
        classes,
        nodes: kept_nodes,
        edges,
        points,
        index: Indexes::default(),
    })
}

/// Where a polyline starts in the graph's points, refused past what an
/// edge can name.
fn run_start(n: usize) -> Result<u32, Refusal> {
    u32::try_from(n).map_err(|_| Refusal("the road graph has too many points".into()))
}

/// Joins the two edges at each node where exactly two of one kind and class
/// meet and run on within `COLLINEAR_DEG`, leaving the second edge `None`.
fn contract(nodes: &[[f64; 2]], edges: &mut [Option<Building>]) {
    let mut at: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
    for (i, e) in edges.iter().enumerate() {
        let e = e.as_ref().expect("every edge is there before contraction");
        at[e.a].push(i);
        at[e.b].push(i);
    }
    let cos_max = COLLINEAR_DEG.to_radians().cos();
    for n in 0..nodes.len() {
        let [i, j] = at[n][..] else { continue };
        if i == j {
            continue;
        }
        let (Some(e1), Some(e2)) = (edges[i].as_ref(), edges[j].as_ref()) else {
            continue;
        };
        if (e1.kind, e1.class) != (e2.kind, e2.class) {
            continue;
        }
        // The direction each runs in at its end nearest n, pointed at n.
        let toward = |e: &Building| {
            let p = &e.points;
            let (end, inner) = if e.b == n {
                (p[p.len() - 1], p[p.len() - 2])
            } else {
                (p[0], p[1])
            };
            [end[0] - inner[0], end[1] - inner[1]]
        };
        let (din, dout) = (toward(e1), toward(e2));
        // Straight on means the second points at n opposite to the first.
        let norm = din[0].hypot(din[1]) * dout[0].hypot(dout[1]);
        let straight = norm > 0.0 && -(din[0] * dout[0] + din[1] * dout[1]) / norm >= cos_max;
        if !straight {
            continue;
        }
        let mut e1 = edges[i].take().expect("checked");
        let mut e2 = edges[j].take().expect("checked");
        // e1 runs into n, e2 out of it.
        if e1.b != n {
            e1.reverse();
        }
        if e2.a != n {
            e2.reverse();
        }
        let far = e2.b;
        e1.points.push(nodes[n]);
        e1.points.extend_from_slice(&e2.points);
        e1.length += e2.length;
        e1.b = far;
        edges[i] = Some(e1);
        for slot in at[far].iter_mut().filter(|s| **s == j) {
            *slot = i;
        }
        at[n].clear();
    }
}

/// A point every `WATER_STEP_M` along each edge from its start, and its last,
/// with the edge each belongs to.
fn water_samples(edges: &[Building]) -> (Vec<[f32; 2]>, Vec<u32>) {
    let (mut samples, mut owner) = (Vec::new(), Vec::new());
    for (i, e) in edges.iter().enumerate() {
        let mut push = |p: [f64; 2]| {
            samples.push([p[0] as f32, p[1] as f32]);
            owner.push(i as u32);
        };
        push(e.points[0]);
        // How far along the current segment the next sample falls.
        let mut next = WATER_STEP_M;
        for s in e.points.windows(2) {
            let len = dist(s[0], s[1]);
            while next <= len {
                let t = next / len;
                push([
                    s[0][0] + t * (s[1][0] - s[0][0]),
                    s[0][1] + t * (s[1][1] - s[0][1]),
                ]);
                next += WATER_STEP_M;
            }
            next -= len;
        }
        push(e.points[e.points.len() - 1]);
    }
    (samples, owner)
}

/// What the format refuses, as an error naming the part.
fn short(what: &str) -> Refusal {
    Refusal(format!("the road graph ends inside its {what}"))
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize, what: &str) -> Result<&[u8], Refusal> {
        let s = self
            .b
            .get(self.at..self.at + n)
            .ok_or_else(|| short(what))?;
        self.at += n;
        Ok(s)
    }
    fn u32(&mut self, what: &str) -> Result<u32, Refusal> {
        Ok(u32::from_le_bytes(self.take(4, what)?.try_into().unwrap()))
    }
    fn f64(&mut self, what: &str) -> Result<f64, Refusal> {
        Ok(f64::from_le_bytes(self.take(8, what)?.try_into().unwrap()))
    }
}

impl Graph {
    /// The payload of `roads.graph`, little-endian: the classes, each a u32
    /// length and its bytes; the nodes, a u32 count and an f64 `x, z` each;
    /// the edges, a u32 count and each as `u8 kind, u8 class, u8 crosses
    /// water, u8 0, u32 a, u32 b, f64 length, u32 n, n × f32 x, z`.
    pub fn encode(&self) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&(self.classes.len() as u32).to_le_bytes());
        for c in &self.classes {
            b.extend_from_slice(&(c.len() as u32).to_le_bytes());
            b.extend_from_slice(c.as_bytes());
        }
        b.extend_from_slice(&(self.nodes.len() as u32).to_le_bytes());
        for n in &self.nodes {
            b.extend_from_slice(&n[0].to_le_bytes());
            b.extend_from_slice(&n[1].to_le_bytes());
        }
        b.extend_from_slice(&(self.edges.len() as u32).to_le_bytes());
        for e in &self.edges {
            b.extend_from_slice(&[e.kind, e.class, u8::from(e.crosses_water), 0]);
            b.extend_from_slice(&e.a.to_le_bytes());
            b.extend_from_slice(&e.b.to_le_bytes());
            b.extend_from_slice(&e.length.to_le_bytes());
            let line = self.line(e);
            b.extend_from_slice(&(line.len() as u32).to_le_bytes());
            for p in line {
                b.extend_from_slice(&p[0].to_le_bytes());
                b.extend_from_slice(&p[1].to_le_bytes());
            }
        }
        b
    }

    pub fn decode(b: &[u8]) -> Result<Graph, Refusal> {
        let mut r = Reader { b, at: 0 };
        let n = r.u32("class table")?;
        let mut classes = Vec::new();
        for _ in 0..n {
            let len = r.u32("class table")? as usize;
            let name = r.take(len, "class table")?;
            classes.push(String::from_utf8_lossy(name).into_owned());
        }
        let n = r.u32("nodes")? as usize;
        // Each count is checked against the bytes left before it sizes
        // anything.
        if n > b.len() / 16 {
            return Err(short("nodes"));
        }
        let mut nodes = Vec::with_capacity(n);
        for _ in 0..n {
            nodes.push([r.f64("nodes")?, r.f64("nodes")?]);
        }
        let n = r.u32("edges")? as usize;
        if n > b.len() / 24 {
            return Err(short("edges"));
        }
        let mut edges = Vec::with_capacity(n);
        // At most every byte left is a point's, and capacity reserved past
        // the points there are is never touched.
        let mut points: Vec<[f32; 2]> = Vec::with_capacity((b.len() - r.at) / 8);
        let f32_at = |c: &[u8]| f32::from_le_bytes(c.try_into().unwrap());
        for _ in 0..n {
            let h = r.take(24, "edges")?;
            let (kind, class, wet) = (h[0], h[1], h[2]);
            let u32_at = |k: usize| u32::from_le_bytes(h[k..k + 4].try_into().unwrap());
            let (a, b_) = (u32_at(4), u32_at(8));
            let length = f64::from_le_bytes(h[12..20].try_into().unwrap());
            let m = u32_at(20) as usize;
            if m > (b.len() - r.at) / 8 {
                return Err(short("edges"));
            }
            let run = r.take(m * 8, "edges")?;
            let start = run_start(points.len())?;
            points.extend(
                run.as_chunks::<8>()
                    .0
                    .iter()
                    .map(|c| [f32_at(&c[..4]), f32_at(&c[4..])]),
            );
            let points = start..run_start(points.len())?;
            if a as usize >= nodes.len() || b_ as usize >= nodes.len() {
                return Err(Refusal("a road graph edge names no node".into()));
            }
            if !(kind == KIND_ROAD || kind == KIND_RAIL) || usize::from(class) >= classes.len() {
                return Err(Refusal(format!(
                    "a road graph edge has kind {kind} and class {class} of {}",
                    classes.len()
                )));
            }
            edges.push(Edge {
                kind,
                class,
                a,
                b: b_,
                length,
                crosses_water: wet != 0,
                points,
            });
        }
        if r.at != b.len() {
            return Err(Refusal("the road graph has bytes past its edges".into()));
        }
        Ok(Graph {
            classes,
            nodes,
            edges,
            points,
            index: Indexes::default(),
        })
    }

    pub fn class(&self, e: &Edge) -> &str {
        &self.classes[usize::from(e.class)]
    }

    /// `e`'s polyline, `[x, z]`, from `a`'s end to `b`'s.
    pub fn line(&self, e: &Edge) -> &[[f32; 2]] {
        &self.points[e.points.start as usize..e.points.end as usize]
    }

    /// `e` as it is searched: its node `a`, its polyline and its node `b`.
    fn path(&self, e: &Edge) -> impl Iterator<Item = [f64; 2]> + '_ {
        let line = self
            .line(e)
            .iter()
            .map(|q| [f64::from(q[0]), f64::from(q[1])]);
        std::iter::once(self.nodes[e.a as usize])
            .chain(line)
            .chain(std::iter::once(self.nodes[e.b as usize]))
    }

    /// The point of `e` closest to `p`, along its polyline and the gaps from
    /// its ends to its nodes; of points as close, the one on the segment
    /// nearest `a`.
    pub fn closest(&self, e: &Edge, p: [f64; 2]) -> [f64; 2] {
        let mut path = self.path(e);
        let mut prev = path.next().expect("a path has its two nodes");
        let (mut best, mut best_d) = (prev, f64::INFINITY);
        for q in path {
            let c = closest_on(prev, q, p);
            let d = (c[0] - p[0]).hypot(c[1] - p[1]);
            if d < best_d {
                (best, best_d) = (c, d);
            }
            prev = q;
        }
        best
    }

    /// The `n` edges of `kind` nearest `p`, nearest first, ties in edge
    /// order: each edge's index and its point closest to `p`.
    pub fn nearest(&self, kind: u8, p: [f64; 2], n: usize) -> Vec<(usize, [f64; 2])> {
        // Each found: its distance, its index and its closest point.
        let mut best: Vec<(f64, usize, [f64; 2])> = Vec::with_capacity(n + 1);
        let ix = self.index.of(self, kind);
        if n == 0 || ix.rows == 0 {
            return Vec::new();
        }
        let before = |a: &(f64, usize, [f64; 2]), b: &(f64, usize, [f64; 2])| {
            a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)).is_lt()
        };
        let mut seen = Seen::new(self.edges.len());
        let (r, c) = ix.cell_of(p);
        let (rows, cols) = (ix.rows as i64, ix.cols as i64);
        // The first ring that reaches the grid.
        let k0 = [0, -r, r - (rows - 1), -c, c - (cols - 1)]
            .into_iter()
            .max()
            .unwrap_or(0);
        for k in k0.. {
            ix.ring(r, c, k, &mut |cell| {
                for &i in ix.cell(cell) {
                    if !seen.first(i as usize) {
                        continue;
                    }
                    let q = self.closest(&self.edges[i as usize], p);
                    let found = ((q[0] - p[0]).hypot(q[1] - p[1]), i as usize, q);
                    if best.len() == n && !before(&found, &best[n - 1]) {
                        continue;
                    }
                    let at = best.partition_point(|b| before(b, &found));
                    best.insert(at, found);
                    best.truncate(n);
                }
            });
            if r - k <= 0 && r + k >= rows - 1 && c - k <= 0 && c + k >= cols - 1 {
                break;
            }
            // Every edge not yet found lies wholly outside the rings so far,
            // so no nearer than their edge.
            if best.len() == n {
                let (x0, z0) = (
                    ix.x0 + (r - k) as f64 * ix.cell,
                    ix.z0 + (c - k) as f64 * ix.cell,
                );
                let (x1, z1) = (
                    x0 + (2 * k + 1) as f64 * ix.cell,
                    z0 + (2 * k + 1) as f64 * ix.cell,
                );
                let clear = (p[0] - x0).min(x1 - p[0]).min(p[1] - z0).min(z1 - p[1]);
                if clear - SLACK_M > best[n - 1].0 {
                    break;
                }
            }
        }
        best.into_iter().map(|(_, i, q)| (i, q)).collect()
    }

    /// Calls `f` once with the index of each edge of `kind` that may run
    /// through `[x0, z0, x1, z1]`; any that does is among them.
    pub fn edges_near(&self, kind: u8, b: [f64; 4], f: &mut dyn FnMut(usize)) {
        let ix = self.index.of(self, kind);
        if ix.rows == 0 {
            return;
        }
        let (r0, c0) = ix.cell_of([b[0], b[1]]);
        let (r1, c1) = ix.cell_of([b[2], b[3]]);
        let (r0, c0) = (r0.max(0), c0.max(0));
        let (r1, c1) = (r1.min(ix.rows as i64 - 1), c1.min(ix.cols as i64 - 1));
        let mut seen = Seen::new(self.edges.len());
        for r in r0..=r1 {
            for c in c0..=c1 {
                for &i in ix.cell(r as usize * ix.cols + c as usize) {
                    if seen.first(i as usize) {
                        f(i as usize);
                    }
                }
            }
        }
    }
}

/// The closest point to `p` on the segment `a`–`b`.
fn closest_on(a: [f64; 2], b: [f64; 2], p: [f64; 2]) -> [f64; 2] {
    let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
    let len2 = dx * dx + dz * dz;
    let t = if len2 > 0.0 {
        (((p[0] - a[0]) * dx + (p[1] - a[1]) * dz) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    [a[0] + t * dx, a[1] + t * dz]
}

/// Segments of a path a box of the index is drawn around: an edge is listed
/// in the cells each run's box meets.
const RUN: usize = 8;
/// The fewest edges whose index is built on several threads, and the fewest
/// points whose box is found on several.
const PARALLEL_EDGES: usize = 50_000;
const PARALLEL_POINTS: usize = 1_000_000;
/// A box holding nothing, which growing by a point makes that point's.
const NO_BOX: [f64; 4] = [
    f64::INFINITY,
    f64::INFINITY,
    f64::NEG_INFINITY,
    f64::NEG_INFINITY,
];

fn grow(b: [f64; 4], p: [f64; 2]) -> [f64; 4] {
    [
        b[0].min(p[0]),
        b[1].min(p[1]),
        b[2].max(p[0]),
        b[3].max(p[1]),
    ]
}

/// The smallest cell of the index, in meters.
const MIN_CELL_M: f64 = 50.0;
/// The most cells an index has.
const MAX_CELLS: f64 = (1 << 22) as f64;
/// How much nearer than a ring's edge a search still looks, in meters: far
/// more than the rounding in a closest point or in placing a point in a
/// cell, so an edge just past the edge is never taken to be farther than it
/// is.
const SLACK_M: f64 = 1e-3;

/// Which edges a search has met, so each is looked at once.
struct Seen(Vec<u64>);

impl Seen {
    fn new(n: usize) -> Seen {
        Seen(vec![0; n.div_ceil(64)])
    }

    /// Whether `i` is met for the first time.
    fn first(&mut self, i: usize) -> bool {
        let (w, bit) = (i / 64, 1u64 << (i % 64));
        let new = self.0[w] & bit == 0;
        self.0[w] |= bit;
        new
    }
}

/// `f` over `0..n` cut into a run per thread, its answers in order: on one
/// thread below `parallel_from`, on up to 8 above, below normal priority as
/// the query pool's threads are.
fn split<T: Send>(n: usize, parallel_from: usize, f: impl Fn(Range<usize>) -> T + Sync) -> Vec<T> {
    let threads = if n < parallel_from {
        1
    } else {
        crate::threads::cap().min(8)
    };
    let per = n.div_ceil(threads).max(1);
    std::thread::scope(|s| {
        let f = &f;
        let runs: Vec<_> = (0..n.div_ceil(per).max(1))
            .map(|t| {
                s.spawn(move || {
                    crate::threads::below_normal();
                    f(t * per..((t + 1) * per).min(n))
                })
            })
            .collect();
        runs.into_iter()
            .map(|r| r.join().expect("an index thread panicked"))
            .collect()
    })
}

/// Each network's index, built when it is first searched, over a grid both
/// share. It follows from the edges, so two graphs with the same edges are
/// equal whichever has built its index.
#[derive(Default, Clone)]
struct Indexes {
    bounds: OnceLock<[f64; 4]>,
    nets: [OnceLock<Index>; 2],
}

impl Indexes {
    fn of(&self, g: &Graph, kind: u8) -> &Index {
        self.nets[usize::from(kind == KIND_RAIL)].get_or_init(|| {
            let all = *self.bounds.get_or_init(|| bounds(g));
            Index::new(g, kind, all)
        })
    }
}

impl PartialEq for Indexes {
    fn eq(&self, _: &Indexes) -> bool {
        true
    }
}

impl std::fmt::Debug for Indexes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Indexes")
    }
}

/// The box holding every node and point of both networks.
fn bounds(g: &Graph) -> [f64; 4] {
    let none = [
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    ];
    let parts = split(g.points.len(), PARALLEL_POINTS, |run| {
        g.points[run].iter().fold(none, |b, p| {
            [
                b[0].min(p[0]),
                b[1].min(p[1]),
                b[2].max(p[0]),
                b[3].max(p[1]),
            ]
        })
    });
    let flat = parts.iter().fold(NO_BOX, |b, p| {
        grow(
            grow(b, [p[0], p[1]].map(f64::from)),
            [p[2], p[3]].map(f64::from),
        )
    });
    g.nodes.iter().fold(flat, |b, &p| grow(b, p))
}

/// The edges of one network by where they run: a grid of square cells over
/// every node and point, each listing in edge order the edges a run of whose
/// segments has a box meeting it.
#[derive(Clone)]
struct Index {
    x0: f64,
    z0: f64,
    cell: f64,
    rows: usize,
    cols: usize,
    /// Cell k's edges are `ids[start[k]..start[k + 1]]`.
    start: Vec<u32>,
    ids: Vec<u32>,
}

impl Index {
    /// The index of `kind`'s edges over a grid covering `all`.
    fn new(g: &Graph, kind: u8, all: [f64; 4]) -> Index {
        let count = g.edges.iter().filter(|e| e.kind == kind).count();
        let mut ix = Index {
            x0: all[0],
            z0: all[1],
            cell: MIN_CELL_M,
            rows: 0,
            cols: 0,
            start: vec![0],
            ids: Vec::new(),
        };
        if count == 0 || !all.iter().all(|v| v.is_finite()) {
            return ix;
        }
        let (w, h) = (all[2] - all[0], all[3] - all[1]);
        // About one edge to a cell, were they spread evenly.
        ix.cell = (w * h / count as f64).sqrt().max(MIN_CELL_M);
        while (w / ix.cell + 1.0) * (h / ix.cell + 1.0) > MAX_CELLS {
            ix.cell *= 2.0;
        }
        ix.rows = (w / ix.cell) as usize + 1;
        ix.cols = (h / ix.cell) as usize + 1;
        let parts = split(g.edges.len(), PARALLEL_EDGES, |run| ix.pairs(g, kind, run));
        let mut start = vec![0u32; ix.rows * ix.cols + 1];
        for &(k, _) in parts.iter().flatten() {
            start[k as usize + 1] += 1;
        }
        for k in 1..start.len() {
            start[k] += start[k - 1];
        }
        let mut fill = start.clone();
        let mut ids = vec![0u32; start[start.len() - 1] as usize];
        for &(k, i) in parts.iter().flatten() {
            ids[fill[k as usize] as usize] = i;
            fill[k as usize] += 1;
        }
        (ix.start, ix.ids) = (start, ids);
        ix
    }

    /// Each cell and edge of `kind` among `edges` it lists, by edge.
    fn pairs(&self, g: &Graph, kind: u8, edges: Range<usize>) -> Vec<(u32, u32)> {
        let mut out = Vec::new();
        let mut cells: Vec<u32> = Vec::new();
        for i in edges {
            let e = &g.edges[i];
            if e.kind != kind {
                continue;
            }
            cells.clear();
            let mut list = |b: [f64; 4]| {
                let (r0, c0) = self.cell_of([b[0], b[1]]);
                let (r1, c1) = self.cell_of([b[2], b[3]]);
                let (rows, cols) = (self.rows as i64 - 1, self.cols as i64 - 1);
                for r in r0.clamp(0, rows)..=r1.clamp(0, rows) {
                    for c in c0.clamp(0, cols)..=c1.clamp(0, cols) {
                        let k = (r as usize * self.cols + c as usize) as u32;
                        if cells.last() != Some(&k) {
                            cells.push(k);
                        }
                    }
                }
            };
            // Runs share their end points, so every segment is in one.
            let (mut b, mut n) = (grow(NO_BOX, g.nodes[e.a as usize]), 1);
            for q in g.line(e) {
                let q = [f64::from(q[0]), f64::from(q[1])];
                b = grow(b, q);
                n += 1;
                if n == RUN + 1 {
                    list(b);
                    (b, n) = (grow(NO_BOX, q), 1);
                }
            }
            list(grow(b, g.nodes[e.b as usize]));
            cells.sort_unstable();
            cells.dedup();
            out.extend(cells.iter().map(|&k| (k, i as u32)));
        }
        out
    }

    /// The row and column of the cell holding `p`, past the grid when `p`
    /// is, but never so far that a ring around it overflows.
    fn cell_of(&self, p: [f64; 2]) -> (i64, i64) {
        let far = 1e9;
        let at = |v: f64, o: f64| ((v - o) / self.cell).floor().clamp(-far, far) as i64;
        (at(p[0], self.x0), at(p[1], self.z0))
    }

    fn cell(&self, k: usize) -> &[u32] {
        &self.ids[self.start[k] as usize..self.start[k + 1] as usize]
    }

    /// Calls `f` with each cell of the grid in the ring `k` cells out
    /// around row `r`, column `c`.
    fn ring(&self, r: i64, c: i64, k: i64, f: &mut dyn FnMut(usize)) {
        let (rows, cols) = (self.rows as i64, self.cols as i64);
        let at = |r: i64, c: i64| r as usize * self.cols + c as usize;
        let (c0, c1) = ((c - k).max(0), (c + k).min(cols - 1));
        for rr in [r - k, r + k].into_iter().take(if k == 0 { 1 } else { 2 }) {
            if (0..rows).contains(&rr) {
                for cc in c0..=c1 {
                    f(at(rr, cc));
                }
            }
        }
        let (r0, r1) = ((r - k + 1).max(0), (r + k - 1).min(rows - 1));
        for cc in [c - k, c + k].into_iter().take(if k == 0 { 0 } else { 2 }) {
            if (0..cols).contains(&cc) {
                for rr in r0..=r1 {
                    f(at(rr, cc));
                }
            }
        }
    }
}

/// Each edge's betweenness in its network, 0..1: Brandes's shortest-path
/// count over the largest connected component of each network, from every
/// node or from `sources` spread through it, on `threads` threads at low
/// priority, divided by the network's largest. An edge outside its
/// network's largest component has 0.
pub fn betweenness(g: &Graph, sources: usize, threads: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; g.edges.len()];
    for kind in [KIND_ROAD, KIND_RAIL] {
        let net = Network::new(g, kind);
        let Some(comp) = net.largest() else { continue };
        let picked = spread_sources(&comp, sources);
        let sums = net.brandes(&picked, threads.max(1));
        let max = sums.iter().copied().fold(0.0, f64::max);
        if max > 0.0 {
            for (e, s) in sums.iter().enumerate() {
                if g.edges[e].kind == kind {
                    out[e] = (s / max) as f32;
                }
            }
        }
    }
    out
}

/// `n` of `nodes`, evenly spaced through them, or all when there are fewer.
fn spread_sources(nodes: &[u32], n: usize) -> Vec<u32> {
    if nodes.len() <= n {
        return nodes.to_vec();
    }
    (0..n).map(|k| nodes[k * nodes.len() / n]).collect()
}

/// One network's adjacency, compressed: node `v`'s neighbors are
/// `to[off[v]..off[v + 1]]`, reached along `via` edges of `len`.
struct Network {
    off: Vec<u32>,
    to: Vec<u32>,
    via: Vec<u32>,
    len: Vec<f64>,
    edges: usize,
}

impl Network {
    fn new(g: &Graph, kind: u8) -> Network {
        let n = g.nodes.len();
        let mut deg = vec![0u32; n + 1];
        let mine = || g.edges.iter().enumerate().filter(|(_, e)| e.kind == kind);
        for (_, e) in mine() {
            deg[e.a as usize] += 1;
            deg[e.b as usize] += 1;
        }
        let mut off = vec![0u32; n + 1];
        for v in 0..n {
            off[v + 1] = off[v] + deg[v];
        }
        let m = off[n] as usize;
        let (mut to, mut via, mut len) = (vec![0u32; m], vec![0u32; m], vec![0f64; m]);
        let mut fill = off.clone();
        for (i, e) in mine() {
            for (from, other) in [(e.a, e.b), (e.b, e.a)] {
                let k = fill[from as usize] as usize;
                // A millimeter at least: an edge of no length would make each
                // end a predecessor of the other on a shortest path.
                (to[k], via[k], len[k]) = (other, i as u32, e.length.max(MIN_LENGTH_M));
                fill[from as usize] += 1;
            }
        }
        Network {
            off,
            to,
            via,
            len,
            edges: g.edges.len(),
        }
    }

    fn around(&self, v: u32) -> std::ops::Range<usize> {
        self.off[v as usize] as usize..self.off[v as usize + 1] as usize
    }

    /// The nodes of the largest connected component with an edge, sorted;
    /// `None` when the network has none.
    fn largest(&self) -> Option<Vec<u32>> {
        let n = self.off.len() - 1;
        let mut seen = vec![false; n];
        let mut best: Vec<u32> = Vec::new();
        for start in 0..n as u32 {
            if seen[start as usize] || self.around(start).is_empty() {
                continue;
            }
            seen[start as usize] = true;
            let mut comp = vec![start];
            let mut k = 0;
            while k < comp.len() {
                let v = comp[k];
                k += 1;
                for i in self.around(v) {
                    let w = self.to[i];
                    if !seen[w as usize] {
                        seen[w as usize] = true;
                        comp.push(w);
                    }
                }
            }
            if comp.len() > best.len() {
                best = comp;
            }
        }
        best.sort_unstable();
        (!best.is_empty()).then_some(best)
    }

    /// The summed edge dependencies of shortest paths from each of
    /// `sources`, on `threads` threads at low priority.
    fn brandes(&self, sources: &[u32], threads: usize) -> Vec<f64> {
        let next = AtomicUsize::new(0);
        let total = Mutex::new(vec![0f64; self.edges]);
        std::thread::scope(|s| {
            for _ in 0..threads {
                s.spawn(|| {
                    crate::threads::low_priority();
                    let mut w = Work::new(self.off.len() - 1, self.edges);
                    loop {
                        let k = next.fetch_add(1, Ordering::Relaxed);
                        let Some(&src) = sources.get(k) else { break };
                        w.from(self, src);
                    }
                    let mut t = total.lock().unwrap();
                    for (t, e) in t.iter_mut().zip(&w.edge) {
                        *t += e;
                    }
                });
            }
        });
        total.into_inner().unwrap()
    }
}

/// One thread's state for growing shortest-path trees, reset between
/// sources only where the last one reached.
struct Work {
    dist: Vec<f64>,
    sigma: Vec<f64>,
    delta: Vec<f64>,
    settled: Vec<bool>,
    /// The nodes the last tree settled, nearest first.
    order: Vec<u32>,
    heap: BinaryHeap<Reverse<(u64, u32)>>,
    edge: Vec<f64>,
}

impl Work {
    fn new(nodes: usize, edges: usize) -> Work {
        Work {
            dist: vec![f64::INFINITY; nodes],
            sigma: vec![0.0; nodes],
            delta: vec![0.0; nodes],
            settled: vec![false; nodes],
            order: Vec::new(),
            heap: BinaryHeap::new(),
            edge: vec![0.0; edges],
        }
    }

    fn from(&mut self, net: &Network, s: u32) {
        for &v in &self.order {
            let v = v as usize;
            (self.dist[v], self.sigma[v], self.delta[v]) = (f64::INFINITY, 0.0, 0.0);
            self.settled[v] = false;
        }
        self.order.clear();
        let su = s as usize;
        (self.dist[su], self.sigma[su]) = (0.0, 1.0);
        // A non-negative f64's bits order as the value does.
        self.heap.push(Reverse((0f64.to_bits(), s)));
        while let Some(Reverse((d, v))) = self.heap.pop() {
            let vu = v as usize;
            if self.settled[vu] {
                continue;
            }
            self.settled[vu] = true;
            self.order.push(v);
            let d = f64::from_bits(d);
            for i in net.around(v) {
                let w = net.to[i] as usize;
                let nd = d + net.len[i];
                if nd < self.dist[w] {
                    self.dist[w] = nd;
                    self.sigma[w] = self.sigma[vu];
                    self.heap.push(Reverse((nd.to_bits(), w as u32)));
                } else if nd == self.dist[w] && !self.settled[w] {
                    self.sigma[w] += self.sigma[vu];
                }
            }
        }
        // Dependencies, farthest first: each node's share passes back along
        // every edge that ends one of its shortest paths.
        for &w in self.order.iter().rev() {
            let wu = w as usize;
            let coeff = (1.0 + self.delta[wu]) / self.sigma[wu];
            for i in net.around(w) {
                let v = net.to[i] as usize;
                if v != wu && self.dist[v] + net.len[i] == self.dist[wu] {
                    let c = self.sigma[v] * coeff;
                    self.edge[net.via[i] as usize] += c;
                    self.delta[v] += c;
                }
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn stretch(kind: u8, class: u8, a: u32, b: u32, points: &[[f64; 2]]) -> Stretch {
        Stretch {
            kind,
            class,
            a: Some(a),
            b: Some(b),
            points: points.to_vec(),
        }
    }

    fn dry(_: f64, _: f64) -> bool {
        false
    }

    fn classes() -> Vec<String> {
        vec!["road".into(), "rdb".into(), "rw".into()]
    }

    #[test]
    fn ends_naming_one_id_meet_at_their_mean_and_an_unnamed_end_stands_alone() {
        let s = vec![
            stretch(KIND_ROAD, 0, 7, 8, &[[0.0, 0.0], [100.0, 0.0]]),
            stretch(KIND_ROAD, 0, 8, 9, &[[104.0, 0.0], [104.0, 100.0]]),
            Stretch {
                a: None,
                ..stretch(KIND_ROAD, 0, 0, 9, &[[300.0, 300.0], [104.0, 104.0]])
            },
        ];
        let g = build(classes(), s, &mut dry).unwrap();
        assert_eq!(g.nodes.len(), 4);
        assert_eq!(g.nodes[1], [102.0, 0.0]);
        // The polyline and the two-meter gaps to the junction it stops short of.
        assert_eq!(g.edges[0].length, 102.0);
        assert_eq!((g.edges[2].a, g.edges[2].b), (3, 2));
    }

    #[test]
    fn a_rail_end_naming_a_road_junction_id_joins_no_road() {
        let s = vec![
            stretch(KIND_ROAD, 0, 1, 2, &[[0.0, 0.0], [100.0, 0.0]]),
            stretch(KIND_RAIL, 2, 2, 3, &[[100.0, 0.0], [200.0, 0.0]]),
        ];
        let g = build(classes(), s, &mut dry).unwrap();
        assert_eq!(g.nodes.len(), 4);
    }

    #[test]
    fn contracts_a_straight_through_node_of_one_class_and_nothing_else() {
        let s = vec![
            // Straight on, one class: joined.
            stretch(KIND_ROAD, 0, 1, 2, &[[0.0, 0.0], [100.0, 0.0]]),
            stretch(KIND_ROAD, 0, 3, 2, &[[200.0, 0.0], [100.0, 0.0]]),
            // A 5° turn: kept.
            stretch(KIND_ROAD, 0, 3, 4, &[[200.0, 0.0], [300.0, 8.75]]),
            // Straight on, another class: kept.
            stretch(KIND_ROAD, 1, 4, 5, &[[300.0, 8.75], [400.0, 17.5]]),
        ];
        let g = build(classes(), s, &mut dry).unwrap();
        let ends: Vec<_> = g.edges.iter().map(|e| (e.a, e.b, e.class)).collect();
        assert_eq!(ends, [(0, 1, 0), (1, 2, 0), (2, 3, 1)]);
        assert_eq!(g.edges[0].length, 200.0);
        assert_eq!(
            g.line(&g.edges[0]),
            [
                [0.0, 0.0],
                [100.0, 0.0],
                [100.0, 0.0],
                [100.0, 0.0],
                [200.0, 0.0]
            ]
        );
        assert_eq!(
            g.nodes,
            [[0.0, 0.0], [200.0, 0.0], [300.0, 8.75], [400.0, 17.5]]
        );
    }

    #[test]
    fn samples_every_25_meters_for_water() {
        let s = vec![stretch(
            KIND_ROAD,
            0,
            1,
            2,
            &[[0.0, 0.0], [60.0, 0.0], [110.0, 0.0]],
        )];
        let mut asked = Vec::new();
        let mut water = |x: f64, _: f64| {
            asked.push(x);
            (80.0..=90.0).contains(&x)
        };
        let g = build(classes(), s, &mut water).unwrap();
        assert_eq!(asked, [0.0, 25.0, 50.0, 75.0, 100.0, 110.0]);
        assert!(!g.edges[0].crosses_water);
        let s = vec![stretch(KIND_ROAD, 0, 1, 2, &[[0.0, 0.0], [110.0, 0.0]])];
        let g = build(classes(), s, &mut |x: f64, _: f64| x == 100.0).unwrap();
        assert!(g.edges[0].crosses_water);
    }

    /// A generator of numbers in 0..1, the same on every run.
    pub(crate) struct Unit(pub(crate) u64);

    impl Unit {
        pub(crate) fn next(&mut self) -> f64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 >> 11) as f64 / (1u64 << 53) as f64
        }
    }

    /// A graph of both kinds, clustered and spread, some edges long, some
    /// with no points or one, some copies of others, all on a lattice of
    /// `step` meters so that distances tie.
    pub(crate) fn random_graph(u: &mut Unit, o: [f64; 2], step: f64) -> Graph {
        let mut g = Graph {
            classes: classes(),
            ..Graph::default()
        };
        let at = |u: &mut Unit, spread: f64, c: [f64; 2]| {
            let snap = |v: f64| (v / step).round() * step;
            [
                snap(c[0] + (u.next() - 0.5) * spread),
                snap(c[1] + (u.next() - 0.5) * spread),
            ]
        };
        for _ in 0..400 {
            let c = if u.next() < 0.7 {
                at(u, 4000.0, [o[0] + 2000.0, o[1] + 2000.0])
            } else {
                at(u, 40_000.0, [o[0] + 20_000.0, o[1] + 20_000.0])
            };
            g.nodes.push(c);
        }
        for i in 0..900 {
            if i % 10 == 9 {
                let copy = g.edges[i - 1 - (u.next() * 5.0) as usize].clone();
                g.edges.push(copy);
                continue;
            }
            let a = (u.next() * 400.0) as u32;
            let b = (u.next() * 400.0) as u32;
            let m = match i % 7 {
                0 => 0,
                1 => 1,
                2 => 60,
                _ => (u.next() * 12.0) as usize,
            };
            let start = g.points.len() as u32;
            let (pa, pb) = (g.nodes[a as usize], g.nodes[b as usize]);
            for k in 0..m {
                let t = (k + 1) as f64 / (m + 1) as f64;
                let c = [pa[0] + t * (pb[0] - pa[0]), pa[1] + t * (pb[1] - pa[1])];
                let q = at(u, 300.0, c);
                g.points.push([q[0] as f32, q[1] as f32]);
            }
            g.edges.push(Edge {
                kind: if i % 5 == 0 { KIND_RAIL } else { KIND_ROAD },
                class: (i % 3) as u8,
                a,
                b,
                length: 0.0,
                crosses_water: false,
                points: start..g.points.len() as u32,
            });
        }
        g
    }

    /// The `n` nearest edges by brute force: every edge's closest point
    /// along its whole path, in edge order, sorted stably by distance.
    fn every_edge(g: &Graph, kind: u8, p: [f64; 2], n: usize) -> Vec<(usize, [f64; 2])> {
        let mut all = Vec::new();
        for (i, e) in g.edges.iter().enumerate().filter(|(_, e)| e.kind == kind) {
            let path: Vec<[f64; 2]> = g.path(e).collect();
            let (mut best, mut best_d) = (path[0], f64::INFINITY);
            for s in path.windows(2) {
                let c = closest_on(s[0], s[1], p);
                let d = (c[0] - p[0]).hypot(c[1] - p[1]);
                if d < best_d {
                    (best, best_d) = (c, d);
                }
            }
            all.push(((best[0] - p[0]).hypot(best[1] - p[1]), i, best));
        }
        all.sort_by(|a, b| a.0.total_cmp(&b.0));
        all.truncate(n);
        all.into_iter().map(|(_, i, q)| (i, q)).collect()
    }

    #[test]
    fn the_index_finds_what_every_edge_ranks_first_ties_included() {
        let mut u = Unit(0x9e37_79b9_7f4a_7c15);
        for (trial, step) in [(0, 100.0), (1, 250.0), (2, 1.0)] {
            let o = [-300_000.0 + 1000.0 * f64::from(trial), 600_000.0];
            let g = random_graph(&mut u, o, step);
            let mut ties = 0;
            for k in 0..400 {
                let mut p = [
                    o[0] + (u.next() * 1.6 - 0.3) * 40_000.0,
                    o[1] + (u.next() * 1.6 - 0.3) * 40_000.0,
                ];
                if k % 2 == 0 {
                    p = p.map(|v| (v / step).round() * step);
                }
                if k == 0 {
                    p = [1e12, -1e12];
                }
                for kind in [KIND_ROAD, KIND_RAIL] {
                    for n in [1, 3, 50, 2000] {
                        let want = every_edge(&g, kind, p, n);
                        let got = g.nearest(kind, p, n);
                        assert_eq!(format!("{got:?}"), format!("{want:?}"), "{p:?} {kind} {n}");
                        let d = |&(_, q): &(usize, [f64; 2])| (q[0] - p[0]).hypot(q[1] - p[1]);
                        ties += want.windows(2).filter(|w| d(&w[0]) == d(&w[1])).count();
                    }
                }
            }
            assert!(ties > 100, "only {ties} ties");
        }
    }

    #[test]
    fn edges_near_a_box_hold_every_edge_whose_path_meets_it() {
        let mut u = Unit(7);
        let o = [10_000.0, -20_000.0];
        let g = random_graph(&mut u, o, 10.0);
        for _ in 0..300 {
            let (x, z) = (
                o[0] + (u.next() * 1.4 - 0.2) * 40_000.0,
                o[1] + (u.next() * 1.4 - 0.2) * 40_000.0,
            );
            let s = u.next() * 3000.0;
            let b = [x - s, z - s, x + s, z + s];
            for kind in [KIND_ROAD, KIND_RAIL] {
                let mut got = Vec::new();
                g.edges_near(kind, b, &mut |i| got.push(i));
                let n = got.len();
                got.sort_unstable();
                got.dedup();
                assert_eq!(got.len(), n, "an edge given twice");
                for (i, e) in g.edges.iter().enumerate() {
                    let meets = g.path(e).collect::<Vec<_>>().windows(2).any(|s| {
                        s[0][0].min(s[1][0]) <= b[2]
                            && s[0][0].max(s[1][0]) >= b[0]
                            && s[0][1].min(s[1][1]) <= b[3]
                            && s[0][1].max(s[1][1]) >= b[1]
                    });
                    if e.kind == kind && meets {
                        assert!(got.binary_search(&i).is_ok(), "edge {i} missed");
                    }
                    if e.kind != kind {
                        assert!(got.binary_search(&i).is_err(), "edge {i} of the other kind");
                    }
                }
            }
        }
    }

    #[test]
    fn round_trips_and_refuses_a_cut_payload() {
        let s = vec![
            stretch(KIND_ROAD, 0, 1, 2, &[[0.0, 0.0], [100.0, 0.0]]),
            stretch(KIND_RAIL, 2, 3, 4, &[[0.0, 50.0], [100.0, 60.0]]),
        ];
        let g = build(classes(), s, &mut |_: f64, z: f64| z > 55.0).unwrap();
        let b = g.encode();
        assert_eq!(Graph::decode(&b).unwrap(), g);
        for cut in [1, 10, b.len() - 1] {
            assert!(Graph::decode(&b[..cut]).is_err(), "{cut}");
        }
        let mut long = b.clone();
        long.push(0);
        assert!(Graph::decode(&long).is_err());
        // The second edge's class byte, past the three classes.
        let mut bad = b.clone();
        let at = b.len() - (4 + 4 + 4 + 8 + 4 + 2 * 8) + 1;
        bad[at] = 3;
        assert!(Graph::decode(&bad).unwrap_err().0.contains("class 3 of 3"));
    }

    /// A square with a tail: the tail's edge carries every path to its end.
    fn square_with_tail() -> Graph {
        let s = vec![
            stretch(KIND_ROAD, 0, 1, 2, &[[0.0, 0.0], [100.0, 0.0]]),
            stretch(KIND_ROAD, 0, 2, 3, &[[100.0, 0.0], [100.0, 100.0]]),
            stretch(KIND_ROAD, 0, 3, 4, &[[100.0, 100.0], [0.0, 100.0]]),
            stretch(KIND_ROAD, 0, 4, 1, &[[0.0, 100.0], [0.0, 0.0]]),
            stretch(KIND_ROAD, 0, 3, 5, &[[100.0, 100.0], [250.0, 250.0]]),
            // Another network, and a road island apart from the square.
            stretch(KIND_RAIL, 2, 6, 7, &[[0.0, -50.0], [100.0, -50.0]]),
            stretch(KIND_ROAD, 0, 8, 9, &[[900.0, 900.0], [990.0, 900.0]]),
        ];
        build(classes(), s, &mut dry).unwrap()
    }

    /// Brandes from every node of `kind`'s largest component, summed and
    /// unnormalized, by brute force over every pair's shortest paths.
    fn brute(g: &Graph, kind: u8) -> Vec<f64> {
        let net = Network::new(g, kind);
        let comp = net.largest().unwrap();
        let mut w = Work::new(g.nodes.len(), g.edges.len());
        for &s in &comp {
            w.from(&net, s);
        }
        w.edge
    }

    #[test]
    fn the_tail_carries_the_most_and_other_components_none() {
        let g = square_with_tail();
        let b = betweenness(&g, SOURCES, 2);
        assert_eq!(b[4], 1.0);
        assert!(b[..4].iter().all(|&v| v > 0.0 && v < 1.0), "{b:?}");
        assert_eq!(b[5], 1.0, "the rail network is its own");
        assert_eq!(b[6], 0.0, "the island is not the largest road component");
        // Ordered pairs: the tail joins its end to the four square nodes.
        assert_eq!(brute(&g, KIND_ROAD)[4], 8.0);
    }

    #[test]
    fn splits_a_tie_between_two_equal_paths() {
        let g = square_with_tail();
        let e = brute(&g, KIND_ROAD);
        // Between opposite corners two shortest paths run, each with half.
        // Edge 0 carries 1→2 and 2→1 whole, 1→3 and 3→1 and 1→5 and 5→1 by
        // half, and 2→4 and 4→2 by half: 2 + 2 + 1 = 5.
        assert_eq!(e[0], 5.0);
    }

    #[test]
    fn threads_and_sampled_sources_agree_with_one_thread() {
        let g = square_with_tail();
        assert_eq!(betweenness(&g, SOURCES, 1), betweenness(&g, SOURCES, 3));
        assert_eq!(spread_sources(&[1, 2, 3, 4, 5, 6], 3), [1, 3, 5]);
        assert_eq!(spread_sources(&[1, 2], 3), [1, 2]);
    }
}
