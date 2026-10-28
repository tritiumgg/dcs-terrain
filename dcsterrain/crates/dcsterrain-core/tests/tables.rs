//! Opt-in: builds `tables.json` from a real install. Set
//! `DCSTERRAIN_INSTALL` to the DCS install root, and `DCSTERRAIN_EXTRACT` to
//! the live Caucasus extract to compare against; without them these pass
//! without reading anything, which is what CI does. The install is only
//! read; the cache goes to a fresh directory under the system temp.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde_json::{Value, json};

use dcsterrain_core::cache::{Cache, TerrainCache};
use dcsterrain_core::decode::install;
use dcsterrain_core::source::install::{TABLES_JSON, Tables, TablesJson};

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

/// A fresh cache directory, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(what: &str) -> Scratch {
        let dir =
            std::env::temp_dir().join(format!("dcsterrain-tables-{what}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn terrain(root: &Path, cache: &Path, id: &str) -> TerrainCache {
    let map = install::open(root, id).unwrap_or_else(|e| panic!("{id}: {e}"));
    Cache::new(cache).terrain(root, &map)
}

fn table(dir: &Path, name: &str) -> Vec<Value> {
    let text = fs::read_to_string(dir.join(name)).unwrap();
    serde_json::from_str::<Value>(&text)
        .unwrap()
        .as_array()
        .unwrap()
        .clone()
}

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

/// How a table is compared: the fields that key a row, the live fields
/// left out, and the fields only `tables.json` carries.
struct Compare<'a> {
    key: &'a [&'a str],
    skip: &'a [&'a str],
    ours_only: &'a [&'a str],
}

/// Every live row equal, field for field, to ours with the same key, and
/// no row on either side without the other.
fn equal(what: &str, live: Vec<Value>, ours: Vec<Value>, c: Compare) {
    let by = |rows: Vec<Value>| -> BTreeMap<String, Value> {
        let n = rows.len();
        let out: BTreeMap<String, Value> = rows
            .into_iter()
            .map(|r| {
                let k: Vec<String> = c.key.iter().map(|k| r[*k].to_string()).collect();
                (k.join("/"), r)
            })
            .collect();
        assert_eq!(out.len(), n, "{what}: a key repeats");
        out
    };
    let (live, ours) = (by(live), by(ours));
    assert_eq!(
        live.keys().collect::<Vec<_>>(),
        ours.keys().collect::<Vec<_>>(),
        "{what}: keys differ"
    );
    let mut off = Vec::new();
    for (k, l) in &live {
        let o = &ours[k];
        for (f, v) in l.as_object().unwrap() {
            if !c.skip.contains(&f.as_str()) && !same(v, &o[f]) {
                off.push(format!("{k}.{f}: live {v}, ours {}", o[f]));
            }
        }
        for f in o.as_object().unwrap().keys() {
            assert!(
                l.get(f).is_some() || c.ours_only.contains(&f.as_str()),
                "{what}: the live table has no {f}"
            );
        }
    }
    assert!(
        off.is_empty(),
        "{what}: {} fields differ: {off:#?}",
        off.len()
    );
    eprintln!("{what}: {} rows equal", live.len());
}

/// A stand as the game reports it: flags as `"1"` or `"0"`, sizes to six
/// places, and `SHELTER`, which no file carries, as the game's `"0"`.
fn as_game(mut stand: Value) -> Value {
    let p = &mut stand["params"];
    for k in ["FOR_HELICOPTERS", "FOR_AIRPLANES"] {
        p[k] = match p[k].as_bool() {
            Some(b) => json!(if b { "1" } else { "0" }),
            None => Value::Null,
        };
    }
    for k in ["WIDTH", "LENGTH"] {
        p[k] = match p[k].as_f64() {
            Some(v) => json!(format!("{v:.6}")),
            None => Value::Null,
        };
    }
    p["SHELTER"] = json!("0");
    stand
}

/// The fields only a running sim knows, which the hook imports. It also
/// gives the game's display name, which the beacons only approximate.
const HOOK: [&str; 4] = ["code", "civilian", "abandoned", "class"];

/// The live airdrome fields `tables.json` does not carry: the scene objects
/// the game ties to an airdrome, and its names by language.
const NOT_CARRIED: [&str; 5] = ["names", "towers", "warehouses", "fueldepots", "shelters"];

/// Every table but the airdromes' hook fields and display names equal to the live extract of
/// 2.9.29.27468, the 2.9.30 terrain files being the same. A town's and an
/// airdrome's position through the projection are held to a centimeter
/// and its degrees to 1e-6, the six places ED writes.
#[test]
fn caucasus_tables_equal_the_live_extract() {
    let (Some(root), Some(extract)) = (install_root(), live_extract()) else {
        return;
    };
    let c = Scratch::new("live");
    let terrain = terrain(&root, &c.0, "Caucasus");
    let b = &terrain.map().build;
    eprintln!("built on DCS {} ({})", b.version, b.timestamp);
    let t = Tables::open(&terrain).unwrap();
    compare(&extract, &t, false);

    let live = table(&extract, "airdromes.json");
    let named = live
        .iter()
        .filter(|l| {
            let o = t
                .airdromes
                .iter()
                .find(|o| l["name_id"] == o.name_id.as_str());
            o.and_then(|o| o.display_name.as_deref()) == l["display_name"].as_str()
        })
        .count();
    eprintln!(
        "display names: {named} of {} the game's from the beacons alone",
        live.len()
    );
}

/// A hook directory from the live extract's build is refused, naming the
/// build and timestamp that differ.
#[test]
fn a_hook_directory_from_another_build_is_refused_by_name() {
    let (Some(root), Some(extract)) = (install_root(), live_extract()) else {
        return;
    };
    let c = Scratch::new("refused");
    let terrain = terrain(&root, &c.0, "Caucasus");
    let manifest: Value =
        serde_json::from_slice(&fs::read(extract.join("manifest.json")).unwrap()).unwrap();
    let b = &terrain.map().build;
    if manifest["dcs_build"] == b.version.as_str() {
        eprintln!("the live extract is from the installed build; skipped");
        return;
    }
    let e = terrain
        .rebuild(&TablesJson::with_hook(&extract))
        .unwrap_err();
    eprintln!("{e}");
    let want = format!(
        "dcs_build {}, not {}",
        manifest["dcs_build"].as_str().unwrap(),
        b.version
    );
    assert!(e.0.contains(&want), "{e}");
    assert!(e.0.contains("build timestamp"), "{e}");
    assert!(
        !e.0.contains("not Caucasus"),
        "the terrain is the same: {e}"
    );
}

/// With the live extract's airdromes imported as the hook's, restamped
/// with the installed build, every field `tables.json` carries equals the
/// live extract's.
#[test]
fn caucasus_with_the_hook_equals_the_live_extract() {
    let (Some(root), Some(extract)) = (install_root(), live_extract()) else {
        return;
    };
    let c = Scratch::new("hooked");
    let terrain = terrain(&root, &c.0, "Caucasus");
    let b = &terrain.map().build;
    let h = Scratch::new("hook");
    let hook = h.0.clone();
    fs::create_dir_all(&hook).unwrap();
    let mut manifest: Value =
        serde_json::from_slice(&fs::read(extract.join("manifest.json")).unwrap()).unwrap();
    manifest["dcs_build"] = json!(b.version);
    manifest["dcs_build_timestamp"] = json!(b.timestamp);
    fs::write(hook.join("manifest.json"), manifest.to_string()).unwrap();
    fs::copy(extract.join("airdromes.json"), hook.join("airdromes.json")).unwrap();
    terrain.rebuild(&TablesJson::with_hook(&hook)).unwrap();
    let t = Tables::open(&terrain).unwrap();
    let i = t.hook.as_ref().unwrap();
    assert_eq!(
        (i.airdromes, i.unmatched.len(), &i.dropped),
        (t.airdromes.len(), 0, &None)
    );
    compare(&extract, &t, true);
}

fn compare(extract: &Path, t: &Tables, hooked: bool) {
    let unfilled: &[&str] = if hooked { &[] } else { &HOOK };
    let plain = |key| Compare {
        key,
        skip: &[],
        ours_only: &[],
    };
    equal(
        "beacons",
        table(extract, "beacons.json"),
        rows(&t.beacons),
        plain(&["beacon_id"]),
    );
    equal(
        "radio",
        table(extract, "radio.json"),
        rows(&t.radio),
        plain(&["radio_id"]),
    );
    equal(
        "nodes",
        table(extract, "nodes.json"),
        rows(&t.nodes),
        plain(&["id"]),
    );
    equal(
        "runways",
        table(extract, "runways.json"),
        rows(&t.runways),
        Compare {
            key: &["airdrome_id", "name"],
            skip: &[],
            ours_only: &["airdrome_name_id"],
        },
    );
    equal(
        "stands",
        table(extract, "stands.json"),
        rows(&t.stands).into_iter().map(as_game).collect(),
        Compare {
            key: &["airdrome_id", "crossroad_index"],
            skip: &[],
            ours_only: &["airdrome_name_id", "stand_class", "heading"],
        },
    );
    let live_towns = table(extract, "towns.json");
    equal(
        "towns",
        live_towns.clone(),
        rows(&t.towns),
        Compare {
            key: &["name"],
            skip: &["x", "z"],
            ours_only: &[],
        },
    );
    let mut skip: Vec<&str> = vec!["lat", "lon", "x", "z", "beacon_ids"];
    skip.extend(unfilled);
    if !hooked {
        skip.push("display_name");
    }
    skip.extend(NOT_CARRIED);
    let live_airdromes = table(extract, "airdromes.json");
    equal(
        "airdromes",
        live_airdromes.clone(),
        rows(&t.airdromes),
        Compare {
            key: &["name_id"],
            skip: &skip,
            ours_only: &[],
        },
    );

    // Where the game and the projection meet.
    let mut worst_m: f64 = 0.0;
    let mut worst_deg: f64 = 0.0;
    for l in &live_towns {
        let o = t
            .towns
            .iter()
            .find(|o| l["name"] == o.name.as_str())
            .unwrap();
        worst_m =
            worst_m.max((o.x - l["x"].as_f64().unwrap()).hypot(o.z - l["z"].as_f64().unwrap()));
    }
    for l in &live_airdromes {
        let o = t
            .airdromes
            .iter()
            .find(|o| l["name_id"] == o.name_id.as_str())
            .unwrap();
        let f = |k: &str| l[k].as_f64().unwrap();
        worst_m = worst_m.max((o.x.unwrap() - f("x")).hypot(o.z.unwrap() - f("z")));
        worst_deg = worst_deg
            .max((o.lat.unwrap() - f("lat")).abs())
            .max((o.lon.unwrap() - f("lon")).abs());
        let mut want: Vec<&str> = l["beacon_ids"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        want.sort();
        let mut got: Vec<&str> = o.beacon_ids.iter().map(String::as_str).collect();
        got.sort();
        assert_eq!(got, want, "{} beacon ids", o.name_id);
        for k in unfilled {
            assert_eq!(
                serde_json::to_value(o).unwrap()[*k],
                Value::Null,
                "{}.{k}",
                o.name_id
            );
        }
    }
    eprintln!("positions: worst {worst_m:.4} m and {worst_deg:.2e} degrees from the game's");
    assert!(worst_m < 0.01, "a position is {worst_m} m from the game's");
    assert!(
        worst_deg < 1e-6,
        "a position is {worst_deg} degrees from the game's"
    );
}

/// Every installed terrain builds its tables within the size and time the
/// design gives, fits its projection within the residual allowed, and reads
/// them back from the cache.
#[test]
fn every_terrain_builds_its_tables() {
    let Some(root) = install_root() else { return };
    let build = install::read_build(&root).unwrap();
    eprintln!("DCS {} ({})", build.version, build.timestamp);
    let c = Scratch::new("all");
    for listed in install::terrains(&root).unwrap() {
        let Some(id) = listed.id else { continue };
        let terrain = terrain(&root, &c.0, &id);
        let start = Instant::now();
        let t = Tables::open(&terrain).unwrap_or_else(|e| panic!("{id}: {e}"));
        let built_ms = start.elapsed().as_secs_f64() * 1e3;
        let start = Instant::now();
        assert!(!terrain.get(&TABLES_JSON).unwrap().built, "{id}: rebuilt");
        assert_eq!(
            Tables::open(&terrain).unwrap(),
            t,
            "{id}: read back differs"
        );
        let read_ms = start.elapsed().as_secs_f64() * 1e3;
        let size = fs::metadata(terrain.dir().join("tables.json"))
            .unwrap()
            .len();
        let with_id = t.airdromes.iter().filter(|a| a.id.is_some()).count();
        eprintln!(
            "{id:<20} {size:>8} B, built {built_ms:>6.0} ms, read {read_ms:>5.1} ms; \
             {with_id}/{} airdromes with an id, {} runways, {} stands, {} beacons, \
             {} towns, {} nodes; crs {} max {:?}",
            t.airdromes.len(),
            t.runways.len(),
            t.stands.len(),
            t.beacons.len(),
            t.towns.len(),
            t.nodes.len(),
            t.crs.source,
            t.crs.max_m,
        );
        t.check_crs(&id, false).unwrap();
        assert!(size < 4 << 20, "{id}: {size} B");
        assert!(built_ms < 2000.0, "{id}: built in {built_ms} ms");
    }
}
