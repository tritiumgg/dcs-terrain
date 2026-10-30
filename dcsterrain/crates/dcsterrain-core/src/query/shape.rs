//! Closed shapes in the DCS x–z plane, and whether two meet: the narrow
//! phase of an overlap test, run on what a box search found near.
//!
//! Two shapes meet when they come within `TOUCH_M` of each other, so a
//! shape that only touches another, edge to edge or corner to edge, meets
//! it. Two convex polygons are tested by separating axes: an axis on which
//! they lie more than `TOUCH_M` apart separates them, and overlap on every
//! axis by no gap at all means they meet. Between the two, where the
//! nearest points may be two corners and no edge's axis measures the gap,
//! their distance decides. Any other pair is decided by its distance:
//! zero where a corner of one lies inside the other or two sides cross,
//! else the least distance between their sides.
//!
//! Callers move shapes near the origin first, `translated`: within 10 km of
//! it a coordinate's rounding is under 2e-12 m, so the tolerance is what
//! decides touching, not the rounding of coordinates a million meters out.

use super::region::{distance_to, inside};

/// How near two shapes may come and still meet: a micrometer, a million
/// times the rounding of the arithmetic near the origin and far below any
/// size a placement or a model has.
pub const TOUCH_M: f64 = 1e-6;

/// The sides of the polygon a disc is drawn as.
pub const DISC_SIDES: usize = 32;

#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    /// A disc; a point is one of radius 0.
    Disc { c: [f64; 2], r: f64 },
    /// A polygon closed by its first point, its vertices running either
    /// way round, inside by the even-odd rule. `convex` when it is, with
    /// at least one side of nonzero length.
    Polygon { points: Vec<[f64; 2]>, convex: bool },
}

fn sub(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn cross(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

fn dot(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}

impl Shape {
    pub fn point(p: [f64; 2]) -> Shape {
        Shape::Disc { c: p, r: 0.0 }
    }

    /// A polygon of `points`, three or more, finite. One whose points all
    /// coincide is that point.
    pub fn polygon(points: Vec<[f64; 2]>) -> Shape {
        if points.iter().all(|&p| p == points[0]) {
            return Shape::point(points[0]);
        }
        let convex = is_convex(&points);
        Shape::Polygon { points, convex }
    }

    /// Min x, min z, max x, max z.
    pub fn bbox(&self) -> [f64; 4] {
        match self {
            Shape::Disc { c, r } => [c[0] - r, c[1] - r, c[0] + r, c[1] + r],
            Shape::Polygon { points, .. } => points.iter().fold(
                [
                    f64::INFINITY,
                    f64::INFINITY,
                    f64::NEG_INFINITY,
                    f64::NEG_INFINITY,
                ],
                |b, p| {
                    [
                        b[0].min(p[0]),
                        b[1].min(p[1]),
                        b[2].max(p[0]),
                        b[3].max(p[1]),
                    ]
                },
            ),
        }
    }

    /// The shape moved by `-by`: into a frame whose origin is `by`.
    pub fn translated(&self, by: [f64; 2]) -> Shape {
        match self {
            Shape::Disc { c, r } => Shape::Disc {
                c: sub(*c, by),
                r: *r,
            },
            Shape::Polygon { points, convex } => Shape::Polygon {
                points: points.iter().map(|&p| sub(p, by)).collect(),
                convex: *convex,
            },
        }
    }

    /// The least distance between the two, 0 where they overlap.
    pub fn distance(&self, o: &Shape) -> f64 {
        match (self, o) {
            (Shape::Disc { c: a, r: ra }, Shape::Disc { c: b, r: rb }) => {
                ((a[0] - b[0]).hypot(a[1] - b[1]) - ra - rb).max(0.0)
            }
            (Shape::Disc { c, r }, Shape::Polygon { points, .. })
            | (Shape::Polygon { points, .. }, Shape::Disc { c, r }) => {
                (polygon_to_point(points, *c) - r).max(0.0)
            }
            (Shape::Polygon { points: a, .. }, Shape::Polygon { points: b, .. }) => {
                polygons_apart(a, b)
            }
        }
    }

    /// Whether the two come within `TOUCH_M` of each other.
    pub fn meets(&self, o: &Shape) -> bool {
        if let (
            Shape::Polygon {
                points: a,
                convex: true,
            },
            Shape::Polygon {
                points: b,
                convex: true,
            },
        ) = (self, o)
        {
            match separating_axes(a, b) {
                Axes::Apart => return false,
                Axes::Overlap => return true,
                Axes::Near => {}
            }
        }
        self.distance(o) <= TOUCH_M
    }

    /// The outline to draw: a polygon's own points, a disc's as a regular
    /// polygon of `DISC_SIDES` sides, a point's as itself.
    pub fn outline(&self) -> Vec<[f64; 2]> {
        match self {
            Shape::Disc { c, r } if *r > 0.0 => (0..DISC_SIDES)
                .map(|k| {
                    let a = k as f64 * std::f64::consts::TAU / DISC_SIDES as f64;
                    [c[0] + r * a.cos(), c[1] + r * a.sin()]
                })
                .collect(),
            Shape::Disc { c, .. } => vec![*c],
            Shape::Polygon { points, .. } => points.clone(),
        }
    }
}

/// Each side of a closed ring, its last point joined to its first.
fn sides(p: &[[f64; 2]]) -> impl Iterator<Item = ([f64; 2], [f64; 2])> + '_ {
    (0..p.len()).map(move |i| (p[i], p[(i + 1) % p.len()]))
}

/// Whether the ring turns one way only, once round: every corner turning
/// the same way, those of no turn aside, and the turns summing to one full
/// turn, which a star whose corners all turn one way does not.
fn is_convex(p: &[[f64; 2]]) -> bool {
    let n = p.len();
    let (mut pos, mut neg) = (false, false);
    let mut turned = 0.0;
    let mut prev: Option<[f64; 2]> = None;
    // Each nonzero side in turn, the first again at the end to close it.
    let edges: Vec<[f64; 2]> = sides(p)
        .map(|(a, b)| sub(b, a))
        .filter(|e| *e != [0.0, 0.0])
        .collect();
    if edges.is_empty() || n < 3 {
        return false;
    }
    for &e in edges.iter().chain(edges.first()) {
        if let Some(d) = prev {
            let c = cross(d, e);
            pos |= c > 0.0;
            neg |= c < 0.0;
            turned += c.atan2(dot(d, e));
        }
        prev = Some(e);
    }
    // A ring of no area turns half a turn at each end of its line.
    !(pos && neg) && (turned.abs() - std::f64::consts::TAU).abs() < 1e-6
}

/// The least distance from `p` to the polygon, 0 inside it.
fn polygon_to_point(p: &[[f64; 2]], c: [f64; 2]) -> f64 {
    if inside(p, c[0], c[1]) {
        return 0.0;
    }
    sides(p)
        .map(|(a, b)| distance_to(a, b, c))
        .fold(f64::INFINITY, f64::min)
}

/// Whether two closed segments cross at a point inside both: their ends
/// on strictly opposite sides of each other. Touching is left to the
/// distance between them.
fn cross_properly(p: [f64; 2], q: [f64; 2], r: [f64; 2], s: [f64; 2]) -> bool {
    let d1 = cross(sub(s, r), sub(p, r));
    let d2 = cross(sub(s, r), sub(q, r));
    let d3 = cross(sub(q, p), sub(r, p));
    let d4 = cross(sub(q, p), sub(s, p));
    d1 * d2 < 0.0 && d3 * d4 < 0.0
}

/// The least distance between two polygons, 0 where they overlap.
fn polygons_apart(a: &[[f64; 2]], b: &[[f64; 2]]) -> f64 {
    if a.iter().any(|v| inside(b, v[0], v[1])) || b.iter().any(|v| inside(a, v[0], v[1])) {
        return 0.0;
    }
    let mut least = f64::INFINITY;
    for (p, q) in sides(a) {
        for (r, s) in sides(b) {
            if cross_properly(p, q, r, s) {
                return 0.0;
            }
            least = least
                .min(distance_to(r, s, p))
                .min(distance_to(r, s, q))
                .min(distance_to(p, q, r))
                .min(distance_to(p, q, s));
        }
    }
    least
}

/// What the separating axes say of two convex polygons.
#[derive(Debug, PartialEq)]
enum Axes {
    /// More than `TOUCH_M` apart on some axis.
    Apart,
    /// No gap on any axis: they overlap or touch.
    Overlap,
    /// A gap on some axis, none over `TOUCH_M`: the axes cannot tell.
    Near,
}

/// The separating-axis test of two convex polygons over each one's side
/// normals; a polygon of no area adds its line's direction, which no
/// normal measures along.
fn separating_axes(a: &[[f64; 2]], b: &[[f64; 2]]) -> Axes {
    let mut gap = f64::NEG_INFINITY;
    for poly in [a, b] {
        let flat = super::geojson::signed_area(poly) == 0.0;
        for (p, q) in sides(poly) {
            let e = sub(q, p);
            let len = e[0].hypot(e[1]);
            if len == 0.0 {
                continue;
            }
            let n = [-e[1] / len, e[0] / len];
            let axes: &[[f64; 2]] = if flat {
                &[n, [e[0] / len, e[1] / len]]
            } else {
                &[n]
            };
            for &axis in axes {
                let span = |v: &[[f64; 2]]| {
                    v.iter()
                        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &p| {
                            let d = dot(p, axis);
                            (lo.min(d), hi.max(d))
                        })
                };
                let ((a0, a1), (b0, b1)) = (span(a), span(b));
                gap = gap.max(b0 - a1).max(a0 - b1);
                if gap > TOUCH_M {
                    return Axes::Apart;
                }
            }
        }
    }
    if gap <= 0.0 {
        Axes::Overlap
    } else {
        Axes::Near
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roads::tests::Unit;

    fn square(c: [f64; 2], half: f64, turn: f64) -> Shape {
        let (s, k) = turn.sin_cos();
        Shape::polygon(
            [[1.0, 1.0], [1.0, -1.0], [-1.0, -1.0], [-1.0, 1.0]]
                .iter()
                .map(|&[u, v]| [c[0] + half * (u * k - v * s), c[1] + half * (u * s + v * k)])
                .collect(),
        )
    }

    #[test]
    fn touching_counts_and_a_gap_past_the_tolerance_does_not() {
        let a = square([0.0, 0.0], 5.0, 0.0);
        // Edge to edge, corner to edge, and corner to corner.
        assert!(a.meets(&square([10.0, 0.0], 5.0, 0.0)));
        assert!(a.meets(&square([10.0, 3.0], 5.0, 0.0)));
        assert!(a.meets(&square([10.0, 10.0], 5.0, 0.0)));
        let r = 5.0 * std::f64::consts::SQRT_2;
        assert!(a.meets(&square([5.0 + r, 0.0], 5.0, std::f64::consts::FRAC_PI_4)));
        // Corner to corner, square on, a gap of 1.5 micrometers: no
        // edge's axis measures more than 1.06 of it, so the distance
        // decides.
        let d = 1.5e-6 / std::f64::consts::SQRT_2;
        assert!(!a.meets(&square([10.0 + d, 10.0 + d], 5.0, 0.0)));
        assert!(a.meets(&square([10.0 + d / 2.0, 10.0 + d / 2.0], 5.0, 0.0)));
        assert!(!a.meets(&square([10.0 + 2e-6, 0.0], 5.0, 0.0)));
        // A disc against the square's side, and a point on its edge.
        assert!(a.meets(&Shape::Disc {
            c: [8.0, 0.0],
            r: 3.0
        }));
        assert!(!a.meets(&Shape::Disc {
            c: [8.0, 0.0],
            r: 2.99
        }));
        assert!(a.meets(&Shape::point([5.0, 1.0])));
    }

    #[test]
    fn a_concave_polygon_meets_only_what_reaches_its_body() {
        // A U open to the north: a box in the notch meets nothing.
        let u = Shape::polygon(vec![
            [0.0, 0.0],
            [30.0, 0.0],
            [30.0, 30.0],
            [20.0, 30.0],
            [20.0, 10.0],
            [10.0, 10.0],
            [10.0, 30.0],
            [0.0, 30.0],
        ]);
        assert!(matches!(u, Shape::Polygon { convex: false, .. }));
        assert!(!u.meets(&square([15.0, 20.0], 4.0, 0.0)));
        assert!(u.meets(&square([15.0, 20.0], 5.0, 0.0)));
        assert!(!u.meets(&Shape::Disc {
            c: [15.0, 25.0],
            r: 4.9
        }));
        // A star's corners all turn one way, twice round: not convex.
        let star: Vec<[f64; 2]> = (0..5)
            .map(|k| {
                let a = f64::from(k * 2) * std::f64::consts::TAU / 5.0;
                [a.cos(), a.sin()]
            })
            .collect();
        assert!(matches!(
            Shape::polygon(star),
            Shape::Polygon { convex: false, .. }
        ));
        // A line of no area along another, apart along it.
        let line =
            |x0: f64, x1: f64| Shape::polygon(vec![[x0, 0.0], [x1, 0.0], [x1, 0.0], [x0, 0.0]]);
        assert!(!line(0.0, 1.0).meets(&line(2.0, 3.0)));
        assert!(line(0.0, 1.0).meets(&line(1.0, 3.0)));
    }

    /// A random convex polygon: a box, a triangle or a hull of up to eight
    /// points on a circle, turned and placed near the origin.
    fn convex(u: &mut Unit) -> Shape {
        let c = [u.next() * 40.0 - 20.0, u.next() * 40.0 - 20.0];
        let k = (u.next() * 3.0) as usize;
        let n = [4, 3, 3 + (u.next() * 6.0) as usize][k];
        let r = 1.0 + u.next() * 15.0;
        let mut a: Vec<f64> = (0..n).map(|_| u.next() * std::f64::consts::TAU).collect();
        a.sort_by(f64::total_cmp);
        let s = Shape::polygon(
            a.iter()
                .map(|t| [c[0] + r * t.cos(), c[1] + r * t.sin()])
                .collect(),
        );
        if k == 0 { square(c, r, a[0]) } else { s }
    }

    #[test]
    fn separating_axes_agree_with_the_distance_between_convex_polygons() {
        let mut u = Unit(0x9e37_79b9_7f4a_7c15);
        let (mut met, mut near) = (0, 0);
        for _ in 0..20_000 {
            let (a, b) = (convex(&mut u), convex(&mut u));
            let Shape::Polygon { convex: true, .. } = a else {
                continue;
            };
            let want = a.distance(&b) <= TOUCH_M;
            assert_eq!(a.meets(&b), want, "{a:?} {b:?}");
            met += usize::from(want);
            // Moved to just touch along the line between their vertices'
            // means, each inside its own polygon: the distance falls along
            // it to 0 by the time the means coincide.
            let mean = |s: &Shape| {
                let o = s.outline();
                let n = o.len() as f64;
                o.iter()
                    .fold([0.0, 0.0], |m, p| [m[0] + p[0] / n, m[1] + p[1] / n])
            };
            if a.distance(&b) > 0.0 {
                let dir = sub(mean(&a), mean(&b));
                let l = dir[0].hypot(dir[1]);
                let mut lo = 0.0;
                let mut hi = l;
                // The step along `dir` that brings b to touch a.
                for _ in 0..200 {
                    let m = (lo + hi) / 2.0;
                    let t = b.translated([-dir[0] / l * m, -dir[1] / l * m]);
                    if a.distance(&t) > 0.0 { lo = m } else { hi = m }
                }
                let t = b.translated([-dir[0] / l * hi, -dir[1] / l * hi]);
                assert!(a.meets(&t), "touching {a:?} {t:?}");
                near += 1;
            }
        }
        assert!(
            met > 2000 && near > 2000,
            "{met} met, {near} brought to touch"
        );
    }

    #[test]
    fn the_distance_is_what_a_dense_scan_of_both_outlines_finds() {
        // Every vertex-to-side pairing the distance takes, against the
        // least distance between points stepped densely along both
        // outlines, which can only be larger.
        let mut u = Unit(0x2545_f491_4f6c_dd1d);
        let along = |s: &Shape| -> Vec<[f64; 2]> {
            let o = s.outline();
            (0..o.len())
                .flat_map(|i| {
                    let (a, b) = (o[i], o[(i + 1) % o.len()]);
                    (0..200).map(move |k| {
                        let t = f64::from(k) / 200.0;
                        [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])]
                    })
                })
                .collect()
        };
        for _ in 0..300 {
            let (a, b) = (convex(&mut u), convex(&mut u));
            let d = a.distance(&b);
            if d == 0.0 {
                continue;
            }
            let (pa, pb) = (along(&a), along(&b));
            let scan = pa
                .iter()
                .flat_map(|p| pb.iter().map(move |q| (p[0] - q[0]).hypot(p[1] - q[1])))
                .fold(f64::INFINITY, f64::min);
            assert!(d <= scan + 1e-9 && scan - d < 0.2, "{d} {scan}");
        }
    }
}
