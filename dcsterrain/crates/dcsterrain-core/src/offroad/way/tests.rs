use super::*;
use crate::layers::Cells;

fn grid() -> Grid {
    Grid {
        cell_size: 50,
        origin_x: 0.0,
        origin_z: 0.0,
        height: 1000,
        width: 1000,
        tile_size: 256,
    }
}

/// A field of `rows` × `cols` cells of costs from 1 to 5 drawn from a
/// seeded generator, about one in eight blocked, and a road through a few.
fn ground_of(rows: u32, cols: u32, seed: u64) -> (CostField, RoadCells) {
    let mut s = seed;
    let mut next = move || {
        // A 64-bit linear congruential generator, Knuth's MMIX constants.
        s = s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (s >> 33) as f64 / (1u64 << 31) as f64
    };
    let n = (rows * cols) as usize;
    let mut values = Vec::with_capacity(n);
    let mut edge = vec![NO_EDGE; n];
    for (i, e) in edge.iter_mut().enumerate() {
        let u = next();
        values.push(if u < 0.06 {
            Err(Block::Water(3))
        } else if u < 0.12 {
            Err(Block::Steep(40.0))
        } else {
            Ok((1.0 + 4.0 * next()) as f32)
        });
        if next() < 0.01 {
            *e = (i % 7) as u32;
        }
    }
    let field = CostField {
        cells: Cells {
            row0: 3,
            col0: 5,
            rows,
            cols,
        },
        values,
    };
    (field, RoadCells { edge })
}

/// Bellman–Ford over the same moves: every edge relaxed until none
/// changes, no move leaving a road's cell.
fn bellman_ford(ground: &Ground, starts: &[(usize, f64)]) -> Vec<f64> {
    let moves = crossings(ground.grid.cell_size);
    let n = ground.field.values.len();
    let mut cost = vec![f64::INFINITY; n];
    for &(s, g) in starts {
        if ground.passage(s).is_ok() {
            cost[s] = cost[s].min(g);
        }
    }
    loop {
        let mut changed = false;
        for v in 0..n {
            if !cost[v].is_finite() || ground.roads.is_road(v) {
                continue;
            }
            for m in 0..MOVES.len() {
                if let Ok((w, c)) = ground.step(&moves, v, m)
                    && cost[v] + c < cost[w]
                {
                    cost[w] = cost[v] + c;
                    changed = true;
                }
            }
        }
        if !changed {
            return cost;
        }
    }
}

#[test]
fn a_move_crosses_cells_for_its_whole_length_and_a_corner_touches_both_sides() {
    let moves = crossings(50);
    for (m, cells) in moves.iter().enumerate() {
        let (dr, dc) = MOVES[m];
        let total: f64 = cells.iter().map(|x| x.meters).sum();
        assert!(((dr as f64).hypot(dc as f64) * 50.0 - total).abs() < 1e-9);
        assert_eq!(cells.first().map(|x| (x.dr, x.dc)), Some((0, 0)));
        assert_eq!(cells.last().map(|x| (x.dr, x.dc)), Some((dr, dc)));
    }
    // A corner move touches the two cells beside the corner, holding none
    // of it.
    let corner: Vec<(i64, i64, f64)> = moves[4].iter().map(|x| (x.dr, x.dc, x.meters)).collect();
    assert_eq!(corner.len(), 4);
    assert!(corner.contains(&(1, 0, 0.0)) && corner.contains(&(0, 1, 0.0)));
    // A knight's move crosses four cells, a quarter of it in each.
    let knight = &moves[8];
    assert_eq!(knight.len(), 4);
    for x in knight {
        assert!((x.meters - 50.0 * 5f64.sqrt() / 4.0).abs() < 1e-9);
    }
}

#[test]
fn a_diagonal_cannot_slip_between_two_cells_that_block() {
    let g = grid();
    let mut values = vec![Ok(1.0f32); 4];
    values[1] = Err(Block::Water(3));
    values[2] = Err(Block::Water(3));
    let field = CostField {
        cells: Cells {
            row0: 0,
            col0: 0,
            rows: 2,
            cols: 2,
        },
        values,
    };
    let roads = RoadCells {
        edge: vec![NO_EDGE, NO_EDGE, NO_EDGE, 0],
    };
    let ground = Ground {
        grid: &g,
        field: &field,
        roads: &roads,
    };
    let s = search(&ground, &[(0, 0.0)], &|_| true, true);
    assert!(s.goals.is_empty());
    assert!(s.blocks.river);
}

#[test]
fn the_search_equals_bellman_ford_and_its_straightened_way_costs_no_more() {
    let g = grid();
    let mut ways = 0;
    for seed in 1..=6 {
        let (field, roads) = ground_of(24, 31, seed);
        let ground = Ground {
            grid: &g,
            field: &field,
            roads: &roads,
        };
        let start = (12 * 31 + 15, 0.0);
        // Every cell reached, every road's cell a goal.
        let all = search(&ground, &[start], &|_| true, false);
        let bf = bellman_ford(&ground, &[start]);
        for (i, (&a, &b)) in all.cost.iter().zip(&bf).enumerate() {
            assert!(
                a == b || (a - b).abs() <= 1e-9 * b.max(1.0),
                "seed {seed}, cell {i}: {a} against {b}"
            );
        }
        // Goals come in order of cost.
        assert!(all.goals.windows(2).all(|w| w[0].1 <= w[1].1));
        // The first goal ends the search, and no cell settled costs more.
        let first = search(&ground, &[start], &|_| true, true);
        let Some(&(goal, cost)) = first.goals.first() else {
            continue;
        };
        assert_eq!(cost, all.goals[0].1);
        assert!(first.most <= cost);
        assert!(first.settled <= all.settled);
        let center = ground.center(start.0);
        let way = found(&ground, &first, center, goal, ground.center(goal)).unwrap();
        assert!(
            (way.cost - cost).abs() < 1e-6 * cost.max(1.0),
            "{} {cost}",
            way.cost
        );
        let straight = straighten(&ground, &way);
        assert!(straight.cost <= way.cost + 1e-9, "seed {seed}");
        assert!(straight.path.len() <= way.path.len());
        assert_eq!(straight.path.first(), way.path.first());
        assert_eq!(straight.path.last(), way.path.last());
        // Each straight piece is drivable at the cost claimed.
        let mut again = 0.0;
        for w in straight.path.windows(2) {
            let end = ground.cell_of(w[1]).unwrap();
            again += ground.line(w[0], w[1], Some(end)).unwrap();
        }
        assert!((again - straight.cost).abs() < 1e-6 * cost.max(1.0));
        ways += 1;
    }
    assert!(ways >= 4, "only {ways} seeds found a road");
}

#[test]
fn many_starts_each_carry_their_cost() {
    let g = grid();
    let (field, roads) = ground_of(20, 20, 9);
    let ground = Ground {
        grid: &g,
        field: &field,
        roads: &roads,
    };
    let starts = [(0, 500.0), (399, 0.0), (210, 25.0)];
    let s = search(&ground, &starts, &|_| true, false);
    let bf = bellman_ford(&ground, &starts);
    for (&a, &b) in s.cost.iter().zip(&bf) {
        assert!(a == b || (a - b).abs() <= 1e-9 * b.max(1.0));
    }
}

#[test]
fn a_disk_bounds_the_search_and_says_so() {
    let g = grid();
    let field = CostField {
        cells: Cells {
            row0: 0,
            col0: 0,
            rows: 21,
            cols: 21,
        },
        values: vec![Ok(1.0); 441],
    };
    let roads = RoadCells {
        edge: vec![NO_EDGE; 441],
    };
    let ground = Ground {
        grid: &g,
        field: &field,
        roads: &roads,
    };
    let center = ground.center(220);
    let inside = |i: usize| {
        let c = ground.center(i);
        (c[0] - center[0]).hypot(c[1] - center[1]) <= 200.0
    };
    let s = search(&ground, &[(220, 0.0)], &inside, false);
    assert!(s.blocks.reach);
    assert!(s.goals.is_empty());
    assert!(s.most <= 200.0 * 1.03);
}

#[test]
fn a_meeting_point_on_a_cells_edge_is_moved_into_the_cell() {
    let g = grid();
    let field = CostField {
        cells: Cells {
            row0: 0,
            col0: 0,
            rows: 2,
            cols: 2,
        },
        values: vec![Ok(1.0); 4],
    };
    let roads = RoadCells {
        edge: vec![0, 0, 0, 0],
    };
    let ground = Ground {
        grid: &g,
        field: &field,
        roads: &roads,
    };
    // On the edge between cells 0 and 2, and a rounding past it.
    for q in [[50.0, 30.0], [50.0 + 1e-12, 30.0], [50.0, 50.0]] {
        let p = ground.inward(q, 0);
        assert_eq!(ground.cell_of(p), Some(0), "{q:?}");
        assert!((p[0] - q[0]).hypot(p[1] - q[1]) < 1e-5, "{q:?} to {p:?}");
        // A way judged from the cell's center to it stays in the cell.
        assert!(ground.line(ground.center(0), p, Some(0)).is_ok());
    }
    // A point inside the cell is kept as it is.
    assert_eq!(ground.inward([10.0, 20.0], 0), [10.0, 20.0]);
}
