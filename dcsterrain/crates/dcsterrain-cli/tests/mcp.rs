//! `dcsterrain mcp` as an MCP client sees it, over the binary's own stdin
//! and stdout: one tool per operation, each tool's input the operation's
//! request schema, and each answer the bytes `query` prints for the same
//! request, a refusal included.

use std::process::{Command, Stdio};

use rmcp::ServiceExt;
use rmcp::model::{CallToolRequestParams, CallToolResult, Tool};
use rmcp::service::{RoleClient, RunningService};
use rmcp::transport::TokioChildProcess;
use serde_json::{Map, Value};

mod common;
use common::{bad_lines, every_operation, synth};

const SYNTH: [&str; 4] = ["--threads", "5", "--terrain", "synth"];

async fn connect() -> RunningService<RoleClient, ()> {
    let mut server = tokio::process::Command::new(env!("CARGO_BIN_EXE_dcsterrain"));
    server.args(["--threads", "5", "mcp", "--terrain", "synth"]);
    let (transport, _) = TokioChildProcess::builder(server)
        .stderr(Stdio::null())
        .spawn()
        .expect("the server starts");
    ().serve(transport).await.expect("the server initializes")
}

/// What `query` prints for one request, without its newline, and whether it
/// refused.
fn query(op: &str, request: &Value) -> (String, bool) {
    let text = request.to_string();
    let (threads, terrain) = SYNTH.split_at(2);
    let out = Command::new(env!("CARGO_BIN_EXE_dcsterrain"))
        .args(threads)
        .arg("query")
        .args(terrain)
        .args([op, &text])
        .stderr(Stdio::null())
        .output()
        .expect("the binary runs");
    let printed = String::from_utf8(out.stdout).expect("stdout is UTF-8");
    let printed = printed.strip_suffix('\n').expect("one line").to_string();
    (printed, !out.status.success())
}

/// The one text a tool answered with, and whether it is an error.
fn answered(result: &CallToolResult) -> (String, bool) {
    assert_eq!(result.content.len(), 1, "{result:?}");
    let text = result.content[0].as_text().expect("a text answer");
    (text.text.clone(), result.is_error == Some(true))
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

/// Every `$ref` name in `v`.
fn refs(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(m) => {
            if let Some(Value::String(r)) = m.get("$ref") {
                out.push(r.trim_start_matches("#/$defs/").to_string());
            }
            m.values().for_each(|x| refs(x, out));
        }
        Value::Array(a) => a.iter().for_each(|x| refs(x, out)),
        _ => {}
    }
}

/// Holds a tool's input schema to the request `schema --op` prints: its root
/// is the request's definition, and its `$defs` are exactly the definitions
/// the request reaches, each as printed.
fn assert_same_request(tool: &Tool, printed: &Value) {
    let name = tool.name.as_ref();
    let defs = &printed["$defs"];
    let request = &printed["properties"]["operations"]["properties"][name]["properties"]["request"];
    let request_name = request["$ref"].as_str().expect("a request is a definition");
    let request_def = &defs[request_name.trim_start_matches("#/$defs/")];

    let mut input = Value::Object(tool.input_schema.as_ref().clone());
    let given_defs = input
        .as_object_mut()
        .unwrap()
        .remove("$defs")
        .unwrap_or_else(|| Value::Object(Map::new()));
    let dialect = input.as_object_mut().unwrap().remove("$schema");
    assert_eq!(dialect.as_ref(), Some(&printed["$schema"]), "{name}");
    assert_eq!(&input, request_def, "{name}");

    let mut reached = Vec::new();
    let mut pending = Vec::new();
    refs(request_def, &mut pending);
    while let Some(n) = pending.pop() {
        if !reached.contains(&n) {
            refs(&defs[&n], &mut pending);
            reached.push(n);
        }
    }
    let given = given_defs.as_object().unwrap();
    let mut given_names: Vec<&String> = given.keys().collect();
    let mut reached_names: Vec<&String> = reached.iter().collect();
    given_names.sort_unstable();
    reached_names.sort_unstable();
    assert_eq!(given_names, reached_names, "{name}");
    for n in &reached {
        assert_eq!(&given[n], &defs[n], "{name}: {n}");
    }
}

#[tokio::test]
async fn there_is_one_tool_per_operation_and_its_input_is_the_request_schema() {
    let client = connect().await;
    let tools = client.list_all_tools().await.expect("the tools are listed");
    client.cancel().await.unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_dcsterrain"))
        .arg("schema")
        .output()
        .unwrap();
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    let mut ops: Vec<&str> = doc["properties"]["operations"]["required"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n.as_str().unwrap())
        .collect();
    let mut names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
    ops.sort_unstable();
    names.sort_unstable();
    assert_eq!(names, ops);

    for tool in &tools {
        let out = Command::new(env!("CARGO_BIN_EXE_dcsterrain"))
            .args(["schema", "--op", tool.name.as_ref()])
            .output()
            .unwrap();
        let printed: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_same_request(tool, &printed);
    }
}

/// The requests of the batch whose lines are mostly bad that a tool call can
/// carry: a line naming an operation with an object for its request, or
/// none. The rest are faults of a batch's framing, with no call to match.
fn bad_requests() -> Vec<(String, Value)> {
    let text = bad_lines();
    let text = text.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&text);
    text.split(|b| *b == b'\n')
        .filter_map(|line| serde_json::from_slice::<Value>(line).ok())
        .filter_map(|line| {
            let line = line.as_object()?;
            let op = line.get("op")?.as_str()?.to_string();
            let request = line.get("request").cloned().unwrap_or_default();
            let known = line
                .keys()
                .all(|k| ["id", "op", "request"].contains(&k.as_str()));
            let request = match request {
                Value::Null => Value::Object(Map::new()),
                r @ Value::Object(_) => r,
                _ => return None,
            };
            known.then_some((op, request))
        })
        .collect()
}

#[tokio::test]
async fn every_tool_answers_as_query_does() {
    let mut requests: Vec<(String, Value)> = every_operation(&synth())
        .into_iter()
        .map(|(op, request)| (op.to_string(), request))
        .collect();
    let bad = bad_requests();
    assert!(bad.len() >= 6, "{bad:?}");
    requests.extend(bad);

    let client = connect().await;
    let mut refused = 0;
    for (op, request) in &requests {
        let arguments = request.as_object().cloned().unwrap_or_default();
        let call = CallToolRequestParams::new(op.clone()).with_arguments(arguments);
        let result = client.call_tool(call).await.expect("the call is answered");
        let (tool_text, tool_refused) = answered(&result);
        let (query_text, query_refused) = query(op, request);
        assert_eq!(tool_refused, query_refused, "{op} {request}");
        assert!(
            timeless(&tool_text) == timeless(&query_text),
            "{op} {request}: the tool answered\n{tool_text}\nand query\n{query_text}"
        );
        refused += usize::from(tool_refused);
    }
    client.cancel().await.unwrap();
    assert!(refused >= 4, "the refusals are compared too");
}

#[test]
fn only_the_time_taken_is_set_aside() {
    let text = r#"{"elapsed_ms":123,"x":{"elapsed_ms":7},"elapsed_ms_not":5}"#;
    assert_eq!(
        timeless(text),
        r#"{"elapsed_ms":0,"x":{"elapsed_ms":0},"elapsed_ms_not":5}"#
    );
}
