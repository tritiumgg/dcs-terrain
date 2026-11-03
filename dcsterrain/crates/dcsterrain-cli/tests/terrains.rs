//! Opening a terrain by its id, and listing an install's terrains, on an
//! install tree the test builds.

use std::path::Path;
use std::process::{Command, Output};

use serde_json::{Value, json};

mod common;
use common::install::{header, install, put, roads, scenery, scratch, surface};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_dcsterrain"))
        .args(args)
        .output()
        .expect("the binary runs")
}

fn stdout_json(out: &Output) -> Value {
    serde_json::from_slice(&out.stdout).expect("stdout is one JSON value")
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
fn refuses_an_unknown_operation_and_a_flag_for_another_terrain() {
    let out = run(&["query", "--terrain", "synth", "nope", "{}"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(stdout_json(&out)["issues"][0]["code"], "unknown_operation");

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
    let c = scratch();
    let cache = c.0.to_str().unwrap();
    let out = run(&[
        "query",
        "--install",
        root,
        "--terrain",
        "GermanyCW",
        "--cache",
        cache,
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
    let want = format!("{}-{}-{}", surface().len(), roads().len(), scenery().len());
    assert_eq!(v["terrain_fingerprint"], want);
    let extent = json!({"min_x": 0.0, "min_z": 0.0, "max_x": 1000.0, "max_z": 1000.0});
    assert_eq!(v["extent"], extent);
    // Its table of contents was built into the cache on the way.
    assert!(c.0.join("GermanyCW").join("mesh.toc").is_file());

    // A folder's name is not an id, and the refusal lists the ids.
    let out = run(&[
        "query",
        "--install",
        root,
        "--terrain",
        "GermanyColdWar",
        "--cache",
        cache,
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
    let c = scratch();
    let cache = c.0.to_str().unwrap();

    let out = run(&["list-terrains", "--install", root, "--cache", cache]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).unwrap();
    let rows: Vec<Vec<&str>> = text
        .lines()
        .skip(4)
        .map(|l| l.split_whitespace().collect())
        .collect();
    assert_eq!(rows[0][..2], ["Caucasus", "Caucasus"], "{text}");
    assert_eq!(rows[0][2..], ["none", "3", "of", "3", "readable"], "{text}");
    assert_eq!(rows[1][..2], ["GermanyCW", "GermanyColdWar"], "{text}");
    // A scene file no decoder reads refuses scenery, not the terrain.
    assert_eq!(rows[1][3..], ["2", "of", "3", "readable"], "{text}");
    assert!(!text.contains("cannot open"), "{text}");
    assert!(
        text.contains(
            "unreadable: Mods/terrains/GermanyColdWar/Scenes/GermanyColdWar.scn5 \
             (landscape5::Scene5File version 10)"
        ),
        "{text}"
    );

    let out = run(&[
        "list-terrains",
        "--install",
        root,
        "--json",
        "--cache",
        cache,
    ]);
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
    assert_eq!(terrains[1]["readable"], true);
    assert_eq!(terrains[1]["refusal"], Value::Null);
    let scene = &terrains[1]["files"][2];
    assert_eq!(
        (&scene["role"], &scene["version"], &scene["readable"]),
        (&"scene".into(), &10.into(), &false.into())
    );

    let out = run(&[
        "query",
        "--install",
        root,
        "--terrain",
        "GermanyCW",
        "--cache",
        cache,
        "describe",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(stdout_json(&out)["terrain"], "GermanyCW");
}

#[test]
fn refuses_a_directory_that_is_no_install() {
    let t = install(false);
    let missing = t.0.join("nowhere");
    let out = run(&["list-terrains", "--install", missing.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
    let out = run(&[
        "list-terrains",
        "--install",
        missing.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(stdout_json(&out)["issues"][0]["code"], "no_install");
}

/// Runs the binary with `input` on its stdin.
fn run_with(args: &[&str], input: &str) -> Output {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = Command::new(env!("CARGO_BIN_EXE_dcsterrain"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the binary runs");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn list_terrains_shows_a_cache_after_index_and_none_after_clear() {
    let t = install(false);
    let root = t.0.to_str().unwrap();
    let c = scratch();
    let cache = c.0.to_str().unwrap();
    let listed = || {
        let out = run(&[
            "list-terrains",
            "--install",
            root,
            "--json",
            "--cache",
            cache,
        ]);
        assert!(out.status.success());
        stdout_json(&out)
    };
    let v = listed();
    assert_eq!(v["cache"], cache);
    assert_eq!(v["terrains"][0]["cache"], Value::Null);

    let out = run(&[
        "index",
        "--install",
        root,
        "--terrain",
        "GermanyCW",
        "--cache",
        cache,
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let dir = c.0.join("GermanyCW");
    assert!(dir.is_dir());
    let v = listed();
    assert_eq!(v["terrains"][0]["cache"], Value::Null, "Caucasus has none");
    let germany = &v["terrains"][1]["cache"];
    assert_eq!(germany["path"], dir.to_str().unwrap());
    let entries = germany["entries"].as_array().unwrap();
    let names: Vec<&str> = entries
        .iter()
        .map(|e| e["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "mesh.toc",
            "places",
            "roads.betweenness",
            "roads.graph",
            "scene.nodes",
            "tables.json"
        ]
    );
    assert!(entries.iter().all(|e| e["size"].as_u64().unwrap() > 0));

    let out = run(&["list-terrains", "--install", root, "--cache", cache]);
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(
        text.contains("GermanyCW  GermanyColdWar  6 entries, "),
        "{text}"
    );

    let out = run(&[
        "cache",
        "list",
        "--install",
        root,
        "--json",
        "--cache",
        cache,
    ]);
    let v = stdout_json(&out);
    assert_eq!(v["stored"][0]["terrain"], "GermanyCW");
    assert_eq!(v["stored"][0]["installed"], true);

    let out = run(&["cache", "clear", "--cache", cache]);
    assert!(out.status.success());
    assert_eq!(listed()["terrains"][1]["cache"], Value::Null);
}

#[test]
fn clear_asks_before_deleting_the_validation_figures() {
    let t = install(false);
    let root = t.0.to_str().unwrap();
    let c = scratch();
    let cache = c.0.to_str().unwrap();
    let index = || {
        let out = run(&[
            "index",
            "--install",
            root,
            "--terrain",
            "Caucasus",
            "--cache",
            cache,
        ]);
        assert!(out.status.success());
    };
    index();
    let dir = c.0.join("Caucasus");
    put(&dir, "validation.json", b"{}");
    put(&dir, "mesh.toc", b"rebuildable");

    let out = run_with(&["cache", "clear", "--cache", cache], "n\n");
    assert!(out.status.success());
    let asked = String::from_utf8_lossy(&out.stderr);
    assert!(asked.contains("Delete them too? [y/N]"), "{asked}");
    assert!(dir.join("validation.json").is_file());
    assert!(!dir.join("mesh.toc").exists());

    // No answer is no.
    let out = run_with(&["cache", "clear", "--cache", cache], "");
    assert!(out.status.success());
    assert!(dir.join("validation.json").is_file());

    let out = run_with(&["cache", "clear", "--cache", cache], "y\n");
    assert!(out.status.success());
    assert!(!c.0.join("Caucasus").exists());

    index();
    put(&dir, "validation.json", b"{}");
    let out = run_with(&["cache", "clear", "--yes", "--cache", cache], "");
    assert!(!String::from_utf8_lossy(&out.stderr).contains("[y/N]"));
    assert!(!c.0.join("Caucasus").exists());
}

#[test]
fn cache_list_names_the_build_each_terrain_was_made_on() {
    let t = install(false);
    let root = t.0.to_str().unwrap();
    let c = scratch();
    let cache = c.0.to_str().unwrap();
    let out = run(&[
        "index",
        "--install",
        root,
        "--terrain",
        "Caucasus",
        "--cache",
        cache,
    ]);
    assert!(out.status.success());
    let out = run(&["cache", "list", "--install", root, "--cache", cache]);
    assert!(out.status.success());
    let said = String::from_utf8(out.stdout).unwrap();
    assert!(
        said.contains("Caucasus on 0.0.0.1 (the installed build):"),
        "{said}"
    );
    let out = run(&[
        "cache",
        "list",
        "--json",
        "--install",
        root,
        "--cache",
        cache,
    ]);
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["stored"][0]["dcs_builds"], json!(["0.0.0.1"]));
    assert_eq!(v["stored"][0]["installed"], true);
    // Nothing is kept per build, so there is nothing to prune.
    let out = run(&["cache", "prune", "--install", root, "--cache", cache]);
    assert_eq!(out.status.code(), Some(2));

    // A directory this tool did not make is never deleted from.
    let other = scratch();
    put(&other.0, "Caucasus/0.0.0.0/keep", b"mine");
    let out = run(&[
        "cache",
        "clear",
        "--yes",
        "--cache",
        other.0.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(1));
    assert!(other.0.join("Caucasus/0.0.0.0/keep").is_file());
}

/// Three beacons for GermanyCW, one 4 m from where its degrees put it: the
/// fit keeps it, and misses it by more than 2 m.
fn beacons_off_by_4_m(root: &Path) {
    let tm = dcsterrain_core::crs::table("GermanyCW").unwrap();
    let mut lua = String::from("beacons = {\n");
    for (k, (x, z)) in [(0.0, 0.0), (50_000.0, 10_000.0), (-20_000.0, 60_000.0)]
        .into_iter()
        .enumerate()
    {
        let (lat, lon) = tm.to_latlon(x, z);
        let x = if k == 0 { x + 4.0 } else { x };
        lua += &format!(
            "{{ beaconId = 'b{k}'; position = {{ {x}, 0, {z} }}; \
             positionGeo = {{ latitude = {lat}, longitude = {lon} }} }},\n"
        );
    }
    lua += "}\n";
    put(
        root,
        "Mods/terrains/GermanyColdWar/Beacons.lua",
        lua.as_bytes(),
    );
}

#[test]
fn index_refuses_a_projection_fitted_over_two_meters_off_unless_allowed() {
    let t = install(false);
    beacons_off_by_4_m(&t.0);
    let root = t.0.to_str().unwrap();
    let c = scratch();
    let cache = c.0.to_str().unwrap();
    let index = |extra: &[&str]| {
        let mut args = vec!["index", "--install", root, "--terrain", "GermanyCW"];
        args.extend(["--cache", cache]);
        args.extend(extra);
        run(&args)
    };
    let out = index(&[]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("misses one by 2.67 m"), "{err}");
    assert!(err.contains("--allow-crs-residual"), "{err}");
    let out = index(&["--allow-crs-residual"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn query_refuses_a_projection_fitted_over_two_meters_off_unless_allowed() {
    let t = install(false);
    beacons_off_by_4_m(&t.0);
    let root = t.0.to_str().unwrap();
    let c = scratch();
    let cache = c.0.to_str().unwrap();
    let query = |extra: &[&str]| {
        let mut args = vec!["query", "--install", root, "--terrain", "GermanyCW"];
        args.extend(["--cache", cache]);
        args.extend(extra);
        args.push("describe");
        run(&args)
    };
    let out = query(&[]);
    assert_eq!(out.status.code(), Some(1));
    let issue = &stdout_json(&out)["issues"][0];
    assert_eq!(issue["code"], "terrain_refused");
    let message = issue["message"].as_str().unwrap();
    assert!(message.contains("misses one by 2.67 m"), "{message}");
    let out = query(&["--allow-crs-residual"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v = stdout_json(&out);
    assert_eq!(v["counts"]["beacons"].as_u64().map(|n| n >= 3), Some(true));
    let entries: Vec<&str> = v["cache"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["name"].as_str().unwrap())
        .collect();
    assert!(entries.contains(&"tables.json"), "{entries:?}");
}

#[test]
fn check_reports_a_terrain_every_decoder_reads_and_passes() {
    let t = install(false);
    let c = scratch();
    let check = |json: bool| {
        let mut args = vec!["check", "--install", t.0.to_str().unwrap()];
        args.extend(["--terrain", "GermanyCW", "--cache", c.0.to_str().unwrap()]);
        if json {
            args.push("--json");
        }
        run(&args)
    };

    let out = check(false);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{text}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        text.starts_with("GermanyCW in GermanyColdWar on DCS 0.0.0.1"),
        "{text}"
    );
    let scene = "scene Mods/terrains/GermanyColdWar/Scenes/GermanyColdWar.scn5 \
                 landscape5::Scene5File version 9";
    assert!(
        text.lines()
            .any(|l| l.split_whitespace().collect::<Vec<_>>().join(" ") == scene),
        "{text}"
    );
    assert!(text.contains("  mesh.toc          missing"), "{text}");
    assert!(text.contains("scenery: 2 models, 1 objects"), "{text}");
    assert!(text.contains("overrides naming no model: none"), "{text}");
    assert!(text.ends_with("\nno problems\n"), "{text}");

    // Asked again, the entries reading the tables and the scenery built are
    // current, and the road graph was never built.
    let v = stdout_json(&check(true));
    let states: Vec<(&str, &str)> = v["cache"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| (e["name"].as_str().unwrap(), e["state"].as_str().unwrap()))
        .collect();
    assert_eq!(
        states,
        [
            ("mesh.toc", "current"),
            ("scene.nodes", "current"),
            ("tables.json", "current"),
            ("roads.graph", "missing"),
            ("roads.betweenness", "missing"),
            ("places", "missing"),
        ]
    );
    assert_eq!(v["files"].as_array().unwrap().len(), 3);
    assert_eq!(v["catalog"]["most_placed"][0]["model"], "house");
    assert_eq!(v["tables"]["lua_unresolved"], json!([]));
    assert_eq!(v["validation"], Value::Null);
    assert_eq!(v["problems"], json!([]));
}

#[test]
fn check_fails_naming_each_file_no_decoder_reads_and_reports_the_rest() {
    let t = install(true);
    // The road file's class renamed, beside the scene file's version.
    put(
        &t.0,
        "Mods/terrains/GermanyColdWar/roads/GermanyColdWar.rn4",
        &header("landscape4::lRoadNetwork6", 5),
    );
    let c = scratch();
    let check = |json: bool| {
        let mut args = vec!["check", "--install", t.0.to_str().unwrap()];
        args.extend(["--terrain", "GermanyCW", "--cache", c.0.to_str().unwrap()]);
        if json {
            args.push("--json");
        }
        run(&args)
    };

    let out = check(false);
    assert_eq!(out.status.code(), Some(1));
    let text = String::from_utf8_lossy(&out.stdout);
    let scene = "Mods/terrains/GermanyColdWar/Scenes/GermanyColdWar.scn5: \
                 landscape5::Scene5File version 10 has no decoder; this build reads version 8, 9";
    let roads = "Mods/terrains/GermanyColdWar/roads/GermanyColdWar.rn4: \
                 no decoder reads landscape4::lRoadNetwork6 (version 5)";
    assert!(
        text.contains(&format!("problems\n  {roads}\n  {scene}\n")),
        "{text}"
    );
    assert!(text.contains("version 10, NO DECODER READS IT"), "{text}");
    // Each refuses only what reads its role: the tables are still read.
    assert!(text.contains("\ntables\n"), "{text}");
    assert!(!text.contains("scenery:"), "{text}");

    let v = stdout_json(&check(true));
    assert_eq!(v["problems"], json!([roads, scene]));
    assert_eq!(v["files"][2]["why"], scene);
    assert_eq!(v["catalog"], Value::Null);
    assert!(v["tables"].is_object());

    // A surface file no decoder reads refuses the terrain, and is said once.
    put(
        &t.0,
        "Mods/terrains/GermanyColdWar/Surface/GermanyColdWar.surface5",
        &header("landscape5::Surface5File", 6),
    );
    let out = check(true);
    assert_eq!(out.status.code(), Some(1));
    let v = stdout_json(&out);
    let problems = v["problems"].as_array().unwrap();
    assert_eq!(problems.len(), 3, "{problems:?}");
    assert!(
        problems[2]
            .as_str()
            .unwrap()
            .contains("Surface5File version 6"),
        "{problems:?}"
    );
    assert_eq!((&v["cache"], &v["tables"]), (&Value::Null, &Value::Null));
}

#[test]
fn check_refuses_an_id_the_install_lacks_naming_those_it_holds() {
    let t = install(false);
    let c = scratch();
    let out = run(&[
        "check",
        "--install",
        t.0.to_str().unwrap(),
        "--terrain",
        "Nowhere",
        "--cache",
        c.0.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains(
            "no terrain Nowhere in this install; the ids it holds are Caucasus, GermanyCW"
        ),
        "{err}"
    );
}

#[test]
fn check_fails_on_a_fit_over_two_meters_and_unreadable_figures() {
    let t = install(false);
    let root = t.0.to_str().unwrap();
    let c = scratch();
    let cache = c.0.to_str().unwrap();
    let args = [
        "--install",
        root,
        "--terrain",
        "GermanyCW",
        "--cache",
        cache,
    ];
    let mut index = vec!["index"];
    index.extend(args);
    assert!(run(&index).status.success());
    beacons_off_by_4_m(&t.0);
    put(&c.0, "GermanyCW/validation.json", b"not json");

    let mut check = vec!["check", "--json"];
    check.extend(args);
    let out = run(&check);
    assert_eq!(out.status.code(), Some(1));
    let v = stdout_json(&out);
    let problems: Vec<&str> = v["problems"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_str().unwrap())
        .collect();
    assert_eq!(problems.len(), 2, "{problems:?}");
    assert!(
        problems[0].contains("validation.json is not JSON"),
        "{problems:?}"
    );
    assert!(problems[1].contains("misses one by 2.67 m"), "{problems:?}");
    assert_eq!(v["validation"], Value::Null);
}

#[test]
fn check_reports_the_scenery_past_an_airfield_file_no_decoder_reads() {
    let t = install(false);
    put(
        &t.0,
        "Mods/terrains/GermanyColdWar/AirfieldsTaxiways/Ghost.rn5",
        &header("t5::rn5file", 1),
    );
    let c = scratch();
    let out = run(&[
        "check",
        "--json",
        "--install",
        t.0.to_str().unwrap(),
        "--terrain",
        "GermanyCW",
        "--cache",
        c.0.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(1));
    let v = stdout_json(&out);
    assert_eq!(
        v["problems"],
        json!(
            ["Mods/terrains/GermanyColdWar/AirfieldsTaxiways/Ghost.rn5: \
             t5::rn5file version 1 has no decoder; this build reads version 0"]
        )
    );
    assert_eq!(v["tables"], Value::Null);
    assert_eq!(v["catalog"]["objects"], 1);
}

#[test]
fn a_batch_reopens_a_terrain_whose_files_changed_under_it() {
    use std::io::{BufRead, BufReader, Write};
    use std::process::Stdio;
    let t = install(false);
    let c = scratch();
    let mut child = Command::new(env!("CARGO_BIN_EXE_dcsterrain"))
        .args(["--verbose", "query", "--terrain", "Caucasus", "--install"])
        .arg(&t.0)
        .arg("--cache")
        .arg(&c.0)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut ask = |line: &str| {
        writeln!(stdin, "{line}").unwrap();
        let mut answer = String::new();
        stdout.read_line(&mut answer).unwrap();
        serde_json::from_str::<Value>(&answer).unwrap()
    };
    let describe = r#"{"op": "describe"}"#;
    assert_eq!(ask(describe)["result"]["dcs_build"], "0.0.0.1");
    assert_eq!(ask(describe)["result"]["dcs_build"], "0.0.0.1");

    let update = br#"{"version": "0.0.0.2", "timestamp": "20000102-000000"}"#;
    put(&t.0, "autoupdate.cfg", update);
    assert_eq!(ask(describe)["result"]["dcs_build"], "0.0.0.2");

    // A terrain gone refuses its line, and the next tries again.
    put(&t.0, "autoupdate.cfg", b"not json");
    assert_eq!(ask(describe)["issues"][0]["code"], "terrain_refused");
    put(&t.0, "autoupdate.cfg", update);
    assert_eq!(ask(describe)["result"]["dcs_build"], "0.0.0.2");

    drop(stdin);
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    let log = String::from_utf8_lossy(&out.stderr);
    assert_eq!(log.matches("opened terrain").count(), 3, "{log}");
}
