//! The outlines of a mask's set cells, as polygons with holes.
//!
//! Each outline runs along cell edges, so it encloses exactly the set cells,
//! and is then simplified to half a cell. Two set cells meeting only at a
//! corner are separate polygons that touch there.

use std::collections::HashMap;

/// A row and a column of cell corners: corner `(r, c)` is the south-west
/// corner of cell `(r, c)`. Rows run north and columns east.
type Corner = (i64, i64);

/// Twice the signed area of a ring of corners, positive counterclockwise
/// on a map, east to the right and north up.
fn area2(ring: &[Corner]) -> i64 {
    let n = ring.len();
    (0..n)
        .map(|i| {
            let ((r0, c0), (r1, c1)) = (ring[i], ring[(i + 1) % n]);
            c0 * r1 - c1 * r0
        })
        .sum()
}

/// Even-odd crossing test of a point, in corner units, against a ring.
fn inside(ring: &[Corner], r: f64, c: f64) -> bool {
    let mut odd = false;
    let n = ring.len();
    for i in 0..n {
        let (a, b) = (ring[i], ring[(i + 1) % n]);
        let (ar, ac, br, bc) = (a.0 as f64, a.1 as f64, b.0 as f64, b.1 as f64);
        if (ar > r) != (br > r) && c < ac + (r - ar) / (br - ar) * (bc - ac) {
            odd = !odd;
        }
    }
    odd
}

/// Every closed outline of the set cells of a `rows` × `cols` mask, row
/// major, with the set cells on its left: counterclockwise around a polygon,
/// clockwise around a hole.
fn trace(set: &dyn Fn(i64, i64) -> bool, rows: i64, cols: i64) -> Vec<Vec<Corner>> {
    // Each cell edge between a set cell and an unset one, directed.
    let mut edges: Vec<(Corner, Corner)> = Vec::new();
    for r in 0..rows {
        for c in 0..cols {
            if !set(r, c) {
                continue;
            }
            if !set(r - 1, c) {
                edges.push(((r, c), (r, c + 1)));
            }
            if !set(r, c + 1) {
                edges.push(((r, c + 1), (r + 1, c + 1)));
            }
            if !set(r + 1, c) {
                edges.push(((r + 1, c + 1), (r + 1, c)));
            }
            if !set(r, c - 1) {
                edges.push(((r + 1, c), (r, c)));
            }
        }
    }
    let mut from: HashMap<Corner, Vec<usize>> = HashMap::new();
    for (i, e) in edges.iter().enumerate() {
        from.entry(e.0).or_default().push(i);
    }
    let mut used = vec![false; edges.len()];
    let mut rings = Vec::new();
    for start in 0..edges.len() {
        if used[start] {
            continue;
        }
        let mut ring = Vec::new();
        let mut e = start;
        loop {
            used[e] = true;
            let (a, b) = edges[e];
            ring.push(a);
            let (dr, dc) = (b.0 - a.0, b.1 - a.1);
            // At a corner two cells share only diagonally, turn left, to
            // stay around the cell this edge bounds.
            let left = (b.0 + dc, b.1 - dr);
            let next = from[&b].iter().copied().filter(|&i| !used[i]);
            let next: Vec<usize> = next.collect();
            match next.as_slice() {
                [] => break,
                [i] => e = *i,
                many => {
                    e = *many
                        .iter()
                        .find(|&&i| edges[i].1 == left)
                        .unwrap_or(&many[0])
                }
            }
        }
        rings.push(ring);
    }
    rings
}

/// The ring without the corners that lie on a straight line between their
/// neighbors.
fn corners_only(ring: &[Corner]) -> Vec<Corner> {
    let n = ring.len();
    (0..n)
        .filter(|&i| {
            let (p, q, s) = (ring[(i + n - 1) % n], ring[i], ring[(i + 1) % n]);
            (q.0 - p.0) * (s.1 - q.1) != (q.1 - p.1) * (s.0 - q.0)
        })
        .map(|i| ring[i])
        .collect()
}

/// Douglas–Peucker over a closed ring: no point of the outline left out
/// lies farther than `tolerance` from the simplified one.
fn simplify(ring: &[[f64; 2]], tolerance: f64) -> Vec<[f64; 2]> {
    fn far(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
        let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
        let len = dx.hypot(dz);
        if len == 0.0 {
            return (p[0] - a[0]).hypot(p[1] - a[1]);
        }
        ((p[0] - a[0]) * dz - (p[1] - a[1]) * dx).abs() / len
    }
    fn keep(pts: &[[f64; 2]], lo: usize, hi: usize, tol: f64, out: &mut Vec<bool>) {
        let (mut best, mut at) = (0.0, lo);
        for i in lo + 1..hi {
            let d = far(pts[i], pts[lo], pts[hi]);
            if d > best {
                (best, at) = (d, i);
            }
        }
        if best > tol {
            out[at] = true;
            keep(pts, lo, at, tol, out);
            keep(pts, at, hi, tol, out);
        }
    }
    let n = ring.len();
    if n <= 4 {
        return ring.to_vec();
    }
    // Split the ring at its first point and the point farthest from it.
    let d = |i: usize| (ring[i][0] - ring[0][0]).hypot(ring[i][1] - ring[0][1]);
    let mid = (1..n).max_by(|&i, &j| d(i).total_cmp(&d(j))).unwrap();
    let mut closed = ring.to_vec();
    closed.push(ring[0]);
    let mut out = vec![false; n + 1];
    (out[0], out[mid]) = (true, true);
    keep(&closed, 0, mid, tolerance, &mut out);
    keep(&closed, mid, n, tolerance, &mut out);
    let s: Vec<_> = (0..n).filter(|&i| out[i]).map(|i| ring[i]).collect();
    if s.len() < 3 { ring.to_vec() } else { s }
}

/// The set cells of a `rows` × `cols` mask, row major, as polygons, each
/// its exterior ring then its holes. `at` places a corner, row and column,
/// in DCS x and z, and `tolerance` is how far, in meters, a simplified edge
/// may stray from the cell edges.
pub fn polygons(
    set: &dyn Fn(i64, i64) -> bool,
    rows: i64,
    cols: i64,
    at: &dyn Fn(f64, f64) -> [f64; 2],
    tolerance: f64,
) -> Vec<Vec<Vec<[f64; 2]>>> {
    let rings: Vec<Vec<Corner>> = trace(set, rows, cols)
        .into_iter()
        .map(|r| corners_only(&r))
        .collect();
    let (outer, holes): (Vec<_>, Vec<_>) = rings.iter().partition(|r| area2(r) > 0);
    let mut polys: Vec<Vec<&Vec<Corner>>> = outer.iter().map(|r| vec![*r]).collect();
    for h in holes {
        // The unset cell to the right of the hole's first edge lies in it.
        let ((r0, c0), (r1, c1)) = (h[0], h[1]);
        let (dr, dc) = ((r1 - r0).signum(), (c1 - c0).signum());
        let (pr, pc) = (
            r0 as f64 + dr as f64 * 0.5 - dc as f64 * 0.5,
            c0 as f64 + dc as f64 * 0.5 + dr as f64 * 0.5,
        );
        let owner = outer
            .iter()
            .enumerate()
            .filter(|(_, o)| inside(o, pr, pc))
            .min_by_key(|(_, o)| area2(o))
            .map(|(i, _)| i);
        // Every hole lies in some polygon, since the mask outside its own
        // edge is unset.
        polys[owner.expect("a hole lies inside a polygon")].push(h);
    }
    polys
        .into_iter()
        .map(|p| {
            p.into_iter()
                .map(|r| {
                    let pts: Vec<_> = r.iter().map(|&(r, c)| at(r as f64, c as f64)).collect();
                    simplify(&pts, tolerance)
                })
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mask<'a>(rows: &'a [&'a str]) -> impl Fn(i64, i64) -> bool + 'a {
        // Written top row first, as it reads; row 0 is the south.
        move |r, c| {
            let n = rows.len() as i64;
            (0..n).contains(&r)
                && rows[(n - 1 - r) as usize]
                    .as_bytes()
                    .get(c as usize)
                    .is_some_and(|&b| b == b'#')
        }
    }

    fn unit(r: f64, c: f64) -> [f64; 2] {
        [r, c]
    }

    #[test]
    fn a_ring_of_cells_is_a_polygon_with_a_hole() {
        let m = mask(&["###", "#.#", "###"]);
        let p = polygons(&m, 3, 3, &unit, 0.0);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].len(), 2);
        assert_eq!(p[0][0].len(), 4);
        assert_eq!(p[0][1].len(), 4);
    }

    #[test]
    fn cells_touching_at_a_corner_are_two_polygons() {
        let m = mask(&[".#", "#."]);
        let p = polygons(&m, 2, 2, &unit, 0.0);
        assert_eq!(p.len(), 2);
        assert!(p.iter().all(|q| q.len() == 1 && q[0].len() == 4));
    }

    #[test]
    fn an_island_in_a_hole_is_its_own_polygon() {
        let m = mask(&["#####", "#...#", "#.#.#", "#...#", "#####"]);
        let p = polygons(&m, 5, 5, &unit, 0.0);
        let mut shape: Vec<_> = p.iter().map(|q| q.len()).collect();
        shape.sort();
        assert_eq!(shape, [1, 2]);
    }

    #[test]
    fn a_simplified_staircase_strays_no_farther_than_the_tolerance() {
        let m = mask(&["#.....", "##....", "###...", "####..", "#####.", "######"]);
        let full = &polygons(&m, 6, 6, &unit, 0.0)[0][0];
        let s = &polygons(&m, 6, 6, &unit, 0.75)[0][0];
        assert!(s.len() < full.len(), "{s:?}");
        let dist = |p: [f64; 2], a: [f64; 2], b: [f64; 2]| {
            let (d0, d1) = (b[0] - a[0], b[1] - a[1]);
            let t =
                (((p[0] - a[0]) * d0 + (p[1] - a[1]) * d1) / (d0 * d0 + d1 * d1)).clamp(0.0, 1.0);
            (p[0] - a[0] - t * d0).hypot(p[1] - a[1] - t * d1)
        };
        for &p in full {
            let near = (0..s.len())
                .map(|i| dist(p, s[i], s[(i + 1) % s.len()]))
                .fold(f64::INFINITY, f64::min);
            assert!(near <= 0.75 + 1e-9, "{p:?} is {near} away");
        }
    }
}
