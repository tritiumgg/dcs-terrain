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

        // The plain list names every top-level field; the operations that
        // print a table are checked by `tables_*`.
        if TABLED.contains(&op) {
            continue;
        }
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

/// The operations that print a table rather than the plain list.
const TABLED: &[&str] = &[
    "find_sites",
    "nearest",
    "route",
    "route_alternatives",
    "point_info",
    "airfield_info",
    "towns",
    "visible",
];

/// The questions each table is shown with on `Synth`: a name for its
/// snapshot, and the command line after the command, where a quoted
/// value holds its spaces. Places are written in each form a person
/// types.
const TABLE_CASES: &[(&str, &str)] = &[
    (
        "find_sites",
        "find-sites --region circle 36TXR1451710114 1500 --footprint 60 40 \
         --criteria slope-max 30 --criteria dist-road-band 0 50000 \
         --criteria cover-in plain --spacing 200 --limit 3",
    ),
    (
        "find_sites-none",
        "find-sites --region circle 12000,15000 1500 --footprint 60 40 \
         --criteria dist-road-band 0 100 --spacing 200",
    ),
    (
        "nearest",
        "nearest --kind road --at \"45.2352N 34.4590E\" --n 3",
    ),
    (
        "nearest-airdromes",
        "nearest --kind airdrome --at \"town:Synth Town 1\" --n 2",
    ),
    (
        "route",
        "route --network roads --from 10000,-12000 --to 20000,10000",
    ),
    (
        "route-seen",
        "route --network roads --from 10000,-12000 --to \"airport:Synth Field\" \
         --exposure-observers 24000,-15000@10",
    ),
    (
        "route_alternatives",
        "route-alternatives --network roads --from 10000,-12000 --to 36TXR1451723140 \
         --k 4 --max-stretch 3 --max-overlap 0.9",
    ),
    (
        "route_alternatives-one",
        "route-alternatives --network roads --from 10000,-12000 --to 20000,10000",
    ),
    (
        "point_info",
        "point-info --points 12000,15000 \"town:Synth Town 1\" \"airport:Synth Field\" \
         45.30N,34.10E 36TXR1451710114",
    ),
    ("airfield_info", "airfield-info --airdrome \"Synth Field\""),
    (
        "airfield_info-fits",
        "airfield-info --airdrome SYNTH --fits width=16 length=17 height=5 helicopter",
    ),
    (
        "towns",
        "towns --region circle \"airport:Synth Field\" 30000 --limit 2",
    ),
    ("towns-none", "towns --size city"),
    (
        "visible",
        "visible --a \"town:Synth Town 2@10\" --b 45.2352N,34.4975E@100",
    ),
    (
        "visible-blocked",
        "visible --a \"town:Synth Town 1@2\" --b \"airport:Synth Field@570msl\"",
    ),
];

/// A command line split as a shell would, keeping quoted words whole.
fn shell_words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    for c in line.chars() {
        match c {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
            }
            c => word.push(c),
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

/// Accepts a changed snapshot under `DCSTERRAIN_UPDATE_SNAPSHOTS=1`.
fn golden(name: &str, printed: &str) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/snapshots/tables")
        .join(format!("{name}.txt"));
    if std::env::var_os("DCSTERRAIN_UPDATE_SNAPSHOTS").is_some_and(|v| v == "1") {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, printed).unwrap();
        return;
    }
    let committed = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        printed == committed,
        "{name}: the table differs from {}; rerun with DCSTERRAIN_UPDATE_SNAPSHOTS=1 to \
         accept it, and review the diff. It printed:\n{printed}",
        path.display()
    );
}

/// Every point with an `x` and a `z` anywhere in `v`.
fn points(v: &Value, out: &mut Vec<(f64, f64)>) {
    match v {
        Value::Object(m) => {
            if let (Some(x), Some(z)) = (
                m.get("x").and_then(Value::as_f64),
                m.get("z").and_then(Value::as_f64),
            ) {
                out.push((x, z));
            }
            m.values().for_each(|v| points(v, out));
        }
        Value::Array(a) => a.iter().for_each(|v| points(v, out)),
        _ => {}
    }
}

/// Each MGRS a page shows, with the x and z written just before it on its
/// line: `12000  15000  36T XR 14517 10114`, or `12000, 15000 (36T …)`.
fn grids_shown(page: &str) -> Vec<(f64, f64, String)> {
    let mut found = Vec::new();
    for line in page.lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        for i in 0..words.len().saturating_sub(3) {
            let zone = words[i].trim_start_matches('(');
            let digits = |w: &str| !w.is_empty() && w.chars().all(|c| c.is_ascii_digit());
            let is_zone = zone.len() >= 2
                && zone.ends_with(|c: char| c.is_ascii_uppercase())
                && digits(&zone[..zone.len() - 1]);
            let square =
                words[i + 1].len() == 2 && words[i + 1].chars().all(|c| c.is_ascii_uppercase());
            let (e, n) = (words[i + 2], words[i + 3].trim_end_matches([')', ',', '.']));
            if !(is_zone && square && digits(e) && digits(n) && e.len() == n.len()) {
                continue;
            }
            let numbers: Vec<f64> = words[..i]
                .iter()
                .filter_map(|w| w.trim_end_matches(',').parse().ok())
                .collect();
            assert!(numbers.len() >= 2, "no x and z before the MGRS in: {line}");
            let (x, z) = (numbers[numbers.len() - 2], numbers[numbers.len() - 1]);
            found.push((x, z, format!("{zone} {} {e} {n}", words[i + 1])));
        }
    }
    found
}

#[test]
fn tables_fit_100_columns_carry_geo_s_mgrs_and_no_color_when_piped() {
    let cases = TABLE_CASES;
    for op in TABLED {
        assert!(
            cases
                .iter()
                .any(|(name, _)| name.split('-').next() == Some(*op)),
            "no table case for {op}"
        );
    }
    for &(name, line) in cases {
        let mut asked = shell_words(line);
        asked.splice(1..1, ["--terrain".to_string(), "synth".into()]);
        let out = run(&asked);
        assert!(
            out.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let page = stdout(&out);
        golden(name, &page);

        // Not a terminal, so no color.
        assert!(!page.contains('\x1b'), "{name}: color when piped");
        for l in page.lines() {
            assert!(l.chars().count() <= 100, "{name}: over 100 columns: {l}");
        }

        // Each MGRS is `geo`'s for a point of the answer or the request
        // within the half meter x and z are rounded to.
        let mut at = Vec::new();
        for extra in ["--json", "--show-request"] {
            let mut a = asked.clone();
            a.push(extra.into());
            let v: Value = serde_json::from_str(&stdout(&run(&a))).unwrap();
            points(&v, &mut at);
        }
        let shown = grids_shown(&page);
        let to_latlon: Vec<Value> = at.iter().map(|&(x, z)| json!({"x": x, "z": z})).collect();
        let geo = run(&[
            "query".into(),
            "--terrain".into(),
            "synth".into(),
            "geo".into(),
            json!({"to_latlon": to_latlon}).to_string(),
        ]);
        let geo: Value = serde_json::from_str(&stdout(&geo)).unwrap();
        for (x, z, grid) in &shown {
            let matches: Vec<&str> = at
                .iter()
                .zip(geo["to_latlon"].as_array().unwrap())
                .filter(|((px, pz), _)| (px - x).abs() <= 0.5 && (pz - z).abs() <= 0.5)
                .filter_map(|(_, g)| g["mgrs"].as_str())
                .collect();
            assert!(
                !matches.is_empty(),
                "{name}: {x}, {z} is no point of the answer"
            );
            assert!(
                matches.contains(&grid.as_str()),
                "{name}: {grid} at {x}, {z}, where geo gives {matches:?}"
            );
        }
        // Only an answer that is empty shows no place.
        if !name.ends_with("-none") {
            assert!(!shown.is_empty(), "{name}: no MGRS shown");
        }
    }
}
