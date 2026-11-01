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
//!
//! A route needs more than nodes: a stretch runs on past where the game joins
//! it, and a junction allows only some ways through. So the graph also holds
//! the passages, stretch by stretch: see `Passages`.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::ops::Range;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use crate::decode::rn4::{Connector, KIND_RAIL, KIND_ROAD, Table};
use crate::mapped::{Column, View};
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
    /// `[x, z]`, from the end at `a` to the end at `b`.
    pub points: Vec<[f64; 2]>,
}

/// Where a stretch's end is reached and how the ends join, as the road file
/// tells it. End `2s` and end `2s + 1` are stretch `s`'s two ends.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Joins {
    /// Meters along each stretch's polyline to its two ends' connection
    /// points. A polyline runs on past them, which is no road a route takes.
    pub stations: Vec<[f64; 2]>,
    /// For each stretch, whether a route may not run from each end to the
    /// other.
    pub one_way: Vec<[bool; 2]>,
    /// Each way through a junction: the end left, the end reached, and the
    /// meters between them.
    pub passes: Vec<(u32, u32, f64)>,
}

/// How far from its edges a connector's ends may lie: they lie on them.
const CONNECTOR_REACH_M: f64 = 1.0;

/// How far `p` lies from `line`, and how many meters along it its closest
/// point is.
fn station(line: &[[f64; 2]], p: [f64; 2]) -> (f64, f64) {
    let (mut best, mut best_at, mut walked) = (f64::INFINITY, 0.0, 0.0);
    for s in line.windows(2) {
        let c = closest_on(s[0], s[1], p);
        let d = dist(c, p);
        if d < best {
            (best, best_at) = (d, walked + dist(s[0], c));
        }
        walked += dist(s[0], s[1]);
    }
    (best, best_at)
}

/// The point `s` meters along `line`, or its last.
fn point_at(line: &[[f64; 2]], s: f64) -> [f64; 2] {
    let mut walked = 0.0;
    for w in line.windows(2) {
        let l = dist(w[0], w[1]);
        if walked + l >= s && l > 0.0 {
            let t = ((s - walked) / l).clamp(0.0, 1.0);
            return [
                w[0][0] + t * (w[1][0] - w[0][0]),
                w[0][1] + t * (w[1][1] - w[0][1]),
            ];
        }
        walked += l;
    }
    line[line.len() - 1]
}

impl Joins {
    /// What `table` and `connectors` say of `stretches`, `stretch_of` naming
    /// the stretch of each edge of the file that has one. A connector's ends
    /// fix where its two ends lie on their edges; a passage between two
    /// networks is left out, since no search crosses between them.
    pub fn read(
        table: &Table,
        connectors: &[Connector],
        stretch_of: &[Option<u32>],
        stretches: &[Stretch],
    ) -> Joins {
        let ends = table
            .ends(stretch_of.len())
            .expect("a table from the scan has two ends to an edge");
        let totals: Vec<f64> = stretches
            .iter()
            .map(|s| polyline_length(&s.points))
            .collect();
        let mut at: HashMap<u32, f64> = HashMap::new();
        let mut lengths: HashMap<(u32, u32), f64> = HashMap::new();
        for c in connectors {
            let of = |e: u32| stretch_of[e as usize].map(|s| (s as usize, ends[e as usize]));
            let (Some((sa, ea)), Some((sb, eb))) = (of(c.a), of(c.b)) else {
                continue;
            };
            let (da, at_a) = station(&stretches[sa].points, c.ends[0]);
            let (db, at_b) = station(&stretches[sb].points, c.ends[1]);
            if da > CONNECTOR_REACH_M || db > CONNECTOR_REACH_M {
                continue;
            }
            // How far from the stretch's end at that junction the point lies:
            // two edges may share both their junctions.
            let off = |s: usize, end: u32, along: f64| {
                let id = Some(table.junction[end as usize]);
                let from_a = if stretches[s].a == id {
                    along
                } else {
                    f64::MAX
                };
                let from_b = if stretches[s].b == id {
                    totals[s] - along
                } else {
                    f64::MAX
                };
                from_a.min(from_b)
            };
            let best = ea
                .iter()
                .flat_map(|&p| {
                    table
                        .links(p as usize)
                        .map(move |(q, _)| (p, q as u32))
                        .filter(|&(p, q)| {
                            eb.contains(&q)
                                && table.junction[p as usize] == table.junction[q as usize]
                        })
                })
                .min_by(|x, y| {
                    let cost = |&(p, q): &(u32, u32)| {
                        off(sa, p, at_a).min(1e12) + off(sb, q, at_b).min(1e12)
                    };
                    cost(x).total_cmp(&cost(y))
                });
            let Some((p, q)) = best else { continue };
            at.insert(p, at_a);
            at.insert(q, at_b);
            lengths.insert((p, q), c.length);
            lengths.insert((q, p), c.length);
        }
        let mut joins = Joins {
            stations: vec![[0.0; 2]; stretches.len()],
            one_way: vec![[false; 2]; stretches.len()],
            passes: Vec::new(),
        };
        let mut end_of = vec![u32::MAX; table.edge.len()];
        for (edge, pair) in ends.iter().enumerate() {
            let Some(s) = stretch_of[edge] else { continue };
            let i = s as usize;
            let total = totals[i];
            let [p, q] = pair.map(|e| at.get(&e).copied());
            // An end no connector reaches lies at the end of the line, past
            // the other from it.
            let far = |o: f64| if o < total / 2.0 { total } else { 0.0 };
            let stations = match (p, q) {
                (Some(p), Some(q)) => [p, q],
                (Some(p), None) => [p, far(p)],
                (None, Some(q)) => [far(q), q],
                (None, None) => {
                    // By the junction either end names: one of them may be
                    // the catch-all, which names none.
                    let [first, second] = pair.map(|e| Some(table.junction[e as usize]));
                    let (a, b) = (stretches[i].a, stretches[i].b);
                    if a != first && (b == first || a == second) {
                        [total, 0.0]
                    } else {
                        [0.0, total]
                    }
                }
            };
            joins.stations[i] = stations;
            let linked = |a: u32, b: u32| table.links(a as usize).any(|(t, _)| t == b as usize);
            joins.one_way[i] = [!linked(pair[0], pair[1]), !linked(pair[1], pair[0])];
            end_of[pair[0] as usize] = s * 2;
            end_of[pair[1] as usize] = s * 2 + 1;
        }
        for from in 0..table.edge.len() {
            let f = end_of[from];
            if f == u32::MAX {
                continue;
            }
            for (to, cost) in table.links(from) {
                let t = end_of[to];
                let through = table.junction[from] == table.junction[to]
                    && table.edge[from] != table.edge[to];
                if t == u32::MAX
                    || !through
                    || stretches[(f / 2) as usize].kind != stretches[(t / 2) as usize].kind
                {
                    continue;
                }
                let length = lengths.get(&(from as u32, to as u32));
                joins
                    .passes
                    .push((f, t, length.copied().unwrap_or(f64::from(cost))));
            }
        }
        joins
    }

    /// Stretches that join wherever two ends name one junction, for a network
    /// with no connectors: each reached at its polyline's end, and every way
    /// through a node open, by way of the node.
    fn open(stretches: &[Stretch], nodes: &[[f64; 2]], ends: &[(usize, usize)]) -> Joins {
        let totals = stretches.iter().map(|s| polyline_length(&s.points));
        let mut at: Vec<Vec<(u32, f64)>> = vec![Vec::new(); nodes.len()];
        for (i, (s, &(a, b))) in stretches.iter().zip(ends).enumerate() {
            let last = s.points[s.points.len() - 1];
            at[a].push((2 * i as u32, dist(s.points[0], nodes[a])));
            at[b].push((2 * i as u32 + 1, dist(last, nodes[b])));
        }
        let mut passes = Vec::new();
        for here in &at {
            for &(f, gap) in here {
                for &(t, on) in here {
                    if f != t {
                        passes.push((f, t, gap + on));
                    }
                }
            }
        }
        Joins {
            stations: totals.map(|t| [0.0, t]).collect(),
            one_way: vec![[false; 2]; stretches.len()],
            passes,
        }
    }
}

/// One stretch's way between its two ends, on the graph's edge that holds it.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub struct Run {
    /// The edge, and in its top two bits `ONE_WAY`.
    edge: u32,
    /// The shares of the way along the edge's path of ends `2r` and `2r + 1`.
    from: f32,
    to: f32,
}

// SAFETY: three 4-byte numbers, with no gap.
unsafe impl bytemuck::Zeroable for Run {}
unsafe impl bytemuck::Pod for Run {}

/// A run's bits for no way from end `2r`, and none from end `2r + 1`.
const ONE_WAY: [u32; 2] = [1 << 31, 1 << 30];

/// One way out of an end: the end reached, on another stretch, and the meters
/// to it.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub struct Pass {
    pub to: u32,
    pub length: f32,
}

// SAFETY: two 4-byte numbers, with no gap.
unsafe impl bytemuck::Zeroable for Pass {}
unsafe impl bytemuck::Pod for Pass {}

/// The network as a route runs it, in the graph's file: ends joined by runs
/// along stretches and by passes through junctions, where a route alternates
/// the two, so it never turns back along a stretch and never takes one
/// passage straight onto another. A rail switch forbids its branch to branch
/// that way, which a graph of nodes cannot say.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Passages {
    runs: Column<Run>,
    /// `[x, z]` of each end.
    places: Column<[f32; 2]>,
    /// End `e`'s passes are `passes[out[e]..out[e + 1]]`.
    out: Column<u32>,
    passes: Column<Pass>,
    /// Edge `e`'s runs are `runs[first[e]..first[e + 1]]`, in order along it.
    first: Column<u32>,
}

impl Passages {
    /// How many ends there are, two to a run.
    pub fn ends(&self) -> usize {
        self.runs.len() * 2
    }

    /// The runs on edge `e`.
    pub fn on_edge(&self, e: usize) -> Range<usize> {
        self.first[e] as usize..self.first[e + 1] as usize
    }

    /// The edge end `e` lies on.
    pub fn edge(&self, end: usize) -> usize {
        (self.runs[end / 2].edge & !(ONE_WAY[0] | ONE_WAY[1])) as usize
    }

    /// The share of the way along its edge end `e` lies at.
    pub fn share(&self, end: usize) -> f64 {
        let r = &self.runs[end / 2];
        f64::from(if end & 1 == 0 { r.from } else { r.to })
    }

    pub fn place(&self, end: usize) -> [f64; 2] {
        self.places[end].map(f64::from)
    }

    /// Whether a route may run from end `e` to the other end of its stretch.
    pub fn runs_from(&self, end: usize) -> bool {
        self.runs[end / 2].edge & ONE_WAY[end % 2] == 0
    }

    /// Whether a route may run along stretch `run` from the share `from` of
    /// its edge to the share `to`.
    pub fn runs_between(&self, run: usize, from: f64, to: f64) -> bool {
        let forward = (to > from) == (self.share(run * 2 + 1) > self.share(run * 2));
        to == from || self.runs_from(run * 2 + usize::from(!forward))
    }

    pub fn out_of(&self, end: usize) -> &[Pass] {
        &self.passes[self.out[end] as usize..self.out[end + 1] as usize]
    }

    /// The passages `encode` wrote at byte `at` of `view`, the last thing
    /// there, in a graph of `edges` edges.
    fn read(view: &View, at: usize, edges: usize) -> Result<Passages, Refusal> {
        let b = view.bytes();
        let cut = || short("passages");
        let head = b.get(at..at + 32).ok_or_else(cut)?;
        let word = |k: usize| u64::from_le_bytes(head[k * 8..k * 8 + 8].try_into().unwrap());
        let (runs, out, passes, first) = (word(0), word(1), word(2), word(3));
        let size = [(runs, 28), (out, 4), (passes, 8), (first, 4)]
            .into_iter()
            .try_fold(at as u64 + 32, |sum, (k, each): (u64, u64)| {
                sum.checked_add(k.checked_mul(each)?)
            })
            .ok_or_else(cut)?;
        if size != b.len() as u64 {
            return Err(Refusal("the road graph's passages do not fill it".into()));
        }
        let (runs, out, passes, first) =
            (runs as usize, out as usize, passes as usize, first as usize);
        let mut from = at + 32;
        let runs: Column<Run> = Column::in_view(view, from, runs).ok_or_else(cut)?;
        from += runs.len() * 12;
        let places = Column::in_view(view, from, runs.len() * 2).ok_or_else(cut)?;
        from += runs.len() * 16;
        let out: Column<u32> = Column::in_view(view, from, out).ok_or_else(cut)?;
        from += out.len() * 4;
        let passes: Column<Pass> = Column::in_view(view, from, passes).ok_or_else(cut)?;
        from += passes.len() * 8;
        let first: Column<u32> = Column::in_view(view, from, first).ok_or_else(cut)?;
        let ends = runs.len() * 2;
        let fits = (out.is_empty() && ends == 0 || out.len() == ends + 1)
            && out.windows(2).all(|w| w[0] <= w[1])
            && out.last().is_none_or(|&l| l as usize == passes.len())
            && passes.iter().all(|p| (p.to as usize) < ends)
            && (first.is_empty() && ends == 0 || first.len() == edges + 1)
            && first.windows(2).all(|w| w[0] <= w[1])
            && first.last().is_none_or(|&l| l as usize == runs.len());
        let p = Passages {
            runs,
            places,
            out,
            passes,
            first,
        };
        if !fits || (0..ends).step_by(2).any(|e| p.edge(e) >= edges) {
            return Err(Refusal("the road graph's passages do not fit it".into()));
        }
        Ok(p)
    }
}

/// One edge of the graph, between two nodes, as `roads.graph` lays it out.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C, packed(4))]
pub struct Edge {
    pub kind: u8,
    pub class: u8,
    wet: u8,
    pad: u8,
    pub a: u32,
    pub b: u32,
    /// Where its polyline lies in the graph's `points`: `[x, z]`, from `a`'s
    /// end to `b`'s, through each contracted node.
    start: u32,
    end: u32,
    /// Meters along the polyline and the gaps from its ends to its nodes, so
    /// the straight line between two nodes is never longer than a path.
    pub length: f64,
}

// SAFETY: integers and an f64, packed to 4 bytes with no gap.
unsafe impl bytemuck::Zeroable for Edge {}
unsafe impl bytemuck::Pod for Edge {}

impl Edge {
    pub fn new(
        (kind, class): (u8, u8),
        (a, b): (u32, u32),
        length: f64,
        crosses_water: bool,
        points: Range<u32>,
    ) -> Edge {
        Edge {
            kind,
            class,
            wet: u8::from(crosses_water),
            pad: 0,
            a,
            b,
            start: points.start,
            end: points.end,
            length,
        }
    }

    pub fn crosses_water(&self) -> bool {
        self.wet != 0
    }
}

/// The graph. Its edges are not changed once a search has run over it, since
/// the index the search builds would no longer match them.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Graph {
    pub classes: Vec<String>,
    /// `[x, z]` of each node.
    pub nodes: Column<[f64; 2]>,
    pub edges: Column<Edge>,
    /// Every edge's polyline, one after another in edge order.
    pub points: Column<[f32; 2]>,
    pub passages: Passages,
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

pub(crate) fn dist(p: [f64; 2], q: [f64; 2]) -> f64 {
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
    /// The stretches it holds in order, each with whether it runs backward.
    stretches: Vec<(u32, bool)>,
}

impl Building {
    fn reverse(&mut self) {
        std::mem::swap(&mut self.a, &mut self.b);
        self.points.reverse();
        self.stretches.reverse();
        for s in &mut self.stretches {
            s.1 = !s.1;
        }
    }
}

/// The graph of `stretches`, whose classes index `classes`, joined wherever
/// their ends name one junction. Stretches of any kind but road and rail are
/// left out. `water` tells which samples of each edge lie on water.
pub fn build(
    classes: Vec<String>,
    stretches: Vec<Stretch>,
    water: &mut dyn Water,
) -> Result<Graph, Refusal> {
    let stretches: Vec<Stretch> = stretches
        .into_iter()
        .filter(|s| (s.kind == KIND_ROAD || s.kind == KIND_RAIL) && s.points.len() >= 2)
        .collect();
    build_joined(classes, stretches, None, water)
}

/// As `build`, of stretches all road or rail of two points or more, joined as
/// `joins` says when it says.
pub fn build_joined(
    classes: Vec<String>,
    stretches: Vec<Stretch>,
    joins: Option<Joins>,
    water: &mut dyn Water,
) -> Result<Graph, Refusal> {
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
    let joins = joins.unwrap_or_else(|| Joins::open(&stretches, &nodes, &ends));
    let totals: Vec<f64> = stretches
        .iter()
        .map(|s| polyline_length(&s.points))
        .collect();
    let counts: Vec<usize> = stretches.iter().map(|s| s.points.len()).collect();
    let end_places: Vec<[[f64; 2]; 2]> = stretches
        .iter()
        .zip(&joins.stations)
        .map(|(s, st)| st.map(|at| point_at(&s.points, at)))
        .collect();
    let mut edges: Vec<Option<Building>> = stretches
        .into_iter()
        .zip(ends)
        .enumerate()
        .map(|(i, (s, (a, b)))| {
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
                stretches: vec![(i as u32, false)],
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
    let passages = passages(&built, &nodes, &joins, &totals, &counts, &end_places);
    let mut points = Vec::new();
    let mut edges = Vec::with_capacity(built.len());
    for ((e, (a, b)), crosses_water) in built.iter().zip(ends).zip(crosses) {
        let start = run_start(points.len())?;
        points.extend(e.points.iter().map(|p| [p[0] as f32, p[1] as f32]));
        edges.push(Edge::new(
            (e.kind, e.class),
            (a, b),
            e.length,
            crosses_water,
            start..run_start(points.len())?,
        ));
    }
    Ok(Graph {
        classes,
        nodes: kept_nodes.into(),
        edges: edges.into(),
        points: points.into(),
        passages,
        index: Indexes::default(),
    })
}

/// The passages of the edges `built`, each stretch's run where contraction
/// left it, with its ends numbered in that order.
fn passages(
    built: &[Building],
    nodes: &[[f64; 2]],
    joins: &Joins,
    totals: &[f64],
    counts: &[usize],
    end_places: &[[[f64; 2]; 2]],
) -> Passages {
    let mut runs = Vec::with_capacity(totals.len());
    let mut places = Vec::with_capacity(totals.len() * 2);
    let mut first = Vec::with_capacity(built.len() + 1);
    let mut renumber = vec![u32::MAX; totals.len() * 2];
    for (i, e) in built.iter().enumerate() {
        first.push(runs.len() as u32);
        // Meters along the edge's path to each of its polyline's points.
        let mut along = Vec::with_capacity(e.points.len());
        let mut walked = dist(nodes[e.a], e.points[0]);
        along.push(walked);
        for w in e.points.windows(2) {
            walked += dist(w[0], w[1]);
            along.push(walked);
        }
        let mut at = 0;
        for &(s, backward) in &e.stretches {
            let s = s as usize;
            let start = along[at];
            let share = |station: f64| {
                let into = if backward {
                    totals[s] - station
                } else {
                    station
                };
                if e.length > 0.0 {
                    ((start + into) / e.length).clamp(0.0, 1.0) as f32
                } else {
                    0.0
                }
            };
            let [from, to] = joins.stations[s].map(share);
            let way = joins.one_way[s];
            let bits = (u32::from(way[0]) * ONE_WAY[0]) | (u32::from(way[1]) * ONE_WAY[1]);
            for k in 0..2 {
                renumber[2 * s + k] = (runs.len() * 2 + k) as u32;
                places.push(end_places[s][k].map(|v| v as f32));
            }
            runs.push(Run {
                edge: i as u32 | bits,
                from,
                to,
            });
            // Its points, and the node contraction put after them.
            at += counts[s] + 1;
        }
    }
    first.push(runs.len() as u32);
    let mut passes: Vec<(u32, Pass)> = joins
        .passes
        .iter()
        .map(|&(f, t, length)| {
            let place = |e: u32| end_places[(e / 2) as usize][(e % 2) as usize];
            // Never under the line between its ends, which the search's
            // estimate takes as the least it can cost.
            let length = length.max(dist(place(f), place(t))) as f32;
            let to = renumber[t as usize];
            (renumber[f as usize], Pass { to, length })
        })
        .collect();
    passes.sort_unstable_by_key(|p| (p.0, p.1.to));
    let mut out = vec![0u32; runs.len() * 2 + 1];
    for p in &passes {
        out[p.0 as usize + 1] += 1;
    }
    for k in 1..out.len() {
        out[k] += out[k - 1];
    }
    let passes: Vec<Pass> = passes.into_iter().map(|p| p.1).collect();
    Passages {
        runs: runs.into(),
        places: places.into(),
        out: out.into(),
        passes: passes.into(),
        first: first.into(),
    }
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
        e1.stretches.append(&mut e2.stretches);
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

/// The bytes of `roads.graph` before its node, edge and point counts and its
/// class table's size, four u64s: an entry opens this far into its file, and
/// the arrays must start on 8 bytes there to be used where they lie.
fn lead(head: u64) -> usize {
    8 + (8 - (head % 8) as usize) % 8
}

impl Graph {
    /// The payload of `roads.graph`, for an entry whose payload starts `head`
    /// bytes into its file, as this machine lays the arrays out: its first
    /// byte, how many bytes lead to the counts, then the counts; the nodes;
    /// the edges; the points; then the classes, each a u32 length and its
    /// bytes. So a reader takes all of it from a view of the file, and needs
    /// no copy of a graph of a hundred MiB.
    pub fn encode(&self, head: u64) -> Vec<u8> {
        let n = lead(head);
        let mut b = vec![0u8; n];
        b[0] = n as u8;
        let mut names = Vec::new();
        for c in &self.classes {
            names.extend_from_slice(&(c.len() as u32).to_le_bytes());
            names.extend_from_slice(c.as_bytes());
        }
        for v in [
            self.nodes.len(),
            self.edges.len(),
            self.points.len(),
            names.len(),
        ] {
            b.extend_from_slice(&(v as u64).to_le_bytes());
        }
        b.extend_from_slice(bytemuck::cast_slice(&self.nodes));
        b.extend_from_slice(bytemuck::cast_slice(&self.edges));
        b.extend_from_slice(bytemuck::cast_slice(&self.points));
        b.extend_from_slice(&names);
        // The grid of each network, so a process that wants one nearest edge
        // does not list half a million of them first, each on 8 bytes from
        // the counts, as the arrays are.
        let pad = |len: usize| n + (len - n).next_multiple_of(8);
        b.resize(pad(b.len()), 0);
        for kind in [KIND_ROAD, KIND_RAIL] {
            let ix = self.index.of(self, kind);
            for v in [ix.x0, ix.z0, ix.cell] {
                b.extend_from_slice(&v.to_le_bytes());
            }
            for v in [ix.rows, ix.cols, ix.start.len(), ix.ids.len()] {
                b.extend_from_slice(&(v as u64).to_le_bytes());
            }
            b.extend_from_slice(bytemuck::cast_slice(&ix.start));
            b.extend_from_slice(bytemuck::cast_slice(&ix.ids));
            b.resize(pad(b.len()), 0);
        }
        // The passages, on 4 bytes, as their arrays are.
        let p = &self.passages;
        for v in [p.runs.len(), p.out.len(), p.passes.len(), p.first.len()] {
            b.extend_from_slice(&(v as u64).to_le_bytes());
        }
        b.extend_from_slice(bytemuck::cast_slice(&p.runs));
        b.extend_from_slice(bytemuck::cast_slice(&p.places));
        b.extend_from_slice(bytemuck::cast_slice(&p.out));
        b.extend_from_slice(bytemuck::cast_slice(&p.passes));
        b.extend_from_slice(bytemuck::cast_slice(&p.first));
        b
    }

    /// The graph `encode` wrote into `view`, its arrays left where they lie.
    pub fn decode(view: &View) -> Result<Graph, Refusal> {
        let b = view.bytes();
        let lead = b.first().map_or(0, |&n| usize::from(n));
        let counts = b.get(lead..lead + 32).filter(|_| lead > 0);
        let counts = counts.ok_or_else(|| short("counts"))?;
        let count = |k: usize| u64::from_le_bytes(counts[k * 8..k * 8 + 8].try_into().unwrap());
        let (n, e, p, c) = (count(0), count(1), count(2), count(3));
        let size = [(n, 16), (e, size_of::<Edge>() as u64), (p, 8), (c, 1)]
            .into_iter()
            .try_fold(lead as u64 + 32, |sum, (k, each)| {
                sum.checked_add(k.checked_mul(each)?)
            })
            .ok_or_else(|| short("counts"))?;
        if size > b.len() as u64 {
            return Err(short("arrays"));
        }
        let (n, e, p) = (n as usize, e as usize, p as usize);
        let nodes_at = lead + 32;
        let edges_at = nodes_at + n * 16;
        let points_at = edges_at + e * size_of::<Edge>();
        let cut = || short("arrays");
        let nodes = Column::in_view(view, nodes_at, n).ok_or_else(cut)?;
        let edges: Column<Edge> = Column::in_view(view, edges_at, e).ok_or_else(cut)?;
        let points = Column::in_view(view, points_at, p).ok_or_else(cut)?;
        let mut classes = Vec::new();
        let mut at = &b[points_at + p * 8..size as usize];
        while !at.is_empty() {
            let len = at.get(..4).ok_or_else(|| short("class table"))?;
            let len = u32::from_le_bytes(len.try_into().unwrap()) as usize;
            let name = at.get(4..4 + len).ok_or_else(|| short("class table"))?;
            classes.push(String::from_utf8_lossy(name).into_owned());
            at = &at[4 + len..];
        }
        let pad = |len: usize| lead + (len - lead).next_multiple_of(8);
        let mut at = pad(size as usize);
        let mut nets = [Index::empty(), Index::empty()];
        for net in &mut nets {
            *net = Index::read(view, &mut at, e as u32)?;
            at = pad(at);
        }
        let passages = Passages::read(view, at, e)?;
        let [roads, rails] = nets;
        for edge in edges.iter() {
            if edge.a as usize >= n || edge.b as usize >= n {
                return Err(Refusal("a road graph edge names no node".into()));
            }
            if edge.start > edge.end || edge.end as usize > p {
                return Err(Refusal("a road graph edge names no points".into()));
            }
            let kind = edge.kind;
            if !(kind == KIND_ROAD || kind == KIND_RAIL) || usize::from(edge.class) >= classes.len()
            {
                return Err(Refusal(format!(
                    "a road graph edge has kind {kind} and class {} of {}",
                    edge.class,
                    classes.len()
                )));
            }
        }
        let index = Indexes::default();
        let _ = index.nets[0].set(roads);
        let _ = index.nets[1].set(rails);
        Ok(Graph {
            classes,
            nodes,
            edges,
            points,
            passages,
            index,
        })
    }

    pub fn class(&self, e: &Edge) -> &str {
        &self.classes[usize::from(e.class)]
    }

    /// `e`'s polyline, `[x, z]`, from `a`'s end to `b`'s.
    pub fn line(&self, e: &Edge) -> &[[f32; 2]] {
        &self.points[e.start as usize..e.end as usize]
    }

    /// `e` as it is searched: its node `a`, its polyline and its node `b`.
    pub fn path(&self, e: &Edge) -> impl Iterator<Item = [f64; 2]> + '_ {
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

    /// The point of `e` closest to `p`, as `closest` finds it, and the share
    /// of the way along `e`'s path from `a` to `b` it lies at, 0 to 1.
    pub fn locate(&self, e: &Edge, p: [f64; 2]) -> ([f64; 2], f64) {
        let mut path = self.path(e);
        let mut prev = path.next().expect("a path has its two nodes");
        let (mut best, mut best_d, mut best_at) = (prev, f64::INFINITY, 0.0);
        let mut walked = 0.0;
        for q in path {
            let c = closest_on(prev, q, p);
            let d = (c[0] - p[0]).hypot(c[1] - p[1]);
            if d < best_d {
                let into = (c[0] - prev[0]).hypot(c[1] - prev[1]);
                (best, best_d, best_at) = (c, d, walked + into);
            }
            walked += (q[0] - prev[0]).hypot(q[1] - prev[1]);
            prev = q;
        }
        let share = if walked > 0.0 { best_at / walked } else { 0.0 };
        (best, share.clamp(0.0, 1.0))
    }

    /// `e`'s path between the shares `from` and `to` of the way along it from
    /// `a`, each 0 to 1, running from `from` to `to` whichever is larger:
    /// its two ends and every point of the path between.
    pub fn piece(&self, e: &Edge, from: f64, to: f64) -> Vec<[f64; 2]> {
        let path: Vec<[f64; 2]> = self.path(e).collect();
        let lengths: Vec<f64> = path
            .windows(2)
            .map(|s| (s[1][0] - s[0][0]).hypot(s[1][1] - s[0][1]))
            .collect();
        let total: f64 = lengths.iter().sum();
        let (lo, hi) = (from.min(to) * total, from.max(to) * total);
        let at = |m: f64| {
            let mut walked = 0.0;
            for (i, &l) in lengths.iter().enumerate() {
                if walked + l >= m && l > 0.0 {
                    let t = ((m - walked) / l).clamp(0.0, 1.0);
                    let (a, b) = (path[i], path[i + 1]);
                    return [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])];
                }
                walked += l;
            }
            path[path.len() - 1]
        };
        let mut out = vec![at(lo)];
        let mut walked = 0.0;
        for (i, &l) in lengths.iter().enumerate() {
            walked += l;
            if walked > lo && walked < hi {
                out.push(path[i + 1]);
            }
        }
        out.push(at(hi));
        out.dedup();
        if out.len() == 1 {
            out.push(out[0]);
        }
        if from > to {
            out.reverse();
        }
        out
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
        let (r, c) = ix.cell_of(p);
        let (rows, cols) = (ix.rows as i64, ix.cols as i64);
        // The first ring that reaches the grid.
        let k0 = [0, -r, r - (rows - 1), -c, c - (cols - 1)]
            .into_iter()
            .max()
            .unwrap_or(0);
        Seen::with(self.edges.len(), |seen| {
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
        });
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
        Seen::with(self.edges.len(), |seen| {
            for r in r0..=r1 {
                for c in c0..=c1 {
                    for &i in ix.cell(r as usize * ix.cols + c as usize) {
                        if seen.first(i as usize) {
                            f(i as usize);
                        }
                    }
                }
            }
        });
    }
}

/// The closest point to `p` on the segment `a`–`b`.
pub(crate) fn closest_on(a: [f64; 2], b: [f64; 2], p: [f64; 2]) -> [f64; 2] {
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
#[derive(Default)]
struct Seen {
    words: Vec<u64>,
    /// The words written, so the next search clears only those: zeroing a
    /// bit for each of half a million edges cost more than the search.
    touched: Vec<u32>,
}

thread_local! {
    static SEEN: std::cell::RefCell<Seen> = std::cell::RefCell::default();
}

impl Seen {
    /// `f` with this thread's set, empty, for `n` edges.
    fn with<T>(n: usize, f: impl FnOnce(&mut Seen) -> T) -> T {
        SEEN.with_borrow_mut(|s| {
            for &w in &s.touched {
                s.words[w as usize] = 0;
            }
            s.touched.clear();
            if s.words.len() < n.div_ceil(64) {
                s.words.resize(n.div_ceil(64), 0);
            }
            f(s)
        })
    }

    /// Whether `i` is met for the first time.
    fn first(&mut self, i: usize) -> bool {
        let (w, bit) = (i / 64, 1u64 << (i % 64));
        let word = &mut self.words[w];
        if *word == 0 {
            self.touched.push(w as u32);
        }
        let new = *word & bit == 0;
        *word |= bit;
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
    start: Column<u32>,
    ids: Column<u32>,
}

impl Index {
    fn empty() -> Index {
        Index {
            x0: 0.0,
            z0: 0.0,
            cell: MIN_CELL_M,
            rows: 0,
            cols: 0,
            start: vec![0].into(),
            ids: Column::default(),
        }
    }

    /// The index `encode` wrote at byte `at` of `view`, of a graph of `edges`
    /// edges, moving `at` past it.
    fn read(view: &View, at: &mut usize, edges: u32) -> Result<Index, Refusal> {
        let b = view.bytes();
        let head = b.get(*at..*at + 56).ok_or_else(|| short("index"))?;
        let word = |k: usize| u64::from_le_bytes(head[k * 8..k * 8 + 8].try_into().unwrap());
        let float = |k: usize| f64::from_bits(word(k));
        let (x0, z0, cell) = (float(0), float(1), float(2));
        let (rows, cols) = (word(3) as usize, word(4) as usize);
        let (starts, ids) = (word(5) as usize, word(6) as usize);
        let cut = || short("index");
        let bytes = starts.checked_add(ids).and_then(|n| n.checked_mul(4));
        let end = bytes
            .and_then(|n| n.checked_add(*at + 56))
            .ok_or_else(cut)?;
        let start: Column<u32> = Column::in_view(view, *at + 56, starts).ok_or_else(cut)?;
        let list: Column<u32> =
            Column::in_view(view, *at + 56 + starts * 4, ids).ok_or_else(cut)?;
        *at = end;
        let cells = rows.checked_mul(cols).and_then(|n| n.checked_add(1));
        let rising = start.windows(2).all(|w| w[0] <= w[1]);
        if cells != Some(starts)
            || !(rising && start.first() == Some(&0) && start.last() == Some(&(ids as u32)))
            || !cell.is_finite()
            || cell <= 0.0
            || list.iter().any(|&i| i >= edges)
        {
            return Err(Refusal("the road graph's index does not fit it".into()));
        }
        Ok(Index {
            x0,
            z0,
            cell,
            rows,
            cols,
            start,
            ids: list,
        })
    }
    /// The index of `kind`'s edges over a grid covering `all`.
    fn new(g: &Graph, kind: u8, all: [f64; 4]) -> Index {
        let count = g.edges.iter().filter(|e| e.kind == kind).count();
        let mut ix = Index {
            x0: all[0],
            z0: all[1],
            ..Index::empty()
        };
        if count == 0 || !all.iter().all(|v| v.is_finite()) {
            return ix;
        }
        let (w, h) = (all[2] - all[0], all[3] - all[1]);
        // About four edges to a cell, were they spread evenly: the grid is
        // kept in the cache, and four times the cells cost it more than they
        // save a search.
        ix.cell = (4.0 * w * h / count as f64).sqrt().max(MIN_CELL_M);
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
        (ix.start, ix.ids) = (start.into(), ids.into());
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
        let net = Adjacency::new(g, kind);
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
#[derive(Clone)]
pub struct Adjacency {
    off: Vec<u32>,
    to: Vec<u32>,
    via: Vec<u32>,
    len: Vec<f64>,
    edges: usize,
}

impl Adjacency {
    pub fn new(g: &Graph, kind: u8) -> Adjacency {
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
        Adjacency {
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

    /// Each node `v` is joined to, and the index of the edge that joins
    /// them, once for each edge.
    pub fn neighbors(&self, v: u32) -> impl Iterator<Item = (u32, u32)> + '_ {
        self.around(v).map(|i| (self.to[i], self.via[i]))
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

    fn from(&mut self, net: &Adjacency, s: u32) {
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
        assert_eq!({ g.edges[0].length }, 102.0);
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
        assert_eq!({ g.edges[0].length }, 200.0);
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
            *g.nodes,
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
        assert!(!g.edges[0].crosses_water());
        let s = vec![stretch(KIND_ROAD, 0, 1, 2, &[[0.0, 0.0], [110.0, 0.0]])];
        let g = build(classes(), s, &mut |x: f64, _: f64| x == 100.0).unwrap();
        assert!(g.edges[0].crosses_water());
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
            passages: Passages::default(),
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
            g.nodes.to_mut().push(c);
        }
        for i in 0..900 {
            if i % 10 == 9 {
                let copy = g.edges[i - 1 - (u.next() * 5.0) as usize];
                g.edges.to_mut().push(copy);
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
                g.points.to_mut().push([q[0] as f32, q[1] as f32]);
            }
            let kind = if i % 5 == 0 { KIND_RAIL } else { KIND_ROAD };
            let end = g.points.len() as u32;
            g.edges.to_mut().push(Edge::new(
                (kind, (i % 3) as u8),
                (a, b),
                0.0,
                false,
                start..end,
            ));
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
    fn locates_a_point_along_an_edge_and_cuts_the_piece_between_two() {
        // An L: 100 m east from the node at the origin, then 100 m north,
        // with a 2 m gap to the node at its far end.
        let s = vec![stretch(
            KIND_ROAD,
            0,
            1,
            2,
            &[[0.0, 0.0], [0.0, 100.0], [98.0, 100.0]],
        )];
        let mut g = build(classes(), s, &mut dry).unwrap();
        g.nodes.to_mut()[1] = [100.0, 100.0];
        let e = g.edges[0];
        let (p, at) = g.locate(&e, [-5.0, 50.0]);
        assert_eq!(p, [0.0, 50.0]);
        assert!((at - 0.25).abs() < 1e-12, "{at}");
        let (p, at) = g.locate(&e, [200.0, 300.0]);
        assert_eq!((p, at), ([100.0, 100.0], 1.0));
        assert_eq!(
            g.piece(&e, 0.25, 0.75),
            [[0.0, 50.0], [0.0, 100.0], [50.0, 100.0]]
        );
        assert_eq!(
            g.piece(&e, 0.75, 0.25),
            [[50.0, 100.0], [0.0, 100.0], [0.0, 50.0]]
        );
        assert_eq!(g.piece(&e, 0.0, 1.0).len(), 4);
        assert_eq!(g.piece(&e, 0.5, 0.5), [[0.0, 100.0], [0.0, 100.0]]);
    }

    #[test]
    fn round_trips_and_refuses_a_cut_payload() {
        let s = vec![
            stretch(KIND_ROAD, 0, 1, 2, &[[0.0, 0.0], [100.0, 0.0]]),
            stretch(KIND_RAIL, 2, 3, 4, &[[0.0, 50.0], [100.0, 60.0]]),
        ];
        let g = build(classes(), s, &mut |_: f64, z: f64| z > 55.0).unwrap();
        for head in [0, 3, 8, 61] {
            let b = g.encode(head);
            assert_eq!((head as usize + b[0] as usize + 32) % 8, 0);
            let decode = |b: &[u8]| Graph::decode(&View::from(b.to_vec()));
            assert_eq!(decode(&b).unwrap(), g);
            for cut in [1, 10, b.len() - 1] {
                assert!(decode(&b[..cut]).is_err(), "{cut}");
            }
            let mut long = b.clone();
            long.push(0);
            assert!(decode(&long).is_err());
            // The second edge's class byte, past the three classes.
            let mut bad = b.clone();
            let edges = b[0] as usize + 32 + 16 * g.nodes.len();
            bad[edges + size_of::<Edge>() + 1] = 3;
            assert!(decode(&bad).unwrap_err().0.contains("class 3 of 3"));
        }
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
        let net = Adjacency::new(g, kind);
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
