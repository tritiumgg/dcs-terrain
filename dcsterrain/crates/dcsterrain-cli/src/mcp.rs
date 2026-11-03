//! `dcsterrain mcp`: the operations as tools of a Model Context Protocol
//! server on stdin and stdout. A tool's input is its operation's request and
//! its answer the one `query` prints for it, byte for byte; a refusal is a
//! tool error whose text is the issue list `query` prints. Nothing here
//! knows a terrain: each call is handed to the opener `query` answers with.

use std::process::ExitCode;
use std::sync::{Arc, mpsc};

use dcsterrain_core::query::Answer;
use dcsterrain_core::types::{Issue, OPERATIONS, request_schema};
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt};
use serde_json::{Value, json};
use tokio::sync::oneshot;

use crate::query::Opener;

/// One call for the thread that holds the terrain.
struct Call {
    op: String,
    request: Value,
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

/// An answer as `query` writes it, without its newline.
fn written(answer: &Answer) -> Result<String, String> {
    serde_json::to_string(answer)
        .map_err(|e| refusal(vec![Issue::new("internal_error", &e.to_string(), "")]))
}

/// Issues as the document `query` prints for them.
fn refusal(issues: Vec<Issue>) -> String {
    json!({ "issues": issues }).to_string()
}

/// Answers each call in turn on this thread, which owns the terrain, until
/// the server is gone. One call at a time, as a batch answers one line at a
/// time, so nothing grows with the number of calls.
fn answer_calls(mut opener: Opener, calls: mpsc::Receiver<Call>) {
    for call in calls {
        let text = opener
            .answer(&call.op, &call.request)
            .map_err(refusal)
            .and_then(|a| written(&a));
        // A caller that gave up has nobody to tell.
        let _ = call.reply.send(text);
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
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let (reply, answered) = oneshot::channel();
        let call = Call {
            op: request.name.into_owned(),
            request: Value::Object(request.arguments.unwrap_or_default()),
            reply,
        };
        let gone = || ErrorData::internal_error("the thread answering calls has stopped", None);
        self.calls.send(call).map_err(|_| gone())?;
        let result = match answered.await.map_err(|_| gone())? {
            Ok(text) => CallToolResult::success(vec![ContentBlock::text(text)]),
            Err(text) => CallToolResult::error(vec![ContentBlock::text(text)]),
        };
        Ok(result.into())
    }
}

/// Serves the tools on stdin and stdout until the client closes them.
pub fn serve(opener: Opener) -> ExitCode {
    let (calls, waiting) = mpsc::channel();
    let answering = std::thread::Builder::new()
        .name("answer".into())
        .spawn(move || answer_calls(opener, waiting));
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
