//! `trafficability`: whether a vehicle can drive across country from a point
//! to a network, by the least-cost way over the cells round it, and the
//! straight line to the nearest road beside it.

use serde_json::json;

use super::geojson::{Features, props};
use super::route::{climb, kind_of, samples_along};
use super::{Terrain, refused};
use crate::grid::Grid;
use crate::layers::Cells;
use crate::offroad::way::{Blocks, Ground, RoadCells, Stop, Way, found, search, straighten};
use crate::offroad::{Block, CostField};
use crate::roads::Graph;
use crate::source::{Refusal, Source};
use crate::types::{
    Issue, OffroadWay, Point, Straight, TrafficabilityRequest, TrafficabilityResponse,
};

/// The steepest limit a request may set: past it the cost model's slope
/// factor says nothing a vehicle could use.
const MAX_SLOPE_LIMIT_DEG: f64 = 60.0;
/// The farthest a way is searched: past it the nearest road is no way out on
/// wheels, and the cells within it are the most the engine's memory holds
/// beside the rest of its work.
pub const MAX_DISTANCE_M: f64 = 20_000.0;
/// The first search reaches this far past twice the nearest road's
/// distance, so a near road costs a small search.
const FIRST_REACH_MARGIN_M: f64 = 1000.0;

fn water_name(w: i32) -> &'static str {
    match w {
        1 => "a lake",
        2 => "the sea",
        3 => "a river",
        _ => "water",
    }
}

/// What blocks, in words, where it does: `a river 120 m along the line`.
fn block_words(b: Block, max_slope_deg: f64, place: &str) -> String {
    match b {
        Block::NoGround => format!("no ground {place}"),
        Block::Water(w) => format!("{} {place}", water_name(w)),
        Block::Cover(_) => format!("cover no vehicle crosses {place}"),
        Block::Steep(d) => {
            format!("a slope of {d:.1}°, over the {max_slope_deg}° allowed, {place}")
        }
    }
}

/// `a, b and c`.
fn listed(words: &[String]) -> String {
    match words {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

fn point(p: [f64; 2]) -> Point {
    Point { x: p[0], z: p[1] }
}

/// The ground of the cells covering a box, and the network's cells in it.
fn ground_over(
    src: &(impl Source + ?Sized),
    g: &Graph,
    kind: u8,
    grid: &Grid,
    bbox: [f64; 4],
    max_slope_deg: f64,
) -> Result<(CostField, RoadCells), Refusal> {
    let field = CostField::new(src, Cells::covering(grid, bbox), max_slope_deg)?;
    let roads = RoadCells::new(g, kind, grid, &field);
    Ok((field, roads))
}

/// The straight line from `p` to `road`, judged over the cells it crosses,
/// a road's own cells never blocking: its cost, or where and why it is
/// blocked.
fn straight_line(
    src: &(impl Source + ?Sized),
    g: &Graph,
    kind: u8,
    p: [f64; 2],
    road: [f64; 2],
    max_slope_deg: f64,
) -> Result<Straight, Refusal> {
    let grid = src.grid();
    let bbox = [
        p[0].min(road[0]),
        p[1].min(road[1]),
        p[0].max(road[0]),
        p[1].max(road[1]),
    ];
    let (field, roads) = ground_over(src, g, kind, &grid, bbox, max_slope_deg)?;
    let ground = Ground {
        grid: &grid,
        field: &field,
        roads: &roads,
    };
    let distance = (road[0] - p[0]).hypot(road[1] - p[1]);
    Ok(match ground.line(p, road, None) {
        Ok(cost) => Straight {
            cost: Some(cost),
            blocking: None,
            reason: format!(
                "passable: no slope over {max_slope_deg}° and no water on the {distance:.0} m \
                 to the road"
            ),
        },
        Err((stop, t)) => {
            let b = match stop {
                Stop::Blocked(b) => b,
                Stop::Road | Stop::Outside => Block::NoGround,
            };
            let at = [p[0] + t * (road[0] - p[0]), p[1] + t * (road[1] - p[1])];
            let along = format!("{:.0} m along the line", t * distance);
            Straight {
                cost: None,
                blocking: Some(point(at)),
                reason: block_words(b, max_slope_deg, &along),
            }
        }
    })
}

/// What the search came to.
pub(super) enum Searched {
    /// A way, and the edge it meets.
    Found(Way, usize),
    /// The point itself cannot be crossed.
    StandsOn(Block),
    /// Every way within reach is blocked.
    Blocked(Blocks),
}

/// The search, with the work it did: the most any cell it settled cost,
/// over every round, the cost of the road's cell it ended at, and how far
/// it reached.
pub(super) struct Search {
    pub outcome: Searched,
    #[cfg_attr(not(test), expect(dead_code, reason = "a test reads the work done"))]
    pub most: f64,
    #[cfg_attr(not(test), expect(dead_code, reason = "a test reads the work done"))]
    pub goal: Option<f64>,
    #[cfg_attr(not(test), expect(dead_code, reason = "a test reads the work done"))]
    pub reach: f64,
}

/// The least-cost way from `p` to the network within `max_distance`,
/// searched over a disk that doubles until the way found costs no more than
/// its radius, so no way leaving the disk, which costs at least its radius,
/// could cost less.
pub(super) fn search_way(
    src: &(impl Source + ?Sized),
    g: &Graph,
    kind: u8,
    p: [f64; 2],
    distance: f64,
    max_slope_deg: f64,
    max_distance: f64,
) -> Result<Search, Refusal> {
    let grid = src.grid();
    // The point's own cell first, so a point that cannot be driven from
    // reads no more.
    let (field, roads) = ground_over(src, g, kind, &grid, [p[0], p[1], p[0], p[1]], max_slope_deg)?;
    let ground = Ground {
        grid: &grid,
        field: &field,
        roads: &roads,
    };
    let own = ground.cell_of(p).map(|i| ground.passage(i));
    if let None | Some(Err(_)) = own {
        return Ok(Search {
            outcome: Searched::StandsOn(own.and_then(Result::err).unwrap_or(Block::NoGround)),
            most: 0.0,
            goal: None,
            reach: 0.0,
        });
    }
    let mut reach = (2.0 * distance + FIRST_REACH_MARGIN_M).min(max_distance);
    let mut most = 0.0f64;
    loop {
        let bbox = [p[0] - reach, p[1] - reach, p[0] + reach, p[1] + reach];
        let (field, roads) = ground_over(src, g, kind, &grid, bbox, max_slope_deg)?;
        let ground = Ground {
            grid: &grid,
            field: &field,
            roads: &roads,
        };
        let done = |outcome| Search {
            outcome,
            most,
            goal: None,
            reach,
        };
        let Some(start) = ground.cell_of(p) else {
            return Ok(done(Searched::StandsOn(Block::NoGround)));
        };
        let spent = match ground.passage(start) {
            Ok(c) => {
                c * {
                    let center = ground.center(start);
                    (center[0] - p[0]).hypot(center[1] - p[1])
                }
            }
            Err(b) => return Ok(done(Searched::StandsOn(b))),
        };
        // The point's own cell is always searched, however short the reach.
        let inside = |i: usize| {
            let c = ground.center(i);
            i == start || (c[0] - p[0]).hypot(c[1] - p[1]) <= reach
        };
        let s = search(&ground, &[(start, spent)], &inside, true);
        most = most.max(s.most);
        let last = reach >= max_distance;
        if let Some(&(goal, cost)) = s.goals.first() {
            if cost <= reach || last {
                let meet = ground.meets(g, goal);
                let way = found(&ground, &s, p, goal, meet).ok_or_else(|| {
                    Refusal("the way the search found could not be judged again".into())
                })?;
                let edge = roads.edge[goal] as usize;
                return Ok(Search {
                    outcome: Searched::Found(straighten(&ground, &way), edge),
                    most,
                    goal: Some(cost),
                    reach,
                });
            }
        } else if !s.blocks.reach || last {
            let mut blocks = s.blocks;
            blocks.reach &= last;
            return Ok(Search {
                outcome: Searched::Blocked(blocks),
                most,
                goal: None,
                reach,
            });
        }
        reach = (2.0 * reach).min(max_distance);
    }
}

pub fn trafficability(
    terrain: &Terrain,
    q: TrafficabilityRequest,
) -> Result<TrafficabilityResponse, Vec<Issue>> {
    if !(q.x.is_finite() && q.z.is_finite()) {
        let path = if q.x.is_finite() { "z" } else { "x" };
        return Err(vec![Issue::new("invalid_point", "must be finite", path)]);
    }
    if !(q.max_slope_deg > 0.0 && q.max_slope_deg <= MAX_SLOPE_LIMIT_DEG) {
        return Err(vec![Issue::new(
            "invalid_request",
            &format!("must be more than 0 and at most {MAX_SLOPE_LIMIT_DEG}"),
            "max_slope_deg",
        )]);
    }
    if !(q.max_distance > 0.0 && q.max_distance <= MAX_DISTANCE_M) {
        return Err(vec![Issue::new(
            "invalid_request",
            &format!("must be more than 0 and at most {MAX_DISTANCE_M}"),
            "max_distance",
        )]);
    }
    let src = terrain.source();
    let g = src.road_graph().map_err(refused)?;
    let kind = kind_of(q.network);
    let p = [q.x, q.z];
    let Some(&(nearest, road)) = g.nearest(kind, p, 1).first() else {
        return Err(vec![Issue::new(
            "no_network",
            "this terrain has no edge of that network",
            "network",
        )]);
    };
    let distance = (road[0] - p[0]).hypot(road[1] - p[1]);
    if distance > q.max_distance {
        return Err(vec![Issue::new(
            "too_far",
            &format!(
                "the nearest road is {distance:.0} m away, past the max_distance of {} m",
                q.max_distance
            ),
            "max_distance",
        )]);
    }
    let straight = straight_line(src, g, kind, p, road, q.max_slope_deg).map_err(refused)?;
    let searched =
        search_way(src, g, kind, p, distance, q.max_slope_deg, q.max_distance).map_err(refused)?;
    // The straight line is a way too: where it passes and costs less than
    // the way searched over the cells' centers, it is the way.
    let line = straight.cost.map(|cost| {
        (
            Way {
                path: vec![p, road],
                cost,
                end: 0,
            },
            nearest,
        )
    });
    let (way, reason) = match (searched.outcome, line) {
        (Searched::Found(w, _), Some(l)) if l.0.cost < w.cost => (
            Some(l),
            format!("the straight line to the road is {}", straight.reason),
        ),
        (Searched::Found(w, e), _) => {
            let why = if straight.cost.is_some() {
                "for less than the straight line costs"
            } else {
                "round what blocks the straight line"
            };
            let reason = format!("a way of {:.0} m reaches the road {why}", w.length());
            (Some((w, e)), reason)
        }
        (_, Some((l, n))) => (
            Some((l, n)),
            format!("the straight line to the road is {}", straight.reason),
        ),
        (Searched::StandsOn(b), None) => (
            None,
            format!(
                "the point cannot be driven from: {}",
                block_words(b, q.max_slope_deg, "where it stands")
            ),
        ),
        (Searched::Blocked(blocks), None) => {
            let words = blocks.words(q.max_slope_deg, q.max_distance);
            (
                None,
                if words.is_empty() {
                    format!("no way to the road within {} m", q.max_distance)
                } else {
                    format!(
                        "no way to the road within {} m: {} block it",
                        q.max_distance,
                        listed(&words)
                    )
                },
            )
        }
    };
    let way = match way {
        Some((w, edge)) => {
            let samples = samples_along(&w.path);
            let ground = src.points(&samples).map_err(refused)?;
            let mut profile = Vec::new();
            let mut along = 0.0;
            for (i, s) in samples.iter().enumerate() {
                if i > 0 {
                    along += (s[0] - samples[i - 1][0]).hypot(s[1] - samples[i - 1][1]);
                }
                if let Some(gr) = ground[i] {
                    profile.push((along, gr.height));
                }
            }
            let (ascent, descent) = climb(&profile);
            let meets = *w.path.last().expect("a way has its two ends");
            Some(OffroadWay {
                length: w.length(),
                path: w.path.iter().copied().map(point).collect(),
                cost: w.cost,
                ascent,
                descent,
                meets: point(meets),
                edge_id: edge as i64,
            })
        }
        None => None,
    };
    let reachable = way.is_some();
    let geojson = if q.geojson {
        let mut f = Features::new(terrain)?;
        if let Some(w) = &way {
            let path: Vec<[f64; 2]> = w.path.iter().map(|p| [p.x, p.z]).collect();
            f.line(
                &path,
                props([
                    ("way", json!(true)),
                    ("length", json!(w.length)),
                    ("cost", json!(w.cost)),
                    ("edge_id", json!(w.edge_id)),
                ]),
            );
            f.point([w.meets.x, w.meets.z], props([("end", json!("meets"))]));
        }
        f.line(
            &[p, road],
            props([
                ("straight", json!(true)),
                ("distance", json!(distance)),
                ("cost", json!(straight.cost)),
            ]),
        );
        f.point(p, props([("end", json!("from"))]));
        f.point(road, props([("end", json!("nearest_road"))]));
        if let Some(b) = &straight.blocking {
            f.point([b.x, b.z], props([("blocking", json!(true))]));
        }
        Some(f.finish())
    } else {
        None
    };
    Ok(TrafficabilityResponse {
        provenance: terrain.provenance(),
        reachable,
        nearest_road: point(road),
        distance,
        way,
        straight,
        reason,
        geojson,
    })
}

#[cfg(test)]
mod tests;
