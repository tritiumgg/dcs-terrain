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
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

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
    /// `[x, z]`, from `a`'s end to `b`'s; through each contracted node.
    pub points: Vec<[f32; 2]>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Graph {
    pub classes: Vec<String>,
    /// `[x, z]` of each node.
    pub nodes: Vec<[f64; 2]>,
    pub edges: Vec<Edge>,
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
    let mut built: Vec<Building> = edges.into_iter().flatten().collect();
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
    let edges = built
        .iter_mut()
        .zip(ends)
        .zip(crosses)
        .map(|((e, (a, b)), crosses_water)| Edge {
            kind: e.kind,
            class: e.class,
            a,
            b,
            length: e.length,
            crosses_water,
            points: e
                .points
                .iter()
                .map(|p| [p[0] as f32, p[1] as f32])
                .collect(),
        })
        .collect();
    Ok(Graph {
        classes,
        nodes: kept_nodes,
        edges,
    })
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
    fn u8(&mut self, what: &str) -> Result<u8, Refusal> {
        Ok(self.take(1, what)?[0])
    }
    fn u32(&mut self, what: &str) -> Result<u32, Refusal> {
        Ok(u32::from_le_bytes(self.take(4, what)?.try_into().unwrap()))
    }
    fn f32(&mut self, what: &str) -> Result<f32, Refusal> {
        Ok(f32::from_le_bytes(self.take(4, what)?.try_into().unwrap()))
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
            b.extend_from_slice(&(e.points.len() as u32).to_le_bytes());
            for p in &e.points {
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
        for _ in 0..n {
            let (kind, class, wet, _) = (
                r.u8("edges")?,
                r.u8("edges")?,
                r.u8("edges")?,
                r.u8("edges")?,
            );
            let (a, b_, length) = (r.u32("edges")?, r.u32("edges")?, r.f64("edges")?);
            let m = r.u32("edges")? as usize;
            if m > (b.len() - r.at) / 8 {
                return Err(short("edges"));
            }
            let mut points = Vec::with_capacity(m);
            for _ in 0..m {
                points.push([r.f32("edges")?, r.f32("edges")?]);
            }
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
        })
    }

    pub fn class(&self, e: &Edge) -> &str {
        &self.classes[usize::from(e.class)]
    }
}

/// Lowers the calling thread's CPU and I/O priority, so a cache build does
/// not take the CPU from a DCS server beside it. DCS runs only on Windows,
/// so elsewhere nothing is lowered.
pub fn low_priority() {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Threading::{
            GetCurrentThread, SetThreadPriority, THREAD_MODE_BACKGROUND_BEGIN,
        };
        // SAFETY: the pseudo-handle names the calling thread, and failing
        // only leaves its priority as it was.
        unsafe {
            SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN);
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
                    low_priority();
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
mod tests {
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
            g.edges[0].points,
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
