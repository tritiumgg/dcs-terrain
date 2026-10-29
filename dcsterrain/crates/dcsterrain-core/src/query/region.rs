//! A request's region as its bounding box and a membership test.

use super::refused;
use crate::source::Source;
use crate::types::{Issue, Region};

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
        &Region::AirdromeBand {
            airdrome_id,
            min_dist,
            max_dist,
        } => {
            if !finite(&[min_dist, max_dist]) || min_dist < 0.0 || min_dist > max_dist {
                return Err(issue(
                    path,
                    "a band needs 0 <= min_dist <= max_dist, both finite",
                ));
            }
            let [x, z] = airdromes(src, Some(airdrome_id), &format!("{path}.airdrome_id"))?[0];
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

/// The reference point of the airdrome `id`, or of every airdrome placed
/// when none is named; the issue, at `path`, when there is none to give.
pub(super) fn airdromes(
    src: &dyn Source,
    id: Option<i64>,
    path: &str,
) -> Result<Vec<[f64; 2]>, Vec<Issue>> {
    let tables = src.tables().map_err(refused)?;
    let none = |message: String| vec![Issue::new("unknown_airdrome", &message, path)];
    let Some(id) = id else {
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
    let a = tables
        .airdromes
        .iter()
        .find(|a| a.id == Some(id))
        .ok_or_else(|| none(format!("no airdrome has the id {id}")))?;
    match (a.x, a.z) {
        (Some(x), Some(z)) => Ok(vec![[x, z]]),
        _ => Err(none(format!("airdrome {id} has no runway to place it"))),
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
            airdrome_id: synth::AIRDROME_ID.into(),
            min_dist: 1000.0,
            max_dist: 2000.0,
        };
        let r = resolve(&s, &ring, "region").unwrap();
        assert!(r.contains(ax + 1500.0, az) && !r.contains(ax + 500.0, az));
        assert!(!r.contains(ax + 1500.0, az + 1500.0));
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
            airdrome_id: 99,
            min_dist: 0.0,
            max_dist: 1.0,
        };
        assert_eq!(code(band), "unknown_airdrome");
    }
}
