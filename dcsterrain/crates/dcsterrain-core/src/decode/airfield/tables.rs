//! The extract's `runways.json` and `stands.json`, from every airfield's
//! decoded `.rn4` and `.rn5` and the map's stand classes.
//!
//! An airdrome's numeric id comes from the map's Lua files or the hook, not
//! from these, so every row is made with it null for the run to fill in.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::Path;

use serde::Serialize;

use super::Airfield;
use crate::decode::assets::StandParams;
use crate::decode::rn5::StandClasses;

pub const RUNWAYS_FILE: &str = "runways.json";
pub const STANDS_FILE: &str = "stands.json";

/// What the game flags a stand helicopters may use, and one they may not.
const FLAG_HELICOPTERS: u32 = 64;
const FLAG_AIRPLANES_ONLY: u32 = 32;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RunwayRow {
    pub airdrome_id: Option<i64>,
    pub airdrome_name_id: String,
    pub name: String,
    pub edge1_name: String,
    pub edge1_x: f64,
    pub edge1_z: f64,
    pub edge2_name: String,
    pub edge2_x: f64,
    pub edge2_z: f64,
    pub course: f64,
}

/// A stand class's parameters under the names the game gives them. Every
/// value is null for a stand whose class has no record.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[allow(non_snake_case)]
pub struct Params {
    /// No file says whether a stand is a shelter.
    pub SHELTER: Option<bool>,
    pub FOR_HELICOPTERS: Option<bool>,
    pub FOR_AIRPLANES: Option<bool>,
    pub WIDTH: Option<f64>,
    pub LENGTH: Option<f64>,
    pub HEIGHT: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StandRow {
    pub airdrome_id: Option<i64>,
    pub airdrome_name_id: String,
    pub crossroad_index: u32,
    pub name: String,
    pub flag: Option<u32>,
    pub x: f64,
    pub z: f64,
    pub stand_class: Option<String>,
    pub heading: f64,
    pub params: Params,
}

/// The rows of each of `a`'s runways.
pub fn runway_rows(name_id: &str, a: &Airfield) -> Vec<RunwayRow> {
    a.runways
        .iter()
        .map(|r| {
            let ([x1, _, z1], [x2, _, z2]) =
                (a.node(r.edge1).position(), a.node(r.edge2).position());
            RunwayRow {
                airdrome_id: None,
                airdrome_name_id: name_id.to_string(),
                name: a.runway_name(r),
                edge1_name: a.name(r.edge1).to_string(),
                edge1_x: x1,
                edge1_z: z1,
                edge2_name: a.name(r.edge2).to_string(),
                edge2_x: x2,
                edge2_z: z2,
                course: a.course(r),
            }
        })
        .collect()
}

/// The rows of each of `a`'s stands, its class from `classes` and that
/// class's parameters from `params`.
pub fn stand_rows(
    name_id: &str,
    a: &Airfield,
    classes: Option<&StandClasses>,
    params: &BTreeMap<String, StandParams>,
) -> Vec<StandRow> {
    a.stands
        .iter()
        .map(|&i| {
            let n = a.node(i);
            let [x, _, z] = n.position();
            let class = classes.and_then(|c| c.of(i));
            let p = class.and_then(|c| params.get(c));
            StandRow {
                airdrome_id: None,
                airdrome_name_id: name_id.to_string(),
                crossroad_index: i,
                name: a.name(i).to_string(),
                flag: p.map(|p| {
                    if p.for_helicopters {
                        FLAG_HELICOPTERS
                    } else {
                        FLAG_AIRPLANES_ONLY
                    }
                }),
                x,
                z,
                stand_class: class.map(str::to_string),
                heading: n.heading(),
                params: Params {
                    SHELTER: None,
                    FOR_HELICOPTERS: p.map(|p| p.for_helicopters),
                    FOR_AIRPLANES: p.map(|p| p.for_airplanes),
                    WIDTH: p.map(|p| p.width),
                    LENGTH: p.map(|p| p.length),
                    HEIGHT: p.map(|p| p.height.clone()),
                },
            }
        })
        .collect()
}

/// Writes `rows` to `path` as one JSON array, by way of a `.tmp` renamed
/// into place.
pub fn write_rows<T: Serialize>(path: &Path, rows: &[T]) -> io::Result<()> {
    let tmp = path.with_file_name(format!(
        "{}.tmp",
        path.file_name().unwrap().to_string_lossy()
    ));
    let mut out = BufWriter::new(File::create(&tmp)?);
    serde_json::to_writer(&mut out, rows)?;
    out.write_all(b"\n")?;
    out.into_inner().map_err(|e| e.into_error())?.sync_all()?;
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::super::build::{self, file};
    use super::super::parse;
    use super::*;
    use crate::scratch::Scratch;

    fn airfield() -> Airfield {
        let node = |name, x, z, links: &[u32]| build::Node {
            name: Some(name),
            at: [x, 20.0, z],
            heading: 0.25,
            links: links.iter().map(|&j| (j, 100.0)).collect(),
        };
        let b = file(
            &["vpp"],
            &[
                node("1#22", -4427.5, 244_000.0, &[]),
                node("40", -4829.5, 244_622.0, &[3]),
                node("90", -5155.0, 244_536.25, &[3]),
                node("04", -6396.5, 242_258.0, &[1, 2]),
            ],
        );
        parse(&b, "Anapa.rn4").unwrap()
    }

    fn classes() -> StandClasses {
        StandClasses {
            classes: vec!["stand_shelter".into(), "stand_vpp_no_takeoff".into()],
            by_node: [(1, 0), (2, 1)].into(),
        }
    }

    fn params() -> BTreeMap<String, StandParams> {
        let p = StandParams {
            width: 67.096466,
            length: 78.722809,
            height: "18".into(),
            for_airplanes: true,
            for_helicopters: false,
        };
        [("stand_shelter".to_string(), p)].into()
    }

    #[test]
    fn a_runway_row_is_its_paired_ends_and_course() {
        let rows = runway_rows("Anapa", &airfield());
        assert_eq!(rows.len(), 1);
        let r = &rows[0];
        assert_eq!(
            (
                r.name.as_str(),
                r.edge1_name.as_str(),
                r.edge2_name.as_str()
            ),
            ("22-04", "22", "04")
        );
        assert_eq!(
            [r.edge1_x, r.edge1_z, r.edge2_x, r.edge2_z],
            [-4427.5, 244_000.0, -6396.5, 242_258.0]
        );
        assert_eq!(r.course, (242_258.0f64 - 244_000.0).atan2(-6396.5 + 4427.5));
        assert_eq!(
            (r.airdrome_id, r.airdrome_name_id.as_str()),
            (None, "Anapa")
        );
    }

    #[test]
    fn a_stand_row_takes_its_class_parameters_and_nulls_where_none() {
        let rows = stand_rows("Anapa", &airfield(), Some(&classes()), &params());
        let got = serde_json::to_value(&rows).unwrap();
        assert_eq!(
            got[0],
            json!({
                "airdrome_id": null, "airdrome_name_id": "Anapa", "crossroad_index": 1,
                "name": "40", "flag": 32, "x": -4829.5, "z": 244_622.0,
                "stand_class": "stand_shelter", "heading": 0.25,
                "params": {
                    "SHELTER": null, "FOR_HELICOPTERS": false, "FOR_AIRPLANES": true,
                    "WIDTH": 67.096466, "LENGTH": 78.722809, "HEIGHT": "18",
                },
            })
        );
        // A class with no record keeps its name and has no parameters.
        assert_eq!(got[1]["stand_class"], "stand_vpp_no_takeoff");
        assert_eq!(got[1]["flag"], Value::Null);
        assert!(
            got[1]["params"]
                .as_object()
                .unwrap()
                .values()
                .all(Value::is_null)
        );

        let rows = stand_rows("Anapa", &airfield(), None, &params());
        assert_eq!((rows[0].stand_class.as_deref(), rows[0].flag), (None, None));
    }

    #[test]
    fn a_stand_helicopters_may_use_is_flagged_64() {
        let mut p = params();
        p.get_mut("stand_shelter").unwrap().for_helicopters = true;
        let rows = stand_rows("Anapa", &airfield(), Some(&classes()), &p);
        assert_eq!(rows[0].flag, Some(64));
    }

    #[test]
    fn writes_an_array_and_leaves_no_tmp() {
        let s = Scratch::new();
        fs::create_dir_all(&s.0).unwrap();
        let path = s.0.join(RUNWAYS_FILE);
        write_rows(&path, &runway_rows("Anapa", &airfield())).unwrap();
        let v: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v[0]["name"], "22-04");
        assert_eq!(fs::read_dir(&s.0).unwrap().count(), 1);
    }
}
