//! `dcsterrain mcp`: the operations as tools of a Model Context Protocol
//! server on stdin and stdout. A tool's input is its operation's request and
//! the terrain the call is about, and its answer the one `query` prints for
//! that request about that terrain, byte for byte, unless it is too large to
//! hand back as text; a refusal is a tool error whose text is the issue list
//! `query` prints. Nothing here knows a terrain: each call is handed to the
//! opener `query` answers with, which holds one terrain open at a time.
//!
//! The server lives as long as its client. A call the client cancels stops
//! its work, as the protocol's `notifications/cancelled` asks, and a long
//! one reports how far it has come to the progress token its caller gave.

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, mpsc};
use std::time::{Duration, SystemTime};

use dcsterrain_core::call::{self, Ticket};
use dcsterrain_core::query::Answer;
use dcsterrain_core::types::{Issue, OPERATIONS, request_schema};
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    ListToolsResult, PaginatedRequestParams, ProgressNotificationParam, ServerCapabilities,
    ServerConfig, Tool,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt};
use serde::Serialize;
use serde_json::{Value, json};
use tokio::sync::{mpsc as queue, oneshot};

use crate::query::Opener;

/// The most text a tool answers with. A larger GeoJSON goes to a file, and
/// then a larger answer: clients cut a tool's text near 25 000 tokens, about
/// this many bytes of JSON, and an answer held whole as text would cost the
/// server its memory limit at `exposure`'s largest.
pub const INLINE_MAX: usize = 64 << 10;

/// How long a file of a large answer is kept: the client and its user have
/// it for a day, and a server starting after that removes it.
const KEPT: Duration = Duration::from_secs(24 * 60 * 60);

/// One call for the thread that holds the terrain.
struct Call {
    op: String,
    /// The terrain the call names, or a refusal of what it gave as one.
    terrain: Result<Option<String>, String>,
    request: Value,
    ticket: Arc<Ticket>,
    reply: oneshot::Sender<Result<String, String>>,
}

/// The server: the tools, listed once, and the way to the thread that
/// answers them.
struct Server {
    tools: Arc<Vec<Tool>>,
    calls: mpsc::Sender<Call>,
}

/// The tool that lists the install's terrains, as `list-terrains --json`.
pub const LIST_TERRAINS: &str = "list_terrains";

/// The input every operation's tool takes beside its request: the terrain
/// the call is about.
pub const TERRAIN: &str = "terrain";

/// One tool per operation, its input the operation's request schema with
/// `terrain` beside its fields, and the tool that lists the terrains.
fn tools() -> Vec<Tool> {
    let mut tools: Vec<Tool> = OPERATIONS
        .iter()
        .map(|o| {
            let mut schema = request_schema(o.name).expect("a listed operation has a schema");
            let description = schema
                .get("description")
                .and_then(Value::as_str)
                .map(|d| d.to_string().into());
            let terrain = json!({
                "type": "string",
                "description": "The terrain to answer about: its id, as `list_terrains` \
                    gives it, such as `Caucasus`, or `synth` for the synthetic terrain. \
                    Omitted, the terrain the server was started with.",
            });
            let fields = schema.entry("properties").or_insert_with(|| json!({}));
            let fields = fields
                .as_object_mut()
                .expect("a request's fields are an object");
            assert!(
                fields.insert(TERRAIN.into(), terrain).is_none(),
                "{} has its own `terrain`",
                o.name
            );
            Tool::new_with_raw(o.name, description, schema)
        })
        .collect();
    let input = json!({"type": "object", "properties": {}, "additionalProperties": false});
    let description = "Every terrain of the DCS install: its id, which the other tools take \
        as `terrain`, whether it can be read, and what the cache holds for it; and the DCS \
        build. As `dcsterrain list-terrains --json` prints it, without each terrain's \
        `files`: the class and version of every terrain file, which `dcsterrain check` \
        reports.";
    let input = input.as_object().cloned().expect("an object");
    tools.push(Tool::new_with_raw(
        LIST_TERRAINS,
        Some(description.into()),
        input,
    ));
    tools
}

/// A listing of the terrains as text, each terrain's `files` left out: they
/// are most of it, about 190 KB on an install of eight terrains, where the
/// rest is a few KB an assistant reads the ids from.
fn without_files(mut listing: Value) -> String {
    let terrains = listing["terrains"].as_array_mut().into_iter().flatten();
    for t in terrains {
        if let Some(t) = t.as_object_mut() {
            t.remove("files");
        }
    }
    listing.to_string()
}

/// Issues as the document `query` prints for them.
fn refusal(issues: Vec<Issue>) -> String {
    json!({ "issues": issues }).to_string()
}

/// JSON on its way out: kept as text while it is short, and moved to a
/// new file named for an operation and an extension once it passes
/// `INLINE_MAX`, so it is never held whole.
enum Spill<'a> {
    Text(Vec<u8>, &'a Path, &'a str, &'a str),
    File(BufWriter<File>, PathBuf, usize),
}

impl Write for Spill<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Spill::Text(text, dir, op, extension) if text.len() + buf.len() > INLINE_MAX => {
                let (file, path) = create(dir, op, extension)?;
                let mut file = BufWriter::new(file);
                let moved = file.write_all(text).and_then(|()| file.write_all(buf));
                if let Err(e) = moved {
                    drop(file);
                    let _ = std::fs::remove_file(&path);
                    return Err(e);
                }
                let written = text.len() + buf.len();
                *self = Spill::File(file, path, written);
            }
            Spill::Text(text, ..) => text.extend_from_slice(buf),
            Spill::File(file, _, n) => {
                file.write_all(buf)?;
                *n += buf.len();
            }
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Spill::Text(..) => Ok(()),
            Spill::File(file, ..) => file.flush(),
        }
    }
}

/// A new file in `dir` named for `op`, the process and the time, with
/// `extension`, never one that is already there: a server given the
/// process id of an earlier one overwrites none of its files.
fn create(dir: &Path, op: &str, extension: &str) -> io::Result<(File, PathBuf)> {
    private(dir)?;
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let pid = std::process::id();
    for k in 0u32.. {
        let path = dir.join(format!("{op}-{pid}-{now}-{k}.{extension}"));
        match File::options().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((file, path)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    unreachable!("some name is free")
}

/// Makes `dir` if it is missing, and refuses it unless it is a directory
/// only its owner may enter: where users share a temp directory, another
/// could otherwise read an answer or plant a link in its place.
fn private(dir: &Path) -> io::Result<()> {
    let mut b = std::fs::DirBuilder::new();
    b.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut b, 0o700);
    b.create(dir)?;
    let meta = std::fs::symlink_metadata(dir)?;
    let open = !meta.is_dir();
    #[cfg(unix)]
    let open = open || std::os::unix::fs::PermissionsExt::mode(&meta.permissions()) & 0o077 != 0;
    if open {
        let why = format!(
            "{} is not a directory only this user may enter",
            dir.display()
        );
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, why));
    }
    Ok(())
}

/// Where JSON went: kept as text, or written to a file of so many bytes.
enum Spilled {
    Text(String),
    File(PathBuf, usize),
}

/// A path as an answer names it.
fn named(path: &Path) -> io::Result<&str> {
    path.to_str().ok_or_else(|| {
        let why = format!("{} is not UTF-8, so no answer can name it", path.display());
        io::Error::new(io::ErrorKind::InvalidData, why)
    })
}

/// The files of the answers too large to hand back.
struct Files {
    dir: PathBuf,
}

impl Files {
    /// `value` as text, or in a file named for `op` with `extension` once
    /// past `INLINE_MAX`. A file left part written is removed.
    fn spill(&self, value: &impl Serialize, op: &str, extension: &str) -> io::Result<Spilled> {
        let mut out = Spill::Text(Vec::new(), &self.dir, op, extension);
        let written = serde_json::to_writer(&mut out, value)
            .map_err(io::Error::from)
            .and_then(|()| out.flush());
        let (file, path, n) = match (out, written) {
            (Spill::Text(text, ..), Ok(())) => {
                let text = String::from_utf8(text).expect("JSON is UTF-8");
                return Ok(Spilled::Text(text));
            }
            (Spill::Text(..), Err(e)) => return Err(e),
            (Spill::File(file, path, n), written) => (file, path, written.map(|()| n)),
        };
        drop(file);
        match n.and_then(|n| named(&path).map(|_| n)) {
            Ok(n) => Ok(Spilled::File(path, n)),
            Err(e) => {
                let _ = std::fs::remove_file(&path);
                Err(e)
            }
        }
    }

    /// An answer as the tool's text: `query`'s, with a GeoJSON over
    /// `INLINE_MAX` swapped for the path of a file holding it, and an answer
    /// still over it swapped for the path of a file holding the answer.
    fn written(&self, op: &str, mut answer: Answer) -> io::Result<String> {
        let value = match &mut answer {
            Answer::Value(v) => v,
            Answer::Rasters(r) => &mut r.value,
        };
        let mut geojson_file = None;
        if let Some(geojson) = value.get("geojson").filter(|g| g.is_object())
            && let Spilled::File(path, bytes) = self.spill(geojson, op, "geojson")?
        {
            let top = value.as_object_mut().expect("an answer is an object");
            top.remove("geojson");
            top.insert("geojson_file".into(), json!(named(&path)?));
            top.insert("geojson_bytes".into(), json!(bytes));
            geojson_file = Some(path);
        }
        let text = match self.spill(&answer, op, "json") {
            Ok(Spilled::Text(text)) => Ok(text),
            Ok(Spilled::File(path, bytes)) => {
                let path = named(&path)?;
                Ok(json!({"answer_file": path, "answer_bytes": bytes}).to_string())
            }
            Err(e) => Err(e),
        };
        // A GeoJSON's file the answer cannot name is named by nothing.
        if let (Err(_), Some(path)) = (&text, geojson_file) {
            let _ = std::fs::remove_file(path);
        }
        text
    }

    /// Removes the files of large answers written more than `KEPT` ago, by
    /// this server or another: only names a server gives, an operation's
    /// followed by a dash, ending `.json` or `.geojson`. A file that cannot
    /// be removed stays for the next.
    fn tidy(&self) {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return;
        };
        let now = SystemTime::now();
        for e in entries.flatten() {
            let name = e.file_name();
            let ours = name.to_str().is_some_and(|n| {
                OPERATIONS.iter().any(|o| {
                    n.strip_prefix(o.name).is_some_and(|rest| {
                        rest.starts_with('-')
                            && (rest.ends_with(".json") || rest.ends_with(".geojson"))
                    })
                })
            });
            let old = e
                .metadata()
                .and_then(|m| m.modified())
                .is_ok_and(|t| now.duration_since(t).is_ok_and(|age| age > KEPT));
            if ours && old {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

/// The directory the files of large answers go to: the system's own place
/// for files a program leaves for another, never the install or the cache.
fn answers_dir() -> PathBuf {
    std::env::temp_dir().join("dcsterrain")
}

/// Answers each call in turn on this thread, which owns the terrain, until
/// the server is gone. One call at a time, as a batch answers one line at a
/// time, so nothing grows with the number of calls.
fn answer_calls(mut opener: Opener, files: Files, calls: mpsc::Receiver<Call>) {
    files.tidy();
    // The terrain a call that names none is about.
    let started_on = opener.id.clone();
    for c in calls {
        let terrain = match c.terrain {
            Ok(t) => t.or_else(|| started_on.clone()),
            Err(text) => {
                let _ = c.reply.send(Err(text));
                continue;
            }
        };
        if c.op == LIST_TERRAINS {
            let (install, cache) = (opener.install.clone(), &opener.cache);
            let listed = call::within(c.ticket, || crate::terrains_json(install, cache));
            let _ = c.reply.send(listed.map(without_files).map_err(refusal));
            continue;
        }
        opener.name(terrain);
        let answered = call::within(c.ticket, || opener.answer(&c.op, &c.request));
        let text = answered.map_err(refusal).and_then(|a| {
            files.written(&c.op, a).map_err(|e| {
                let why = format!("the answer cannot be written: {e}");
                refusal(vec![Issue::new("internal_error", &why, "")])
            })
        });
        // A caller that gave up has nobody to tell.
        let _ = c.reply.send(text);
    }
}

impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("dcsterrain", env!("CARGO_PKG_VERSION")))
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(self.tools.as_ref().clone()))
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.tools.iter().find(|t| t.name == name).cloned()
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let token = context.meta.get_progress_token();
        let (report, mut progress) = queue::unbounded_channel();
        let ticket = Ticket::new(Box::new(move |share, doing: &str| {
            let _ = report.send((share, doing.to_string()));
        }));
        let (reply, mut answered) = oneshot::channel();
        let mut arguments = request.arguments.unwrap_or_default();
        let terrain = match arguments.remove(TERRAIN) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(id)) => Ok(Some(id)),
            Some(_) => Err(refusal(vec![Issue::new(
                "invalid_request",
                "`terrain` is a terrain's id, a string",
                TERRAIN,
            )])),
        };
        let call = Call {
            op: request.name.into_owned(),
            terrain,
            request: Value::Object(arguments),
            ticket: ticket.clone(),
            reply,
        };
        let gone = || ErrorData::internal_error("the thread answering calls has stopped", None);
        self.calls.send(call).map_err(|_| gone())?;
        let text = loop {
            tokio::select! {
                // In this order: the answering thread queues every notice
                // before its reply, so none is lost to a reply that wins.
                biased;
                Some((share, doing)) = progress.recv() => {
                    if let Some(token) = &token {
                        let notice = ProgressNotificationParam::new(token.clone(), share)
                            .with_total(1.0)
                            .with_message(doing);
                        let _ = context.peer.notify_progress(notice).await;
                    }
                }
                () = context.ct.cancelled() => {
                    // The work stops at its next look, and the next call waits
                    // behind it on the answering thread until it has.
                    ticket.cancel();
                    return Err(ErrorData::internal_error("the call was cancelled", None));
                }
                text = &mut answered => break text.map_err(|_| gone())?,
            }
        };
        let result = match text {
            Ok(text) => CallToolResult::success(vec![ContentBlock::text(text)]),
            Err(text) => CallToolResult::error(vec![ContentBlock::text(text)]),
        };
        Ok(result.into())
    }
}

/// Serves the tools on stdin and stdout until the client closes them.
pub fn serve(opener: Opener) -> ExitCode {
    let (calls, waiting) = mpsc::channel();
    let files = Files { dir: answers_dir() };
    let answering = std::thread::Builder::new()
        .name("answer".into())
        .spawn(move || answer_calls(opener, files, waiting));
    if let Err(e) = answering {
        tracing::error!("cannot start the thread that answers: {e}");
        return ExitCode::FAILURE;
    }
    let server = Server {
        tools: Arc::new(tools()),
        calls,
    };
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("cannot start the server: {e}");
            return ExitCode::FAILURE;
        }
    };
    let served = runtime.block_on(async {
        let running = server.serve(rmcp::transport::stdio()).await?;
        running.waiting().await?;
        Ok::<_, Box<dyn std::error::Error>>(())
    });
    match served {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!("{e}");
            ExitCode::FAILURE
        }
    }
}
