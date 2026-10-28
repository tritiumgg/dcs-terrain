//! The extract's `runways.json` and `stands.json`, from every airfield's
//! decoded `.rn4` and `.rn5` and the map's stand classes.
//!
//! An airdrome's numeric id comes from the map's Lua files or the hook, not
//! from these, so every row is made with it null for the run to fill in.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::Airfield;
use crate::decode::assets::{self, StandParams};
use crate::decode::install::Refusal;
use crate::decode::rn5::{self, StandClasses};
use crate::extract::FileEntry;

pub const RUNWAYS_FILE: &str = "runways.json";
pub const STANDS_FILE: &str = "stands.json";

/// What the game flags a stand helicopters may use, and one they may not.
const FLAG_HELICOPTERS: u32 = 64;
const FLAG_AIRPLANES_ONLY: u32 = 32;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
/// value is null for a stand the files give no class.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
/// class's parameters from `params`. A named node whose class has no
/// stand record in `assets.assets` is not a stand: a holding point, say.
pub fn stand_rows(
    name_id: &str,
    a: &Airfield,
    classes: Option<&StandClasses>,
    params: &BTreeMap<String, StandParams>,
) -> Vec<StandRow> {
    a.stands
        .iter()
        .filter_map(|&i| {
            let n = a.node(i);
            let [x, _, z] = n.position();
            let class = classes.and_then(|c| c.of(i));
            let p = class.and_then(|c| params.get(c));
            if class.is_some() && p.is_none() {
                return None;
            }
            Some(StandRow {
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
            })
        })
        .collect()
}

/// Every airfield of a map, as rows.
#[derive(Debug, Clone, PartialEq)]
pub struct Airfields {
    pub runways: Vec<RunwayRow>,
    pub stands: Vec<StandRow>,
}

/// The basename of a path relative to the install, without its extension.
fn stem(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.').map_or(name, |(s, _)| s)
}

/// Decodes every airfield among a map's `files`, relative to `install`.
/// Each `.rn4` takes the `.rn5` of the same basename, without regard to
/// case, and the map's `assets.assets` gives the classes' parameters. An
/// airdrome's `name_id` is its `.rn4`'s basename.
pub fn decode(install: &Path, files: &[FileEntry]) -> Result<Airfields, Refusal> {
    let role = |r: &'static str| files.iter().filter(move |f| f.role == r);
    let params = match role("assets").next() {
        Some(f) => {
            assets::read(&install.join(&f.path)).map_err(|e| Refusal(format!("{}: {e}", f.path)))?
        }
        // Every classed stand would lack its record and be left out.
        None if role("airfield_rn5").next().is_some() => {
            return Err(Refusal(
                "the map has airfield .rn5 files but no misc/assets.assets to give their stand classes"
                    .into(),
            ));
        }
        None => BTreeMap::new(),
    };
    let mut out = Airfields {
        runways: Vec::new(),
        stands: Vec::new(),
    };
    for f in role("airfield_rn4") {
        let name_id = stem(&f.path);
        let a =
            super::read(&install.join(&f.path)).map_err(|e| Refusal(format!("{}: {e}", f.path)))?;
        let classes =
            match role("airfield_rn5").find(|g| stem(&g.path).eq_ignore_ascii_case(name_id)) {
                Some(g) => Some(
                    rn5::read(&install.join(&g.path), &a)
                        .map_err(|e| Refusal(format!("{}: {e}", g.path)))?,
                ),
                None => None,
            };
        out.runways.extend(runway_rows(name_id, &a));
        out.stands
            .extend(stand_rows(name_id, &a, classes.as_ref(), &params));
    }
    Ok(out)
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

    fn airfield_file() -> Vec<u8> {
        let node = |name, x, z, links: &[u32]| build::Node {
            name: Some(name),
            at: [x, 20.0, z],
            heading: 0.25,
            links: links.iter().map(|&j| (j, 100.0)).collect(),
        };
        file(
            &["vpp"],
            &[
                node("1#22", -4427.5, 244_000.0, &[]),
                node("40", -4829.5, 244_622.0, &[3]),
                node("90", -5155.0, 244_536.25, &[3]),
                node("04", -6396.5, 242_258.0, &[1, 2]),
            ],
        )
    }

    fn airfield() -> Airfield {
        parse(&airfield_file(), "Anapa.rn4").unwrap()
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
    fn a_stand_row_takes_its_class_parameters_and_a_class_with_none_is_no_stand() {
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
        // Node 2's class has no record, so it is not a stand.
        assert_eq!(got.as_array().unwrap().len(), 1);

        // With no class at all, both are written, with no parameters.
        let rows = stand_rows("Anapa", &airfield(), None, &params());
        assert_eq!(rows.len(), 2);
        assert_eq!((rows[0].stand_class.as_deref(), rows[0].flag), (None, None));
        let p = serde_json::to_value(&rows[1].params).unwrap();
        assert!(p.as_object().unwrap().values().all(Value::is_null));
    }

    #[test]
    fn a_stand_helicopters_may_use_is_flagged_64() {
        let mut p = params();
        p.get_mut("stand_shelter").unwrap().for_helicopters = true;
        let rows = stand_rows("Anapa", &airfield(), Some(&classes()), &p);
        assert_eq!(rows[0].flag, Some(64));
    }

    fn entry(role: &str, path: &str) -> FileEntry {
        FileEntry {
            role: role.into(),
            path: path.into(),
            class: None,
            version: None,
            size: 0,
            payload_size: None,
        }
    }

    #[test]
    fn decodes_each_airfield_with_the_rn5_of_its_name_in_any_case() {
        let s = Scratch::new();
        let dir = s.0.join("Mods/terrains/X/AirfieldsTaxiways");
        fs::create_dir_all(&dir).unwrap();
        fs::create_dir_all(s.0.join("Mods/terrains/X/misc")).unwrap();
        fs::write(dir.join("Anapa.rn4"), airfield_file()).unwrap();
        fs::write(
            dir.join("ANAPA.rn5"),
            rn5::build::file(&[(0, 1, &[], -4829.5, 244_622.0)]),
        )
        .unwrap();
        let mut a = vec![0; 8];
        assets::build::record(
            &mut a,
            "stand_end_24m",
            &[("height", "8")],
            &[[-11.0, -13.0], [11.0, 13.0]],
        );
        fs::write(s.0.join("Mods/terrains/X/misc/assets.assets"), a).unwrap();
        let files = [
            entry(
                "airfield_rn4",
                "Mods/terrains/X/AirfieldsTaxiways/Anapa.rn4",
            ),
            entry(
                "airfield_rn5",
                "Mods/terrains/X/AirfieldsTaxiways/ANAPA.rn5",
            ),
            entry("assets", "Mods/terrains/X/misc/assets.assets"),
        ];
        let got = decode(&s.0, &files).unwrap();
        assert_eq!(got.runways.len(), 1);
        assert_eq!(got.runways[0].airdrome_name_id, "Anapa");
        let stands: Vec<_> = got
            .stands
            .iter()
            .map(|r| {
                (
                    r.crossroad_index,
                    r.stand_class.as_deref(),
                    r.params.WIDTH,
                    r.flag,
                )
            })
            .collect();
        assert_eq!(
            stands,
            [
                (1, Some("stand_end_24m"), Some(22.0), Some(64)),
                (2, None, None, None)
            ]
        );

        // Without its rn5 or the assets, the stands are there unclassed.
        let got = decode(&s.0, &files[..1]).unwrap();
        assert_eq!(got.stands.len(), 2);
        assert!(got.stands.iter().all(|r| r.stand_class.is_none()));

        // With its rn5 but no assets, every classed stand would be lost.
        let e = decode(&s.0, &files[..2]).unwrap_err();
        assert!(e.0.contains("no misc/assets.assets"), "{e}");
    }

    #[test]
    fn a_file_that_will_not_decode_is_refused_by_name() {
        let s = Scratch::new();
        fs::create_dir_all(&s.0).unwrap();
        fs::write(s.0.join("Bad.rn4"), b"not a container").unwrap();
        let e = decode(&s.0, &[entry("airfield_rn4", "Bad.rn4")]).unwrap_err();
        assert!(e.0.starts_with("Bad.rn4: not a container"), "{e}");
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
