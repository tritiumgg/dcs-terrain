//! The commands for people as the shell sees them: for every request the
//! other surfaces are tested with, the same question asked with flags gives
//! `query`'s answer byte for byte with `--json`, a plain list naming every
//! field of it without, and `query`'s GeoJSON in the file `--geojson-out`
//! names.

use std::process::{Command, Output, Stdio};

use serde_json::{Value, json};

mod common;
use common::{Around, every_operation, synth};

fn run(args: &[String]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_dcsterrain"))
        .args(["--threads", "5"])
        .args(args)
        .stdin(Stdio::null())
        .output()
        .expect("the binary runs")
}

fn words(line: &str) -> Vec<String> {
    line.split_whitespace().map(String::from).collect()
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("stdout is UTF-8")
}

/// `text` with every time taken set to 0, the one thing two runs of the same
/// request may print differently.
fn timeless(text: &str) -> String {
    const KEY: &str = "\"elapsed_ms\":";
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(KEY) {
        let (head, tail) = rest.split_at(at + KEY.len());
        out.push_str(head);
        out.push('0');
        rest = tail.trim_start_matches(|c: char| c.is_ascii_digit());
    }
    out.push_str(rest);
    out
}

/// Every number as a float, so `12000` and `12000.0` compare equal.
fn floats(v: &Value) -> Value {
    match v {
        Value::Number(n) => json!(n.as_f64()),
        Value::Array(a) => Value::Array(a.iter().map(floats).collect()),
        Value::Object(m) => Value::Object(m.iter().map(|(k, v)| (k.clone(), floats(v))).collect()),
        v => v.clone(),
    }
}

/// The flags that ask each request `every_operation` sends, with
/// `--geojson-out` left to the caller.
fn flags(op: &str, at: &Around) -> String {
    let p = |x: f64, z: f64| format!("{x},{z}");
    let here = p(at.x, at.z);
    let east = p(at.x, at.z + 3000.0);
    let north = p(at.x + 3000.0, at.z);
    let (from, to) = (
        p(
            at.from["x"].as_f64().unwrap(),
            at.from["z"].as_f64().unwrap(),
        ),
        p(at.to["x"].as_f64().unwrap(), at.to["z"].as_f64().unwrap()),
    );
    let corner = |d: f64| p(at.x + d, at.z + d);
    let criteria = "--criteria slope-max 30 --criteria dist-road-band 0 50000";
    let placement =
        |id: &str, shape: &str| format!("--placements id={id} at={here} heading=0 {shape}");
    match op {
        "describe" => String::new(),
        "sample" => format!("--points {here} --layers height slope dist_road"),
        "geo" => format!("--to-latlon {here}"),
        "score_site" => format!("--at {here} --footprint 60 40 {criteria}"),
        "find_sites" => format!(
            "--region circle {here} 1500 --footprint 60 40 {criteria} --spacing 200 --limit 3"
        ),
        "visible" => format!("--a {here}@10 --b {east}@100"),
        "viewshed" => format!("--observer {here}@10 --radius 2000 --target-alt-agl 2"),
        "radar_floor" => format!("--site {here}@10 --radius 2000"),
        "coverage" => format!("--site {here}@10 --envelope 2000 30,100 --asset circle {here} 1500"),
        "exposure" => format!("--sites {here}@10 2000 --target-alt-agl 30"),
        "approach_spectrum" => format!("--site {here}@10 --radius 2000 --alt-bands-agl 30"),
        "unmask_profile" => format!("--site {here}@10 --route {east}@100 {north}@100"),
        "route" => format!("--network roads --from {from} --to {to}"),
        "route_alternatives" => format!("--network roads --from {from} --to {to} --k 2"),
        "nearest" => format!("--kind road --at {here} --n 2"),
        "scenery_in" => format!(
            "--region box {} {} --max-rows 1000000",
            corner(1000.0),
            corner(-1000.0)
        ),
        "chokepoints" => format!("--region circle {here} 20000 --network roads --limit 5"),
        "trafficability" => format!("--at {here} --max-slope-deg 30 --network roads"),
        "point_info" => format!("--points {here}"),
        "airfield_info" => format!("--airdrome {}", at.airdrome),
        "validate_placements" => [
            placement("point", "shape=point"),
            placement("rect", "shape=rect shape.width=60 shape.depth=40"),
            placement("circle", "shape=circle shape.radius=30"),
        ]
        .join(" "),
        "towns" => "--limit 3".into(),
        _ => panic!("no flags for {op}"),
    }
}

#[test]
fn every_request_asked_with_flags_answers_as_query_does() {
    let at = synth();
    let dir = std::env::temp_dir().join(format!("dcsterrain-people-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for (op, request) in every_operation(&at) {
        let command = op.replace('_', "-");
        let geojson = request["geojson"] == true;
        let file = dir.join(format!("{op}.geojson"));
        let mut asked = vec![command.clone(), "--terrain".into(), "synth".into()];
        asked.extend(words(&flags(op, &at)));
        if geojson {
            asked.extend(["--geojson-out".into(), file.display().to_string()]);
        }
        let shown = |extra: &str| {
            let mut a = asked.clone();
            a.push(extra.into());
            run(&a)
        };

        // The flags make the request the other surfaces send.
        let made = shown("--show-request");
        assert!(
            made.status.success(),
            "{op}: {}",
            String::from_utf8_lossy(&made.stderr)
        );
        let made: Value = serde_json::from_str(&stdout(&made)).unwrap();
        assert_eq!(floats(&made), floats(&request), "{op}");

        // `--json` prints `query`'s answer byte for byte.
        let mut q = vec![
            "query".into(),
            "--terrain".into(),
            "synth".into(),
            op.into(),
        ];
        q.push(request.to_string());
        let by_query = run(&q);
        assert!(by_query.status.success(), "{op}");
        let by_flags = shown("--json");
        assert!(
            by_flags.status.success(),
            "{op}: {}",
            String::from_utf8_lossy(&by_flags.stderr)
        );
        assert_eq!(
            timeless(&stdout(&by_flags)),
            timeless(&stdout(&by_query)),
            "{op}"
        );

        // `--geojson-out` writes the `FeatureCollection` `query` returns.
        let answer: Value = serde_json::from_str(&stdout(&by_query)).unwrap();
        if geojson {
            let written = std::fs::read_to_string(&file).unwrap();
            let written: Value = serde_json::from_str(&written).unwrap();
            assert_eq!(written["type"], "FeatureCollection", "{op}");
            assert_eq!(written, answer["geojson"], "{op}");
            std::fs::remove_file(&file).unwrap();
        }

        // The plain list names every top-level field.
        let plain = run(&asked);
        assert!(plain.status.success(), "{op}");
        let text = stdout(&plain);
        for k in answer.as_object().unwrap().keys() {
            assert!(
                text.lines().any(|l| l.starts_with(&format!("{k}:"))),
                "{op}: no `{k}` in\n{text}"
            );
        }
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn every_operation_is_asked() {
    let at = synth();
    let asked: Vec<&str> = every_operation(&at).iter().map(|(op, _)| *op).collect();
    let listed = run(&words("schema"));
    let schema: Value = serde_json::from_str(&stdout(&listed)).unwrap();
    let ops = schema["properties"]["operations"]["properties"]
        .as_object()
        .unwrap();
    for op in ops.keys() {
        assert!(asked.contains(&op.as_str()), "{op} is never asked");
    }
}

#[test]
fn a_bad_flag_exits_2_naming_the_closest_and_a_refusal_exits_1_in_words() {
    let bad = run(&words("find-sites --terrain synth --spacng 200"));
    assert_eq!(bad.status.code(), Some(2));
    let said = String::from_utf8_lossy(&bad.stderr);
    assert!(said.contains("Did you mean `--spacing`?"), "{said}");
    assert!(bad.stdout.is_empty());

    let line = "find-sites --terrain synth --region circle 12000,15000 1500 --footprint 60 40 \
                --criteria slope-max 95 --spacing 200";
    let refused = run(&words(line));
    assert_eq!(refused.status.code(), Some(1));
    let said = String::from_utf8_lossy(&refused.stderr);
    assert!(said.starts_with("error: --criteria #1 deg: "), "{said}");

    // With `--json`, the refusal is `query`'s.
    let mut asked = words(line);
    asked.push("--json".into());
    let as_json = run(&asked);
    assert_eq!(as_json.status.code(), Some(1));
    let request = json!({"region": {"Circle": {"x": 12000, "z": 15000, "radius": 1500}},
        "footprint": {"width": 60, "depth": 40},
        "criteria": [{"name": "slope_max", "params": {"deg": 95}}], "spacing": 200});
    let by_query = run(&[
        "query".into(),
        "--terrain".into(),
        "synth".into(),
        "find_sites".into(),
        request.to_string(),
    ]);
    assert_eq!(stdout(&as_json), stdout(&by_query));
}

#[test]
fn a_place_by_name_is_read_on_the_terrain() {
    let by_name = run(&words(
        "nearest --terrain synth --kind airdrome --at town:Synth_Town_1 --json",
    ));
    assert!(
        by_name.status.success(),
        "{}",
        String::from_utf8_lossy(&by_name.stderr)
    );
    let answer: Value = serde_json::from_str(&stdout(&by_name)).unwrap();
    assert_eq!(answer["results"][0]["name"], "Synth Field");

    let bare = run(&words(
        "nearest --terrain synth --kind airdrome --at Synth_Field",
    ));
    assert_eq!(bare.status.code(), Some(1));
    let said = String::from_utf8_lossy(&bare.stderr);
    assert!(
        said.contains("needs a kind. Type airport:Synth Field"),
        "{said}"
    );

    let info = run(&words(
        "airfield-info --terrain synth --airdrome Synth_Field --json",
    ));
    assert!(
        info.status.success(),
        "{}",
        String::from_utf8_lossy(&info.stderr)
    );
    let answer: Value = serde_json::from_str(&stdout(&info)).unwrap();
    assert_eq!(answer["name_id"], "SYNTH");
}

#[test]
fn help_reads_from_the_schema_with_an_example() {
    for line in ["find-sites --help", "help find-sites", "find_sites -h"] {
        let out = run(&words(line));
        assert!(out.status.success(), "{line}");
        let text = stdout(&out);
        assert!(text.starts_with("Find the best sites"), "{line}: {text}");
        assert!(text.contains("--spacing <number>"), "{line}");
        assert!(text.contains("Example:\n  dcsterrain find-sites"), "{line}");
    }
    let top = stdout(&run(&words("--help")));
    assert!(
        top.contains("find-sites") && top.contains("route-alternatives"),
        "{top}"
    );
}
