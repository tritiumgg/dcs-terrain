//! Opening a terrain by its id, and listing an install's terrains, on an
//! install tree the test builds: headers only, no terrain data.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::Value;

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_dcsterrain"))
        .args(args)
        .output()
        .expect("the binary runs")
}

fn stdout_json(out: &Output) -> Value {
    serde_json::from_slice(&out.stdout).expect("stdout is one JSON value")
}

/// A fresh directory under the system temp, removed when dropped.
struct Tree(PathBuf);

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn put(root: &Path, rel: &str, bytes: &[u8]) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, bytes).unwrap();
}

/// A container header as every terrain file opens with: container version
/// 2, header size 0x30, a data base, sixteen zero bytes, then the class name
/// with its length and the class's format version.
fn header(class: &str, version: u32) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&2u32.to_le_bytes());
    b.extend_from_slice(&0x30u32.to_le_bytes());
    b.extend_from_slice(&0u64.to_le_bytes());
    b.extend_from_slice(&[0; 16]);
    b.extend_from_slice(&(class.len() as u32).to_le_bytes());
    b.extend_from_slice(class.as_bytes());
    b.extend_from_slice(&version.to_le_bytes());
    b
}

fn terrain(root: &Path, folder: &str, id: &str, scene_version: u32) {
    let m = format!("Mods/terrains/{folder}");
    let entry = format!(
        "theatre =\n{{\n\t['update_id'] = \"{}\";\n\t['id'] = \"{id}\";\n}}\n",
        id.to_uppercase()
    );
    put(root, &format!("{m}/entry.lua"), entry.as_bytes());
    let files = [
        ("Surface", "surface5", "landscape5::Surface5File", 5),
        ("roads", "rn4", "landscape4::lRoadNetwork", 5),
        ("Scenes", "scn5", "landscape5::Scene5File", scene_version),
    ];
    for (dir, ext, class, version) in files {
        put(
            root,
            &format!("{m}/{dir}/{folder}.{ext}"),
            &header(class, version),
        );
    }
}

/// An install holding Caucasus, and Germany under a folder that is not its
/// id, whose scene file states a version no decoder reads when `bad`.
fn install(bad_scene: bool) -> Tree {
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "dcsterrain-cli-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dir);
    put(
        &dir,
        "autoupdate.cfg",
        br#"{"version": "0.0.0.1", "timestamp": "20000101-000000"}"#,
    );
    terrain(&dir, "Caucasus", "Caucasus", 9);
    terrain(
        &dir,
        "GermanyColdWar",
        "GermanyCW",
        if bad_scene { 10 } else { 9 },
    );
    Tree(dir)
}

#[test]
fn describes_the_synthetic_terrain_with_no_install() {
    let out = run(&["query", "--terrain", "synth", "describe"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v = stdout_json(&out);
    assert_eq!(v["terrain"], "synth");
    assert_eq!(v["cell_size"], 50.0);
    assert!(v["extent"].is_object());

    let out = run(&["query", "--terrain", "synth", "describe", "{}"]);
    assert!(out.status.success());
}

#[test]
fn refuses_an_operation_not_built_and_a_flag_for_another_terrain() {
    let out = run(&["query", "--terrain", "synth", "visible", "{}"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(stdout_json(&out)["issues"][0]["code"], "not_built");

    let out = run(&["query", "--terrain", "synth", "--cell", "100", "describe"]);
    assert_eq!(stdout_json(&out)["issues"][0]["code"], "terrain_refused");

    let t = install(false);
    let root = t.0.to_str().unwrap();
    let args = [
        "query",
        "--install",
        root,
        "--terrain",
        "Caucasus",
        "--seed",
        "2",
    ];
    let out = run(&[&args[..], &["describe"]].concat());
    assert_eq!(stdout_json(&out)["issues"][0]["code"], "invalid_flag");
}

#[test]
fn opens_a_terrain_by_the_id_its_entry_declares() {
    let t = install(false);
    let root = t.0.to_str().unwrap();
    let out = run(&[
        "query",
        "--install",
        root,
        "--terrain",
        "GermanyCW",
        "describe",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v = stdout_json(&out);
    assert_eq!(v["terrain"], "GermanyCW");
    assert_eq!(v["dcs_build"], "0.0.0.1");
    assert_eq!(v["dcs_build_timestamp"], "20000101-000000");
    let path = v["files"][0]["path"].as_str().unwrap();
    assert!(path.starts_with("Mods/terrains/GermanyColdWar/"), "{path}");
    let size = |class: &str| header(class, 0).len();
    let want = format!(
        "{}-{}-{}",
        size("landscape5::Surface5File"),
        size("landscape4::lRoadNetwork"),
        size("landscape5::Scene5File")
    );
    assert_eq!(v["terrain_fingerprint"], want);
    assert_eq!(v["extent"], Value::Null);

    // A folder's name is not an id, and the refusal lists the ids.
    let out = run(&[
        "query",
        "--install",
        root,
        "--terrain",
        "GermanyColdWar",
        "describe",
    ]);
    assert_eq!(out.status.code(), Some(1));
    let issue = &stdout_json(&out)["issues"][0];
    assert_eq!(issue["code"], "terrain_refused");
    let message = issue["message"].as_str().unwrap();
    assert!(
        message.ends_with("the ids it holds are Caucasus, GermanyCW"),
        "{message}"
    );
}

#[test]
fn lists_both_terrains_with_their_folders_and_an_unreadable_file() {
    let t = install(true);
    let root = t.0.to_str().unwrap();

    let out = run(&["list-terrains", "--install", root]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).unwrap();
    let rows: Vec<Vec<&str>> = text
        .lines()
        .skip(3)
        .map(|l| l.split_whitespace().collect())
        .collect();
    assert_eq!(rows[0][..2], ["Caucasus", "Caucasus"], "{text}");
    assert_eq!(rows[0][2..], ["3", "of", "3", "readable"], "{text}");
    assert_eq!(rows[1][..2], ["GermanyCW", "GermanyColdWar"], "{text}");
    assert!(text.contains("2 of 3 readable, cannot open"), "{text}");
    assert!(
        text.contains(
            "unreadable: Mods/terrains/GermanyColdWar/Scenes/GermanyColdWar.scn5 \
             (landscape5::Scene5File version 10)"
        ),
        "{text}"
    );

    let out = run(&["list-terrains", "--install", root, "--json"]);
    assert!(out.status.success());
    let v = stdout_json(&out);
    assert_eq!(v["dcs_build"], "0.0.0.1");
    let terrains = v["terrains"].as_array().unwrap();
    let ids: Vec<(&str, &str)> = terrains
        .iter()
        .map(|t| (t["id"].as_str().unwrap(), t["folder"].as_str().unwrap()))
        .collect();
    assert_eq!(
        ids,
        [("Caucasus", "Caucasus"), ("GermanyCW", "GermanyColdWar")]
    );
    assert_eq!(terrains[0]["readable"], true);
    assert_eq!(terrains[1]["readable"], false);
    let scene = &terrains[1]["files"][2];
    assert_eq!(
        (&scene["role"], &scene["version"], &scene["readable"]),
        (&"scene".into(), &10.into(), &false.into())
    );
    assert!(
        terrains[1]["refusal"]
            .as_str()
            .unwrap()
            .contains("version 10")
    );
}

#[test]
fn refuses_a_directory_that_is_no_install() {
    let t = install(false);
    let missing = t.0.join("nowhere");
    let out = run(&["list-terrains", "--install", missing.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
}
