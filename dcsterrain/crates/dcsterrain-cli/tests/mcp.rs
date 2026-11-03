//! `dcsterrain mcp` as an MCP client sees it, over the binary's own stdin
//! and stdout: one tool per operation, each tool's input the operation's
//! request schema, and each answer the bytes `query` prints for the same
//! request, a refusal included.

use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rmcp::model::{
    CallToolRequestParams, CallToolResult, ClientRequest, ProgressNotificationParam, Request, Tool,
};
use rmcp::service::{NotificationContext, PeerRequestOptions, RoleClient, RunningService};
use rmcp::transport::TokioChildProcess;
use rmcp::{ClientHandler, ServiceExt};
use serde_json::{Map, Value, json};
use tokio::sync::Notify;

mod common;
use common::{bad_lines, every_operation, synth};

const SYNTH: [&str; 4] = ["--threads", "5", "--terrain", "synth"];

async fn connect() -> RunningService<RoleClient, ()> {
    connect_with(()).await
}

async fn connect_with<H: ClientHandler>(handler: H) -> RunningService<RoleClient, H> {
    let mut server = tokio::process::Command::new(env!("CARGO_BIN_EXE_dcsterrain"));
    server.args(["--threads", "5", "mcp", "--terrain", "synth"]);
    let (transport, _) = TokioChildProcess::builder(server)
        .stderr(Stdio::null())
        .spawn()
        .expect("the server starts");
    handler
        .serve(transport)
        .await
        .expect("the server initializes")
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

/// An answer with what went to files put back: the whole answer when it
/// went to one, else the GeoJSON when it did. Each file is then removed.
fn restored(text: String) -> String {
    let Ok(Value::Object(mut answer)) = serde_json::from_str::<Value>(&text) else {
        return text;
    };
    let mut take = |field: &str| {
        let path = answer.remove(field)?;
        let path = path.as_str().expect("a path").to_string();
        let text = std::fs::read_to_string(&path).expect("the file is there");
        std::fs::remove_file(&path).unwrap();
        Some(text)
    };
    if let Some(whole) = take("answer_file") {
        return whole;
    }
    match take("geojson_file") {
        Some(geojson) => {
            answer.remove("geojson_bytes");
            let geojson = serde_json::from_str(&geojson).expect("the file is JSON");
            answer.insert("geojson".into(), geojson);
            Value::Object(answer).to_string()
        }
        None => text,
    }
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
        let tool_text = restored(tool_text);
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

/// A client that keeps every progress notice the server sends.
#[derive(Clone, Default)]
struct Notices(Arc<Mutex<Vec<ProgressNotificationParam>>>, Arc<Notify>);

impl ClientHandler for Notices {
    async fn on_progress(
        &self,
        params: ProgressNotificationParam,
        _context: NotificationContext<RoleClient>,
    ) {
        self.0.lock().unwrap().push(params);
        self.1.notify_one();
    }
}

fn call(op: &str, request: Value) -> CallToolRequestParams {
    let arguments = request.as_object().cloned().unwrap_or_default();
    CallToolRequestParams::new(op.to_string()).with_arguments(arguments)
}

/// A search over sightlines from 15 km about the center: seconds of work.
fn long_search() -> Value {
    json!({
        "region": {"Circle": {"x": 12000, "z": 15000, "radius": 15000}},
        "footprint": {"width": 60, "depth": 40},
        "criteria": [
            {"name": "slope_max", "params": {"deg": 30}},
            {"name": "not_visible_from", "params": {"observers": [
                {"x": 12000, "z": 15000, "alt_agl": 10},
                {"x": 2000, "z": 5000, "alt_agl": 10},
            ], "max_fraction": 0.5}},
        ],
        "spacing": 200, "limit": 3,
    })
}

#[tokio::test]
async fn a_cancelled_search_stops_within_a_second() {
    let notices = Notices::default();
    let client = connect_with(notices.clone()).await;
    let request = ClientRequest::CallToolRequest(Request::new(call("find_sites", long_search())));
    let handle = client
        .peer()
        .send_cancellable_request(request, PeerRequestOptions::no_options())
        .await
        .expect("the call is sent");
    // Once the search reports, it is working: the rest is seconds more.
    notices.1.notified().await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    handle.cancel(None).await.expect("the cancel is sent");
    let cancelled = Instant::now();
    // Calls are answered one at a time, so the next is answered only once
    // the search has stopped.
    let result = client.call_tool(call("describe", json!({}))).await.unwrap();
    let took = cancelled.elapsed();
    assert!(!answered(&result).1);
    assert!(
        took < Duration::from_secs(1),
        "the search stopped {took:?} after its cancel"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn find_sites_and_exposure_report_progress_to_the_callers_token() {
    let notices = Notices::default();
    let client = connect_with(notices.clone()).await;
    let at = synth();
    let observer = json!({"x": at.x, "z": at.z, "alt_agl": 10});
    let calls = [
        (
            "find_sites",
            json!({"region": {"Circle": {"x": at.x, "z": at.z, "radius": 3000}},
                   "footprint": {"width": 60, "depth": 40},
                   "criteria": [{"name": "not_visible_from",
                                 "params": {"observers": [observer], "max_fraction": 0.5}}],
                   "spacing": 200, "limit": 3}),
        ),
        (
            "exposure",
            json!({"sites": [{"observer": observer, "radius": 2000}], "target_alt_agl": 30}),
        ),
    ];
    for (op, request) in calls {
        notices.0.lock().unwrap().clear();
        let result = client.call_tool(call(op, request)).await.unwrap();
        assert!(!answered(&result).1, "{op}: {result:?}");
        let seen = notices.0.lock().unwrap().clone();
        assert!(!seen.is_empty(), "{op} reported nothing");
        let token = &seen[0].progress_token;
        assert!(seen.iter().all(|n| &n.progress_token == token));
        assert!(seen.windows(2).all(|w| w[0].progress < w[1].progress));
        assert!(
            seen.iter()
                .all(|n| n.total == Some(1.0) && n.progress <= 1.0)
        );
    }
    client.cancel().await.unwrap();
}

/// The JSON a tool answered with.
fn parsed(result: &CallToolResult) -> Value {
    let (text, refused) = answered(result);
    assert!(!refused, "{text}");
    serde_json::from_str(&text).unwrap()
}

/// The JSON of a file an answer named, which the test then removes.
fn file(answer: &Value, field: &str) -> Value {
    let path = answer[field].as_str().expect("a path");
    let text = std::fs::read_to_string(path).expect("the file is there");
    std::fs::remove_file(path).unwrap();
    serde_json::from_str(&text).expect("the file is JSON")
}

#[tokio::test]
async fn a_large_geojson_and_a_large_answer_come_back_as_files() {
    let client = connect().await;
    let at = synth();
    let observer = json!({"x": at.x, "z": at.z, "alt_agl": 10});

    // Two hundred sites and their outlines are over the limit; the sites
    // alone are not.
    let request = json!({
        "region": {"Circle": {"x": at.x, "z": at.z, "radius": 8000}},
        "footprint": {"width": 60, "depth": 40},
        "criteria": [{"name": "slope_max", "params": {"deg": 30}}],
        "spacing": 0, "limit": 200, "geojson": true,
    });
    let mut answer = parsed(
        &client
            .call_tool(call("find_sites", request.clone()))
            .await
            .unwrap(),
    );
    let geojson = file(&answer, "geojson_file");
    assert_eq!(geojson["type"], "FeatureCollection");
    assert!(answer["geojson_bytes"].as_u64().unwrap() > 64 << 10);
    let (text, _) = query("find_sites", &request);
    let mut queried: Value = serde_json::from_str(&timeless(&text)).unwrap();
    assert_eq!(geojson, queried["geojson"]);
    let top = answer.as_object_mut().unwrap();
    top.remove("geojson_file");
    top.remove("geojson_bytes");
    queried.as_object_mut().unwrap().remove("geojson");
    answer["elapsed_ms"] = json!(0);
    assert_eq!(answer, queried);

    // Exposure's rasters 6 km about are over it with no GeoJSON at all.
    let request = json!({"sites": [{"observer": observer, "radius": 6000}], "target_alt_agl": 30});
    let answer = parsed(
        &client
            .call_tool(call("exposure", request.clone()))
            .await
            .unwrap(),
    );
    assert!(answer["answer_bytes"].as_u64().unwrap() > 64 << 10);
    let whole = file(&answer, "answer_file");
    let (text, _) = query("exposure", &request);
    let queried: Value = serde_json::from_str(&timeless(&text)).unwrap();
    let whole: Value = serde_json::from_str(&timeless(&whole.to_string())).unwrap();
    assert_eq!(whole, queried);
    client.cancel().await.unwrap();
}
