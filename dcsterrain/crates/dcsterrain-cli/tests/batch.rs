//! `dcsterrain query` as the shell sees it: one request per operation, a
//! JSON-lines batch of them, and `schema`, checked against each other.
//!
//! Set `DCSTERRAIN_INSTALL` to also run every operation in a batch on every
//! terrain the install holds. Its cache is the user's own unless
//! `DCSTERRAIN_CACHE` names another, since `chokepoints` and `towns` answer
//! only from what `index` built. The install is only read.
//!
//! To accept a change to a golden file, run with
//! `DCSTERRAIN_UPDATE_SNAPSHOTS=1` and review the diff.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Instant;

use serde_json::{Map, Value, json};

/// Every request on its own line, written from stdin to stdout.
fn query(args: &[&str], input: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_dcsterrain"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the binary runs");
    let mut stdin = child.stdin.take().unwrap();
    let input = input.to_vec();
    let writer = std::thread::spawn(move || stdin.write_all(&input));
    let out = child.wait_with_output().unwrap();
    writer.join().unwrap().unwrap();
    out
}

fn lines(out: &Output) -> Vec<Value> {
    String::from_utf8(out.stdout.clone())
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).expect("each line is one JSON value"))
        .collect()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// The JSON lines of `requests`.
fn input(requests: &[(&str, Value)]) -> Vec<u8> {
    let mut text = String::new();
    for (i, (op, request)) in requests.iter().enumerate() {
        let line = json!({"id": i, "op": op, "request": request});
        text.push_str(&format!("{line}\n"));
    }
    text.into_bytes()
}

/// Where the requests of one batch look: a center on the ground, an
/// airdrome, and two ends of a road.
struct Around {
    x: f64,
    z: f64,
    airdrome: Value,
    from: Value,
    to: Value,
}

/// One request per operation, small enough for a test.
fn every_operation(at: &Around) -> Vec<(&'static str, Value)> {
    let (x, z) = (at.x, at.z);
    let observer = json!({"x": x, "z": z, "alt_agl": 10});
    let point = json!({"x": x, "z": z});
    let circle = json!({"Circle": {"x": x, "z": z, "radius": 1500}});
    let box_ = json!({"Box": {
        "min_x": x - 1000.0, "min_z": z - 1000.0, "max_x": x + 1000.0, "max_z": z + 1000.0,
    }});
    let wide = json!({"Circle": {"x": x, "z": z, "radius": 20000}});
    let east = json!({"x": x, "z": z + 3000.0, "alt_agl": 100});
    let north = json!({"x": x + 3000.0, "z": z, "alt_agl": 100});
    let footprint = json!({"width": 60, "depth": 40});
    let criteria = json!([
        {"name": "slope_max", "params": {"deg": 30}},
        {"name": "dist_road_band", "params": {"min": 0, "max": 50000}},
    ]);
    let placement =
        |id: &str, shape: Value| json!({"id": id, "x": x, "z": z, "heading": 0, "shape": shape});
    vec![
        ("describe", json!({})),
        (
            "sample",
            json!({"points": [point], "layers": ["height", "slope", "dist_road"]}),
        ),
        ("geo", json!({"to_latlon": [point]})),
        (
            "score_site",
            json!({"x": x, "z": z, "footprint": footprint, "criteria": criteria}),
        ),
        (
            "find_sites",
            json!({"region": circle, "footprint": footprint, "criteria": criteria,
                   "spacing": 200, "limit": 3, "geojson": true}),
        ),
        (
            "visible",
            json!({"a": {"x": x, "z": z, "alt_agl": 10}, "b": east, "geojson": true}),
        ),
        (
            "viewshed",
            json!({"observer": observer, "radius": 2000, "target_alt_agl": 2, "geojson": true}),
        ),
        (
            "radar_floor",
            json!({"site": observer, "radius": 2000, "geojson": true}),
        ),
        (
            "coverage",
            json!({"site": observer, "envelope": {"radius": 2000, "alt_bands_agl": [30, 100]},
                   "asset": circle, "geojson": true}),
        ),
        (
            "exposure",
            json!({"sites": [{"observer": observer, "radius": 2000}], "target_alt_agl": 30,
                   "geojson": true}),
        ),
        (
            "approach_spectrum",
            json!({"site": observer, "radius": 2000, "alt_bands_agl": [30], "geojson": true}),
        ),
        (
            "unmask_profile",
            json!({"site": observer, "route": [east, north], "geojson": true}),
        ),
        (
            "route",
            json!({"network": "roads", "from": at.from, "to": at.to, "geojson": true}),
        ),
        (
            "route_alternatives",
            json!({"network": "roads", "from": at.from, "to": at.to, "k": 2, "geojson": true}),
        ),
        (
            "nearest",
            json!({"kind": "road", "x": x, "z": z, "n": 2, "geojson": true}),
        ),
        (
            "scenery_in",
            json!({"region": box_, "max_rows": 1000000, "geojson": true}),
        ),
        (
            "chokepoints",
            json!({"region": wide, "network": "roads", "limit": 5, "geojson": true}),
        ),
        (
            "trafficability",
            json!({"x": x, "z": z, "max_slope_deg": 30, "network": "roads", "geojson": true}),
        ),
        ("point_info", json!({"points": [point], "geojson": true})),
        (
            "airfield_info",
            json!({"airdrome": at.airdrome, "geojson": true}),
        ),
        (
            "validate_placements",
            json!({"placements": [
                placement("point", json!("Point")),
                placement("rect", json!({"Rect": {"width": 60, "depth": 40}})),
                placement("circle", json!({"Circle": {"radius": 30}})),
            ], "geojson": true}),
        ),
        ("towns", json!({"limit": 3, "geojson": true})),
    ]
}

/// The synthetic terrain's own center, airdrome and road.
fn synth() -> Around {
    Around {
        x: 12000.0,
        z: 15000.0,
        airdrome: json!(1),
        from: json!({"x": 10000, "z": -12000}),
        to: json!({"x": 20000, "z": 10000}),
    }
}

/// The surface of an answer without what a platform's floating point or a
/// run may change: every float, the lengths of what geometry sized, and the
/// time taken. Keys, short strings, integers and booleans stay; a long array
/// keeps three items.
fn surface(v: &Value) -> Value {
    match v {
        Value::Number(n) if n.is_f64() => json!("<float>"),
        Value::String(s) if s.len() > 60 => json!("<long>"),
        Value::Array(a) if a.len() > 3 => {
            let mut kept: Vec<Value> = a[..3].iter().map(surface).collect();
            kept.push(json!("<more>"));
            Value::Array(kept)
        }
        Value::Array(a) => Value::Array(a.iter().map(surface).collect()),
        Value::Object(m) => {
            let mut out = Map::new();
            for (k, x) in m {
                let x = if k == "elapsed_ms" || k == "mgrs" {
                    json!("<varies>")
                } else {
                    surface(x)
                };
                out.insert(k.clone(), x);
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

fn golden(name: &str, printed: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/snapshots")
        .join(name);
    if std::env::var_os("DCSTERRAIN_UPDATE_SNAPSHOTS").is_some_and(|v| v == "1") {
        std::fs::write(&path, printed).unwrap();
        return;
    }
    let committed = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        printed == committed,
        "the output differs from {}; rerun with DCSTERRAIN_UPDATE_SNAPSHOTS=1 to accept it, \
         and review the diff",
        path.display()
    );
}

/// An answer with the time it took taken out, to compare two of them.
fn timeless(v: &Value) -> Value {
    let mut v = v.clone();
    v["elapsed_ms"] = json!(0);
    v
}

const SYNTH: [&str; 5] = ["--threads", "5", "query", "--terrain", "synth"];

#[test]
fn a_batch_answers_every_operation_with_the_terrain_opened_once() {
    let requests = every_operation(&synth());
    let verbose = [&["--verbose"], &SYNTH[..]].concat();
    let out = query(&verbose, &input(&requests));
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stderr(&out).matches("opened terrain").count(), 1);

    let answers = lines(&out);
    assert_eq!(answers.len(), requests.len());
    let mut printed = String::new();
    for (i, ((op, _), answer)) in requests.iter().zip(&answers).enumerate() {
        assert_eq!(answer["op"], *op);
        assert_eq!(answer["id"], i);
        assert_eq!(answer["line"], i + 1);
        assert!(answer["result"].is_object(), "{op}: {answer}");
        printed.push_str(&format!("{}\n", surface(answer)));
    }
    golden("batch-every-operation.jsonl", &printed);
}

#[test]
fn the_requests_here_cover_every_operation_the_schema_lists() {
    let out = query(&["schema"], b"");
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    let listed = doc["properties"]["operations"]["required"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n.as_str().unwrap().to_string());
    let mut listed: Vec<String> = listed.collect();
    let mut here: Vec<String> = every_operation(&synth())
        .iter()
        .map(|(op, _)| op.to_string())
        .collect();
    listed.sort_unstable();
    here.sort_unstable();
    assert_eq!(listed, here);
    assert!(!listed.iter().any(|o| o == "low_level_route"));
}

#[test]
fn a_batch_answers_as_one_process_per_request_does() {
    let requests = every_operation(&synth());
    let answers = lines(&query(&SYNTH, &input(&requests)));
    for ((op, request), answer) in requests.iter().zip(&answers) {
        let text = request.to_string();
        let args = [&SYNTH[..], &[op, &text]].concat();
        let alone = query(&args, b"");
        assert!(alone.status.success(), "{op}: {}", stderr(&alone));
        let alone: Value = serde_json::from_slice(&alone.stdout).unwrap();
        assert_eq!(timeless(&alone), timeless(&answer["result"]), "{op}");
    }
}

#[test]
fn a_hundred_requests_open_the_terrain_once() {
    let every = every_operation(&synth());
    let requests: Vec<_> = every.iter().cycle().take(100).cloned().collect();
    let verbose = [&["--verbose"], &SYNTH[..]].concat();
    let out = query(&verbose, &input(&requests));
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(lines(&out).len(), 100);
    assert_eq!(stderr(&out).matches("opened terrain").count(), 1);
    assert!(lines(&out).iter().all(|a| a["result"].is_object()));
}

#[test]
fn a_bad_line_gets_its_own_answer_and_the_batch_goes_on() {
    let mut text = Vec::new();
    text.extend_from_slice(b"\xEF\xBB\xBF{\"op\": \"describe\"}\r\n");
    text.extend_from_slice(b"\n   \n");
    text.extend_from_slice(b"not json\n");
    text.extend_from_slice(b"{\"id\": 1, \"op\": \"nope\"}\n");
    text.extend_from_slice(b"{\"id\": 2}\n");
    text.extend_from_slice(b"{\"id\": 3, \"op\": \"visible\", \"x\": 1}\n");
    text.extend_from_slice(b"{\"id\": 4, \"op\": 5}\n");
    text.extend_from_slice(b"[1, 2]\n");
    text.extend_from_slice(b"\"\xFF\"\n");
    let bad = r#"{"id": 5, "op": "sample", "request": {"points": [{"x": "north", "z": 0}], "layers": []}}"#;
    text.extend_from_slice(bad.as_bytes());
    text.push(b'\n');
    text.extend_from_slice(b"{\"id\": 6, \"op\": \"visible\", \"request\": {\"a\": {\"x\": 0, \"z\": 0}, \"b\": {\"x\": 1, \"z\": 1, \"alt\": 5}}}\n");
    text.extend_from_slice(
        b"{\"id\": 7, \"op\": \"sample\", \"request\": {\"points\": [], \"layers\": [\"nope\"]}}\n",
    );
    // The last line needs no newline.
    text.extend_from_slice(b"{\"id\": [\"last\"], \"op\": \"describe\"}");

    let out = query(&SYNTH, &text);
    assert!(out.status.success(), "the batch itself succeeded");
    let answers = lines(&out);
    assert_eq!(answers.len(), 12);
    assert!(answers[0]["result"].is_object());
    assert_eq!(answers[0]["line"], 1);
    assert_eq!(answers[1]["line"], 4, "blank lines are counted");
    assert_eq!(answers[11]["id"], json!(["last"]));
    assert!(answers[11]["result"].is_object());
    let printed: String = answers
        .iter()
        .map(|a| format!("{}\n", surface(a)))
        .collect();
    golden("batch-bad-lines.jsonl", &printed);
}

#[test]
fn a_refused_terrain_answers_every_line_and_exits_clean() {
    let text = b"{\"id\": 1, \"op\": \"describe\"}\n{\"id\": 2, \"op\": \"describe\"}\n";
    let missing = std::env::temp_dir().join("dcsterrain-batch-no-such-install");
    let args = [
        "query",
        "--install",
        missing.to_str().unwrap(),
        "--terrain",
        "Caucasus",
        "--cache",
        missing.to_str().unwrap(),
    ];
    let out = query(&args, text);
    assert!(out.status.success());
    let answers = lines(&out);
    assert_eq!(answers.len(), 2);
    for (i, a) in answers.iter().enumerate() {
        assert_eq!(a["id"], i + 1);
        assert_eq!(a["issues"][0]["code"], "terrain_refused");
    }
}

#[test]
fn an_answer_arrives_before_the_next_line_is_written() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_dcsterrain"))
        .args(SYNTH)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    for id in [1, 2] {
        writeln!(stdin, "{}", json!({"id": id, "op": "describe"})).unwrap();
        stdin.flush().unwrap();
        line.clear();
        stdout.read_line(&mut line).unwrap();
        let answer: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(answer["id"], id);
    }
    drop(stdin);
    assert!(child.wait().unwrap().success());
}

#[test]
fn one_request_is_a_bare_answer_and_a_failure_a_bare_issue_list() {
    let args = [&SYNTH[..], &["describe"]].concat();
    let out = query(&args, b"");
    assert!(out.status.success());
    let answer: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(answer["terrain"], "synth");

    let args = [&SYNTH[..], &["sample", "{\"layers\": 3}"]].concat();
    let out = query(&args, b"");
    assert_eq!(out.status.code(), Some(1));
    let refused: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(refused["issues"][0]["code"], "invalid_request");
    assert_eq!(refused["issues"][0]["path"], "layers");
}

/// A value against a JSON Schema, for the keywords `schema` prints.
fn fits(doc: &Value, schema: &Value, v: &Value, at: &str, wrong: &mut Vec<String>) {
    let Some(s) = schema.as_object() else {
        return;
    };
    if let Some(r) = s.get("$ref").and_then(Value::as_str) {
        let name = r.strip_prefix("#/$defs/").expect(r);
        return fits(doc, &doc["$defs"][name], v, at, wrong);
    }
    if let Some(t) = s.get("type") {
        let is = |t: &Value| match t.as_str().unwrap() {
            "object" => v.is_object(),
            "array" => v.is_array(),
            "string" => v.is_string(),
            "boolean" => v.is_boolean(),
            "null" => v.is_null(),
            "number" => v.is_number(),
            "integer" => v.is_i64() || v.is_u64(),
            other => panic!("the type {other}"),
        };
        let ok = match t {
            Value::Array(ts) => ts.iter().any(is),
            one => is(one),
        };
        if !ok {
            wrong.push(format!("{at}: {v} is not {t}"));
            return;
        }
    }
    if let Some(e) = s.get("enum").and_then(Value::as_array)
        && !e.contains(v)
    {
        wrong.push(format!("{at}: {v} is not one of {e:?}"));
    }
    if let Some(c) = s.get("const")
        && c != v
    {
        wrong.push(format!("{at}: {v} is not {c}"));
    }
    let branches = |key: &str| s.get(key).and_then(Value::as_array);
    for key in ["oneOf", "anyOf"] {
        if let Some(options) = branches(key) {
            let passing = options.iter().filter(|o| {
                let mut w = Vec::new();
                fits(doc, o, v, at, &mut w);
                w.is_empty()
            });
            let n = passing.count();
            if n == 0 || (key == "oneOf" && n > 1) {
                wrong.push(format!("{at}: {n} of the {key} fit {v}"));
            }
        }
    }
    for branch in branches("allOf").into_iter().flatten() {
        fits(doc, branch, v, at, wrong);
    }
    if let Some(m) = v.as_object() {
        for name in s
            .get("required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if !m.contains_key(name.as_str().unwrap()) {
                wrong.push(format!("{at}: {name} is missing"));
            }
        }
        let known = s.get("properties").and_then(Value::as_object);
        for (k, x) in m {
            match (known.and_then(|p| p.get(k)), s.get("additionalProperties")) {
                (Some(p), _) => fits(doc, p, x, &format!("{at}.{k}"), wrong),
                (None, Some(p)) => fits(doc, p, x, &format!("{at}.{k}"), wrong),
                (None, None) => {}
            }
        }
    }
    if let (Some(a), Some(items)) = (v.as_array(), s.get("items")) {
        for (i, x) in a.iter().enumerate() {
            fits(doc, items, x, &format!("{at}.{i}"), wrong);
        }
    }
}

/// Holds each answer, and the line around it, to `schema`.
fn assert_fits_schema(doc: &Value, op: &str, answer: &Value) {
    let mut wrong = Vec::new();
    let response = &doc["properties"]["operations"]["properties"][op]["properties"]["response"];
    assert!(response.is_object(), "{op} has a schema");
    fits(doc, response, &answer["result"], op, &mut wrong);
    let line = &doc["properties"]["batch"]["properties"]["response"];
    fits(doc, line, answer, "line", &mut wrong);
    assert!(wrong.is_empty(), "{wrong:#?}");
}

#[test]
fn every_answer_fits_the_schema_printed() {
    let out = query(&["schema"], b"");
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    let requests = every_operation(&synth());
    let answers = lines(&query(&SYNTH, &input(&requests)));
    for ((op, request), answer) in requests.iter().zip(&answers) {
        assert_fits_schema(&doc, op, answer);
        let mut wrong = Vec::new();
        let schema = &doc["properties"]["operations"]["properties"][op]["properties"]["request"];
        fits(&doc, schema, request, op, &mut wrong);
        assert!(wrong.is_empty(), "{wrong:#?}");
    }
    let line = json!({"id": 1, "op": "describe"});
    let schema = &doc["properties"]["batch"]["properties"]["request"];
    let mut wrong = Vec::new();
    fits(&doc, schema, &line, "batch", &mut wrong);
    assert!(wrong.is_empty(), "{wrong:#?}");
}

/// Where a terrain's batch looks: its first airdrome, the road beside it and
/// the next road along.
fn around_on(args: &[&str], reference: (f64, f64)) -> Around {
    let ask = |lines: &[(&str, Value)]| {
        let out = query(args, &input(lines));
        assert!(out.status.success(), "{}", stderr(&out));
        self::lines(&out)
    };
    let (x, z) = reference;
    let found = ask(&[("nearest", json!({"kind": "airdrome", "x": x, "z": z}))]);
    let airdrome = &found[0]["result"]["results"][0];
    let (x, z) = (
        airdrome["x"].as_f64().unwrap(),
        airdrome["z"].as_f64().unwrap(),
    );
    let roads = ask(&[("nearest", json!({"kind": "road", "x": x, "z": z, "n": 50}))]);
    let roads = roads[0]["result"]["results"].as_array().unwrap().clone();
    let end = |r: &Value| json!({"x": r["x"], "z": r["z"]});
    // The farthest of the near roads a route reaches: not every road is
    // joined to the first, on a terrain of islands.
    let from = end(&roads[0]);
    let probes: Vec<(&str, Value)> = (roads.iter().rev())
        .map(|r| {
            (
                "route",
                json!({"network": "roads", "from": from, "to": end(r)}),
            )
        })
        .collect();
    let reached = ask(&probes);
    let to = (reached.iter().zip(roads.iter().rev()))
        .find(|(answer, _)| answer["result"]["length"].as_f64().is_some_and(|l| l > 0.0))
        .map_or(from.clone(), |(_, r)| end(r));
    Around {
        x,
        z,
        airdrome: airdrome["id"].clone(),
        from,
        to,
    }
}

/// The middle of a terrain, as `describe` gives its extent.
fn middle(args: &[&str]) -> (f64, f64) {
    let out = query(args, b"{\"op\": \"describe\"}\n");
    let answer = &lines(&out)[0]["result"]["extent"];
    let of = |a: &str, b: &str| (answer[a].as_f64().unwrap() + answer[b].as_f64().unwrap()) / 2.0;
    (of("min_x", "max_x"), of("min_z", "max_z"))
}

#[test]
fn every_operation_answers_on_every_installed_terrain() {
    let Some(root) = std::env::var_os("DCSTERRAIN_INSTALL") else {
        eprintln!("DCSTERRAIN_INSTALL is not set; skipped");
        return;
    };
    let root = PathBuf::from(root);
    let root = root.to_str().unwrap();
    let cache = std::env::var("DCSTERRAIN_CACHE").ok();
    let listed = query(&["list-terrains", "--json", "--install", root], b"");
    let listed: Value = serde_json::from_slice(&listed.stdout).unwrap();
    let doc: Value = serde_json::from_slice(&query(&["schema"], b"").stdout).unwrap();
    let mut failed = Vec::new();
    for t in listed["terrains"].as_array().unwrap() {
        let id = t["id"].as_str().unwrap();
        let mut args = vec!["--verbose", "--threads", "5", "query", "--install", root];
        args.extend(["--terrain", id]);
        if let Some(c) = &cache {
            args.extend(["--cache", c]);
        }
        let at = around_on(&args, middle(&args));
        let requests = every_operation(&at);
        let started = Instant::now();
        let batch: Vec<_> = requests.iter().cycle().take(100).cloned().collect();
        let out = query(&args, &input(&batch));
        let took = started.elapsed().as_secs_f64();
        let opens = stderr(&out).matches("opened terrain").count();
        let answers = lines(&out);
        let mut issues = Vec::new();
        for ((op, _), answer) in batch.iter().zip(&answers).take(requests.len()) {
            if answer["result"].is_object() {
                assert_fits_schema(&doc, op, answer);
            } else {
                issues.push(format!("{op} {}", answer["issues"][0]));
            }
        }
        let build = answers[0]["result"]["dcs_build"].as_str().unwrap_or("?");
        eprintln!(
            "{id} on {build}: {} lines, {} ops answered, {opens} open(s), {took:.2} s, around \
             airdrome {}",
            answers.len(),
            requests.len() - issues.len(),
            at.airdrome,
        );
        for i in &issues {
            eprintln!("    {i}");
        }
        if answers.len() != 100 || opens != 1 || !issues.is_empty() {
            failed.push(id.to_string());
        }
    }
    assert!(failed.is_empty(), "{failed:?}");
}
