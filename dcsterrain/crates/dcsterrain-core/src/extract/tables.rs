//! The extract's JSON and JSON Lines tables: each must parse and carry its
//! keys. A key whose value is null is present; only an absent one fails,
//! because a run writes null for the fields it could not read.

use std::fs;
use std::io::{self, BufRead, BufReader};
use std::path::{Component, Path};

use serde_json::Value;

use super::{Extract, Failure};

/// How a table's file holds its rows.
#[derive(Clone, Copy)]
enum Shape {
    /// One object.
    Object,
    /// An array of objects.
    Array,
    /// One object per line.
    Lines,
}

/// Each table the manifest's `tables` must name, its shape, and the keys of
/// every row.
const TABLES: [(&str, Shape, &[&str]); 13] = [
    (
        "config",
        Shape::Object,
        &[
            "id",
            "crs",
            "latlon_samples",
            "bounds_km",
            "default_bullseye",
            "sea_enabled",
            "default_camera_km",
            "summer_time_delta",
            "shape",
        ],
    ),
    (
        "airdromes",
        Shape::Array,
        &[
            "id",
            "name_id",
            "display_name",
            "names",
            "x",
            "z",
            "lat",
            "lon",
            "runway_names",
            "beacon_ids",
            "radio_ids",
            "roadnet",
            "roadnet5",
            "towers",
            "warehouses",
            "fueldepots",
            "shelters",
        ],
    ),
    (
        "runways",
        Shape::Array,
        &[
            "airdrome_id",
            "airdrome_name_id",
            "name",
            "edge1_name",
            "edge1_x",
            "edge1_z",
            "edge2_name",
            "edge2_x",
            "edge2_z",
            "course",
        ],
    ),
    (
        "stands",
        Shape::Array,
        &[
            "airdrome_id",
            "airdrome_name_id",
            "crossroad_index",
            "name",
            "flag",
            "x",
            "z",
            "stand_class",
            "heading",
            "params",
        ],
    ),
    (
        "beacons",
        Shape::Array,
        &[
            "beacon_id",
            "callsign",
            "display_name",
            "type",
            "frequency_hz",
            "channel",
            "direction",
            "x",
            "alt",
            "z",
            "lat",
            "lon",
            "scene_objects",
        ],
    ),
    (
        "radio",
        Shape::Array,
        &[
            "radio_id",
            "callsigns",
            "roles",
            "frequencies_hz",
            "scene_objects",
        ],
    ),
    (
        "towns",
        Shape::Array,
        &["name", "display_name", "lat", "lon", "x", "z"],
    ),
    ("nodes", Shape::Array, &["id", "name", "red", "blue"]),
    ("roads", Shape::Lines, &["id", "class", "a", "b", "points"]),
    (
        "railroads",
        Shape::Lines,
        &["id", "class", "a", "b", "points"],
    ),
    (
        "scenery",
        Shape::Lines,
        &["id", "model", "x", "alt", "z", "rotation"],
    ),
    ("scenery_models", Shape::Array, &["model", "counts", "box"]),
    ("ground", Shape::Lines, &["class", "rings"]),
];

pub(super) fn check_tables(x: &Extract, out: &mut Vec<Failure>) {
    let named = &x.manifest.tables;
    for (key, shape, keys) in TABLES {
        let Some(name) = named.get(key) else {
            out.push(Failure::new(super::MANIFEST, format!("tables lacks {key}")));
            continue;
        };
        // A table sits beside the manifest. A separator, a parent step or,
        // on Windows, a drive prefix such as `C:x.json` would point anywhere.
        let mut parts = Path::new(name).components();
        let plain = matches!(
            (parts.next(), parts.next()),
            (Some(Component::Normal(_)), None)
        );
        if !plain || name.contains([':', '/', '\\']) {
            out.push(Failure::new(
                super::MANIFEST,
                format!("tables.{key} is {name:?}, not a file name"),
            ));
            continue;
        }
        let path = x.dir.join(name);
        // A device name such as `CON` is no file, and reading one can block.
        if !path.is_file() {
            out.push(Failure::new(name, "not a file in the extract"));
            continue;
        }
        let mut keyed = Keyed::new(keys);
        if let Err(why) = read_rows(&path, shape, &mut keyed) {
            out.push(Failure::new(name, why));
        } else if let Some(why) = keyed.report() {
            out.push(Failure::new(name, why));
        }
    }
}

/// Feeds each row to `keyed`, with its 1-based position: the array index or
/// the line number. A JSON Lines table can run to millions of rows, so it is
/// read a line at a time and no row is kept.
fn read_rows(path: &Path, shape: Shape, keyed: &mut Keyed) -> Result<(), String> {
    let cannot = |e: io::Error| format!("cannot read: {e}");
    let parse = |s: &str| serde_json::from_str::<Value>(s).map_err(|e| format!("not JSON: {e}"));
    match shape {
        Shape::Object => keyed.row(1, &parse(&fs::read_to_string(path).map_err(cannot)?)?),
        Shape::Array => match parse(&fs::read_to_string(path).map_err(cannot)?)? {
            Value::Array(a) => a.iter().zip(1..).for_each(|(v, n)| keyed.row(n, v)),
            _ => return Err("not a JSON array".into()),
        },
        Shape::Lines => {
            let file = fs::File::open(path).map_err(cannot)?;
            for (line, n) in BufReader::new(file).lines().zip(1..) {
                let line = line.map_err(cannot)?;
                if line.trim().is_empty() {
                    continue;
                }
                let v = serde_json::from_str(&line)
                    .map_err(|e| format!("line {n} is not JSON: {e}"))?;
                keyed.row(n, &v);
            }
        }
    }
    Ok(())
}

/// The first row that lacks a key, and how many more do, so one bad writer
/// does not bury the report under a line per row.
struct Keyed<'a> {
    keys: &'a [&'a str],
    first: Option<String>,
    more: usize,
}

impl<'a> Keyed<'a> {
    fn new(keys: &'a [&'a str]) -> Self {
        Keyed {
            keys,
            first: None,
            more: 0,
        }
    }

    fn row(&mut self, n: usize, row: &Value) {
        let why = match row.as_object() {
            None => format!("row {n} is not an object"),
            Some(obj) => {
                let missing: Vec<&str> = self
                    .keys
                    .iter()
                    .copied()
                    .filter(|k| !obj.contains_key(*k))
                    .collect();
                if missing.is_empty() {
                    return;
                }
                format!("row {n} lacks {}", missing.join(", "))
            }
        };
        if self.first.is_none() {
            self.first = Some(why);
        } else {
            self.more += 1;
        }
    }

    fn report(&self) -> Option<String> {
        let first = self.first.as_ref()?;
        Some(if self.more > 0 {
            format!("{first}, and {} more rows are wrong", self.more)
        } else {
            first.clone()
        })
    }
}

/// Writes one row of every table, each key null, and returns the manifest's
/// `tables`. A Lines table gets two rows.
#[cfg(test)]
pub(super) fn write_rows(dir: &Path) -> serde_json::Map<String, Value> {
    let mut named = serde_json::Map::new();
    for (key, shape, keys) in TABLES {
        let row: serde_json::Map<String, Value> =
            keys.iter().map(|k| (k.to_string(), Value::Null)).collect();
        let row = Value::Object(row);
        let (name, text) = match shape {
            Shape::Object => (format!("{key}.json"), row.to_string()),
            Shape::Array => (format!("{key}.json"), Value::Array(vec![row]).to_string()),
            Shape::Lines => (
                format!("{key}.jsonl"),
                format!(
                    "{row}
{row}
"
                ),
            ),
        };
        fs::write(dir.join(&name), text).unwrap();
        named.insert(key.to_string(), name.into());
    }
    named
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::MANIFEST;
    use crate::extract::check::tests::{edit_manifest, failures, fixture_extract, has};

    #[test]
    fn refuses_a_row_that_lacks_a_key() {
        let s = fixture_extract();
        fs::write(
            s.0.join("towns.json"),
            r#"[{"name": "A", "display_name": "A", "lat": 0, "lon": 0, "x": 0, "z": 0},
                {"name": "B", "lat": 0, "lon": 0, "x": 0, "z": 0},
                {"name": "C"}]"#,
        )
        .unwrap();
        let f = failures(&s.0);
        assert!(
            has(&f, "towns.json", "row 2 lacks display_name, and 1 more"),
            "{f:?}"
        );
    }

    #[test]
    fn a_null_field_is_present() {
        let s = fixture_extract();
        let path = s.0.join("config.json");
        let mut c: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        c["bounds_km"] = Value::Null;
        fs::write(&path, c.to_string()).unwrap();
        assert!(crate::extract::check(&s.0).is_ok());
    }

    #[test]
    fn refuses_a_line_that_is_not_json() {
        let s = fixture_extract();
        let path = s.0.join("ground.jsonl");
        let mut text = fs::read_to_string(&path).unwrap();
        text.push_str("{\"class\": \n");
        fs::write(&path, text).unwrap();
        let f = failures(&s.0);
        assert!(has(&f, "ground.jsonl", "line 3 is not JSON"), "{f:?}");
    }

    #[test]
    fn refuses_a_missing_table() {
        let s = fixture_extract();
        fs::remove_file(s.0.join("radio.json")).unwrap();
        let f = failures(&s.0);
        assert!(has(&f, "radio.json", "not a file"), "{f:?}");
    }

    #[test]
    fn refuses_a_table_name_that_leaves_the_directory() {
        for name in ["../nodes.json", "C:nodes.json", "/nodes.json", ".", ""] {
            let s = fixture_extract();
            edit_manifest(&s.0, |m| m["tables"]["nodes"] = name.into());
            let f = failures(&s.0);
            assert!(has(&f, MANIFEST, "tables.nodes"), "{name}: {f:?}");
        }
    }

    #[test]
    fn refuses_a_table_that_is_a_directory() {
        let s = fixture_extract();
        edit_manifest(&s.0, |m| m["tables"]["nodes"] = "tiles".into());
        let f = failures(&s.0);
        assert!(has(&f, "tiles", "not a file"), "{f:?}");
    }

    #[test]
    fn refuses_a_manifest_that_names_no_table() {
        let s = fixture_extract();
        edit_manifest(&s.0, |m| {
            m["tables"].as_object_mut().unwrap().remove("ground");
        });
        let f = failures(&s.0);
        assert!(has(&f, MANIFEST, "tables lacks ground"), "{f:?}");
    }
}
