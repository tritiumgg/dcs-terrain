//! The extract's JSON and JSON Lines tables: each must parse and carry its
//! keys. A key whose value is null is present; only an absent one fails,
//! because without the hook a run writes null for the fields only a running
//! DCS knows.

use std::fs;

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
            "code",
            "display_name",
            "names",
            "x",
            "z",
            "lat",
            "lon",
            "civilian",
            "abandoned",
            "class",
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
        // A table sits beside the manifest; a name with a separator could
        // point anywhere.
        if name.is_empty() || name.contains(['/', '\\']) || name == "." || name == ".." {
            out.push(Failure::new(
                super::MANIFEST,
                format!("tables.{key} is {name:?}, not a file name"),
            ));
            continue;
        }
        let text = match fs::read_to_string(x.dir.join(name)) {
            Ok(t) => t,
            Err(e) => {
                out.push(Failure::new(name, format!("cannot read: {e}")));
                continue;
            }
        };
        match rows(&text, shape) {
            Ok(rows) => check_keys(name, &rows, keys, out),
            Err(why) => out.push(Failure::new(name, why)),
        }
    }
}

/// The table's rows, each with its 1-based position: the array index or the
/// line number.
fn rows(text: &str, shape: Shape) -> Result<Vec<(usize, Value)>, String> {
    let parse = |s: &str| serde_json::from_str::<Value>(s).map_err(|e| format!("not JSON: {e}"));
    match shape {
        Shape::Object => Ok(vec![(1, parse(text)?)]),
        Shape::Array => match parse(text)? {
            Value::Array(a) => Ok(a.into_iter().zip(1..).map(|(v, i)| (i, v)).collect()),
            _ => Err("not a JSON array".into()),
        },
        Shape::Lines => text
            .lines()
            .zip(1..)
            .filter(|(l, _)| !l.trim().is_empty())
            .map(|(l, n)| {
                serde_json::from_str(l)
                    .map(|v| (n, v))
                    .map_err(|e| format!("line {n} is not JSON: {e}"))
            })
            .collect(),
    }
}

/// Reports the first row that lacks a key, and how many more do, so one bad
/// writer does not bury the report under a line per row.
fn check_keys(name: &str, rows: &[(usize, Value)], keys: &[&str], out: &mut Vec<Failure>) {
    let mut bad = rows.iter().filter_map(|(n, row)| {
        let Some(obj) = row.as_object() else {
            return Some(format!("row {n} is not an object"));
        };
        let missing: Vec<&str> = keys
            .iter()
            .copied()
            .filter(|k| !obj.contains_key(*k))
            .collect();
        (!missing.is_empty()).then(|| format!("row {n} lacks {}", missing.join(", ")))
    });
    if let Some(first) = bad.next() {
        let more = bad.count();
        let tail = if more > 0 {
            format!(", and {more} more rows are wrong")
        } else {
            String::new()
        };
        out.push(Failure::new(name, format!("{first}{tail}")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::MANIFEST;
    use crate::extract::check::tests::{edit_manifest, failures, has, synth_extract};

    #[test]
    fn refuses_a_row_that_lacks_a_key() {
        let s = synth_extract();
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
    fn a_null_hook_field_is_present() {
        let s = synth_extract();
        let path = s.0.join("config.json");
        let mut c: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        c["bounds_km"] = Value::Null;
        fs::write(&path, c.to_string()).unwrap();
        assert!(crate::extract::check(&s.0).is_ok());
    }

    #[test]
    fn refuses_a_line_that_is_not_json() {
        let s = synth_extract();
        let path = s.0.join("ground.jsonl");
        let mut text = fs::read_to_string(&path).unwrap();
        text.push_str("{\"class\": \n");
        fs::write(&path, text).unwrap();
        let f = failures(&s.0);
        assert!(has(&f, "ground.jsonl", "line 3 is not JSON"), "{f:?}");
    }

    #[test]
    fn refuses_a_missing_table() {
        let s = synth_extract();
        fs::remove_file(s.0.join("radio.json")).unwrap();
        let f = failures(&s.0);
        assert!(has(&f, "radio.json", "cannot read"), "{f:?}");
    }

    #[test]
    fn refuses_a_table_name_that_leaves_the_directory() {
        let s = synth_extract();
        edit_manifest(&s.0, |m| m["tables"]["nodes"] = "../nodes.json".into());
        let f = failures(&s.0);
        assert!(has(&f, MANIFEST, "tables.nodes"), "{f:?}");
    }

    #[test]
    fn refuses_a_manifest_that_names_no_table() {
        let s = synth_extract();
        edit_manifest(&s.0, |m| {
            m["tables"].as_object_mut().unwrap().remove("ground");
        });
        let f = failures(&s.0);
        assert!(has(&f, MANIFEST, "tables lacks ground"), "{f:?}");
    }
}
