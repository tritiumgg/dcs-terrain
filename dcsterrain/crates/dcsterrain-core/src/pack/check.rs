//! Checks a packed file's invariants. For now that is `meta`: every key
//! present, each non-nullable key with a value, numbers that parse, and the
//! groups that must be NULL together.

use std::path::Path;

use super::meta::{KEYS, NULLABLE};
use super::{CELL_SIZES, FORMAT_VERSION, read_meta};

/// Every `meta` row, the expected keys first in their order and any others
/// after by name, and every failure found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub meta: Vec<(String, Option<String>)>,
    pub failures: Vec<String>,
}

impl Report {
    pub fn passes(&self) -> bool {
        self.failures.is_empty()
    }
}

/// Keys whose value is a number.
const NUMERIC: [&str; 21] = [
    "bounds_sw_x",
    "bounds_sw_z",
    "bounds_ne_x",
    "bounds_ne_z",
    "grid_origin_x",
    "grid_origin_z",
    "grid_width",
    "grid_height",
    "cell_size",
    "tile_size",
    "bullseye_blue_x",
    "bullseye_blue_z",
    "bullseye_red_x",
    "bullseye_red_z",
    "authored_bounds_min_x",
    "authored_bounds_min_z",
    "authored_bounds_max_x",
    "authored_bounds_max_z",
    "validation_height_rmse",
    "validation_los_agreement",
    "validation_route_ratio",
];

/// Keys that are one fact: all NULL or none. The authored rectangle is ADR
/// 0009's rule; a half-known bounds rectangle is as corrupt.
const TOGETHER: [&[&str]; 2] = [
    &[
        "authored_bounds_min_x",
        "authored_bounds_min_z",
        "authored_bounds_max_x",
        "authored_bounds_max_z",
        "authored_bounds_source",
    ],
    &["bounds_sw_x", "bounds_sw_z", "bounds_ne_x", "bounds_ne_z"],
];

/// Checks the packed file at `path`. Fails only when it cannot be read as a
/// packed file at all; everything else is a failure in the report.
pub fn check(path: &Path) -> Result<Report, String> {
    if !path.is_file() {
        return Err(format!("{}: no file here", path.display()));
    }
    let mut rows = read_meta(path).map_err(|e| {
        format!(
            "{}: not a packed file, its meta cannot be read: {e}",
            path.display()
        )
    })?;
    let rank = |k: &str| KEYS.iter().position(|e| *e == k).unwrap_or(KEYS.len());
    rows.sort_by(|a, b| (rank(&a.0), &a.0).cmp(&(rank(&b.0), &b.0)));

    let mut failures = Vec::new();
    let get = |k: &str| rows.iter().find(|r| r.0 == k).map(|r| r.1.as_deref());
    for k in KEYS {
        match get(k) {
            None => failures.push(format!("meta lacks {k}")),
            Some(None) if !NULLABLE.contains(&k) => failures.push(format!("meta {k} is NULL")),
            _ => {}
        }
    }
    for k in NUMERIC {
        if let Some(Some(v)) = get(k)
            && v.parse::<f64>().is_err()
        {
            failures.push(format!("meta {k} is {v:?}, not a number"));
        }
    }
    if let Some(Some(v)) = get("format_version")
        && v != FORMAT_VERSION.to_string()
    {
        failures.push(format!(
            "meta format_version is {v}, not one this build reads ({FORMAT_VERSION})"
        ));
    }
    if let Some(Some(v)) = get("cell_size")
        && !CELL_SIZES.iter().any(|c| c.to_string() == v)
    {
        failures.push(format!("meta cell_size is {v}, not 50 or 100"));
    }
    for group in TOGETHER {
        let nulls = group.iter().filter(|k| get(k) == Some(None)).count();
        if nulls != 0 && nulls != group.len() {
            failures.push(format!(
                "meta {} must be NULL together or not at all",
                group.join(", ")
            ));
        }
    }
    Ok(Report {
        meta: rows,
        failures,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::{Options, pack};
    use crate::scratch::Scratch;
    use crate::synth;
    use rusqlite::Connection;
    use std::path::PathBuf;

    fn packed(s: &Scratch) -> PathBuf {
        let dir = s.0.join("extract");
        let t = synth::Theatre::new(synth::DEFAULT_SEED, synth::DEFAULT_SIZE_KM, 50).unwrap();
        synth::write(&t, &dir).unwrap();
        let out = s.0.join("x.sqlite");
        pack(&dir, Some(&out), &Options::default()).unwrap();
        out
    }

    fn sql(path: &Path, statement: &str) {
        Connection::open(path)
            .unwrap()
            .execute(statement, [])
            .unwrap();
    }

    #[test]
    fn reports_every_key_of_a_fresh_pack_and_passes() {
        let s = Scratch::new();
        let r = check(&packed(&s)).unwrap();
        assert!(r.passes(), "{:?}", r.failures);
        assert!(r.meta.iter().map(|m| m.0.as_str()).eq(KEYS));
        // A NULL is a value to report, not a key to skip.
        assert_eq!(
            r.meta.iter().find(|m| m.0 == "validation_at").unwrap().1,
            None
        );
    }

    #[test]
    fn a_stamped_key_is_reported_after_the_expected_ones() {
        let s = Scratch::new();
        let path = packed(&s);
        sql(&path, "INSERT INTO meta VALUES ('aaa_note', 'x')");
        let r = check(&path).unwrap();
        assert!(r.passes(), "{:?}", r.failures);
        assert_eq!(r.meta.last().unwrap().0, "aaa_note");
    }

    #[test]
    fn fails_each_broken_invariant() {
        let cases = [
            (
                "DELETE FROM meta WHERE key = 'dcs_build'",
                "meta lacks dcs_build",
            ),
            (
                "UPDATE meta SET value = NULL WHERE key = 'theatre_id'",
                "meta theatre_id is NULL",
            ),
            (
                "UPDATE meta SET value = 'wide' WHERE key = 'grid_width'",
                "grid_width is \"wide\"",
            ),
            (
                "UPDATE meta SET value = '2' WHERE key = 'format_version'",
                "format_version is 2",
            ),
            (
                "UPDATE meta SET value = '75' WHERE key = 'cell_size'",
                "cell_size is 75",
            ),
            (
                "UPDATE meta SET value = NULL WHERE key = 'authored_bounds_source'",
                "authored_bounds_min_x",
            ),
            (
                "UPDATE meta SET value = NULL WHERE key = 'bounds_ne_z'",
                "bounds_sw_x",
            ),
        ];
        for (statement, want) in cases {
            let s = Scratch::new();
            let path = packed(&s);
            sql(&path, statement);
            let r = check(&path).unwrap();
            assert!(
                r.failures.iter().any(|f| f.contains(want)),
                "{statement}: {:?}",
                r.failures
            );
        }
    }

    #[test]
    fn the_authored_rectangle_may_be_null_whole() {
        let s = Scratch::new();
        let path = packed(&s);
        sql(
            &path,
            "UPDATE meta SET value = NULL WHERE key LIKE 'authored_bounds_%'",
        );
        assert!(check(&path).unwrap().passes());
    }

    #[test]
    fn refuses_what_is_not_a_packed_file() {
        let s = Scratch::new();
        std::fs::create_dir_all(&s.0).unwrap();
        let text = s.0.join("notes.txt");
        std::fs::write(&text, "not a database").unwrap();
        assert!(check(&text).unwrap_err().contains("not a packed file"));
        assert!(
            check(&s.0.join("absent.sqlite"))
                .unwrap_err()
                .contains("no file")
        );
    }
}
