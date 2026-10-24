//! Opt-in: reads real Lua tables. Set `DCSTERRAIN_INSTALL` to the DCS install
//! root, and `DCSTERRAIN_EXTRACT` to the live Caucasus extract to compare
//! against; without them these pass without reading anything, which is what
//! CI does. The install is only read.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use dcsterrain_core::crs;
use dcsterrain_core::decode::install::{self, TERRAINS_DIR};
use dcsterrain_core::decode::lua::tables::{self, LuaTables};

fn install_root() -> Option<PathBuf> {
    match std::env::var_os("DCSTERRAIN_INSTALL") {
        Some(p) => Some(PathBuf::from(p)),
        None => {
            eprintln!("DCSTERRAIN_INSTALL is not set; skipped");
            None
        }
    }
}

fn live_extract() -> Option<PathBuf> {
    match std::env::var_os("DCSTERRAIN_EXTRACT") {
        Some(p) => Some(PathBuf::from(p)),
        None => {
            eprintln!("DCSTERRAIN_EXTRACT is not set; skipped");
            None
        }
    }
}

fn maps(root: &Path) -> Vec<String> {
    let mut out: Vec<String> = fs::read_dir(root.join(TERRAINS_DIR))
        .unwrap()
        .flatten()
        .filter(|e| e.path().join(install::ENTRY_FILE).is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    out.sort();
    out
}

fn decode(root: &Path, dir: &str) -> (install::Map, LuaTables) {
    let map = install::read_map(root, dir).unwrap_or_else(|e| panic!("{dir}: {e}"));
    let t = tables::decode(root, &map.theatre, &map.files).unwrap_or_else(|e| panic!("{dir}: {e}"));
    (map, t)
}

fn table(dir: &Path, name: &str) -> Vec<Value> {
    let text = fs::read_to_string(dir.join(name)).unwrap();
    serde_json::from_str::<Value>(&text)
        .unwrap()
        .as_array()
        .unwrap()
        .clone()
}

/// Rows keyed by `key`, every key distinct.
fn by(rows: Vec<Value>, key: &str) -> BTreeMap<String, Value> {
    let n = rows.len();
    let out: BTreeMap<String, Value> = rows.into_iter().map(|r| (r[key].to_string(), r)).collect();
    assert_eq!(out.len(), n, "{key} repeats");
    out
}

/// The decode as the extract would hold it.
fn rows<T: serde::Serialize>(v: &[T]) -> Vec<Value> {
    v.iter().map(|r| serde_json::to_value(r).unwrap()).collect()
}

/// `a` and `b` as numbers where both are, so 443000 equals 443000.0.
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(a, b)| same(a, b))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same(v, w)))
        }
        _ => a == b,
    }
}

/// Every row of the live table equal, field for field, to the decoded row
/// with the same key, but for the fields in `skip`.
fn equal(what: &str, live: Vec<Value>, ours: Vec<Value>, key: &str, skip: &[&str]) {
    let (live, ours) = (by(live, key), by(ours, key));
    assert_eq!(
        live.keys().collect::<Vec<_>>(),
        ours.keys().collect::<Vec<_>>(),
        "{what}: keys differ"
    );
    let mut off = Vec::new();
    for (k, l) in &live {
        let o = &ours[k];
        for (f, v) in l.as_object().unwrap() {
            if !skip.contains(&f.as_str()) && !same(v, &o[f]) {
                off.push(format!("{k}.{f}: live {v}, decoded {}", o[f]));
            }
        }
        for f in o.as_object().unwrap().keys() {
            assert!(l.get(f).is_some(), "{what}: the live table has no {f}");
        }
    }
    assert!(
        off.is_empty(),
        "{what}: {} fields differ: {off:#?}",
        off.len()
    );
    eprintln!("{what}: {} rows equal", live.len());
}

/// The live tables of 2.9.29.27468 against the decode of the same files.
/// Values are compared exactly: the game's Lua and this reader both round
/// each numeral to the nearest double. A town's `x, z` is the game's own
/// projection against the fit, so it is held to a centimetre.
#[test]
fn caucasus_matches_the_live_extract() {
    let (Some(root), Some(extract)) = (install_root(), live_extract()) else {
        return;
    };
    let (map, t) = decode(&root, "Caucasus");
    eprintln!("decoding {} of {}", map.theatre, map.build.version);
    equal(
        "beacons",
        table(&extract, "beacons.json"),
        rows(&t.beacons),
        "beacon_id",
        &[],
    );
    equal(
        "radio",
        table(&extract, "radio.json"),
        rows(&t.radio),
        "radio_id",
        &[],
    );
    equal(
        "nodes",
        table(&extract, "nodes.json"),
        rows(&t.nodes),
        "id",
        &[],
    );
    let live_towns = table(&extract, "towns.json");
    equal(
        "towns",
        live_towns.clone(),
        rows(&t.towns),
        "name",
        &["x", "z"],
    );

    let ours: BTreeMap<&str, (f64, f64)> = t
        .towns
        .iter()
        .map(|r| (r.name.as_str(), (r.x, r.z)))
        .collect();
    let worst = live_towns
        .iter()
        .map(|l| {
            let (x, z) = ours[l["name"].as_str().unwrap()];
            (x - l["x"].as_f64().unwrap()).hypot(z - l["z"].as_f64().unwrap())
        })
        .fold(0.0, f64::max);
    eprintln!(
        "towns: {} through the fit, worst {worst:.4} m from the game's",
        live_towns.len()
    );
    assert!(worst < 0.01, "a town is {worst} m from the game's position");

    // Every airdrome the live table names, by the label Radio.lua gives it.
    let mut missing = Vec::new();
    for a in table(&extract, "airdromes.json") {
        let name_id = a["name_id"].as_str().unwrap();
        let id = a["id"].as_i64().unwrap();
        match t.airdrome_id(name_id) {
            Some(got) => assert_eq!(got, id, "{name_id}"),
            None => missing.push(name_id.to_string()),
        }
        let ids = |k: &str| -> Vec<String> {
            a[k].as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().to_string())
                .collect()
        };
        assert_eq!(t.radio_ids(id), ids("radio_ids"), "{name_id} radio");
        let mut want = ids("beacon_ids");
        want.sort();
        let mut got = t.beacon_ids(id);
        got.sort();
        assert_eq!(got, want, "{name_id} beacons");
    }
    assert!(missing.is_empty(), "no id for {missing:?}");
    // ED's `wsTypes.lua` reads an `SA2_SR` it never defines, and its
    // `BeaconTypes.lua` requires a module the sandbox stands in for; neither
    // reaches a table.
    let path = "Mods/terrains/Caucasus/Beacons.lua";
    assert_eq!(
        t.unresolved,
        [
            format!("{path}: SA2_SR"),
            format!("{path}: require(\"utils\")")
        ]
    );
}

/// The fit on every installed map against the table of measured rows, which
/// is the design's table plus Nevada. A map the fit cannot reach takes the
/// row itself.
#[test]
fn every_map_fits_the_measured_projection() {
    let Some(root) = install_root() else { return };
    let build = install::read_build(&root).unwrap();
    eprintln!("build {} {}", build.version, build.timestamp);
    for dir in maps(&root) {
        let (map, t) = decode(&root, &dir);
        let c = &t.crs;
        let fields: Vec<&str> = map
            .files
            .iter()
            .filter(|f| f.role == "airfield_rn4")
            .map(|f| f.path.rsplit('/').next().unwrap().trim_end_matches(".rn4"))
            .collect();
        let with_id = fields.iter().filter(|n| t.airdrome_id(n).is_some()).count();
        let want = crs::table(&map.theatre).unwrap_or_else(|| panic!("{}: no row", map.theatre));
        eprintln!(
            "{:<20} {:<7} lon_0 {:>4} easting {:>12.3} northing {:>12.3} n {:?} rms {:?} max {:?}; \
             {} beacons, {} radio, {} towns, {} nodes, ids for {}/{} airfields, unresolved {:?}",
            map.theatre,
            c.source,
            c.lon_0,
            c.easting,
            c.northing,
            c.n,
            c.rms_m,
            c.max_m,
            t.beacons.len(),
            t.radio.len(),
            t.towns.len(),
            t.nodes.len(),
            with_id,
            fields.len(),
            t.unresolved,
        );
        assert_eq!(c.lon_0, want.lon_0, "{dir}");
        assert!(
            (c.easting - want.easting).abs() <= 0.01 && (c.northing - want.northing).abs() <= 0.01,
            "{dir}: fitted {} {}, measured {} {}",
            c.easting,
            c.northing,
            want.easting,
            want.northing
        );
        if c.source == "beacons" {
            assert!(c.max_m.unwrap() < 0.1, "{dir}: {c:?}");
            assert_eq!(c.n, Some(t.latlon_samples.len()));
        }
    }
}
