//! Opt-in: decodes real airfield files. Set `DCSTERRAIN_INSTALL` to the DCS
//! install root, and `DCSTERRAIN_EXTRACT` to the live Caucasus extract to
//! compare against; without them these pass without reading anything, which
//! is what CI does. The install is only read.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use dcsterrain_core::decode::airfield::{self, Airfields};
use dcsterrain_core::decode::install::{self, TERRAINS_DIR};

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

fn decode(root: &Path, dir: &str) -> Airfields {
    let map = install::read_map(root, dir).unwrap_or_else(|e| panic!("{dir}: {e}"));
    airfield::decode(root, &map.files).unwrap_or_else(|e| panic!("{dir}: {e}"))
}

fn table(dir: &Path, name: &str) -> Vec<Value> {
    let text = fs::read_to_string(dir.join(name)).unwrap();
    serde_json::from_str::<Value>(&text)
        .unwrap()
        .as_array()
        .unwrap()
        .clone()
}

/// The live extract's `name_id` of each airdrome id.
fn live_name_ids(extract: &Path) -> HashMap<i64, String> {
    table(extract, "airdromes.json")
        .iter()
        .map(|a| {
            (
                a["id"].as_i64().unwrap(),
                a["name_id"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

/// The game reports a flag as `"1"` or `"0"` and a size to six places.
fn flag(b: Option<bool>) -> Option<String> {
    b.map(|b| if b { "1" } else { "0" }.to_string())
}

/// The live stands and runways, positions bit for bit, against the decode of
/// the same terrain files. Every stand parameter is compared but `SHELTER`,
/// which no file carries; the game gives 0 for every Caucasus stand.
#[test]
fn caucasus_stands_and_runways_equal_the_live_extract() {
    let (Some(root), Some(extract)) = (install_root(), live_extract()) else {
        return;
    };
    let build = install::read_build(&root).unwrap();
    let manifest: Value =
        serde_json::from_str(&fs::read_to_string(extract.join("manifest.json")).unwrap()).unwrap();
    eprintln!(
        "decoded on DCS {} ({}), against a live extract of {}",
        build.version, build.timestamp, manifest["dcs_build"]
    );
    let name_ids = live_name_ids(&extract);
    let got = decode(&root, "Caucasus");
    let mut misses = Vec::new();

    let stands: HashMap<(&str, u32), &airfield::StandRow> = got
        .stands
        .iter()
        .map(|r| ((r.airdrome_name_id.as_str(), r.crossroad_index), r))
        .collect();
    let live = table(&extract, "stands.json");
    let mut matched = 0;
    for s in &live {
        let name_id = name_ids[&s["airdrome_id"].as_i64().unwrap()].as_str();
        let index = s["crossroad_index"].as_u64().unwrap() as u32;
        let Some(r) = stands.get(&(name_id, index)) else {
            misses.push(format!("{name_id} stand {index}: not decoded"));
            continue;
        };
        let p = &s["params"];
        let text = |k: &str| p[k].as_str().map(str::to_string);
        let checks = [
            ("name", r.name == s["name"].as_str().unwrap()),
            ("x", r.x == s["x"].as_f64().unwrap()),
            ("z", r.z == s["z"].as_f64().unwrap()),
            ("flag", r.flag.map(u64::from) == s["flag"].as_u64()),
            (
                "FOR_HELICOPTERS",
                flag(r.params.FOR_HELICOPTERS) == text("FOR_HELICOPTERS"),
            ),
            (
                "FOR_AIRPLANES",
                flag(r.params.FOR_AIRPLANES) == text("FOR_AIRPLANES"),
            ),
            (
                "WIDTH",
                r.params.WIDTH.map(|v| format!("{v:.6}")) == text("WIDTH"),
            ),
            (
                "LENGTH",
                r.params.LENGTH.map(|v| format!("{v:.6}")) == text("LENGTH"),
            ),
            ("HEIGHT", r.params.HEIGHT == text("HEIGHT")),
        ];
        let wrong: Vec<&str> = checks.iter().filter(|c| !c.1).map(|c| c.0).collect();
        if wrong.is_empty() {
            matched += 1;
        } else {
            misses.push(format!(
                "{name_id} stand {index} ({:?}): {wrong:?}",
                r.stand_class
            ));
        }
    }
    eprintln!(
        "stands: {matched} of {} live equal on every parameter; {} decoded",
        live.len(),
        got.stands.len()
    );

    let live = table(&extract, "runways.json");
    let mut matched_rw = 0;
    for l in &live {
        let name_id = name_ids[&l["airdrome_id"].as_i64().unwrap()].as_str();
        let f = |k: &str| l[k].as_f64().unwrap();
        let s = |k: &str| l[k].as_str().unwrap();
        let hit = got.runways.iter().any(|r| {
            r.airdrome_name_id == name_id
                && (
                    r.name.as_str(),
                    r.edge1_name.as_str(),
                    r.edge2_name.as_str(),
                ) == (s("name"), s("edge1_name"), s("edge2_name"))
                && [r.edge1_x, r.edge1_z, r.edge2_x, r.edge2_z, r.course]
                    == ["edge1_x", "edge1_z", "edge2_x", "edge2_z", "course"].map(f)
        });
        if hit {
            matched_rw += 1;
        } else {
            misses.push(format!("{name_id} runway {}", s("name")));
        }
    }
    eprintln!(
        "runways: {matched_rw} of {} live equal; {} decoded",
        live.len(),
        got.runways.len()
    );

    assert!(misses.is_empty(), "{misses:#?}");
    assert_eq!(got.stands.len(), matched, "stands the game does not list");
    assert_eq!(
        got.runways.len(),
        matched_rw,
        "runways the game does not list"
    );
}

/// Every installed map's airfields decode. Reports what the decode cannot
/// give a stand: a class, or a class with a record.
#[test]
fn every_map_decodes_its_airfields() {
    let Some(root) = install_root() else { return };
    let build = install::read_build(&root).unwrap();
    eprintln!("DCS {} ({})", build.version, build.timestamp);
    for dir in maps(&root) {
        let got = decode(&root, &dir);
        let airfields: BTreeMap<&str, ()> = got
            .stands
            .iter()
            .map(|r| (r.airdrome_name_id.as_str(), ()))
            .chain(
                got.runways
                    .iter()
                    .map(|r| (r.airdrome_name_id.as_str(), ())),
            )
            .collect();
        let unclassed: Vec<String> = got
            .stands
            .iter()
            .filter(|r| r.stand_class.is_none())
            .map(|r| format!("{}/{}", r.airdrome_name_id, r.name))
            .collect();
        let mut no_record: BTreeMap<&str, usize> = BTreeMap::new();
        for r in got.stands.iter().filter(|r| r.flag.is_none()) {
            if let Some(c) = &r.stand_class {
                *no_record.entry(c).or_default() += 1;
            }
        }
        eprintln!(
            "{dir}: {} airfields, {} runways, {} stands; {} without a class {:?}; classes without a record {no_record:?}",
            airfields.len(),
            got.runways.len(),
            got.stands.len(),
            unclassed.len(),
            &unclassed[..unclassed.len().min(6)]
        );
        assert!(!got.runways.is_empty(), "{dir}: no runways");
        assert!(unclassed.is_empty(), "{dir}: stands without a class");
    }
}
