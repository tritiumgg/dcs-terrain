//! `geo`: DCS x and z to latitude and longitude and back, through the
//! terrain's projection.

use super::{Terrain, mgrs, refused};
use crate::types::{GeoPoint, GeoRequest, GeoResponse, Issue, Point};

pub fn geo(terrain: &Terrain, q: GeoRequest) -> Result<GeoResponse, Vec<Issue>> {
    let mut issues = Vec::new();
    for (k, p) in q.to_latlon.iter().enumerate() {
        if !(p.x.is_finite() && p.z.is_finite()) {
            issues.push(Issue::new(
                "invalid_point",
                "x and z must be finite",
                &format!("to_latlon.{k}"),
            ));
        }
    }
    for (k, p) in q.to_xz.iter().enumerate() {
        if !((-90.0..=90.0).contains(&p.lat) && p.lon.is_finite()) {
            issues.push(Issue::new(
                "invalid_point",
                "lat must lie in -90..90 and lon be finite",
                &format!("to_xz.{k}"),
            ));
        }
    }
    if !issues.is_empty() {
        return Err(issues);
    }
    let tm = terrain.source().tables().map_err(refused)?.crs.tmerc();
    let mut to_latlon = Vec::with_capacity(q.to_latlon.len());
    for (k, p) in q.to_latlon.iter().enumerate() {
        let (lat, lon) = tm.to_latlon(p.x, p.z);
        if !(lat.is_finite() && lon.is_finite()) {
            return Err(vec![Issue::new(
                "invalid_point",
                "the terrain's projection has no position this far out",
                &format!("to_latlon.{k}"),
            )]);
        }
        to_latlon.push(GeoPoint {
            lat,
            lon,
            mgrs: mgrs(lat, lon),
        });
    }
    Ok(GeoResponse {
        provenance: terrain.provenance(),
        to_latlon,
        to_xz: q
            .to_xz
            .iter()
            .map(|p| {
                let (x, z) = tm.to_xz(p.lat, p.lon);
                Point { x, z }
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::super::answer;
    use super::super::tests::synth;
    use serde_json::json;

    #[test]
    fn a_point_past_the_projection_is_refused() {
        let far = json!({"to_latlon": [{"x": 0, "z": 0}, {"x": 1e308, "z": 1e308}]});
        let e = answer(&synth(), "geo", &far).unwrap_err();
        assert_eq!(e[0].code, "invalid_point");
        assert_eq!(e[0].path, "to_latlon.1");
    }

    #[test]
    fn converts_both_ways_through_the_tables_projection() {
        let t = synth();
        let tables = t.source().tables().unwrap();
        let a = &tables.airdromes[0];
        let v = answer(
            &t,
            "geo",
            &json!({
                "to_latlon": [{"x": a.x, "z": a.z}, {"x": 0, "z": 0}],
                "to_xz": [{"lat": a.lat, "lon": a.lon}]
            }),
        )
        .unwrap();
        let ll = &v["to_latlon"][0];
        assert!((ll["lat"].as_f64().unwrap() - a.lat.unwrap()).abs() < 1e-9);
        assert!((ll["lon"].as_f64().unwrap() - a.lon.unwrap()).abs() < 1e-9);
        let m = ll["mgrs"].as_str().unwrap();
        // The synthetic terrain carries Caucasus's projection, at about 45° N.
        assert!(m.starts_with("36T ") || m.starts_with("37T "), "{m}");
        let xz = &v["to_xz"][0];
        assert!((xz["x"].as_f64().unwrap() - a.x.unwrap()).abs() < 1e-3);
        assert!((xz["z"].as_f64().unwrap() - a.z.unwrap()).abs() < 1e-3);
        assert_eq!(v["to_latlon"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn a_bad_coordinate_is_an_issue_naming_it() {
        let e = answer(
            &synth(),
            "geo",
            &json!({"to_xz": [{"lat": 0, "lon": 0}, {"lat": 91, "lon": 0}]}),
        )
        .unwrap_err();
        assert_eq!(
            (e[0].code.as_str(), e[0].path.as_str()),
            ("invalid_point", "to_xz.1")
        );
    }
}
