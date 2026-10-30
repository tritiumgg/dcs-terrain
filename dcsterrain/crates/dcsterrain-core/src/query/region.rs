//! A request's region as its bounding box and a membership test.

use super::refused;
use crate::decode::lua::tables::named;
use crate::source::Source;
use crate::source::install::{AirdromeRow, Tables};
use crate::types::{AirdromeRef, Issue, Region};

/// A region ready to test: its box, min x, min z, max x, max z, and its
/// shape within the box.
#[derive(Debug, Clone, PartialEq)]
pub struct Area {
    pub bbox: [f64; 4],
    shape: Shape,
}

#[derive(Debug, Clone, PartialEq)]
enum Shape {
    Box,
    Circle { x: f64, z: f64, r: f64 },
    Polygon(Vec<[f64; 2]>),
    Ring { x: f64, z: f64, min: f64, max: f64 },
}

impl Area {
    /// Whether `(x, z)` lies in the region, its edge included.
    pub fn contains(&self, x: f64, z: f64) -> bool {
        let [x0, z0, x1, z1] = self.bbox;
        if !(x >= x0 && x <= x1 && z >= z0 && z <= z1) {
            return false;
        }
        match &self.shape {
            Shape::Box => true,
            Shape::Circle { x: cx, z: cz, r } => (x - cx).hypot(z - cz) <= *r,
            Shape::Ring {
                x: cx,
                z: cz,
                min,
                max,
            } => (min..=max).contains(&&(x - cx).hypot(z - cz)),
            Shape::Polygon(p) => inside(p, x, z),
        }
    }

    /// Whether any point of the segment from `a` to `b`, `[x, z]`, lies in
    /// the region, its edge included.
    pub fn meets(&self, a: [f64; 2], b: [f64; 2]) -> bool {
        let [x0, z0, x1, z1] = self.bbox;
        if a[0].max(b[0]) < x0 || a[0].min(b[0]) > x1 || a[1].max(b[1]) < z0 || a[1].min(b[1]) > z1
        {
            return false;
        }
        if self.contains(a[0], a[1]) || self.contains(b[0], b[1]) {
            return true;
        }
        // Both ends lie outside, so the segment meets the region only by
        // crossing its boundary.
        match &self.shape {
            Shape::Box => {
                let corners = [[x0, z0], [x1, z0], [x1, z1], [x0, z1]];
                crosses_ring(&corners, a, b)
            }
            Shape::Polygon(p) => crosses_ring(p, a, b),
            Shape::Circle { x, z, r } => distance_to(a, b, [*x, *z]) <= *r,
            Shape::Ring { x, z, min, max } => {
                let c = [*x, *z];
                let far = (a[0] - x).hypot(a[1] - z).max((b[0] - x).hypot(b[1] - z));
                // The distance to the center runs continuously from its
                // least to its greatest along the segment.
                distance_to(a, b, c) <= *max && far >= *min
            }
        }
    }
}

/// The least distance from `p` to the segment from `a` to `b`.
fn distance_to(a: [f64; 2], b: [f64; 2], p: [f64; 2]) -> f64 {
    let c = crate::roads::closest_on(a, b, p);
    (c[0] - p[0]).hypot(c[1] - p[1])
}

/// Whether the segment from `a` to `b` meets any side of the closed ring
/// `ring`, touching included.
fn crosses_ring(ring: &[[f64; 2]], a: [f64; 2], b: [f64; 2]) -> bool {
    let n = ring.len();
    (0..n).any(|i| segments_meet(a, b, ring[i], ring[(i + 1) % n]))
}

/// Whether two closed segments share a point.
fn segments_meet(p: [f64; 2], q: [f64; 2], r: [f64; 2], s: [f64; 2]) -> bool {
    let cross = |o: [f64; 2], a: [f64; 2], b: [f64; 2]| {
        (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
    };
    let within = |o: [f64; 2], a: [f64; 2], b: [f64; 2]| {
        o[0] >= a[0].min(b[0])
            && o[0] <= a[0].max(b[0])
            && o[1] >= a[1].min(b[1])
            && o[1] <= a[1].max(b[1])
    };
    let (d1, d2) = (cross(r, s, p), cross(r, s, q));
    let (d3, d4) = (cross(p, q, r), cross(p, q, s));
    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0))
        && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
    {
        return true;
    }
    (d1 == 0.0 && within(p, r, s))
        || (d2 == 0.0 && within(q, r, s))
        || (d3 == 0.0 && within(r, p, q))
        || (d4 == 0.0 && within(s, p, q))
}

impl Area {
    /// The region's outline as one polygon, its exterior then any hole, in
    /// DCS x and z; a circle as a 64-sided one.
    pub fn outline(&self) -> Vec<Vec<[f64; 2]>> {
        let circle = |x: f64, z: f64, r: f64| -> Vec<[f64; 2]> {
            (0..64)
                .map(|k| {
                    let a = f64::from(k) * std::f64::consts::TAU / 64.0;
                    [x + r * a.cos(), z + r * a.sin()]
                })
                .collect()
        };
        let [x0, z0, x1, z1] = self.bbox;
        match &self.shape {
            Shape::Box => vec![vec![[x0, z0], [x1, z0], [x1, z1], [x0, z1]]],
            Shape::Circle { x, z, r } => vec![circle(*x, *z, *r)],
            Shape::Polygon(p) => vec![p.clone()],
            Shape::Ring { x, z, min, max } => {
                let mut rings = vec![circle(*x, *z, *max)];
                if *min > 0.0 {
                    rings.push(circle(*x, *z, *min));
                }
                rings
            }
        }
    }
}

/// Even-odd crossing test.
fn inside(p: &[[f64; 2]], x: f64, z: f64) -> bool {
    let mut odd = false;
    let mut j = p.len() - 1;
    for i in 0..p.len() {
        let ([xi, zi], [xj, zj]) = (p[i], p[j]);
        if (zi > z) != (zj > z) && x < xi + (z - zi) / (zj - zi) * (xj - xi) {
            odd = !odd;
        }
        j = i;
    }
    odd
}

fn issue(path: &str, message: &str) -> Vec<Issue> {
    vec![Issue::new("invalid_region", message, path)]
}

/// The region `r`, found at `path` in the request. An airdrome band reads
/// the airdrome's reference point from the tables.
pub fn resolve(src: &dyn Source, r: &Region, path: &str) -> Result<Area, Vec<Issue>> {
    let finite = |v: &[f64]| v.iter().all(|v| v.is_finite());
    match r {
        Region::Box(b) => {
            if !finite(&[b.min_x, b.min_z, b.max_x, b.max_z]) {
                return Err(issue(path, "a box's corners must be finite"));
            }
            if b.min_x > b.max_x || b.min_z > b.max_z {
                return Err(issue(path, "a box's minimum lies past its maximum"));
            }
            Ok(Area {
                bbox: [b.min_x, b.min_z, b.max_x, b.max_z],
                shape: Shape::Box,
            })
        }
        &Region::Circle { x, z, radius } => {
            if !finite(&[x, z, radius]) || radius < 0.0 {
                return Err(issue(path, "a circle needs a finite center and radius"));
            }
            Ok(Area {
                bbox: [x - radius, z - radius, x + radius, z + radius],
                shape: Shape::Circle { x, z, r: radius },
            })
        }
        Region::Polygon(p) => {
            if p.points.len() < 3 || !p.points.iter().all(|q| finite(q)) {
                return Err(issue(path, "a polygon needs 3 or more finite points"));
            }
            let mut b = [
                f64::INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
            ];
            for &[x, z] in &p.points {
                b = [b[0].min(x), b[1].min(z), b[2].max(x), b[3].max(z)];
            }
            Ok(Area {
                bbox: b,
                shape: Shape::Polygon(p.points.clone()),
            })
        }
        Region::AirdromeBand {
            airdrome,
            min_dist,
            max_dist,
        } => {
            let (min_dist, max_dist) = (*min_dist, *max_dist);
            if !finite(&[min_dist, max_dist]) || min_dist < 0.0 || min_dist > max_dist {
                return Err(issue(
                    path,
                    "a band needs 0 <= min_dist <= max_dist, both finite",
                ));
            }
            let [x, z] = airdromes(src, Some(airdrome), &format!("{path}.airdrome"))?[0];
            Ok(Area {
                bbox: [x - max_dist, z - max_dist, x + max_dist, z + max_dist],
                shape: Shape::Ring {
                    x,
                    z,
                    min: min_dist,
                    max: max_dist,
                },
            })
        }
    }
}

/// The airdrome `r` names: a number is its id; a string is its `name_id`,
/// spelled as the files spell it or, failing that, the one that differs from
/// it only in case, and failing any, its id written in digits.
pub(super) fn airdrome<'a>(tables: &'a Tables, r: &AirdromeRef) -> Option<&'a AirdromeRow> {
    let all = &tables.airdromes;
    let by_id = |id: i64| all.iter().find(|a| a.id == Some(id));
    let name = match r {
        AirdromeRef::Id(id) => return by_id(*id),
        AirdromeRef::Name(name) => name,
    };
    if let Some(a) = named(all.iter().map(|a| (a.name_id.as_str(), a)), name) {
        return Some(a);
    }
    let digits = !name.is_empty() && name.bytes().all(|c| c.is_ascii_digit());
    digits.then(|| name.parse().ok()).flatten().and_then(by_id)
}

/// The airdrome `r` names, as `airdrome` finds it, or the issue, at `path`,
/// when none has that name.
pub(super) fn known_airdrome<'a>(
    tables: &'a Tables,
    r: &AirdromeRef,
    path: &str,
) -> Result<&'a AirdromeRow, Vec<Issue>> {
    airdrome(tables, r).ok_or_else(|| {
        let message = match r {
            AirdromeRef::Id(id) => format!("no airdrome has the id {id}"),
            AirdromeRef::Name(n) => format!("no airdrome has the name_id or id `{n}`"),
        };
        vec![Issue::new("unknown_airdrome", &message, path)]
    })
}

/// The reference point of the airdrome `r` names, or of every airdrome
/// placed when none is named; the issue, at `path`, when there is none to
/// give.
pub(super) fn airdromes(
    src: &dyn Source,
    r: Option<&AirdromeRef>,
    path: &str,
) -> Result<Vec<[f64; 2]>, Vec<Issue>> {
    let tables = src.tables().map_err(refused)?;
    let none = |message: String| vec![Issue::new("unknown_airdrome", &message, path)];
    let Some(r) = r else {
        let at: Vec<[f64; 2]> = tables
            .airdromes
            .iter()
            .filter_map(|a| Some([a.x?, a.z?]))
            .collect();
        if at.is_empty() {
            return Err(none(
                "no airdrome on this terrain has a runway to place it".to_string(),
            ));
        }
        return Ok(at);
    };
    let a = known_airdrome(tables, r, path)?;
    match (a.x, a.z) {
        (Some(x), Some(z)) => Ok(vec![[x, z]]),
        _ => Err(none(format!(
            "airdrome {} has no runway to place it",
            a.name_id
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth::{self, Synth};
    use crate::types::{BoundingBox, Polygon};

    fn synth() -> Synth {
        Synth::new(
            synth::DEFAULT_SEED,
            synth::DEFAULT_SIZE_KM,
            synth::CELL_SIZE_M,
        )
        .unwrap()
    }

    #[test]
    fn each_form_is_its_box_and_its_shape() {
        let s = synth();
        let b = resolve(
            &s,
            &Region::Box(BoundingBox {
                min_x: 0.0,
                min_z: 0.0,
                max_x: 10.0,
                max_z: 20.0,
            }),
            "region",
        )
        .unwrap();
        assert!(b.contains(10.0, 20.0) && !b.contains(10.1, 5.0));

        let c = resolve(
            &s,
            &Region::Circle {
                x: 0.0,
                z: 0.0,
                radius: 10.0,
            },
            "region",
        )
        .unwrap();
        assert_eq!(c.bbox, [-10.0, -10.0, 10.0, 10.0]);
        assert!(c.contains(6.0, 8.0) && !c.contains(8.0, 8.0));

        // An L, whose notch is in its box but not in it.
        let l = Polygon {
            points: vec![
                [0.0, 0.0],
                [10.0, 0.0],
                [10.0, 4.0],
                [4.0, 4.0],
                [4.0, 10.0],
                [0.0, 10.0],
            ],
        };
        let p = resolve(&s, &Region::Polygon(l), "region").unwrap();
        assert_eq!(p.bbox, [0.0, 0.0, 10.0, 10.0]);
        assert!(p.contains(2.0, 8.0) && p.contains(8.0, 2.0) && !p.contains(8.0, 8.0));

        let t = s.tables().unwrap();
        let (ax, az) = (t.airdromes[0].x.unwrap(), t.airdromes[0].z.unwrap());
        let ring = Region::AirdromeBand {
            airdrome: AirdromeRef::Id(synth::AIRDROME_ID.into()),
            min_dist: 1000.0,
            max_dist: 2000.0,
        };
        let r = resolve(&s, &ring, "region").unwrap();
        assert!(r.contains(ax + 1500.0, az) && !r.contains(ax + 500.0, az));
        assert!(!r.contains(ax + 1500.0, az + 1500.0));
    }

    #[test]
    fn an_id_in_digits_and_a_name_id_name_the_same_airdrome() {
        let s = synth();
        let t = s.tables().unwrap();
        let name = |n: &str| airdrome(t, &AirdromeRef::Name(n.into())).map(|a| &a.name_id);
        let id = synth::AIRDROME_ID.to_string();
        let want = Some(&t.airdromes[0].name_id);
        assert_eq!(name(synth::AIRDROME_NAME_ID), want);
        assert_eq!(name(&id), want);
        assert_eq!(name(&synth::AIRDROME_NAME_ID.to_lowercase()), want);
        let by_id = airdrome(t, &AirdromeRef::Id(synth::AIRDROME_ID.into()));
        assert_eq!(by_id.map(|a| &a.name_id), want);
        for n in ["99", "", "+1", "Nowhere"] {
            assert_eq!(name(n), None, "{n}");
        }
    }

    #[test]
    fn a_malformed_region_or_unknown_airdrome_is_an_issue() {
        let s = synth();
        let code = |r: Region| resolve(&s, &r, "region").unwrap_err()[0].code.clone();
        let flipped = BoundingBox {
            min_x: 1.0,
            min_z: 0.0,
            max_x: 0.0,
            max_z: 1.0,
        };
        assert_eq!(code(Region::Box(flipped)), "invalid_region");
        let two = Polygon {
            points: vec![[0.0, 0.0], [1.0, 1.0]],
        };
        assert_eq!(code(Region::Polygon(two)), "invalid_region");
        let nan = Region::Circle {
            x: f64::NAN,
            z: 0.0,
            radius: 1.0,
        };
        assert_eq!(code(nan), "invalid_region");
        let band = Region::AirdromeBand {
            airdrome: AirdromeRef::Id(99),
            min_dist: 0.0,
            max_dist: 1.0,
        };
        assert_eq!(code(band), "unknown_airdrome");
    }

    #[test]
    fn a_segment_meets_a_region_where_any_point_of_it_lies_in_it() {
        let shapes = [
            Area {
                bbox: [0.0, 0.0, 100.0, 50.0],
                shape: Shape::Box,
            },
            Area {
                bbox: [20.0, 20.0, 80.0, 80.0],
                shape: Shape::Circle {
                    x: 50.0,
                    z: 50.0,
                    r: 30.0,
                },
            },
            Area {
                bbox: [10.0, 10.0, 90.0, 90.0],
                shape: Shape::Ring {
                    x: 50.0,
                    z: 50.0,
                    min: 20.0,
                    max: 40.0,
                },
            },
            Area {
                bbox: [0.0, 0.0, 100.0, 100.0],
                shape: Shape::Polygon(vec![
                    [0.0, 0.0],
                    [100.0, 0.0],
                    [100.0, 40.0],
                    [40.0, 40.0],
                    [40.0, 100.0],
                    [0.0, 100.0],
                ]),
            },
        ];
        let mut state = 0x853c_49e6_748f_ea9b_u64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 11) as f64 / (1u64 << 53) as f64 * 160.0 - 30.0
        };
        let mut met = [0; 4];
        for _ in 0..4000 {
            let (a, b) = ([next(), next()], [next(), next()]);
            for (k, s) in shapes.iter().enumerate() {
                let sampled = (0..=4000).any(|i| {
                    let t = f64::from(i) / 4000.0;
                    s.contains(a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1]))
                });
                // Sampling can miss a corner the segment only clips.
                if sampled {
                    assert!(s.meets(a, b), "shape {k}: {a:?} {b:?}");
                    met[k] += 1;
                } else if s.meets(a, b) {
                    let fine = (0..=1_000_000).any(|i| {
                        let t = f64::from(i) / 1e6;
                        s.contains(a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1]))
                    });
                    assert!(fine, "shape {k}: {a:?} {b:?} meets nothing");
                }
            }
        }
        assert!(met.iter().all(|&m| m > 500), "{met:?}");
        // A segment wholly inside the ring's hole meets nothing.
        assert!(!shapes[2].meets([45.0, 45.0], [55.0, 52.0]));
        // One through the box's corner alone meets it; one passing beside it
        // does not.
        assert!(shapes[0].meets([90.0, 60.0], [110.0, 40.0]));
        assert!(!shapes[0].meets([100.0, 60.0], [110.0, 50.0]));
    }
}
