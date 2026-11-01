//! The skill an assistant loads, held to the command line: an example that
//! stopped parsing, or an operation it does not teach, would otherwise go
//! unnoticed until an assistant fails on it.
//!
//! The examples name Caucasus positions, so on the synthetic terrain many
//! answer that the point is off the terrain or the airdrome unknown. Those
//! prove the request parsed, which is what is asked of them here.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use dcsterrain_core::types::OPERATIONS;
use serde_json::{Value, json};

/// The codes a request gets for its shape rather than for where it points.
const STALE: [&str; 5] = [
    "invalid_request",
    "invalid_criterion",
    "unknown_class",
    "unknown_layer",
    "unknown_operation",
];

fn skill_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../skill")
}

fn skill_text() -> Vec<String> {
    let dir = skill_dir();
    ["SKILL.md", "references/tasks.md", "references/metrics.md"]
        .iter()
        .map(|f| std::fs::read_to_string(dir.join(f)).expect("the skill's file reads"))
        .collect()
}

/// The schemas `schema` stands for: itself, what it refers to, and each of
/// its alternatives.
fn branches<'a>(schema: &'a Value, defs: &'a Value, out: &mut Vec<&'a Value>) {
    out.push(schema);
    if let Some(name) = schema["$ref"].as_str() {
        let name = name.trim_start_matches("#/$defs/");
        branches(&defs[name], defs, out);
    }
    for key in ["anyOf", "oneOf", "allOf"] {
        for s in schema[key].as_array().into_iter().flatten() {
            branches(s, defs, out);
        }
    }
}

/// Each field of `value` that none of `schemas` names. A request ignores a
/// field it does not know, so a renamed optional field would still parse.
fn unknown_fields(
    value: &Value,
    schemas: &[&Value],
    defs: &Value,
    path: &str,
    out: &mut Vec<String>,
) {
    let mut all = Vec::new();
    for s in schemas {
        branches(s, defs, &mut all);
    }
    match value {
        Value::Array(items) => {
            let inner: Vec<&Value> = all
                .iter()
                .map(|s| &s["items"])
                .filter(|s| !s.is_null())
                .collect();
            for item in items {
                unknown_fields(item, &inner, defs, path, out);
            }
        }
        Value::Object(fields) => {
            // A map, such as `weights`, names no properties.
            if !all.iter().any(|s| s["properties"].is_object()) {
                return;
            }
            for (key, field) in fields {
                let inner: Vec<&Value> = all
                    .iter()
                    .map(|s| &s["properties"][key])
                    .filter(|s| !s.is_null())
                    .collect();
                let here = format!("{path}.{key}");
                if inner.is_empty() {
                    out.push(here);
                } else {
                    unknown_fields(field, &inner, defs, &here, out);
                }
            }
        }
        _ => {}
    }
}

/// `find_sites` as the name of its request type, `FindSitesRequest`.
fn request_type(op: &str) -> String {
    let mut name = String::new();
    for word in op.split('_') {
        let mut chars = word.chars();
        name.extend(chars.next().map(|c| c.to_ascii_uppercase()));
        name.push_str(chars.as_str());
    }
    name + "Request"
}

fn all_examples() -> Vec<String> {
    skill_text().iter().flat_map(|t| examples(t)).collect()
}

#[test]
fn every_example_field_is_one_the_schema_names() {
    let out = Command::new(env!("CARGO_BIN_EXE_dcsterrain"))
        .arg("schema")
        .output()
        .expect("the binary runs");
    let schema: Value = serde_json::from_slice(&out.stdout).unwrap();
    let defs = &schema["$defs"];

    // The walk itself: a misspelled field in a real request is found.
    let mut found = Vec::new();
    let misspelled = json!({"kind": "town", "x": 0, "z": 0, "lmit": 1});
    unknown_fields(
        &misspelled,
        &[&defs["NearestRequest"]],
        defs,
        "request",
        &mut found,
    );
    assert_eq!(found, ["request.lmit"]);

    for line in all_examples() {
        let v: Value = serde_json::from_str(&line).unwrap();
        let name = request_type(v["op"].as_str().unwrap());
        let request = &defs[name.as_str()];
        assert!(request.is_object(), "no type {name}");
        let mut found = Vec::new();
        unknown_fields(&v["request"], &[request], defs, "request", &mut found);
        assert!(found.is_empty(), "{found:?} in {line}");
    }
}

/// Every line of the fenced `jsonl` blocks.
fn examples(text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        if line.starts_with("```") {
            inside = line == "```jsonl";
        } else if inside {
            lines.push(line.to_string());
        }
    }
    lines
}

#[test]
fn every_example_is_a_request_the_command_line_takes() {
    let lines = all_examples();
    assert!(lines.len() >= 20, "only {} examples found", lines.len());

    let mut child = Command::new(env!("CARGO_BIN_EXE_dcsterrain"))
        .args(["query", "--terrain", "synth"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the binary runs");
    let mut stdin = child.stdin.take().unwrap();
    let input = lines.join("\n");
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()));
    let out = child.wait_with_output().unwrap();
    writer.join().unwrap().unwrap();

    let answers: Vec<Value> = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(answers.len(), lines.len());
    for (line, answer) in lines.iter().zip(&answers) {
        for issue in answer["issues"].as_array().into_iter().flatten() {
            let code = issue["code"].as_str().unwrap();
            assert!(!STALE.contains(&code), "{line}\n{issue}");
        }
    }
}

#[test]
fn the_skill_names_every_operation() {
    let text = skill_text().join("\n");
    for op in OPERATIONS {
        let name = format!("`{}`", op.name);
        assert!(text.contains(&name), "the skill never names {name}");
    }
}
