//! `route` and `route_alternatives`: the least-cost way along the road or
//! rail network between two points, each snapped to the nearest point of
//! its network, part way along an edge.
//!
//! The search is A* over the ends of the network's stretches and the two
//! snapped ends, as the game's own router runs: from an end through a passage
//! a junction allows and along the stretch beyond, in turn, so it never turns
//! back along a stretch. A stretch costs its length times one plus the
//! exposure weight times the share of its edge the observers see, and times
//! any penalty an earlier alternative put on the edge, so no way costs less
//! than its length, and a little under the straight line to the far end is a
//! consistent estimate: the route is the least-cost one, exactly. Ends are
//! searched only within the ellipse whose foci are the snapped ends and on
//! which every point lies `max_detour_factor` times the straight line between
//! them, summed to both: no route longer than that passes outside it.
//!
//! Alternatives come by the penalty method: each route found has its edges'
//! costs multiplied by `penalty`, and the search runs again. A route is kept
//! when it is no longer than `max_stretch` times the least-cost route and
//! shares no more than `max_overlap` of its length with any route kept, or
//! of theirs with it. It finds routes a player tells apart, where the k shortest paths
//! differ by a turn; it promises nothing of the alternatives it misses.

use std::collections::HashMap;

use serde_json::json;

use super::geojson::{Features, props};
use super::region::{Area, resolve};
use super::sightline::{Heights, walk};
use super::{Terrain, refused};
use crate::decode::rn4::{KIND_RAIL, KIND_ROAD};
use crate::roads::{Edge, Graph, Pass, dist};
use crate::search;
use crate::source::{Refusal, Source};
use crate::types::{
    Alternative, Issue, Leg, Network, Point, Route, RouteAlternativesRequest,
    RouteAlternativesResponse, RouteRequest, RouteResponse,
};

/// How far apart along a route its ground is sampled for height and for
/// the observers' sight.
pub const SAMPLE_STEP_M: f64 = 25.0;
/// The window the steepest grade is measured over.
const GRADE_WINDOW_M: f64 = 100.0;
/// The least distance between two waypoints that are not graph nodes.
const WAYPOINT_SPACING_M: f64 = 500.0;
/// How high above its road an observer looks for a vehicle.
pub const TARGET_AGL_M: f64 = 2.0;
/// How much longer than the straight line a route may always be, however
/// short the line: a road between two near points can run far round.
const MIN_DETOUR_M: f64 = 5000.0;
/// The most alternatives one request asks for.
const K_MAX: u32 = 10;
/// Searches per alternative asked for, at most.
const SEARCHES_PER_K: u32 = 4;
/// The largest exposure weight and penalty taken: compounded over every
/// search an alternative makes, larger ones would carry a cost past what a
/// float holds.
const EXPOSURE_WEIGHT_MAX: f64 = 1000.0;
const PENALTY_MAX: f64 = 10.0;

/// The share of the straight line the search estimates is left: an end's
/// place is rounded to centimeters, so the line itself may overstate a short
/// stretch.
const ESTIMATE_SHARE: f64 = 0.99;
/// How near two lengths are that are one length measured twice.
const JOINED_M: f64 = 1e-3;

/// The edge kind a request's network names.
pub(super) fn kind_of(n: Network) -> u8 {
    match n {
        Network::Roads => KIND_ROAD,
        Network::Railroads => KIND_RAIL,
    }
}

/// Where a point snapped to: an edge, the stretch on it, how far along the
/// edge from its node `a`, as a share of its path, and the point there.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Snap {
    edge: usize,
    run: usize,
    at: f64,
    point: [f64; 2],
}

/// A pass through a junction: its two ends and its meters.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Lead {
    line: [[f64; 2]; 2],
    length: f64,
}

/// Part of one edge, between two shares of the way along it from `a`, in
/// the order the route runs, and the pass that led onto it.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Piece {
    edge: usize,
    from: f64,
    to: f64,
    pass: Option<Lead>,
}

impl Piece {
    fn length(&self, g: &Graph) -> f64 {
        g.edges[self.edge].length * (self.to - self.from).abs()
            + self.pass.map_or(0.0, |p| p.length)
    }

    /// Its line, from the start of its pass.
    fn line(&self, g: &Graph) -> Vec<[f64; 2]> {
        let mut line = g.piece(&g.edges[self.edge], self.from, self.to);
        if let Some(p) = self.pass {
            line.splice(0..0, p.line);
            line.dedup();
        }
        line
    }

    fn span(&self) -> (f64, f64) {
        (self.from.min(self.to), self.from.max(self.to))
    }
}

/// A route found: what it cost the search, and its pieces from start to
/// end.
#[derive(Debug, Clone, PartialEq)]
struct Found {
    cost: f64,
    pieces: Vec<Piece>,
    /// How many nodes the search settled.
    settled: usize,
}

impl Found {
    fn length(&self, g: &Graph) -> f64 {
        self.pieces.iter().map(|p| p.length(g)).sum()
    }
}

fn issue(code: &str, message: &str, path: &str) -> Vec<Issue> {
    vec![Issue::new(code, message, path)]
}

/// One request's search over one network: the snapped ends, what is
/// avoided, and the exposure found so far.
struct Planner<'a> {
    src: &'a dyn Source,
    g: &'a Graph,
    from: Snap,
    to: Snap,
    avoid: Vec<Area>,
    /// Edges any point of whose path lies in a region avoided.
    blocked: Vec<bool>,
    /// Each observer's eye, x, z and absolute altitude.
    observers: Vec<[f64; 3]>,
    exposure_weight: f64,
    /// The share of each edge the observers see, NaN until found.
    exposure: Vec<f32>,
    heights: Heights<'a>,
    /// The most a node's distances to the two ends may sum to.
    bound: f64,
}

impl<'a> Planner<'a> {
    fn new(terrain: &'a Terrain, q: &RouteRequest) -> Result<Planner<'a>, Vec<Issue>> {
        let src = terrain.source();
        for (v, path) in [
            (q.from.x, "from.x"),
            (q.from.z, "from.z"),
            (q.to.x, "to.x"),
            (q.to.z, "to.z"),
        ] {
            if !v.is_finite() {
                return Err(issue("invalid_point", "must be finite", path));
            }
        }
        if !(0.0..=EXPOSURE_WEIGHT_MAX).contains(&q.exposure_weight) {
            return Err(issue(
                "invalid_request",
                &format!("must be 0 to {EXPOSURE_WEIGHT_MAX}"),
                "exposure_weight",
            ));
        }
        if !(q.max_detour_factor >= 1.0 && q.max_detour_factor.is_finite()) {
            return Err(issue(
                "invalid_request",
                "must be finite and 1 or more",
                "max_detour_factor",
            ));
        }
        let avoid = q
            .avoid
            .iter()
            .enumerate()
            .map(|(i, r)| resolve(src, r, &format!("avoid.{i}")))
            .collect::<Result<Vec<_>, _>>()?;
        let mut observers = Vec::new();
        if !q.exposure_observers.is_empty() {
            let at: Vec<[f64; 2]> = q.exposure_observers.iter().map(|o| [o.x, o.z]).collect();
            let ground = if at.iter().flatten().all(|v| v.is_finite()) {
                src.points(&at).map_err(refused)?
            } else {
                vec![None; at.len()]
            };
            for (i, (o, gr)) in q.exposure_observers.iter().zip(ground).enumerate() {
                let path = format!("exposure_observers.{i}");
                if !o.alt_agl.is_finite() {
                    return Err(issue("invalid_point", "must be finite", &path));
                }
                let Some(gr) = gr else {
                    return Err(issue(
                        "off_terrain",
                        "no mesh covers this point, so it has no ground to stand on",
                        &path,
                    ));
                };
                observers.push([o.x, o.z, gr.height + o.alt_agl]);
            }
        }
        let g = src.road_graph().map_err(refused)?;
        let kind = kind_of(q.network);
        let snap = |p: &Point, path: &str| -> Result<Snap, Vec<Issue>> {
            let Some(&(edge, _)) = g.nearest(kind, [p.x, p.z], 1).first() else {
                return Err(issue(
                    "no_network",
                    "this terrain has no edge of that network",
                    path,
                ));
            };
            let (point, at) = g.locate(&g.edges[edge], [p.x, p.z]);
            let run = run_at(g, edge, at);
            Ok(Snap {
                edge,
                run,
                at,
                point,
            })
        };
        let (from, to) = (snap(&q.from, "from")?, snap(&q.to, "to")?);
        let mut blocked = vec![false; g.edges.len()];
        for a in &avoid {
            g.edges_near(kind, a.bbox, &mut |i| {
                if !blocked[i] {
                    blocked[i] = meets(g, &g.edges[i], 0.0, 1.0, a);
                }
            });
        }
        let line = dist(from.point, to.point);
        Ok(Planner {
            src,
            g,
            from,
            to,
            avoid,
            blocked,
            observers,
            exposure_weight: q.exposure_weight,
            exposure: vec![f32::NAN; g.edges.len()],
            heights: Heights::new(src),
            bound: (q.max_detour_factor * line).max(line + MIN_DETOUR_M),
        })
    }

    /// Whether the piece of edge `e` between the shares `from` and `to` meets
    /// no region avoided.
    fn open(&self, e: usize, from: f64, to: f64) -> bool {
        if !self.blocked[e] {
            return true;
        }
        let edge = &self.g.edges[e];
        !self.avoid.iter().any(|a| meets(self.g, edge, from, to, a))
    }

    /// Whether a pass between two points runs through a region avoided.
    fn closed(&self, a: [f64; 2], b: [f64; 2]) -> bool {
        self.avoid.iter().any(|r| r.meets(a, b))
    }

    /// The share of `e` the observers see, found the first time it is asked
    /// for.
    fn exposure_of(&mut self, e: usize) -> Result<f64, Refusal> {
        if self.observers.is_empty() {
            return Ok(0.0);
        }
        if self.exposure[e].is_nan() {
            let line = self.g.piece(&self.g.edges[e], 0.0, 1.0);
            let share = seen_share(&mut self.heights, &self.observers, &samples_along(&line))?;
            self.exposure[e] = share as f32;
        }
        Ok(f64::from(self.exposure[e]))
    }

    /// The least-cost route, each edge's cost multiplied by its penalty in
    /// `penalty`, or `None` when the ends do not connect within the bound.
    /// `estimate` false runs Dijkstra's algorithm rather than A*.
    fn search(
        &mut self,
        penalty: &HashMap<usize, f64>,
        estimate: bool,
    ) -> Result<Option<Found>, Refusal> {
        let g = self.g;
        let p = &g.passages;
        let n = p.ends();
        let (s, t) = (n, n + 1);
        let (from, to, bound) = (self.from, self.to, self.bound);
        let place = move |v: usize| -> [f64; 2] {
            if v == s {
                from.point
            } else if v == t {
                to.point
            } else {
                p.place(v)
            }
        };
        let inside = move |v: usize| dist(from.point, place(v)) + dist(place(v), to.point) <= bound;
        let mut failed: Option<Refusal> = None;
        // Each edge's cost per meter, once its exposure is known.
        let mut rate: HashMap<usize, f64> = HashMap::new();
        let found = {
            let mut rate_of = |me: &mut Planner, e: usize| -> f64 {
                if let Some(&r) = rate.get(&e) {
                    return r;
                }
                let x = me.exposure_of(e).unwrap_or_else(|err| {
                    failed.get_or_insert(err);
                    0.0
                });
                let r = penalty.get(&e).copied().unwrap_or(1.0) * (1.0 + me.exposure_weight * x);
                rate.insert(e, r);
                r
            };
            let me = &mut *self;
            let neighbors = |v: usize, f: &mut dyn FnMut(usize, f64)| {
                if v == t {
                    return;
                }
                if v == s {
                    let e = from.edge;
                    let len = g.edges[e].length;
                    let r = rate_of(me, e);
                    for end in [from.run * 2, from.run * 2 + 1] {
                        let share = p.share(end);
                        if p.runs_between(from.run, from.at, share)
                            && me.open(e, from.at, share)
                            && inside(end)
                        {
                            f(end, len * (from.at - share).abs() * r);
                        }
                    }
                    if to.run == from.run
                        && p.runs_between(from.run, from.at, to.at)
                        && me.open(e, from.at, to.at)
                    {
                        f(t, len * (to.at - from.at).abs() * r);
                    }
                    return;
                }
                for pass in p.out_of(v) {
                    let reached = pass.to as usize;
                    if me.closed(p.place(v), p.place(reached)) {
                        continue;
                    }
                    let (e, share) = (p.edge(reached), p.share(reached));
                    let len = g.edges[e].length;
                    let r = rate_of(me, e);
                    let hop = f64::from(pass.length) * r;
                    let reaches_end = reached / 2 == to.run && p.runs_between(to.run, share, to.at);
                    if reaches_end && me.open(e, share, to.at) {
                        f(t, hop + len * (share - to.at).abs() * r);
                    }
                    let (next, along) = (reached ^ 1, p.share(reached ^ 1));
                    if p.runs_from(reached) && me.open(e, share, along) && inside(next) {
                        f(next, hop + len * (share - along).abs() * r);
                    }
                }
            };
            if estimate {
                let left = |v: usize| dist(place(v), to.point) * ESTIMATE_SHARE;
                search::astar(n + 2, s, t, neighbors, left)
            } else {
                search::dijkstra(n + 2, s, t, neighbors)
            }
        };
        if let Some(e) = failed {
            return Err(e);
        }
        let Some(path) = found else {
            return Ok(None);
        };
        Ok(Some(Found {
            cost: path.cost,
            pieces: self.pieces(&path.nodes, &rate),
            settled: path.settled,
        }))
    }

    /// The pieces a search's ends ran along, each the pass into a stretch and
    /// the run along it, as the search took them.
    fn pieces(&self, nodes: &[usize], rate: &HashMap<usize, f64>) -> Vec<Piece> {
        let g = self.g;
        let p = &g.passages;
        let n = p.ends();
        let (s, t) = (n, n + 1);
        let (from, to) = (self.from, self.to);
        let along = |e: usize, a: f64, b: f64| {
            g.edges[e].length * (a - b).abs() * rate.get(&e).copied().unwrap_or(1.0)
        };
        let mut out: Vec<Piece> = Vec::new();
        for w in nodes.windows(2) {
            let (u, v) = (w[0], w[1]);
            if u == s {
                let end = if v == t { to.at } else { p.share(v) };
                out.push(Piece {
                    edge: from.edge,
                    from: from.at,
                    to: end,
                    pass: None,
                });
                continue;
            }
            let ends = |r: usize| -> (f64, usize) {
                let e = p.edge(r);
                if v == t {
                    (to.at, e)
                } else {
                    (p.share(r ^ 1), e)
                }
            };
            let cost = |pass: &Pass| {
                let r = pass.to as usize;
                let (end, e) = ends(r);
                f64::from(pass.length) * rate.get(&e).copied().unwrap_or(1.0)
                    + along(e, p.share(r), end)
            };
            let pass = p
                .out_of(u)
                .iter()
                .filter(|q| {
                    let r = q.to as usize;
                    let (end, e) = ends(r);
                    let way = if v == t {
                        r / 2 == to.run && p.runs_between(to.run, p.share(r), end)
                    } else {
                        r ^ 1 == v && p.runs_from(r)
                    };
                    way && self.open(e, p.share(r), end) && !self.closed(p.place(u), p.place(r))
                })
                .min_by(|a, b| cost(a).total_cmp(&cost(b)).then(a.to.cmp(&b.to)))
                .expect("the search took a pass between them");
            let r = pass.to as usize;
            let (end, e) = ends(r);
            let share = p.share(r);
            let meters = f64::from(pass.length);
            // A pass along the line of a contracted edge, or of no length, is
            // no piece of its own.
            let gap = out
                .last()
                .filter(|l| l.edge == e)
                .map(|l| (l.to - share).abs());
            if let Some(last) = out
                .last_mut()
                .filter(|_| gap.is_some_and(|d| (d * g.edges[e].length - meters).abs() < JOINED_M))
            {
                last.to = end;
                continue;
            }
            out.push(Piece {
                edge: e,
                from: share,
                to: end,
                pass: (meters > 0.0).then(|| Lead {
                    line: [p.place(u), p.place(r)],
                    length: meters,
                }),
            });
        }
        // An end snapped onto a node leaves a piece of no length, which no
        // leg should report.
        if out.iter().any(|p| p.from != p.to || p.pass.is_some()) {
            out.retain(|p| p.from != p.to || p.pass.is_some());
        }
        out
    }

    /// The route the pieces make, with its legs measured.
    fn describe(&mut self, found: &Found, straight_line: f64) -> Result<Route, Vec<Issue>> {
        let g = self.g;
        let lines: Vec<Vec<[f64; 2]>> = found.pieces.iter().map(|p| p.line(g)).collect();
        // Every leg sampled from its start, its end included; the samples
        // of the whole route are the legs' joined.
        let per_leg: Vec<Vec<[f64; 2]>> = lines.iter().map(|l| samples_along(l)).collect();
        let all: Vec<[f64; 2]> = per_leg.iter().flatten().copied().collect();
        let ground = self.src.points(&all).map_err(refused)?;
        let mut legs = Vec::new();
        let mut profile: Vec<(f64, f64)> = Vec::new();
        let mut at = 0;
        let mut walked = 0.0;
        for (k, (p, pts)) in found.pieces.iter().zip(&per_leg).enumerate() {
            let mut leg_profile = Vec::new();
            let mut along = 0.0;
            for (i, q) in pts.iter().enumerate() {
                if i > 0 {
                    along += dist(pts[i - 1], *q);
                }
                if let Some(gr) = ground[at + i] {
                    leg_profile.push((along, gr.height));
                }
            }
            at += pts.len();
            let (ascent, descent) = climb(&leg_profile);
            let exposure = seen_share(&mut self.heights, &self.observers, pts).map_err(refused)?;
            let e = &g.edges[p.edge];
            let line = &lines[k];
            legs.push(Leg {
                from: point(line[0]),
                to: point(line[line.len() - 1]),
                edge_id: p.edge as i64,
                class: g.class(e).to_string(),
                length: p.length(g),
                ascent,
                descent,
                max_grade_pct: steepest(&leg_profile),
                exposure,
            });
            profile.extend(leg_profile.iter().map(|&(d, h)| (walked + d, h)));
            walked += along;
        }
        let (ascent, descent) = climb(&profile);
        Ok(Route {
            waypoints: waypoints(&lines),
            length: found.length(g),
            ascent,
            descent,
            max_grade_pct: steepest(&profile),
            legs,
            snapped_from: point(self.from.point),
            snapped_to: point(self.to.point),
            straight_line,
        })
    }
}

/// The stretch on edge `e` that holds the share `at`, or lies nearest it: it
/// may lie past both ends of a stretch, on a line that runs on beyond them.
fn run_at(g: &Graph, e: usize, at: f64) -> usize {
    let p = &g.passages;
    let off = |r: usize| {
        let (a, b) = (p.share(r * 2), p.share(r * 2 + 1));
        (a.min(b) - at).max(at - a.max(b)).max(0.0)
    };
    p.on_edge(e)
        .min_by(|&x, &y| off(x).total_cmp(&off(y)).then(x.cmp(&y)))
        .expect("every edge holds a stretch")
}

/// Whether the piece of `e`'s path between the shares `from` and `to`
/// meets the region.
fn meets(g: &Graph, e: &Edge, from: f64, to: f64, a: &Area) -> bool {
    g.piece(e, from, to).windows(2).any(|s| a.meets(s[0], s[1]))
}

fn point(p: [f64; 2]) -> Point {
    Point { x: p[0], z: p[1] }
}

/// Points along `line` every `SAMPLE_STEP_M` from its start, and its end.
pub(super) fn samples_along(line: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let mut out = vec![line[0]];
    let mut next = SAMPLE_STEP_M;
    let mut walked = 0.0;
    for s in line.windows(2) {
        let l = dist(s[0], s[1]);
        while next <= walked + l && l > 0.0 {
            let t = (next - walked) / l;
            out.push([
                s[0][0] + t * (s[1][0] - s[0][0]),
                s[0][1] + t * (s[1][1] - s[0][1]),
            ]);
            next += SAMPLE_STEP_M;
        }
        walked += l;
    }
    // A sample within rounding of the end is the end.
    let end = line[line.len() - 1];
    if out.len() > 1 && dist(out[out.len() - 1], end) < 1e-6 {
        out.pop();
    }
    if out.last() != Some(&end) {
        out.push(end);
    }
    out
}

/// The share of `points` any of `observers` sees a vehicle at, over the
/// raster as `viewshed` judges, 0 with no observers.
fn seen_share(
    heights: &mut Heights,
    observers: &[[f64; 3]],
    points: &[[f64; 2]],
) -> Result<f64, Refusal> {
    if observers.is_empty() {
        return Ok(0.0);
    }
    let step = crate::query::CELL_SIZE_M / 2.0;
    let (mut seen, mut judged) = (0usize, 0usize);
    for &p in points {
        let Some(gr) = heights.at(p[0], p[1])? else {
            continue;
        };
        judged += 1;
        let target = [p[0], p[1], gr + TARGET_AGL_M];
        for o in observers {
            if walk(heights, *o, target, step)?.blocking.is_none() {
                seen += 1;
                break;
            }
        }
    }
    Ok(if judged == 0 {
        0.0
    } else {
        seen as f64 / judged as f64
    })
}

/// Meters climbed and descended over a profile of distance and height.
pub(super) fn climb(profile: &[(f64, f64)]) -> (f64, f64) {
    let (mut up, mut down) = (0.0, 0.0);
    for w in profile.windows(2) {
        let dh = w[1].1 - w[0].1;
        if dh > 0.0 {
            up += dh;
        } else {
            down -= dh;
        }
    }
    (up, down)
}

/// The steepest grade over any window of `GRADE_WINDOW_M` of a profile, in
/// percent, or over the whole of a shorter one.
fn steepest(profile: &[(f64, f64)]) -> f64 {
    let mut best: f64 = 0.0;
    let mut j = 0;
    for i in 0..profile.len() {
        while j < profile.len() && profile[j].0 - profile[i].0 < GRADE_WINDOW_M {
            j += 1;
        }
        let k = j.min(profile.len() - 1);
        let run = profile[k].0 - profile[i].0;
        if run > 0.0 {
            best = best.max((profile[k].1 - profile[i].1).abs() / run * 100.0);
        }
        if j == profile.len() {
            break;
        }
    }
    best
}

/// The route's line simplified: every graph node, and between them a point
/// of the road's own geometry each time `WAYPOINT_SPACING_M` has passed
/// since the last kept.
fn waypoints(lines: &[Vec<[f64; 2]>]) -> Vec<Point> {
    let mut out = vec![lines[0][0]];
    for line in lines {
        let mut since = 0.0;
        for w in line.windows(2) {
            since += dist(w[0], w[1]);
            let end = std::ptr::eq(&w[1], &line[line.len() - 1]);
            if end || since >= WAYPOINT_SPACING_M {
                if out.last() != Some(&w[1]) {
                    out.push(w[1]);
                }
                since = 0.0;
            }
        }
    }
    out.into_iter().map(point).collect()
}

/// Meters of `a` that `b` runs along too, edge by edge.
fn shared(g: &Graph, a: &Found, b: &Found) -> f64 {
    let mut sum = 0.0;
    for p in &a.pieces {
        for q in b.pieces.iter().filter(|q| q.edge == p.edge) {
            let ((p0, p1), (q0, q1)) = (p.span(), q.span());
            let over = p1.min(q1) - p0.max(q0);
            if over > 0.0 {
                sum += over * g.edges[p.edge].length;
            }
        }
    }
    sum
}

/// The edges every route runs along some stretch of, each with the shares
/// all of them run along, ascending by edge.
fn shared_by_all(routes: &[&Found]) -> Vec<(usize, f64, f64)> {
    let mut out = Vec::new();
    let Some(first) = routes.first() else {
        return out;
    };
    let mut edges: Vec<usize> = first.pieces.iter().map(|p| p.edge).collect();
    edges.sort_unstable();
    edges.dedup();
    for e in edges {
        // The shares each route covers of `e`, intersected over the routes.
        let mut common: Vec<(f64, f64)> = vec![(0.0, 1.0)];
        for r in routes {
            let spans: Vec<(f64, f64)> = r
                .pieces
                .iter()
                .filter(|p| p.edge == e)
                .map(Piece::span)
                .collect();
            common = common
                .iter()
                .flat_map(|&(c0, c1)| {
                    spans
                        .iter()
                        .map(move |&(s0, s1)| (c0.max(s0), c1.min(s1)))
                        .filter(|(a, b)| b > a)
                })
                .collect();
        }
        // An edge run along twice may hold stretches apart: the longest.
        if let Some(&(lo, hi)) = common
            .iter()
            .max_by(|x, y| (x.1 - x.0).total_cmp(&(y.1 - y.0)))
        {
            out.push((e, lo, hi));
        }
    }
    out
}

/// The edges of `found` in the top tenth of their network by betweenness,
/// or `None` while betweenness is still being built.
fn top_decile(
    src: &dyn Source,
    g: &Graph,
    kind: u8,
    found: &[&Found],
) -> Result<Vec<Option<Vec<i64>>>, Vec<Issue>> {
    let Some(b) = src.road_betweenness().map_err(refused)? else {
        return Ok(vec![None; found.len()]);
    };
    let mut mine: Vec<f32> = g
        .edges
        .iter()
        .zip(b)
        .filter(|(e, _)| e.kind == kind)
        .map(|(_, &v)| v)
        .collect();
    let at = mine.len() * 9 / 10;
    if at >= mine.len() {
        return Ok(vec![Some(Vec::new()); found.len()]);
    }
    let (_, &mut cut, _) = mine.select_nth_unstable_by(at, f32::total_cmp);
    Ok(found
        .iter()
        .map(|f| {
            let mut edges: Vec<i64> = f
                .pieces
                .iter()
                .filter(|p| b[p.edge] >= cut && b[p.edge] > 0.0)
                .map(|p| p.edge as i64)
                .collect();
            edges.sort_unstable();
            edges.dedup();
            Some(edges)
        })
        .collect())
}

fn no_route() -> Vec<Issue> {
    issue(
        "no_route",
        "the network joins no route between the ends within the detour allowed, past the regions avoided",
        "",
    )
}

pub fn route(terrain: &Terrain, q: RouteRequest) -> Result<RouteResponse, Vec<Issue>> {
    let mut p = Planner::new(terrain, &q)?;
    let found = p
        .search(&HashMap::new(), true)
        .map_err(refused)?
        .ok_or_else(no_route)?;
    let straight = dist([q.from.x, q.from.z], [q.to.x, q.to.z]);
    let route = p.describe(&found, straight)?;
    let geojson = if q.geojson {
        let mut f = Features::new(terrain)?;
        draw(&mut f, p.g, &found, &route, Vec::new());
        ends(&mut f, &route);
        Some(f.finish())
    } else {
        None
    };
    Ok(RouteResponse {
        provenance: terrain.provenance(),
        route,
        geojson,
    })
}

/// A route's line with `extra` among its properties, and its legs'.
fn draw(
    f: &mut Features,
    g: &Graph,
    found: &Found,
    route: &Route,
    extra: Vec<(&str, serde_json::Value)>,
) {
    let mut line: Vec<[f64; 2]> = Vec::new();
    let mut legs = Vec::new();
    for p in &found.pieces {
        let l = p.line(g);
        for &q in &l {
            if line.last() != Some(&q) {
                line.push(q);
            }
        }
        legs.push(l);
    }
    if line.len() == 1 {
        line.push(line[0]);
    }
    let mut props_route = props([
        ("length", json!(route.length)),
        ("ascent", json!(route.ascent)),
        ("descent", json!(route.descent)),
        ("max_grade_pct", json!(route.max_grade_pct)),
    ]);
    for (k, v) in extra {
        props_route.insert(k.to_string(), v);
    }
    f.line(&line, props_route);
    for (l, leg) in legs.iter().zip(&route.legs) {
        f.line(
            l,
            props([
                ("leg", json!(true)),
                ("edge_id", json!(leg.edge_id)),
                ("length", json!(leg.length)),
                ("ascent", json!(leg.ascent)),
                ("descent", json!(leg.descent)),
                ("max_grade_pct", json!(leg.max_grade_pct)),
                ("exposure", json!(leg.exposure)),
                ("class", json!(leg.class)),
            ]),
        );
    }
}

fn ends(f: &mut Features, route: &Route) {
    for (end, p) in [("from", route.snapped_from), ("to", route.snapped_to)] {
        f.point([p.x, p.z], props([("end", json!(end))]));
    }
}

pub fn route_alternatives(
    terrain: &Terrain,
    q: RouteAlternativesRequest,
) -> Result<RouteAlternativesResponse, Vec<Issue>> {
    if !(1..=K_MAX).contains(&q.k) {
        return Err(issue(
            "invalid_request",
            &format!("must be 1 to {K_MAX}"),
            "k",
        ));
    }
    if !(q.max_stretch >= 1.0 && q.max_stretch.is_finite()) {
        return Err(issue(
            "invalid_request",
            "must be finite and 1 or more",
            "max_stretch",
        ));
    }
    if !(0.0..=1.0).contains(&q.max_overlap) {
        return Err(issue("invalid_request", "must be 0 to 1", "max_overlap"));
    }
    if !(q.penalty > 1.0 && q.penalty <= PENALTY_MAX) {
        return Err(issue(
            "invalid_request",
            &format!("must be more than 1 and at most {PENALTY_MAX}"),
            "penalty",
        ));
    }
    let mut p = Planner::new(terrain, &q.route)?;
    let g = p.g;
    let mut penalty: HashMap<usize, f64> = HashMap::new();
    let first = p
        .search(&penalty, true)
        .map_err(refused)?
        .ok_or_else(no_route)?;
    // Stretch is against the least-cost route, which with no exposure
    // weight is the shortest.
    let reference = first.length(g);
    let mut kept = vec![first];
    let mut last = kept[0].clone();
    for _ in 1..SEARCHES_PER_K * q.k {
        if kept.len() == q.k as usize {
            break;
        }
        let mut used: Vec<usize> = last.pieces.iter().map(|p| p.edge).collect();
        used.sort_unstable();
        used.dedup();
        for e in used {
            *penalty.entry(e).or_insert(1.0) *= q.penalty;
        }
        let Some(c) = p.search(&penalty, true).map_err(refused)? else {
            break;
        };
        let len = c.length(g);
        let new = !kept.iter().any(|k| k.pieces == c.pieces);
        let within = reference <= 0.0 || len <= q.max_stretch * reference;
        let apart = kept.iter().all(|k| {
            let s = shared(g, &c, k);
            let share = |l: f64| if l > 0.0 { s / l } else { 1.0 };
            share(len) <= q.max_overlap && share(k.length(g)) <= q.max_overlap
        });
        if new && within && apart {
            kept.push(c.clone());
        }
        last = c;
    }
    let refs: Vec<&Found> = kept.iter().collect();
    let common = shared_by_all(&refs);
    let shared_ids: Vec<i64> = common.iter().map(|&(e, _, _)| e as i64).collect();
    let chokes = top_decile(p.src, g, kind_of(q.route.network), &refs)?;
    let straight = dist(
        [q.route.from.x, q.route.from.z],
        [q.route.to.x, q.route.to.z],
    );
    let mut routes = Vec::new();
    for (i, (f, chokepoints)) in kept.iter().zip(chokes).enumerate() {
        let route = p.describe(f, straight)?;
        let len = f.length(g);
        let overlap_with = kept
            .iter()
            .enumerate()
            .filter(|&(j, _)| j != i)
            .map(|(_, o)| {
                if len > 0.0 {
                    shared(g, f, o) / len
                } else {
                    1.0
                }
            })
            .collect();
        routes.push(Alternative {
            route,
            stretch: if reference > 0.0 {
                len / reference
            } else {
                1.0
            },
            overlap_with,
            shared_edges_with_all: shared_ids.clone(),
            chokepoints,
        });
    }
    let geojson = if q.route.geojson {
        let mut fs = Features::new(terrain)?;
        for (i, (f, a)) in kept.iter().zip(&routes).enumerate() {
            draw(
                &mut fs,
                g,
                f,
                &a.route,
                vec![
                    ("rank", json!(i + 1)),
                    ("stretch", json!(a.stretch)),
                    ("overlap_with", json!(a.overlap_with)),
                ],
            );
        }
        for &(e, lo, hi) in &common {
            fs.line(
                &g.piece(&g.edges[e], lo, hi),
                props([("shared", json!(true)), ("edge_id", json!(e))]),
            );
        }
        ends(&mut fs, &routes[0].route);
        Some(fs.finish())
    } else {
        None
    };
    Ok(RouteAlternativesResponse {
        provenance: terrain.provenance(),
        routes,
        geojson,
    })
}

#[cfg(test)]
mod tests {
    use super::super::answer;
    use super::super::geojson::tests::assert_valid;
    use super::super::tests::synth;
    use super::*;
    use crate::synth::*;
    use serde_json::{Value, json};

    fn form(t: &Terrain) -> Theatre {
        match t {
            Terrain::Synth(s) => *s.form(),
            _ => unreachable!(),
        }
    }

    /// A point given north and east of the synthetic terrain's origin.
    fn at(t: &Terrain, north: f64, east: f64) -> Value {
        let (x, z) = form(t).abs(north, east);
        json!({"x": x, "z": z})
    }

    fn ask(t: &Terrain, op: &str, req: Value) -> Value {
        answer(t, op, &req).unwrap_or_else(|e| panic!("{e:?}"))
    }

    fn bridge(t: &Terrain) -> i64 {
        let g = t.source().road_graph().unwrap();
        let wet: Vec<usize> = (0..g.edges.len())
            .filter(|&i| g.edges[i].kind == KIND_ROAD && g.edges[i].crosses_water())
            .collect();
        assert_eq!(wet.len(), 1);
        wet[0] as i64
    }

    fn edges_of(v: &Value) -> Vec<i64> {
        v["legs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l["edge_id"].as_i64().unwrap())
            .collect()
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() <= 1e-6 * b.abs().max(1.0)
    }

    #[test]
    fn crosses_the_river_on_the_bridge_along_the_east_west_road() {
        let t = synth();
        let req = json!({
            "network": "roads",
            "from": at(&t, ROAD_EW_NORTH_M + 200.0, 10000.0),
            "to": at(&t, ROAD_EW_NORTH_M - 100.0, 60000.0),
        });
        let v = ask(&t, "route", req);
        assert!(
            close(v["length"].as_f64().unwrap(), 50000.0),
            "{}",
            v["length"]
        );
        assert!(edges_of(&v).contains(&bridge(&t)));
        assert!(close(
            v["straight_line"].as_f64().unwrap(),
            50000.0f64.hypot(300.0)
        ));
        let from = at(&t, ROAD_EW_NORTH_M, 10000.0);
        assert!(close(
            v["snapped_from"]["x"].as_f64().unwrap(),
            from["x"].as_f64().unwrap()
        ));
        // A point every 500 m and the nodes between.
        let w = v["waypoints"].as_array().unwrap();
        assert!(w.len() >= 100 && w.len() <= 106, "{}", w.len());
        let back: RouteResponse = serde_json::from_value(v).unwrap();
        assert_eq!(back.route.legs[0].class, ROAD_EW_CLASS);
    }

    #[test]
    fn a_route_south_to_the_east_road_takes_the_bridge_the_straight_line_misses() {
        let t = synth();
        let req = json!({
            "network": "roads",
            "from": at(&t, ROAD_NS_SOUTH_NORTH_M, ROAD_NS_EAST_M),
            "to": at(&t, ROAD_EAST_SOUTH_NORTH_M, ROAD_EAST_EAST_M),
        });
        let v = ask(&t, "route", req);
        let want = (ROAD_EW_NORTH_M - ROAD_NS_SOUTH_NORTH_M)
            + (ROAD_EAST_EAST_M - ROAD_NS_EAST_M)
            + (ROAD_EW_NORTH_M - ROAD_EAST_SOUTH_NORTH_M);
        assert!(
            close(v["length"].as_f64().unwrap(), want),
            "{}",
            v["length"]
        );
        assert!(edges_of(&v).contains(&bridge(&t)));
        // Up the north-south road the ground climbs with the plane and the
        // col, and the profile sums to the difference in height.
        let f = form(&t);
        let rise = f.ground(ROAD_EAST_SOUTH_NORTH_M, ROAD_EAST_EAST_M)
            - f.ground(ROAD_NS_SOUTH_NORTH_M, ROAD_NS_EAST_M);
        let net = v["ascent"].as_f64().unwrap() - v["descent"].as_f64().unwrap();
        assert!((net - rise).abs() < 1.0, "{net} against {rise}");
        let grade = v["max_grade_pct"].as_f64().unwrap();
        assert!(grade > 1.0 && grade < 30.0, "{grade}");
    }

    /// From just east of the north-south road to just past the bridge, on
    /// the east-west road: straight along it, or back round the loop.
    fn loop_request(t: &Terrain) -> Value {
        json!({
            "network": "roads",
            "from": at(t, ROAD_EW_NORTH_M, ROAD_NS_EAST_M + 500.0),
            "to": at(t, ROAD_EW_NORTH_M, RIVER_EAST_M + 1525.0),
        })
    }

    const DIRECT_M: f64 = RIVER_EAST_M + 1525.0 - ROAD_NS_EAST_M - 500.0;
    const ROUND_M: f64 = 500.0
        + 2.0 * (ROAD_LOOP_NORTH_M - ROAD_EW_NORTH_M)
        + (ROAD_LOOP_EAST_M - ROAD_NS_EAST_M)
        + (RIVER_EAST_M + 1525.0 - ROAD_LOOP_EAST_M);

    #[test]
    fn an_avoided_box_sends_the_route_round_the_loop() {
        let t = synth();
        let direct = ask(&t, "route", loop_request(&t));
        assert!(close(direct["length"].as_f64().unwrap(), DIRECT_M));
        let (x, z) = form(&t).abs(ROAD_EW_NORTH_M, 37000.0);
        let mut req = loop_request(&t);
        req["avoid"] = json!([{"Box": {"min_x": x - 200.0, "min_z": z - 200.0,
            "max_x": x + 200.0, "max_z": z + 200.0}}]);
        req["geojson"] = json!(true);
        let v = ask(&t, "route", req.clone());
        assert!(
            close(v["length"].as_f64().unwrap(), ROUND_M),
            "{}",
            v["length"]
        );
        for w in v["waypoints"].as_array().unwrap() {
            let (wx, wz) = (w["x"].as_f64().unwrap(), w["z"].as_f64().unwrap());
            assert!(!((wx - x).abs() <= 200.0 && (wz - z).abs() <= 200.0), "{w}");
        }
        assert!(edges_of(&v).contains(&bridge(&t)));
        assert_valid(&v["geojson"]);
        // A circle over both roads east of the start leaves no way through.
        let (cx, cz) = form(&t).abs(ROAD_EW_NORTH_M, ROAD_NS_EAST_M + 4000.0);
        req["avoid"] = json!([{"Circle": {"x": cx, "z": cz, "radius": 3000.0}}]);
        let e = answer(&t, "route", &req).unwrap_err();
        assert_eq!(e[0].code, "no_route");
    }

    #[test]
    fn the_two_roads_are_the_alternatives_and_both_cross_the_bridge() {
        let t = synth();
        let mut req = loop_request(&t);
        req["geojson"] = json!(true);
        let v = ask(&t, "route_alternatives", req);
        let routes = v["routes"].as_array().unwrap();
        assert_eq!(routes.len(), 2, "{routes:?}");
        let len = |r: &Value| r["length"].as_f64().unwrap();
        assert!(close(len(&routes[0]), DIRECT_M));
        assert!(close(len(&routes[1]), ROUND_M));
        assert!(close(
            routes[1]["stretch"].as_f64().unwrap(),
            ROUND_M / DIRECT_M
        ));
        let b = bridge(&t);
        for r in routes {
            assert_eq!(r["shared_edges_with_all"], json!([b]));
            assert!(r["chokepoints"].as_array().unwrap().contains(&json!(b)));
        }
        // They share the road from the loop's corner to the end.
        let shared = RIVER_EAST_M + 1525.0 - ROAD_LOOP_EAST_M;
        let o = routes[0]["overlap_with"][0].as_f64().unwrap();
        assert!(close(o, shared / DIRECT_M), "{o}");
        assert_valid(&v["geojson"]);
        let shared_lines = v["geojson"]["features"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|f| f["properties"]["shared"] == json!(true))
            .count();
        assert_eq!(shared_lines, 1);
        let back: RouteAlternativesResponse = serde_json::from_value(v).unwrap();
        assert_eq!(back.routes.len(), 2);
    }

    #[test]
    fn a_tight_overlap_or_stretch_keeps_the_shortest_alone() {
        let t = synth();
        for (key, value) in [("max_overlap", 0.3), ("max_stretch", 1.2)] {
            let mut req = loop_request(&t);
            req[key] = json!(value);
            let v = ask(&t, "route_alternatives", req);
            assert_eq!(v["routes"].as_array().unwrap().len(), 1, "{key}");
        }
    }

    /// The least route length between two snapped ends by Bellman–Ford
    /// over the network's nodes, joined to the ends by the pieces of their
    /// edges: nothing of the search under test.
    fn oracle(p: &Planner) -> Option<f64> {
        let g = p.g;
        let net = &g.passages;
        let along = |e: usize, a: f64, b: f64| g.edges[e].length * (a - b).abs();
        let mut d = vec![f64::INFINITY; net.ends()];
        for end in [p.from.run * 2, p.from.run * 2 + 1] {
            d[end] = along(p.from.edge, p.from.at, net.share(end));
        }
        let mut best = f64::INFINITY;
        if p.from.run == p.to.run {
            best = along(p.from.edge, p.from.at, p.to.at);
        }
        loop {
            let mut changed = false;
            for v in 0..net.ends() {
                for pass in net.out_of(v) {
                    let r = pass.to as usize;
                    let (e, hop) = (net.edge(r), d[v] + f64::from(pass.length));
                    if p.blocked[e] {
                        continue;
                    }
                    if r / 2 == p.to.run {
                        best = best.min(hop + along(e, net.share(r), p.to.at));
                    }
                    let next = hop + along(e, net.share(r), net.share(r ^ 1));
                    if next < d[r ^ 1] {
                        d[r ^ 1] = next;
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
        best.is_finite().then_some(best)
    }

    #[test]
    fn a_star_equals_dijkstra_and_brute_force_on_random_synthetic_pairs() {
        let t = synth();
        let mut u = crate::roads::tests::Unit(0x5851_f42d_4c95_7f2d);
        let mut checked = 0;
        for k in 0..200 {
            let p = |u: &mut crate::roads::tests::Unit| Point {
                x: f64::from(ORIGIN_X_M) + 10_000.0 + u.next() * 55_000.0,
                z: f64::from(ORIGIN_Z_M) + 5_000.0 + u.next() * 60_000.0,
            };
            let q = RouteRequest {
                network: Network::Roads,
                from: p(&mut u),
                to: p(&mut u),
                avoid: Vec::new(),
                exposure_observers: Vec::new(),
                exposure_weight: 1.0,
                // Wide enough, every other pair, that the bound cuts nothing
                // the oracle uses.
                max_detour_factor: if k % 2 == 0 { 3.0 } else { 100.0 },
                geojson: false,
            };
            let mut planner = Planner::new(&t, &q).unwrap();
            let a = planner.search(&HashMap::new(), true).unwrap();
            let d = planner.search(&HashMap::new(), false).unwrap();
            assert_eq!(a.is_some(), d.is_some());
            let (Some(a), Some(d)) = (a, d) else { continue };
            assert!(close(a.cost, d.cost), "{} {}", a.cost, d.cost);
            assert!(close(a.length(planner.g), a.cost));
            if k % 2 == 1 {
                let want = oracle(&planner).unwrap();
                assert!(close(a.cost, want), "{} against {want}", a.cost);
                checked += 1;
            }
        }
        assert!(checked > 80, "{checked}");
    }

    #[test]
    fn exposure_costs_the_share_an_observer_sees_and_legs_report_it() {
        let t = synth();
        // Along the east-west road, west of the north-south road.
        let mut req = json!({
            "network": "roads",
            "from": at(&t, ROAD_EW_NORTH_M, 10000.0),
            "to": at(&t, ROAD_EW_NORTH_M, 20000.0),
        });
        let none = ask(&t, "route", req.clone());
        let legs = none["legs"].as_array().unwrap();
        assert!(legs.iter().all(|l| l["exposure"] == 0.0));
        let (x, z) = form(&t).abs(ROAD_EW_NORTH_M + 50.0, 15000.0);
        req["exposure_observers"] = json!([{"x": x, "z": z, "alt_agl": 20.0}]);
        req["exposure_weight"] = json!(2.0);
        let seen = ask(&t, "route", req);
        let x = seen["legs"][0]["exposure"].as_f64().unwrap();
        assert!(x > 0.5 && x <= 1.0, "{x}");
        assert!(close(seen["length"].as_f64().unwrap(), 10000.0));
    }

    #[test]
    fn a_bad_request_is_an_issue() {
        let t = synth();
        let code = |op: &str, patch: Value| {
            let mut req = loop_request(&t);
            for (k, v) in patch.as_object().unwrap() {
                req[k] = v.clone();
            }
            answer(&t, op, &req).unwrap_err()[0].code.clone()
        };
        let bad = |op, patch| assert_eq!(code(op, patch), "invalid_request");
        bad("route", json!({"exposure_weight": -1}));
        bad("route", json!({"max_detour_factor": 0.5}));
        bad("route_alternatives", json!({"k": 11}));
        bad("route_alternatives", json!({"penalty": 1.0}));
        bad("route_alternatives", json!({"max_overlap": 2}));
        let far = json!({"exposure_observers": [{"x": 1e7, "z": 0, "alt_agl": 2}]});
        assert_eq!(code("route", far), "off_terrain");
    }

    #[test]
    fn a_start_on_an_edge_from_a_node_to_itself_leaves_by_the_open_side() {
        use crate::roads::{Stretch, build};
        // A square loop from node 0 back to itself, and a road west from it.
        let id = |n| Some(n);
        let stretches = vec![
            Stretch {
                kind: KIND_ROAD,
                class: 0,
                a: id(1),
                b: id(1),
                points: vec![
                    [0.0, 0.0],
                    [0.0, 500.0],
                    [500.0, 500.0],
                    [500.0, 0.0],
                    [0.0, 0.0],
                ],
            },
            Stretch {
                kind: KIND_ROAD,
                class: 0,
                a: id(1),
                b: id(2),
                points: vec![[0.0, 0.0], [-1000.0, 0.0]],
            },
        ];
        let g = build(vec!["road".into()], stretches, &mut |_: f64, _: f64| false).unwrap();
        let t = synth();
        let src = t.source();
        // Four fifths of the way round, 400 m short of the node, the short
        // way back to it through a box avoided.
        let loop_edge = g.edges.iter().position(|e| e.a == e.b).unwrap();
        let other = 1 - loop_edge;
        let run = |e: usize| g.passages.on_edge(e).start;
        let from = Snap {
            edge: loop_edge,
            run: run(loop_edge),
            at: 0.8,
            point: [400.0, 0.0],
        };
        let west = g.edges[other];
        let to = Snap {
            edge: other,
            run: run(other),
            at: if g.nodes[west.b as usize][0] < 0.0 {
                1.0
            } else {
                0.0
            },
            point: [-1000.0, 0.0],
        };
        let r = crate::types::Region::Box(crate::types::BoundingBox {
            min_x: 150.0,
            min_z: -50.0,
            max_x: 250.0,
            max_z: 50.0,
        });
        let avoid = vec![resolve(src, &r, "avoid.0").unwrap()];
        let blocked = g
            .edges
            .iter()
            .map(|e| meets(&g, e, 0.0, 1.0, &avoid[0]))
            .collect();
        let mut p = Planner {
            src,
            g: &g,
            from,
            to,
            avoid,
            blocked,
            observers: Vec::new(),
            exposure_weight: 0.0,
            exposure: vec![f32::NAN; g.edges.len()],
            heights: Heights::new(src),
            bound: f64::INFINITY,
        };
        let found = p.search(&HashMap::new(), true).unwrap().unwrap();
        assert_eq!(found.pieces[0].to, 0.0, "{:?}", found.pieces);
        assert!(
            close(found.length(&g), 1600.0 + 1000.0),
            "{}",
            found.length(&g)
        );
    }

    /// A planner over `g`, on the synthetic terrain's ground, between the
    /// road points nearest `from` and `to`.
    fn over<'a>(t: &'a Terrain, g: &'a Graph, from: [f64; 2], to: [f64; 2]) -> Planner<'a> {
        let src = t.source();
        let snap = |p: [f64; 2]| {
            let edge = g.nearest(KIND_ROAD, p, 1)[0].0;
            let (point, at) = g.locate(&g.edges[edge], p);
            let run = run_at(g, edge, at);
            Snap {
                edge,
                run,
                at,
                point,
            }
        };
        Planner {
            src,
            g,
            from: snap(from),
            to: snap(to),
            avoid: Vec::new(),
            blocked: vec![false; g.edges.len()],
            observers: Vec::new(),
            exposure_weight: 0.0,
            exposure: vec![f32::NAN; g.edges.len()],
            heights: Heights::new(src),
            bound: f64::INFINITY,
        }
    }

    /// Roads west, east and south of a junction at the origin, and a bypass
    /// from the west road's far end to the south road's. Ends 0 and 1 are the
    /// west road's, 2 and 3 the east's, 4 and 5 the south's, 6 and 7 the
    /// bypass's.
    fn fork() -> Vec<crate::roads::Stretch> {
        let road = |a, b, points: &[[f64; 2]]| crate::roads::Stretch {
            kind: KIND_ROAD,
            class: 0,
            a: Some(a),
            b: Some(b),
            points: points.to_vec(),
        };
        vec![
            road(1, 2, &[[-1000.0, 0.0], [0.0, 0.0]]),
            road(2, 3, &[[0.0, 0.0], [1000.0, 0.0]]),
            road(2, 4, &[[0.0, 0.0], [0.0, -1000.0]]),
            road(1, 4, &[[-1000.0, 0.0], [0.0, -1000.0]]),
        ]
    }

    fn joined(stretches: Vec<crate::roads::Stretch>, passes: &[(u32, u32)]) -> Graph {
        let stations = stretches
            .iter()
            .map(|s| [0.0, dist(s.points[0], s.points[1])])
            .collect();
        let joins = crate::roads::Joins {
            stations,
            one_way: vec![[false; 2]; stretches.len()],
            passes: passes
                .iter()
                .flat_map(|&(a, b)| [(a, b, 0.0), (b, a, 0.0)])
                .collect(),
        };
        let dry = &mut |_: f64, _: f64| false;
        crate::roads::build_joined(vec!["road".into()], stretches, Some(joins), dry).unwrap()
    }

    /// From the middle of the west road to the middle of the south road.
    fn west_to_south(t: &Terrain, g: &Graph) -> Option<f64> {
        let mut p = over(t, g, [-500.0, 0.0], [0.0, -500.0]);
        let a = p.search(&HashMap::new(), true).unwrap();
        let d = p.search(&HashMap::new(), false).unwrap();
        assert_eq!(a.is_some(), d.is_some());
        a.map(|f| f.length(g))
    }

    #[test]
    fn a_junction_with_a_forbidden_turn_routes_around_it() {
        let t = synth();
        let open = joined(fork(), &[(1, 2), (1, 4), (2, 4), (0, 6), (7, 5)]);
        assert!(close(west_to_south(&t, &open).unwrap(), 1000.0));
        // West to south is forbidden, west to east and east to south are not.
        let turn = joined(fork(), &[(1, 2), (2, 4), (0, 6), (7, 5)]);
        let round = 1000.0 + 1000.0 * 2f64.sqrt();
        assert!(close(west_to_south(&t, &turn).unwrap(), round));
    }

    #[test]
    fn a_route_never_turns_back_along_a_stretch_to_reach_a_forbidden_turn() {
        let t = synth();
        // With no bypass, only going out along the east road and back would
        // reach the south road, as a train cannot.
        let g = joined(fork()[..3].to_vec(), &[(1, 2), (2, 4)]);
        assert_eq!(west_to_south(&t, &g), None);
    }

    #[test]
    fn a_one_way_stretch_runs_one_way() {
        let t = synth();
        let mut joins = crate::roads::Joins {
            stations: vec![[0.0, 1000.0]; 2],
            one_way: vec![[false; 2]; 2],
            passes: vec![(1, 2, 0.0), (2, 1, 0.0)],
        };
        let two: Vec<_> = fork()[..2].to_vec();
        let dry = &mut |_: f64, _: f64| false;
        let build = |j: &crate::roads::Joins, dry: &mut dyn crate::roads::Water| {
            crate::roads::build_joined(vec!["road".into()], two.clone(), Some(j.clone()), dry)
                .unwrap()
        };
        let length = |g: &Graph, a: [f64; 2], b: [f64; 2]| {
            let found = over(&t, g, a, b).search(&HashMap::new(), true).unwrap();
            found.map(|f| f.length(g))
        };
        let (west, east) = ([-500.0, 0.0], [500.0, 0.0]);
        let g = build(&joins, dry);
        assert_eq!(g.edges.len(), 1, "they are contracted");
        assert!(close(length(&g, east, west).unwrap(), 1000.0));
        // No way from the east road's far end back to the junction.
        joins.one_way[1] = [false, true];
        let g = build(&joins, dry);
        assert!(close(length(&g, west, east).unwrap(), 1000.0));
        assert_eq!(length(&g, east, west), None);
        // A route of no length runs no way along it.
        assert_eq!(length(&g, east, east), Some(0.0));
    }

    #[test]
    fn a_line_past_its_connector_is_no_road() {
        // The west road runs 30 m past the connector to a road north, whose
        // own line starts 20 m back from it.
        use crate::decode::rn4::{self, Reach, build};
        let e = |a, b, points: Vec<[f64; 3]>| build::Edge {
            kind: rn4::KIND_ROAD,
            class: 0,
            a,
            b,
            points,
        };
        let edges = [
            e(1, 2, vec![[-1000.0, 5.0, 0.0], [30.0, 5.0, 0.0]]),
            e(2, 3, vec![[0.0, 5.0, -20.0], [0.0, 5.0, 1000.0]]),
        ];
        let bytes: Vec<Vec<u8>> = edges.iter().map(build::edge).collect();
        let connector = vec![[0.0, 5.0, 0.0], [0.0, 5.0, 10.0]];
        let file = build::file_of(&["road"], &edges, &bytes, &[(0, 1)], &[(0, 1, connector)]);
        let far = 1e6;
        let reach = Reach {
            min_x: -far,
            max_x: far,
            min_y: -100.0,
            max_y: 100.0,
            min_z: -far,
            max_z: far,
        };
        let net = rn4::parse(&file, "X.rn4", &reach).unwrap();
        let stretches: Vec<crate::roads::Stretch> = net
            .edges
            .iter()
            .map(|e| crate::roads::Stretch {
                kind: e.kind,
                class: e.class,
                a: e.a,
                b: e.b,
                points: e.points.iter().map(|p| [p[0], p[2]]).collect(),
            })
            .collect();
        let of = [Some(0), Some(1)];
        let joins = crate::roads::Joins::read(&net.table, &net.connectors, &of, &stretches);
        assert_eq!(joins.stations, [[0.0, 1000.0], [30.0, 1020.0]]);
        assert_eq!(joins.passes, [(1, 2, 10.0), (2, 1, 10.0)]);
        let dry = &mut |_: f64, _: f64| false;
        let g = crate::roads::build_joined(net.classes, stretches, Some(joins), dry).unwrap();
        let t = synth();
        let mut p = over(&t, &g, [-500.0, 0.0], [0.0, 500.0]);
        let found = p.search(&HashMap::new(), true).unwrap().unwrap();
        // 500 m to the connector, its 10 m, and 490 m on: nothing of the 30 m
        // past it, or of the 30 m behind it.
        assert!(close(found.length(&g), 1000.0), "{}", found.length(&g));
        assert!(close(found.cost, 1000.0));
        let route = p.describe(&found, 0.0).unwrap();
        assert_eq!(route.legs.len(), 2);
        assert!(close(route.legs[1].length, 500.0));
        assert_eq!((route.legs[1].from.x, route.legs[1].from.z), (0.0, 0.0));
        assert!(route.waypoints.iter().all(|w| w.x < 0.1));
    }

    #[test]
    fn samples_and_grades_measure_what_they_say() {
        let s = samples_along(&[[0.0, 0.0], [60.0, 0.0], [60.0, 10.0]]);
        assert_eq!(s, [[0.0, 0.0], [25.0, 0.0], [50.0, 0.0], [60.0, 10.0]]);
        let flat_then_steep = [(0.0, 0.0), (100.0, 0.0), (150.0, 10.0), (200.0, 10.0)];
        assert!(close(steepest(&flat_then_steep), 10.0));
        assert_eq!(steepest(&[(0.0, 0.0), (50.0, 5.0)]), 10.0);
        assert_eq!(climb(&[(0.0, 0.0), (1.0, 3.0), (2.0, 1.0)]), (3.0, 2.0));
    }
}

/// Opt-in: routes on every terrain of a real install, A* against Dijkstra
/// on random pairs. Set `DCSTERRAIN_INSTALL` to the install root, and
/// `DCSTERRAIN_ROUTES` to the pairs per terrain if not 100; without the
/// install this passes reading nothing, which is what CI does. Run it with
/// `--release` and `--nocapture` for the figures. The install is only read;
/// the cache is a scratch directory, so each terrain's road graph is built.
#[cfg(test)]
pub(crate) mod install_tests {
    use std::path::PathBuf;
    use std::time::Instant;

    use super::*;
    use crate::cache::Cache;
    use crate::decode::install::terrains;
    use crate::roads::tests::Unit;
    use crate::scratch::Scratch;

    pub(crate) fn percentile(v: &[f64], p: f64) -> f64 {
        if v.is_empty() {
            return f64::NAN;
        }
        let mut v = v.to_vec();
        v.sort_unstable_by(f64::total_cmp);
        v[((v.len() - 1) as f64 * p).round() as usize]
    }

    fn ms(t: Instant) -> f64 {
        t.elapsed().as_secs_f64() * 1000.0
    }

    /// `n` pairs of points within 300 m of road nodes, 2 to 80 km apart.
    fn pairs(g: &Graph, u: &mut Unit, n: usize) -> Vec<(Point, Point)> {
        let roads: Vec<[f64; 2]> = {
            let mut used = vec![false; g.nodes.len()];
            for e in g.edges.iter().filter(|e| e.kind == KIND_ROAD) {
                used[e.a as usize] = true;
                used[e.b as usize] = true;
            }
            (0..g.nodes.len())
                .filter(|&i| used[i])
                .map(|i| g.nodes[i])
                .collect()
        };
        let pick = |u: &mut Unit| {
            let p = roads[(u.next() * roads.len() as f64) as usize % roads.len()];
            Point {
                x: p[0] + (u.next() - 0.5) * 600.0,
                z: p[1] + (u.next() - 0.5) * 600.0,
            }
        };
        let mut out = Vec::new();
        while out.len() < n {
            let (a, b) = (pick(u), pick(u));
            let d = (a.x - b.x).hypot(a.z - b.z);
            if (2000.0..=80_000.0).contains(&d) {
                out.push((a, b));
            }
        }
        out
    }

    fn request(from: Point, to: Point) -> RouteRequest {
        RouteRequest {
            network: Network::Roads,
            from,
            to,
            avoid: Vec::new(),
            exposure_observers: Vec::new(),
            exposure_weight: 1.0,
            max_detour_factor: 3.0,
            geojson: false,
        }
    }

    #[test]
    fn a_star_equals_dijkstra_on_every_installed_terrain() {
        let Some(root) = std::env::var_os("DCSTERRAIN_INSTALL").map(PathBuf::from) else {
            eprintln!("DCSTERRAIN_INSTALL is not set; skipped");
            return;
        };
        // Five threads, so the runs beside this one keep their share.
        crate::threads::init(Some(5));
        let n: usize = std::env::var("DCSTERRAIN_ROUTES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(100);
        let build = std::fs::read_to_string(root.join("autoupdate.cfg")).unwrap_or_default();
        let version = build
            .lines()
            .find(|l| l.contains("\"version\""))
            .and_then(|l| l.split('"').nth(3))
            .unwrap_or("unknown")
            .to_string();
        println!("build {version}, {n} pairs per terrain");
        println!(
            "| terrain | road edges | graph s | pairs routed | A* p50 / p95 ms | Dijkstra p50 / p95 ms | settled A* / Dijkstra p50 | length p50 km | detour p50 / p95 | route op p50 / p95 ms | alternatives p50 ms | k p50 |"
        );
        println!("|---|---|---|---|---|---|---|---|---|---|---|---|");
        let scratch = Scratch::new();
        let cache = Cache::new(&scratch.0);
        let mut u = Unit(0x2545_f491_4f6c_dd1d);
        for listed in terrains(&root).unwrap() {
            let Some(id) = listed.id else { continue };
            let t = match Terrain::open(&root, &id, &cache, true) {
                Ok(t) => t,
                Err(e) => {
                    println!("| {id} | refused: {} |", e.0);
                    continue;
                }
            };
            let started = Instant::now();
            let g = t.source().road_graph().unwrap();
            let graph_s = started.elapsed().as_secs_f64();
            let road_edges = g.edges.iter().filter(|e| e.kind == KIND_ROAD).count();
            let (mut ta, mut td, mut sa, mut sd, mut len, mut detour) =
                (vec![], vec![], vec![], vec![], vec![], vec![]);
            let mut routed = 0;
            let mut op = vec![];
            let mut alt = vec![];
            let mut ks = vec![];
            for (i, (a, b)) in pairs(g, &mut u, n).into_iter().enumerate() {
                let q = request(a, b);
                let mut p = Planner::new(&t, &q).unwrap();
                let clock = Instant::now();
                let fa = p.search(&HashMap::new(), true).unwrap();
                ta.push(ms(clock));
                let clock = Instant::now();
                let fd = p.search(&HashMap::new(), false).unwrap();
                td.push(ms(clock));
                assert_eq!(fa.is_some(), fd.is_some(), "{id}: {a:?} {b:?}");
                let (Some(fa), Some(fd)) = (fa, fd) else {
                    continue;
                };
                assert!(
                    (fa.cost - fd.cost).abs() <= 1e-9 * fd.cost.max(1.0),
                    "{id}: {a:?} {b:?}: A* {} against Dijkstra {}",
                    fa.cost,
                    fd.cost
                );
                routed += 1;
                sa.push(fa.settled as f64);
                sd.push(fd.settled as f64);
                let l = fa.length(g);
                len.push(l / 1000.0);
                detour.push(l / (a.x - b.x).hypot(a.z - b.z));
                if i < 20 {
                    let req = serde_json::to_value(&q).unwrap();
                    let clock = Instant::now();
                    let v = crate::query::answer(&t, "route", &req).unwrap();
                    op.push(ms(clock));
                    assert!((v["length"].as_f64().unwrap() - l).abs() < 1e-6 * l.max(1.0));
                }
                if i < 5 {
                    let req = serde_json::to_value(RouteAlternativesRequest {
                        route: q.clone(),
                        k: 3,
                        max_stretch: 1.5,
                        max_overlap: 0.5,
                        penalty: 1.6,
                    })
                    .unwrap();
                    let clock = Instant::now();
                    let v = crate::query::answer(&t, "route_alternatives", &req).unwrap();
                    alt.push(ms(clock));
                    ks.push(v["routes"].as_array().unwrap().len() as f64);
                }
            }
            println!(
                "| {id} | {road_edges} | {graph_s:.1} | {routed} of {n} | {:.2} / {:.2} | {:.2} / {:.2} | {:.0} / {:.0} | {:.1} | {:.2} / {:.2} | {:.1} / {:.1} | {:.0} | {:.0} |",
                percentile(&ta, 0.5),
                percentile(&ta, 0.95),
                percentile(&td, 0.5),
                percentile(&td, 0.95),
                percentile(&sa, 0.5),
                percentile(&sd, 0.5),
                percentile(&len, 0.5),
                percentile(&detour, 0.5),
                percentile(&detour, 0.95),
                percentile(&op, 0.5),
                percentile(&op, 0.95),
                percentile(&alt, 0.5),
                percentile(&ks, 0.5),
            );
        }
    }
}
