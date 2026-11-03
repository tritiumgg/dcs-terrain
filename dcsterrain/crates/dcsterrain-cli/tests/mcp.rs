//! `dcsterrain mcp` as an MCP client sees it, over the binary's own stdin
//! and stdout: one tool per operation, each tool's input the operation's
//! request schema, and each answer the bytes `query` prints for the same
//! request, a refusal included.

use std::cell::{Cell, RefCell};
use std::ffi::OsStr;
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
use common::install::{install, put, scratch};
use common::{bad_lines, every_operation, synth};

const SYNTH: [&str; 4] = ["--threads", "5", "--terrain", "synth"];

async fn connect() -> RunningService<RoleClient, ()> {
    connect_with(()).await
}

async fn connect_with<H: ClientHandler>(handler: H) -> RunningService<RoleClient, H> {
    serve(handler, &["--terrain".as_ref(), "synth".as_ref()]).await
}

/// A client of a server serving `terrain`, a list of the flags that name it.
async fn serve<H: ClientHandler>(handler: H, terrain: &[&OsStr]) -> RunningService<RoleClient, H> {
    let mut server = tokio::process::Command::new(env!("CARGO_BIN_EXE_dcsterrain"));
    server.args(["--threads", "5", "mcp"]).args(terrain);
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
        // An answer in a file may name its GeoJSON's file in turn.
        return restored(whole);
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

/// How long after its cancel a call of `op` stops, cancelled once it has
/// reported `share` of its work done.
async fn stopped_after(op: &str, request: Value, share: f64) -> Duration {
    let notices = Notices::default();
    let client = connect_with(notices.clone()).await;
    let request = ClientRequest::CallToolRequest(Request::new(call(op, request)));
    let handle = client
        .peer()
        .send_cancellable_request(request, PeerRequestOptions::no_options())
        .await
        .expect("the call is sent");
    loop {
        notices.1.notified().await;
        let last = notices.0.lock().unwrap().last().map(|n| n.progress);
        if last.is_some_and(|p| p >= share) {
            break;
        }
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
    handle.cancel(None).await.expect("the cancel is sent");
    let cancelled = Instant::now();
    // Calls are answered one at a time, so the next is answered only once
    // the cancelled one has stopped.
    let result = client.call_tool(call("describe", json!({}))).await.unwrap();
    let took = cancelled.elapsed();
    assert!(!answered(&result).1);
    client.cancel().await.unwrap();
    took
}

#[tokio::test]
async fn a_cancelled_call_stops_within_a_second() {
    // The search takes over 20 s here: cancelled as it reads the layers,
    // and as it traces sightlines.
    for share in [0.0, 0.5] {
        let took = stopped_after("find_sites", long_search(), share).await;
        assert!(
            took < Duration::from_secs(1),
            "find_sites at {share}: {took:?}"
        );
    }
    // Each of these sites takes a second or more to sweep: cancelled in the
    // second.
    let site = |x: f64| json!({"observer": {"x": x, "z": -10000, "alt_agl": 10}, "radius": 40000});
    let sweeps = json!({"sites": [site(5000.0), site(0.0), site(-5000.0)], "target_alt_agl": 30});
    let took = stopped_after("exposure", sweeps, 0.3).await;
    assert!(took < Duration::from_secs(1), "exposure: {took:?}");
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
        let (text, refused) = answered(&result);
        assert!(!refused, "{op}: {text}");
        // Removes any file the answer went to.
        restored(text);
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

#[tokio::test]
async fn a_terrain_whose_files_change_is_reopened_and_answers() {
    let t = install(false);
    let c = scratch();
    let terrain: [&OsStr; 6] = [
        "--terrain".as_ref(),
        "Caucasus".as_ref(),
        "--install".as_ref(),
        t.0.as_os_str(),
        "--cache".as_ref(),
        c.0.as_os_str(),
    ];
    let client = serve((), &terrain).await;
    let describe = async || parsed(&client.call_tool(call("describe", json!({}))).await.unwrap());
    assert_eq!(describe().await["dcs_build"], "0.0.0.1");

    let update = br#"{"version": "0.0.0.2", "timestamp": "20000102-000000"}"#;
    put(&t.0, "autoupdate.cfg", update);
    assert_eq!(describe().await["dcs_build"], "0.0.0.2");

    // A terrain gone refuses its call, and the next tries again.
    put(&t.0, "autoupdate.cfg", b"not json");
    let result = client.call_tool(call("describe", json!({}))).await.unwrap();
    let (text, refused) = answered(&result);
    assert!(refused && text.contains("terrain_refused"), "{text}");
    put(&t.0, "autoupdate.cfg", update);
    assert_eq!(describe().await["dcs_build"], "0.0.0.2");
    client.cancel().await.unwrap();
}

/// Opt-in: a session of 100 calls on Caucasus, the heaviest among them,
/// held to 64 MiB of peak commit for the server's whole life. Set
/// `DCSTERRAIN_INSTALL` to the DCS install root, and `DCSTERRAIN_CACHE` to a
/// cache built by `index` if not the default; without the install this
/// passes without reading anything, which is what CI does.
#[tokio::test]
async fn a_session_of_100_calls_on_caucasus_holds_its_memory() {
    let Some(root) = std::env::var_os("DCSTERRAIN_INSTALL") else {
        eprintln!("DCSTERRAIN_INSTALL is not set; skipped");
        return;
    };
    let mut server = Command::new(env!("CARGO_BIN_EXE_dcsterrain"));
    server.args([
        "--threads",
        "5",
        "mcp",
        "--terrain",
        "Caucasus",
        "--install",
    ]);
    server.arg(&root);
    if let Some(cache) = std::env::var_os("DCSTERRAIN_CACHE") {
        server.arg("--cache").arg(cache);
    }
    let mut child = server
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("the server starts");
    let stdin = tokio::process::ChildStdin::from_std(child.stdin.take().unwrap()).unwrap();
    let stdout = tokio::process::ChildStdout::from_std(child.stdout.take().unwrap()).unwrap();
    let client = ().serve((stdout, stdin)).await.expect("the server initializes");

    let calls = Cell::new(0);
    let issues = RefCell::new(Vec::new());
    let ask = async |op: &str, request: Value| {
        calls.set(calls.get() + 1);
        let result = client.call_tool(call(op, request)).await.unwrap();
        let (text, refused) = answered(&result);
        let text = restored(text);
        if refused {
            issues.borrow_mut().push(format!("{op}: {text}"));
            return Value::Null;
        }
        serde_json::from_str::<Value>(&text).expect("an answer is JSON")
    };

    // Where to ask: an airdrome near the middle, and a road from beside it
    // to the farthest of its near roads a route reaches.
    let described = ask("describe", json!({})).await;
    let build = described["dcs_build"].as_str().unwrap_or("?").to_string();
    let e = &described["extent"];
    let mid = |a: &str, b: &str| (e[a].as_f64().unwrap() + e[b].as_f64().unwrap()) / 2.0;
    let (x, z) = (mid("min_x", "max_x"), mid("min_z", "max_z"));
    let found = ask("nearest", json!({"kind": "airdrome", "x": x, "z": z})).await;
    let airdrome = found["results"][0].clone();
    let (x, z) = (
        airdrome["x"].as_f64().unwrap(),
        airdrome["z"].as_f64().unwrap(),
    );
    let roads = ask("nearest", json!({"kind": "road", "x": x, "z": z, "n": 20})).await;
    let roads = roads["results"].as_array().unwrap().clone();
    let end = |r: &Value| json!({"x": r["x"], "z": r["z"]});
    let from = end(&roads[0]);
    let mut to = from.clone();
    for r in roads.iter().skip(1).rev() {
        let route = json!({"network": "roads", "from": from, "to": end(r)});
        if ask("route", route).await["length"]
            .as_f64()
            .is_some_and(|l| l > 0.0)
        {
            to = end(r);
            break;
        }
    }
    issues.borrow_mut().clear();
    let at = common::Around {
        x,
        z,
        airdrome: airdrome["id"].clone(),
        from,
        to,
    };

    // Every operation, and the heaviest calls: a sweep whose rasters go to a
    // file, and a search whose outlines do.
    let observer = json!({"x": x, "z": z, "alt_agl": 10});
    let mut session = every_operation(&at);
    session.push((
        "exposure",
        json!({"sites": [{"observer": observer, "radius": 20000}], "target_alt_agl": 30,
               "geojson": true}),
    ));
    session.push((
        "find_sites",
        json!({"region": {"Circle": {"x": x, "z": z, "radius": 8000}},
               "footprint": {"width": 60, "depth": 40},
               "criteria": [{"name": "slope_max", "params": {"deg": 15}},
                            {"name": "not_visible_from",
                             "params": {"observers": [observer], "max_fraction": 0.5}}],
               "spacing": 0, "limit": 200, "geojson": true}),
    ));
    let (started, before) = (Instant::now(), calls.get());
    for (op, request) in session.iter().cycle() {
        if calls.get() == 100 {
            break;
        }
        ask(op, request.clone()).await;
    }
    let took = started.elapsed().as_secs_f64();
    client.cancel().await.unwrap();
    let (_, cost) = dcsterrain_core::threads::wait_costed(child).unwrap();
    let peak = cost.peak_bytes as f64 / f64::from(1 << 20);
    let issues = issues.into_inner();
    eprintln!(
        "Caucasus on {build}: {} calls in one session, the last {} in {took:.1} s; peak commit \
         {peak:.1} MiB",
        calls.get(),
        calls.get() - before,
    );
    for i in &issues {
        eprintln!("    {i}");
    }
    assert!(issues.is_empty(), "{issues:?}");
    if cfg!(windows) {
        assert!(peak <= 64.0, "peak commit {peak:.1} MiB");
    }
}

/// The skill's FARP request, as its worked tasks print it.
fn farp_request() -> Value {
    let tasks =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../skill/references/tasks.md");
    let tasks = std::fs::read_to_string(tasks).expect("the skill's tasks read");
    let line = tasks
        .lines()
        .find(|l| l.starts_with(r#"{"id": "farp""#))
        .expect("the tasks hold the FARP request");
    serde_json::from_str::<Value>(line).unwrap()["request"].clone()
}

/// Opt-in: the FARP task as an assistant with only the tools works it on
/// Caucasus, each answer the one `query` gives: the search, then for each
/// candidate the scenery within 500 m and 150 m and one `score_site` whose
/// 36 ring observers count as the 36 `visible` calls do. Set
/// `DCSTERRAIN_INSTALL` as for the session above.
#[tokio::test]
async fn the_farp_task_through_the_tools_answers_as_query_does_on_caucasus() {
    let Some(root) = std::env::var_os("DCSTERRAIN_INSTALL") else {
        eprintln!("DCSTERRAIN_INSTALL is not set; skipped");
        return;
    };
    let mut terrain: Vec<&OsStr> = vec!["--terrain".as_ref(), "Caucasus".as_ref()];
    terrain.extend(["--install".as_ref(), root.as_os_str()]);
    let cache = std::env::var_os("DCSTERRAIN_CACHE");
    if let Some(cache) = &cache {
        terrain.extend(["--cache".as_ref(), cache.as_os_str()]);
    }
    let client = serve((), &terrain).await;
    let ask = async |op: &str, request: &Value| {
        let result = client.call_tool(call(op, request.clone())).await.unwrap();
        let (text, refused) = answered(&result);
        assert!(!refused, "{op}: {text}");
        let printed = Command::new(env!("CARGO_BIN_EXE_dcsterrain"))
            .args(["--threads", "5", "query"])
            .args(&terrain)
            .args([op, &request.to_string()])
            .stderr(Stdio::null())
            .output()
            .expect("the binary runs");
        let printed = String::from_utf8(printed.stdout).expect("stdout is UTF-8");
        assert_eq!(timeless(&text), timeless(printed.trim_end()), "{op}");
        serde_json::from_str::<Value>(&text).expect("an answer is JSON")
    };

    let found = ask("find_sites", &farp_request()).await;
    let candidates = found["candidates"].as_array().unwrap();
    assert_eq!(candidates.len(), 8, "{found}");
    for c in candidates {
        let (x, z) = (c["x"].as_f64().unwrap(), c["z"].as_f64().unwrap());
        for radius in [500, 150] {
            let around = json!({"region": {"Circle": {"x": x, "z": z, "radius": radius}}});
            ask("scenery_in", &around).await;
        }
        let mut ring = Vec::new();
        for r in [3000.0, 6000.0, 10000.0] {
            for b in (0..360).step_by(30) {
                let b = f64::from(b).to_radians();
                let at = |v: f64| (v * 1000.0).round() / 1000.0;
                ring.push(
                    json!({"x": at(x + r * b.cos()), "z": at(z + r * b.sin()), "alt_agl": 2}),
                );
            }
        }
        let scored = ask(
            "score_site",
            &json!({"x": x, "z": z,
                    "footprint": {"width": 120, "depth": 80, "orientations_deg": [0]},
                    "criteria": [{"name": "not_visible_from",
                                  "params": {"observers": ring, "max_fraction": 1}}]}),
        )
        .await;
        let mut seen = 0;
        for o in &ring {
            let line = json!({"a": o, "b": {"x": x, "z": z, "alt_agl": 2}});
            if ask("visible", &line).await["visible"] == json!(true) {
                seen += 1;
            }
        }
        let share = scored["terms"][1]["value"].as_f64().unwrap();
        assert_eq!((share * 36.0).round() as i32, seen, "at {x}, {z}");
    }
    client.cancel().await.unwrap();
}
