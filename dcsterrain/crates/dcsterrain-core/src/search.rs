//! Least-cost paths over any graph a caller describes by its neighbors: the
//! road network, or a grid of cells.
//!
//! The search is A* (Hart, Nilsson and Raphael, 1968) with a closed set.
//! Given a heuristic that is consistent, never more than an edge's cost plus
//! the heuristic at its far end, and zero at the goal, the first time the
//! goal is settled its cost is the least of any path: exact, up to the
//! rounding of the sums. With a heuristic of zero it is Dijkstra's
//! algorithm. Straight-line distance times the least cost per meter any edge
//! has is consistent on any graph whose edge costs are at least the distance
//! between their ends times that factor.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

/// A least-cost path.
#[derive(Debug, Clone, PartialEq)]
pub struct Path {
    pub cost: f64,
    /// From the start to the goal, both included.
    pub nodes: Vec<usize>,
    /// How many nodes the search settled: its work.
    pub settled: usize,
}

const NONE: u32 = u32::MAX;

/// The least-cost path from `start` to `goal` over nodes `0..n`, or `None`
/// when the goal cannot be reached. `neighbors(v, f)` calls `f(w, cost)` for
/// each edge out of `v`, every cost finite and not negative; `heuristic(v)`
/// estimates the cost from `v` to the goal, and must be consistent for the
/// answer to be the least. Ties go to the node settled first, which the
/// heap orders by estimate, then by cost so far, then by node, so the same
/// graph gives the same path every run.
pub fn astar(
    n: usize,
    start: usize,
    goal: usize,
    mut neighbors: impl FnMut(usize, &mut dyn FnMut(usize, f64)),
    heuristic: impl Fn(usize) -> f64,
) -> Option<Path> {
    assert!(n < NONE as usize, "{n} nodes are more than a search holds");
    let mut cost = vec![f64::INFINITY; n];
    let mut parent = vec![NONE; n];
    let mut settled = vec![false; n];
    // A non-negative f64's bits order as the value does.
    let key = |f: f64, g: f64, v: usize| Reverse((f.to_bits(), g.to_bits(), v as u32));
    let mut heap = BinaryHeap::new();
    cost[start] = 0.0;
    heap.push(key(heuristic(start).max(0.0), 0.0, start));
    let mut count = 0;
    while let Some(Reverse((_, g, v))) = heap.pop() {
        let v = v as usize;
        if settled[v] {
            continue;
        }
        settled[v] = true;
        count += 1;
        let g = f64::from_bits(g);
        if v == goal {
            let mut nodes = vec![v];
            while parent[*nodes.last().unwrap()] != NONE {
                nodes.push(parent[*nodes.last().unwrap()] as usize);
            }
            nodes.reverse();
            return Some(Path {
                cost: g,
                nodes,
                settled: count,
            });
        }
        neighbors(v, &mut |w, c| {
            debug_assert!(c >= 0.0 && c.is_finite(), "edge {v}–{w} costs {c}");
            let ng = g + c;
            if !settled[w] && ng < cost[w] {
                cost[w] = ng;
                parent[w] = v as u32;
                heap.push(key(ng + heuristic(w).max(0.0), ng, w));
            }
        });
    }
    None
}

/// Dijkstra's algorithm: `astar` with no estimate.
pub fn dijkstra(
    n: usize,
    start: usize,
    goal: usize,
    neighbors: impl FnMut(usize, &mut dyn FnMut(usize, f64)),
) -> Option<Path> {
    astar(n, start, goal, neighbors, |_| 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A generator of numbers in 0..1, the same on every run.
    struct Unit(u64);

    impl Unit {
        fn next(&mut self) -> f64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 >> 11) as f64 / (1u64 << 53) as f64
        }
    }

    /// Each node's edges out, to a node at a cost.
    type Adjacency = Vec<Vec<(usize, f64)>>;

    /// Points in a square and edges between random pairs, each costing at
    /// least the distance between its ends, some of them far more; directed
    /// one way or both.
    fn random(u: &mut Unit, n: usize, m: usize) -> (Vec<[f64; 2]>, Adjacency) {
        let p: Vec<[f64; 2]> = (0..n)
            .map(|_| [u.next() * 10_000.0, u.next() * 10_000.0])
            .collect();
        let mut adj = vec![Vec::new(); n];
        for _ in 0..m {
            let a = (u.next() * n as f64) as usize;
            let b = (u.next() * n as f64) as usize;
            let d = (p[a][0] - p[b][0]).hypot(p[a][1] - p[b][1]);
            let c = d * (1.0 + if u.next() < 0.3 { 4.0 * u.next() } else { 0.0 });
            adj[a].push((b, c));
            if u.next() < 0.8 {
                adj[b].push((a, c));
            }
        }
        (p, adj)
    }

    /// Every node's least cost from `s` by Bellman–Ford: relax every edge
    /// until nothing changes, which needs no ordering and so shares nothing
    /// with the search under test.
    fn bellman_ford(adj: &[Vec<(usize, f64)>], s: usize) -> Vec<f64> {
        let mut d = vec![f64::INFINITY; adj.len()];
        d[s] = 0.0;
        loop {
            let mut changed = false;
            for (v, out) in adj.iter().enumerate() {
                for &(w, c) in out {
                    if d[v] + c < d[w] {
                        d[w] = d[v] + c;
                        changed = true;
                    }
                }
            }
            if !changed {
                return d;
            }
        }
    }

    #[test]
    fn astar_and_dijkstra_find_the_least_cost_brute_force_finds() {
        let mut u = Unit(0x2545_f491_4f6c_dd1d);
        let mut reached = 0;
        for trial in 0..40 {
            let (n, m) = (50 + trial * 5, 120 + trial * 12);
            let (p, adj) = random(&mut u, n, m);
            let neighbors = |v: usize, f: &mut dyn FnMut(usize, f64)| {
                for &(w, c) in &adj[v] {
                    f(w, c);
                }
            };
            for _ in 0..10 {
                let s = (u.next() * n as f64) as usize;
                let t = (u.next() * n as f64) as usize;
                let want = bellman_ford(&adj, s)[t];
                let h = |v: usize| (p[v][0] - p[t][0]).hypot(p[v][1] - p[t][1]);
                let a = astar(n, s, t, neighbors, h);
                let d = dijkstra(n, s, t, neighbors);
                if want.is_infinite() {
                    assert!(a.is_none() && d.is_none());
                    continue;
                }
                reached += 1;
                let (a, d) = (a.unwrap(), d.unwrap());
                for path in [&a, &d] {
                    assert!((path.cost - want).abs() <= 1e-9 * want.max(1.0));
                    assert_eq!((path.nodes[0], *path.nodes.last().unwrap()), (s, t));
                    // The path is made of edges and costs what it says.
                    let mut sum = 0.0;
                    for w in path.nodes.windows(2) {
                        let c = adj[w[0]]
                            .iter()
                            .filter(|e| e.0 == w[1])
                            .map(|e| e.1)
                            .fold(f64::INFINITY, f64::min);
                        sum += c;
                    }
                    assert!((sum - path.cost).abs() <= 1e-9 * want.max(1.0));
                }
                assert!(a.settled <= d.settled, "{} > {}", a.settled, d.settled);
            }
        }
        assert!(reached > 200, "only {reached} pairs connected");
    }

    #[test]
    fn the_start_is_its_own_path_and_an_unreached_goal_none() {
        let adj: Vec<Vec<(usize, f64)>> = vec![vec![(1, 2.0)], vec![], vec![(0, 1.0)]];
        let nb = |v: usize, f: &mut dyn FnMut(usize, f64)| {
            for &(w, c) in &adj[v] {
                f(w, c);
            }
        };
        let p = dijkstra(3, 0, 0, nb).unwrap();
        assert_eq!((p.cost, p.nodes, p.settled), (0.0, vec![0], 1));
        assert!(dijkstra(3, 0, 2, nb).is_none());
        assert_eq!(dijkstra(3, 2, 1, nb).unwrap().nodes, [2, 0, 1]);
    }

    #[test]
    fn ties_break_the_same_way_every_run() {
        // A square: two paths of equal cost from corner 0 to corner 3.
        let adj: Vec<Vec<(usize, f64)>> = vec![
            vec![(1, 1.0), (2, 1.0)],
            vec![(3, 1.0)],
            vec![(3, 1.0)],
            vec![],
        ];
        let nb = |v: usize, f: &mut dyn FnMut(usize, f64)| {
            for &(w, c) in &adj[v] {
                f(w, c);
            }
        };
        let first = dijkstra(4, 0, 3, nb).unwrap();
        for _ in 0..10 {
            assert_eq!(dijkstra(4, 0, 3, nb).unwrap(), first);
        }
        assert_eq!(first.nodes, [0, 1, 3]);
    }
}
