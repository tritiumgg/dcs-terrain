//! `dcsterrain mcp`: the operations as tools of a Model Context Protocol
//! server on stdin and stdout. A tool's input is its operation's request and
//! its answer the one `query` prints for it, byte for byte, unless it is too
//! large to hand back as text; a refusal is a tool error whose text is the
//! issue list `query` prints. Nothing here knows a terrain: each call is
//! handed to the opener `query` answers with.
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

/// One tool per operation, its input the operation's request schema.
fn tools() -> Vec<Tool> {
    OPERATIONS
        .iter()
        .map(|o| {
            let schema = request_schema(o.name).expect("a listed operation has a schema");
            let description = schema
                .get("description")
                .and_then(Value::as_str)
                .map(|d| d.to_string().into());
            Tool::new_with_raw(o.name, description, schema)
        })
        .collect()
}

/// Issues as the document `query` prints for them.
fn refusal(issues: Vec<Issue>) -> String {
    json!({ "issues": issues }).to_string()
}

/// JSON on its way out: kept as text while it is short, and moved to a
/// file once it passes `INLINE_MAX`, so it is never held whole.
enum Spill<'a> {
    Text(Vec<u8>, &'a Path),
    File(BufWriter<File>, PathBuf, usize),
}

impl Write for Spill<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Spill::Text(text, path) if text.len() + buf.len() > INLINE_MAX => {
                let path: &Path = path;
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir)?;
                }
                let mut file = BufWriter::new(File::create(path)?);
                file.write_all(text)?;
                file.write_all(buf)?;
                let written = text.len() + buf.len();
                *self = Spill::File(file, path.to_path_buf(), written);
            }
            Spill::Text(text, _) => text.extend_from_slice(buf),
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

/// Where JSON went: kept as text, or written to a file of so many bytes.
enum Spilled {
    Text(String),
    File(PathBuf, usize),
}

/// The files of the answers too large to hand back, one name each.
struct Files {
    dir: PathBuf,
    made: u64,
}

impl Files {
    /// `value` as text, or in a file named for `op` with `extension` once
    /// past `INLINE_MAX`.
    fn spill(&mut self, value: &impl Serialize, op: &str, extension: &str) -> io::Result<Spilled> {
        self.made += 1;
        let name = format!("{op}-{}-{}.{extension}", std::process::id(), self.made);
        let path = self.dir.join(name);
        let mut out = Spill::Text(Vec::new(), &path);
        serde_json::to_writer(&mut out, value)?;
        out.flush()?;
        Ok(match out {
            Spill::Text(text, _) => Spilled::Text(String::from_utf8(text).expect("JSON is UTF-8")),
            Spill::File(_, path, n) => Spilled::File(path, n),
        })
    }

    /// An answer as the tool's text: `query`'s, with a GeoJSON over
    /// `INLINE_MAX` swapped for the path of a file holding it, and an answer
    /// still over it swapped for the path of a file holding the answer.
    fn written(&mut self, op: &str, mut answer: Answer) -> io::Result<String> {
        let value = match &mut answer {
            Answer::Value(v) => v,
            Answer::Rasters(r) => &mut r.value,
        };
        if let Some(geojson) = value.get("geojson").filter(|g| g.is_object())
            && let Spilled::File(path, bytes) = self.spill(geojson, op, "geojson")?
        {
            let top = value.as_object_mut().expect("an answer is an object");
            top.remove("geojson");
            top.insert("geojson_file".into(), json!(path));
            top.insert("geojson_bytes".into(), json!(bytes));
        }
        match self.spill(&answer, op, "json")? {
            Spilled::Text(text) => Ok(text),
            Spilled::File(path, bytes) => {
                Ok(json!({"answer_file": path, "answer_bytes": bytes}).to_string())
            }
        }
    }

    /// Removes the files a server wrote more than `KEPT` ago. A file that
    /// cannot be removed stays for the next.
    fn tidy(&self) {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return;
        };
        let now = SystemTime::now();
        for e in entries.flatten() {
            let old = e
                .metadata()
                .and_then(|m| m.modified())
                .is_ok_and(|t| now.duration_since(t).is_ok_and(|age| age > KEPT));
            let ours = matches!(
                e.path().extension().and_then(|x| x.to_str()),
                Some("json" | "geojson")
            );
            if old && ours {
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
fn answer_calls(mut opener: Opener, mut files: Files, calls: mpsc::Receiver<Call>) {
    files.tidy();
    for c in calls {
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
        let call = Call {
            op: request.name.into_owned(),
            request: Value::Object(request.arguments.unwrap_or_default()),
            ticket: ticket.clone(),
            reply,
        };
        let gone = || ErrorData::internal_error("the thread answering calls has stopped", None);
        self.calls.send(call).map_err(|_| gone())?;
        let text = loop {
            tokio::select! {
                text = &mut answered => break text.map_err(|_| gone())?,
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
    let files = Files {
        dir: answers_dir(),
        made: 0,
    };
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
