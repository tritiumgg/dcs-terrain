//! The least-cost way over the cells from one or more starts to a network.
//!
//! The cells are a graph: each joins its sixteen neighbors, the eight around
//! it and the eight a knight's move away, by the straight line between their
//! centers. A move costs, for each cell that line crosses, the meters it
//! runs in the cell times the cell's cost per meter; a move that touches a
//! cell that cannot be crossed, even only at a corner, is not made. Sixteen
//! neighbors keep a way within about three percent of the straight line's
//! length on open ground, where eight leave it up to eight.
//!
//! A cell a road of the network runs through is a goal. The search reaches
//! it but never leaves it, so a way ends at the first road it meets; and a
//! move may cross a road's cell only to end in it. A start in a road's cell
//! is left, when the search goes on past its first goal, so a point beside a
//! piece of road joined to nothing can reach another; and a search may cross
//! a road's cell whose own ground can be crossed, reaching it as a goal and
//! going on past it. A road's cell is crossed
//! at the cost of its ground, or at one per meter where its ground cannot
//! be crossed, as on a bridge.
//!
//! The search is Dijkstra's algorithm from every start at once, each start
//! carrying a cost already spent. Every cell costs at least one per meter,
//! so a way's cost is never less than its length: a search confined to a
//! disk of radius `r` round a start has found the least-cost way overall
//! once that way costs no more than `r`.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use super::{Block, CostField, cross};
use crate::grid::Grid;
use crate::roads::{Graph, closest_on};

/// No edge: a cell no road runs through.
pub const NO_EDGE: u32 = u32::MAX;

/// The edge of the network that runs through each cell of a field, or
/// `NO_EDGE`; of several, the one of lowest index.
pub struct RoadCells {
    pub edge: Vec<u32>,
}

impl RoadCells {
    /// The cells of `field` the edges of `kind` run through, each for some
    /// length.
    pub fn new(g: &Graph, kind: u8, grid: &Grid, field: &CostField) -> RoadCells {
        let mut edge = vec![NO_EDGE; field.values.len()];
        let cs = f64::from(grid.cell_size);
        let c = field.cells;
        let bounds = [
            grid.origin_x + c.row0 as f64 * cs,
            grid.origin_z + c.col0 as f64 * cs,
            grid.origin_x + (c.row0 + i64::from(c.rows)) as f64 * cs,
            grid.origin_z + (c.col0 + i64::from(c.cols)) as f64 * cs,
        ];
        g.edges_near(kind, bounds, &mut |e| {
            let path: Vec<[f64; 2]> = g.path(&g.edges[e]).collect();
            for s in path.windows(2) {
                cross(grid, s[0], s[1], &mut |r, k, t0, t1| {
                    if t1 > t0
                        && let Some(i) = field.index(r, k)
                    {
                        edge[i] = edge[i].min(e as u32);
                    }
                    true
                });
            }
        });
        RoadCells { edge }
    }

    pub fn is_road(&self, i: usize) -> bool {
        self.edge[i] != NO_EDGE
    }
}

/// The ground a search runs over: each cell's cost, and the network's cells,
/// and each cell's cost multiplied by a factor of its own where `scale` is
/// given, at least one, so a way never costs less than its length.
///
/// A road's cell is a wall to a way, which may only end in it, unless
/// `cross` lets it cross the cell's own ground.
pub struct Ground<'a> {
    pub grid: &'a Grid,
    pub field: &'a CostField,
    pub roads: &'a RoadCells,
    pub scale: Option<&'a [f32]>,
    /// Whether a way may cross a road's cell whose own ground can be crossed,
    /// rather than only end in it. A road's cell over water, a bridge's, is
    /// never crossed: a way would cross the river beside the bridge.
    pub cross: bool,
}

/// Where a way may not go: water of each class, slopes, no ground, cover,
/// and the edge of the search's reach.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Blocks {
    pub lake: bool,
    pub sea: bool,
    pub river: bool,
    pub other_water: bool,
    pub steep: bool,
    pub no_ground: bool,
    pub cover: bool,
    /// The search reached the edge of its disk.
    pub reach: bool,
}

impl Blocks {
    pub fn add(&mut self, b: Block) {
        match b {
            Block::Water(1) => self.lake = true,
            Block::Water(2) => self.sea = true,
            Block::Water(3) => self.river = true,
            Block::Water(_) => self.other_water = true,
            Block::Steep(_) => self.steep = true,
            Block::NoGround => self.no_ground = true,
            Block::Cover(_) => self.cover = true,
        }
    }

    /// What blocked, in words, water first and the reach last: `a river`,
    /// `slopes over 20°`, `the reach of 5000 m`.
    pub fn words(&self, max_slope_deg: f64, reach_m: f64) -> Vec<String> {
        [
            (self.river, "a river".to_string()),
            (self.lake, "a lake".to_string()),
            (self.sea, "the sea".to_string()),
            (self.other_water, "water".to_string()),
            (self.steep, format!("slopes over {max_slope_deg}°")),
            (self.cover, "cover no vehicle crosses".to_string()),
            (self.no_ground, "the edge of the terrain".to_string()),
            (self.reach, format!("the reach of {reach_m:.0} m")),
        ]
        .into_iter()
        .filter_map(|(b, w)| b.then_some(w))
        .collect()
    }
}

/// The sixteen moves, as rows and columns: the four sides, the four
/// corners, then the eight knight's moves.
pub const MOVES: [(i64, i64); 16] = [
    (0, 1),
    (1, 0),
    (0, -1),
    (-1, 0),
    (1, 1),
    (1, -1),
    (-1, 1),
    (-1, -1),
    (1, 2),
    (2, 1),
    (-1, 2),
    (-2, 1),
    (1, -2),
    (2, -1),
    (-1, -2),
    (-2, -1),
];

/// One cell a move crosses: its row and column from the start, and the
/// meters the move runs in it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Crossing {
    pub dr: i64,
    pub dc: i64,
    pub meters: f64,
}

/// The cells each move crosses, the start's half first and the end's half
/// last, with every cell a move only touches at a corner.
pub fn crossings(cell_size: u32) -> Vec<Vec<Crossing>> {
    let cs = f64::from(cell_size);
    let unit = Grid {
        cell_size,
        origin_x: 0.0,
        origin_z: 0.0,
        height: u32::MAX,
        width: u32::MAX,
        tile_size: 1,
    };
    MOVES
        .iter()
        .map(|&(dr, dc)| {
            let a = [cs / 2.0, cs / 2.0];
            let b = [a[0] + dr as f64 * cs, a[1] + dc as f64 * cs];
            let length = (dr as f64).hypot(dc as f64) * cs;
            let mut out = Vec::new();
            cross(&unit, a, b, &mut |r, c, t0, t1| {
                out.push(Crossing {
                    dr: r,
                    dc: c,
                    meters: (t1 - t0) * length,
                });
                true
            });
            out
        })
        .collect()
}

/// Why a move or a line was not made.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Stop {
    /// A cell it touches cannot be crossed.
    Blocked(Block),
    /// It runs through a road's cell other than the one it ends in.
    Road,
    /// It leaves the field.
    Outside,
}

impl Ground<'_> {
    /// Whether cell `i` is a road's that a way may end in but not cross.
    pub fn wall(&self, i: usize) -> bool {
        self.roads.is_road(i) && !(self.cross && self.field.values[i].is_ok())
    }

    /// The cost per meter of crossing cell `i`.
    pub fn passage(&self, i: usize) -> Result<f64, Block> {
        let base = match self.field.values[i] {
            Ok(c) => f64::from(c),
            Err(_) if self.roads.is_road(i) => 1.0,
            Err(b) => return Err(b),
        };
        Ok(self.scale.map_or(base, |s| base * f64::from(s[i])))
    }

    /// The cost of the move `m` out of cell `i`, and the cell it ends in.
    fn step(&self, moves: &[Vec<Crossing>], i: usize, m: usize) -> Result<(usize, f64), Stop> {
        let (r, c) = self.field.row_col(i);
        let cells = &moves[m];
        let mut cost = 0.0;
        let mut end = i;
        for (k, x) in cells.iter().enumerate() {
            let j = self.field.index(r + x.dr, c + x.dc).ok_or(Stop::Outside)?;
            let last = k + 1 == cells.len();
            if !last && j != i && x.meters > 0.0 && self.wall(j) {
                return Err(Stop::Road);
            }
            cost += x.meters * self.passage(j).map_err(Stop::Blocked)?;
            end = j;
        }
        Ok((end, cost))
    }

    /// The cost of the straight line from `a` to `b`, or why it cannot be
    /// driven and how far along it that is, as a share. With `end`, it may
    /// run through no road's cell but that one and the one it starts in, as a
    /// move may not; without it, roads' cells are crossed like any other.
    pub fn line(&self, a: [f64; 2], b: [f64; 2], end: Option<usize>) -> Result<f64, (Stop, f64)> {
        let length = (b[0] - a[0]).hypot(b[1] - a[1]);
        let mut cost = 0.0;
        let mut stop = None;
        cross(self.grid, a, b, &mut |r, c, t0, t1| {
            let Some(j) = self.field.index(r, c) else {
                stop = Some((Stop::Outside, t0));
                return false;
            };
            if let Some(end) = end
                && j != end
                && t1 > t0
                && t0 > 0.0
                && self.wall(j)
            {
                stop = Some((Stop::Road, t0));
                return false;
            }
            match self.passage(j) {
                Ok(p) => {
                    cost += (t1 - t0) * length * p;
                    true
                }
                Err(b) => {
                    stop = Some((Stop::Blocked(b), t0));
                    false
                }
            }
        });
        match stop {
            Some(s) => Err(s),
            None => Ok(cost),
        }
    }

    /// The center of cell `i`, `[x, z]`.
    pub fn center(&self, i: usize) -> [f64; 2] {
        let (r, c) = self.field.row_col(i);
        let cs = f64::from(self.grid.cell_size);
        [
            self.grid.origin_x + (r as f64 + 0.5) * cs,
            self.grid.origin_z + (c as f64 + 0.5) * cs,
        ]
    }

    /// The cell holding `p`, or `None` outside the field.
    pub fn cell_of(&self, p: [f64; 2]) -> Option<usize> {
        let cs = f64::from(self.grid.cell_size);
        let r = ((p[0] - self.grid.origin_x) / cs).floor();
        let c = ((p[1] - self.grid.origin_z) / cs).floor();
        if !(r.is_finite() && c.is_finite()) {
            return None;
        }
        self.field.index(r as i64, c as i64)
    }

    /// Where the road through cell `i` runs closest to the cell's center,
    /// within the cell: the point a way that ends there meets it.
    pub fn meets(&self, g: &Graph, i: usize) -> [f64; 2] {
        let e = &g.edges[self.roads.edge[i] as usize];
        let center = self.center(i);
        let (row, col) = self.field.row_col(i);
        let path: Vec<[f64; 2]> = g.path(e).collect();
        let mut best = (f64::INFINITY, center);
        for s in path.windows(2) {
            cross(self.grid, s[0], s[1], &mut |r, c, t0, t1| {
                if (r, c) == (row, col) && t1 > t0 {
                    let at = |t: f64| {
                        [
                            s[0][0] + t * (s[1][0] - s[0][0]),
                            s[0][1] + t * (s[1][1] - s[0][1]),
                        ]
                    };
                    let q = closest_on(at(t0), at(t1), center);
                    let d = (q[0] - center[0]).hypot(q[1] - center[1]);
                    if d < best.0 {
                        best = (d, q);
                    }
                }
                true
            });
        }
        self.inward(best.1, i)
    }

    /// `q`, a point of cell `i` or of its edge, moved toward the cell's
    /// center by a micrometer or a millimeter, until it rounds into the
    /// cell: a road that only clips the cell's corner is closest to its
    /// center on the cell's edge, where rounding can put the point in the
    /// next cell, and a way judged to it would run through that one.
    pub fn inward(&self, q: [f64; 2], i: usize) -> [f64; 2] {
        let center = self.center(i);
        let d = (center[0] - q[0]).hypot(center[1] - q[1]);
        for step in [0.0, 1e-6, 1e-3] {
            let f = if d > 0.0 { (step / d).min(1.0) } else { 0.0 };
            let p = [q[0] + f * (center[0] - q[0]), q[1] + f * (center[1] - q[1])];
            if self.cell_of(p) == Some(i) {
                return p;
            }
        }
        center
    }
}

/// No move: a start, or a cell not reached.
const START: u8 = 16;
const UNREACHED: u8 = u8::MAX;

/// What a search found.
#[derive(Default)]
pub struct Searched {
    /// The least cost to each cell reached, infinite elsewhere.
    pub cost: Vec<f64>,
    /// The move that reached each cell, `START` at a start.
    from: Vec<u8>,
    /// Each road's cell reached, with its cost, in order of cost.
    pub goals: Vec<(usize, f64)>,
    /// How many cells were settled, and the most any of them cost.
    pub settled: usize,
    pub most: f64,
    /// What stopped a move out of a cell reached.
    pub blocks: Blocks,
}

impl Searched {
    /// The cells of the least-cost way to cell `i`, from its start.
    pub fn cells_to(&self, i: usize, field: &CostField) -> Vec<usize> {
        let mut out = vec![i];
        let mut at = i;
        while self.from[at] != START {
            let (dr, dc) = MOVES[self.from[at] as usize];
            let (r, c) = field.row_col(at);
            at = field
                .index(r - dr, c - dc)
                .expect("a way runs inside its field");
            out.push(at);
        }
        out.reverse();
        out
    }
}

/// Searches the cells of `ground` from `starts`, each a cell and the cost
/// already spent to reach it, through the cells `inside` admits, and stops
/// at the first road's cell when `first` is set. A start that cannot be
/// crossed is left out; a start in a road's cell is a goal, and is left
/// when the search goes on.
pub fn search(
    ground: &Ground,
    starts: &[(usize, f64)],
    inside: &dyn Fn(usize) -> bool,
    first: bool,
) -> Searched {
    let n = ground.field.values.len();
    assert!(
        n < u32::MAX as usize,
        "{n} cells are more than a search holds"
    );
    let moves = crossings(ground.grid.cell_size);
    let mut cost = vec![f64::INFINITY; n];
    let mut from = vec![UNREACHED; n];
    let mut heap = BinaryHeap::new();
    // A non-negative f64's bits order as the value does; ties go to the
    // lower cell, so the same ground gives the same way every run.
    let key = |g: f64, v: usize| Reverse((g.to_bits(), v as u32));
    let mut blocks = Blocks::default();
    for &(s, g) in starts {
        match ground.passage(s) {
            Ok(_) if inside(s) && g < cost[s] => {
                cost[s] = g;
                from[s] = START;
                heap.push(key(g, s));
            }
            Ok(_) => {}
            Err(b) => blocks.add(b),
        }
    }
    let mut settled = vec![false; n];
    let (mut count, mut most) = (0, 0.0f64);
    let mut goals = Vec::new();
    while let Some(Reverse((g, v))) = heap.pop() {
        let v = v as usize;
        if settled[v] {
            continue;
        }
        settled[v] = true;
        let g = f64::from_bits(g);
        count += 1;
        most = most.max(g);
        if ground.roads.is_road(v) {
            goals.push((v, g));
            if first {
                break;
            }
            if from[v] != START && ground.wall(v) {
                continue;
            }
        }
        for m in 0..MOVES.len() {
            match ground.step(&moves, v, m) {
                Ok((w, c)) => {
                    if !inside(w) {
                        blocks.reach = true;
                        continue;
                    }
                    let ng = g + c;
                    if !settled[w] && ng < cost[w] {
                        cost[w] = ng;
                        from[w] = m as u8;
                        heap.push(key(ng, w));
                    }
                }
                Err(Stop::Blocked(b)) => blocks.add(b),
                Err(Stop::Outside) => blocks.reach = true,
                Err(Stop::Road) => {}
            }
        }
    }
    Searched {
        cost,
        from,
        goals,
        settled: count,
        most,
        blocks,
    }
}

/// A way off the road: its turns from the start to where it meets the road,
/// its cost, and the cell it ends in.
#[derive(Debug, Clone, PartialEq)]
pub struct Way {
    pub path: Vec<[f64; 2]>,
    pub cost: f64,
    pub end: usize,
}

impl Way {
    pub fn length(&self) -> f64 {
        self.path
            .windows(2)
            .map(|s| (s[1][0] - s[0][0]).hypot(s[1][1] - s[0][1]))
            .sum()
    }
}

/// The way the search found to the road's cell `goal`, from `start` through
/// the cells' centers and on to `meet`, a point of that cell where it meets
/// the road, judged over the cells it crosses; `None` if a piece of it
/// cannot be driven, which a search's own moves never give.
pub fn found(
    ground: &Ground,
    s: &Searched,
    start: [f64; 2],
    goal: usize,
    meet: [f64; 2],
) -> Option<Way> {
    let cells = s.cells_to(goal, ground.field);
    let mut path = vec![start];
    let mut ends = vec![cells[0]];
    for &c in &cells {
        path.push(ground.center(c));
        ends.push(c);
    }
    path.push(meet);
    ends.push(goal);
    let mut cost = 0.0;
    for (k, w) in path.windows(2).enumerate() {
        cost += ground.line(w[0], w[1], Some(ends[k + 1])).ok()?;
    }
    Some(Way {
        path,
        cost,
        end: goal,
    })
}

/// `way` straightened: from each turn, on to the farthest turn after it
/// that a straight line reaches for no more than the way between them
/// costs, judged over the cells it crosses, stopping at the first that
/// cannot. The turns' cells are where each piece of the way ends.
pub fn straighten(ground: &Ground, way: &Way) -> Way {
    straighten_where(ground, way, &|_, _| true)
}

/// `way` straightened as `straighten` does, taking only the straight lines
/// `allowed` lets it.
pub fn straighten_where(
    ground: &Ground,
    way: &Way,
    allowed: &dyn Fn([f64; 2], [f64; 2]) -> bool,
) -> Way {
    let path = &way.path;
    let n = path.len();
    if n < 3 {
        return way.clone();
    }
    // The cell each turn lies in, the way's last being its road's cell.
    let ends: Vec<usize> = (0..n)
        .map(|k| {
            if k + 1 == n {
                way.end
            } else {
                ground.cell_of(path[k]).unwrap_or(way.end)
            }
        })
        .collect();
    let mut spent = vec![0.0];
    for (k, w) in path.windows(2).enumerate() {
        let c = ground
            .line(w[0], w[1], Some(ends[k + 1]))
            .unwrap_or(f64::INFINITY);
        spent.push(spent[k] + c);
    }
    let mut out = vec![path[0]];
    let mut cost = 0.0;
    let mut i = 0;
    while i + 1 < n {
        let mut best = (i + 1, spent[i + 1] - spent[i]);
        for j in i + 2..n {
            let between = spent[j] - spent[i];
            match ground.line(path[i], path[j], Some(ends[j])) {
                Ok(c) if c <= between * (1.0 + 1e-12) + 1e-9 && allowed(path[i], path[j]) => {
                    best = (j, c)
                }
                _ => break,
            }
        }
        out.push(path[best.0]);
        cost += best.1;
        i = best.0;
    }
    if cost > way.cost {
        return way.clone();
    }
    Way {
        path: out,
        cost,
        end: way.end,
    }
}

#[cfg(test)]
mod tests;
