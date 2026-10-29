//! The `geojson` field every operation returning geometry fills: an RFC 7946
//! `FeatureCollection` in longitude and latitude, through the terrain's own
//! projection, so any map program draws the answer.

use serde_json::{Map, Value, json};

use super::{Terrain, refused};
use crate::crs::Tmerc;
use crate::types::Issue;

/// A ring of DCS `[x, z]` vertices, closed by its first point, which is not
/// repeated at its end.
pub type Ring = Vec<[f64; 2]>;

/// The features of one answer, in the order they are added.
pub struct Features {
    tm: Tmerc,
    features: Vec<Value>,
}

/// Degrees to seven places, about a centimeter: finer says nothing more.
fn round(v: f64) -> f64 {
    (v * 1e7).round() / 1e7
}

impl Features {
    pub fn new(terrain: &Terrain) -> Result<Features, Vec<Issue>> {
        Ok(Features {
            tm: terrain.source().tables().map_err(refused)?.crs.tmerc(),
            features: Vec::new(),
        })
    }

    /// RFC 7946 orders a position longitude first.
    fn position(&self, [x, z]: [f64; 2]) -> [f64; 2] {
        let (lat, lon) = self.tm.to_latlon(x, z);
        [round(lon), round(lat)]
    }

    /// A ring as positions, closed, counterclockwise when `outer` and
    /// clockwise when not, as RFC 7946 asks of an exterior ring and a hole.
    fn ring(&self, ring: &[[f64; 2]], outer: bool) -> Vec<[f64; 2]> {
        let mut p: Vec<_> = ring.iter().map(|&v| self.position(v)).collect();
        if (signed_area(&p) > 0.0) != outer {
            p.reverse();
        }
        p.push(p[0]);
        p
    }

    fn push(&mut self, geometry: Value, properties: Map<String, Value>) {
        self.features.push(json!({
            "type": "Feature",
            "geometry": geometry,
            "properties": properties,
        }));
    }

    pub fn point(&mut self, p: [f64; 2], properties: Map<String, Value>) {
        let g = json!({"type": "Point", "coordinates": self.position(p)});
        self.push(g, properties);
    }

    pub fn line(&mut self, points: &[[f64; 2]], properties: Map<String, Value>) {
        let c: Vec<_> = points.iter().map(|&v| self.position(v)).collect();
        self.push(json!({"type": "LineString", "coordinates": c}), properties);
    }

    /// One `MultiPolygon`, each polygon its exterior ring then its holes,
    /// whichever way each ring runs.
    pub fn polygons(&mut self, polygons: &[Vec<Ring>], properties: Map<String, Value>) {
        let c: Vec<Vec<_>> = polygons
            .iter()
            .map(|rings| {
                rings
                    .iter()
                    .enumerate()
                    .map(|(i, r)| self.ring(r, i == 0))
                    .collect()
            })
            .collect();
        self.push(
            json!({"type": "MultiPolygon", "coordinates": c}),
            properties,
        );
    }

    pub fn finish(self) -> Value {
        json!({"type": "FeatureCollection", "features": self.features})
    }
}

/// Twice the signed area of an open ring in its own plane, positive
/// counterclockwise with the first axis to the right of the second.
pub fn signed_area(ring: &[[f64; 2]]) -> f64 {
    let n = ring.len();
    (0..n)
        .map(|i| {
            let ([a0, a1], [b0, b1]) = (ring[i], ring[(i + 1) % n]);
            a0 * b1 - b0 * a1
        })
        .sum()
}

/// Properties from pairs, in order.
pub fn props<const N: usize>(pairs: [(&str, Value); N]) -> Map<String, Value> {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::super::tests::synth;
    use super::*;

    /// Every structural rule of RFC 7946 an answer here can break: the
    /// collection, each feature's members, positions as longitude and
    /// latitude in range, a line of two positions or more, and rings closed,
    /// of four positions or more, exteriors counterclockwise and holes
    /// clockwise.
    pub fn assert_valid(v: &Value) {
        assert_eq!(v["type"], "FeatureCollection");
        let position = |p: &Value| {
            let p = p.as_array().unwrap();
            assert_eq!(p.len(), 2, "{p:?}");
            let (lon, lat) = (p[0].as_f64().unwrap(), p[1].as_f64().unwrap());
            assert!((-180.0..=180.0).contains(&lon) && (-90.0..=90.0).contains(&lat));
            [lon, lat]
        };
        for f in v["features"].as_array().unwrap() {
            assert_eq!(f["type"], "Feature");
            assert!(f["properties"].is_object());
            let g = &f["geometry"];
            let c = &g["coordinates"];
            match g["type"].as_str().unwrap() {
                "Point" => {
                    position(c);
                }
                "LineString" => {
                    let c = c.as_array().unwrap();
                    assert!(c.len() >= 2);
                    c.iter().for_each(|p| {
                        position(p);
                    });
                }
                "MultiPolygon" => {
                    for poly in c.as_array().unwrap() {
                        for (i, ring) in poly.as_array().unwrap().iter().enumerate() {
                            let r: Vec<_> = ring.as_array().unwrap().iter().map(position).collect();
                            assert!(r.len() >= 4);
                            assert_eq!(r[0], r[r.len() - 1], "a ring is closed");
                            let a = signed_area(&r[..r.len() - 1]);
                            assert_eq!(a > 0.0, i == 0, "exterior ccw, holes cw");
                        }
                    }
                }
                t => panic!("geometry {t}"),
            }
        }
    }

    #[test]
    fn writes_a_valid_collection_that_projects_back() {
        let t = synth();
        let mut f = Features::new(&t).unwrap();
        f.point([1000.0, 2000.0], props([("name", json!("a"))]));
        f.line(&[[0.0, 0.0], [500.0, 500.0]], Map::new());
        // A square with a square hole, both given clockwise in (lon, lat).
        let outer = vec![[0.0, 0.0], [0.0, 1000.0], [1000.0, 1000.0], [1000.0, 0.0]];
        let hole = vec![
            [250.0, 250.0],
            [250.0, 750.0],
            [750.0, 750.0],
            [750.0, 250.0],
        ];
        f.polygons(&[vec![outer.clone(), hole.clone()]], Map::new());
        f.polygons(&[vec![outer.into_iter().rev().collect(), hole]], Map::new());
        let v = f.finish();
        assert_valid(&v);
        assert_eq!(v["features"][0]["properties"]["name"], "a");
        let p = &v["features"][0]["geometry"]["coordinates"];
        let tm = t.source().tables().unwrap().crs.tmerc();
        let (x, z) = tm.to_xz(p[1].as_f64().unwrap(), p[0].as_f64().unwrap());
        assert!((x - 1000.0).abs() < 0.05 && (z - 2000.0).abs() < 0.05);
    }
}
