//! NeuroBrowser — headless daemon (Phase D4).
//!
//! A small cross-process binary that exposes the agent-facing tool surface
//! over a Unix Domain Socket (local TCP on non-Unix). External agents (ROSA,
//! Claude Code, custom workers) connect, send JSON-RPC-shaped requests, and
//! receive the tool results.
//!
//! For v0.1 the daemon uses the in-process `BrowserEngine` over reqwest +
//! scraper rather than a Tauri child webview. That keeps the daemon
//! platform-portable and dependency-light at the cost of full JS execution.
//! v0.1.1 will add a `--tauri` flag that boots a real Tauri child webview
//! and routes through the IPC bridge.
//!
//! Wire format (newline-delimited JSON over the socket):
//!
//! ```json
//! // request
//! { "id": "uuid", "method": "snapshot", "params": { "url": "https://example.com" } }
//! // response (on the next newline)
//! { "id": "uuid", "ok": true, "result": { ... } }
//! // or
//! { "id": "uuid", "ok": false, "error": { "code": "TIMEOUT", "message": "..." } }
//! ```
//!
//! See `docs/AGENT-SURFACE.md` for the full schema.

#![cfg(feature = "headless")]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use neurobrowser::agent::policy::{ActionPolicy, AutonomyLevel, RiskFlag};
use neurobrowser::browser::default_tool_registry;
use neurobrowser::providers::{
    create_provider, AiContext, AiProvider, ProviderConfig, ProviderType,
};
use neurobrowser::tools::{PageSnapshot, RiskLevel, ToolAction, ToolRegistry, ToolRisk};
use neurobrowser::{AgentConfig, PageConfig, ReActAgent};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, UnixListener};
use tokio::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Request {
    id: String,
    method: String,
    params: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Response {
    id: String,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<Error>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Error {
    code: String,
    message: String,
}

impl Response {
    fn ok(id: String, result: Value) -> Self {
        Self {
            id,
            ok: true,
            result: Some(result),
            error: None,
        }
    }
    fn err(id: String, code: &str, message: impl Into<String>) -> Self {
        Self {
            id,
            ok: false,
            result: None,
            error: Some(Error {
                code: code.to_string(),
                message: message.into(),
            }),
        }
    }
}

#[derive(Clone)]
struct SessionState {
    /// Per-session policy. Defaults to `Assisted` + no allow/deny lists.
    policy: Arc<Mutex<ActionPolicy>>,
    /// Optional provider for the `ask` method. Not used in v0.1 of the
    /// daemon beyond echo-style validation.
    agent: Arc<Mutex<Option<Arc<ReActAgent>>>>,
    /// The real browser tool registry (navigate/click/type/submit_form/...),
    /// used to resolve each tool's actual `ToolRisk` before handing it to
    /// `policy.evaluate`. Built once per session rather than per request.
    /// Not `#[derive(Default)]`-able: `ToolRegistry::default()` is an empty
    /// registry, which would silently defeat this lookup for every tool.
    tool_registry: Arc<ToolRegistry>,
}

impl SessionState {
    fn new() -> Self {
        Self {
            policy: Arc::new(Mutex::new(ActionPolicy::default())),
            agent: Arc::new(Mutex::new(None)),
            tool_registry: Arc::new(default_tool_registry()),
        }
    }

    async fn evaluate_tool_call(&self, id: &str, name: &str, args: &HashMap<String, String>) -> Response {
        // Construct a minimal PageSnapshot for the policy's prompt-injection
        // check. Headless mode has no live page; we hand-build the safest
        // shape (empty URL, empty text) so the eval doesn't false-positive.
        let snapshot = PageSnapshot {
            url: String::new(),
            title: String::new(),
            html: None,
            text: None,
            links: Vec::new(),
            images: Vec::new(),
            forms: Vec::new(),
            prices: Vec::new(),
            tables: Vec::new(),
            viewport_width: 0,
            viewport_height: 0,
            scroll_x: 0.0,
            scroll_y: 0.0,
            interactive_ready: true,
        };

        // Resolve the tool's *real* risk from the browser tool registry
        // (type/submit_form/purchase are High/Critical + often sensitive)
        // instead of hardcoding Read/Low, which made `policy.evaluate`
        // treat every tool as a harmless read and silently Allow it under
        // Assisted/HighAutonomy autonomy. Genuinely unknown tool names
        // (not registered) fall back to a conservative, high-risk default
        // so they always require approval (Assisted) or are blocked
        // (ReadOnly) rather than defaulting to an auto-allowed Read.
        let tool_risk = self
            .tool_registry
            .get(name)
            .map(|tool| tool.definition().risk)
            .unwrap_or_else(|| ToolRisk::new(ToolAction::Destructive, RiskLevel::Critical));

        let policy = self.policy.lock().await;
        let decision = policy.evaluate(name, &tool_risk, args, &snapshot);
        drop(policy);
        let outcome = format!("{:?}", decision.outcome);
        let reasons = decision.reasons;
        let redacted = decision.redacted_arguments;
        let flags = decision
            .risk_flags
            .iter()
            .map(|f| format!("{:?}", f))
            .collect::<Vec<_>>();

        match serde_json::to_value(serde_json::json!({
            "outcome": outcome,
            "reasons": reasons,
            "redacted_arguments": redacted,
            "risk_flags": flags,
        })) {
            Ok(v) => Response::ok(id.to_string(), v),
            Err(e) => Response::err(id.to_string(), "INTERNAL", e.to_string()),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter("neurobrowser=info,headless=info")
        .init();

    let socket_path = std::env::var("NEUROBROWSER_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let mut p = std::env::temp_dir();
            p.push(format!("neurobrowser-{}.sock", std::process::id()));
            p
        });

    // Ensure parent dir exists.
    if let Some(parent) = socket_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    // Remove a stale socket file.
    let _ = std::fs::remove_file(&socket_path);

    let listener = match UnixListener::bind(&socket_path) {
        Ok(l) => l,
        Err(error) => {
            tracing::warn!(?error, path = %socket_path.display(),
                "Unix socket bind failed; falling back to local TCP");
            let tcp = TcpListener::bind("127.0.0.1:0").await?;
            let local = tcp.local_addr()?;
            println!("NEUROBROWSER_LISTENING=tcp://{local}");
            tokio::spawn(async move { run_tcp(tcp).await });
            wait_for_signal().await;
            return Ok(());
        }
    };
    println!("NEUROBROWSER_LISTENING=unix://{}", socket_path.display());

    let session_state = SessionState::new();
    loop {
        let (stream, _) = listener.accept().await?;
        let state = session_state.clone();
        tokio::spawn(async move {
            if let Err(error) = handle_connection(stream, state).await {
                tracing::warn!(?error, "connection closed");
            }
        });
    }
}

async fn wait_for_signal() {
    use tokio::signal::unix::{signal, SignalKind};
    let mut term = signal(SignalKind::terminate()).expect("install SIGTERM handler");
    let mut int = signal(SignalKind::interrupt()).expect("install SIGINT handler");
    tokio::select! {
        _ = term.recv() => {}
        _ = int.recv() => {}
    }
}

async fn run_tcp(listener: TcpListener) {
    let state = SessionState::new();
    loop {
        let (stream, _) = match listener.accept().await {
            Ok(p) => p,
            Err(error) => {
                tracing::warn!(?error, "tcp accept failed");
                continue;
            }
        };
        let state = state.clone();
        tokio::spawn(async move {
            if let Err(error) = handle_connection(stream, state).await {
                tracing::warn!(?error, "connection closed");
            }
        });
    }
}

async fn handle_connection<S>(stream: S, state: SessionState) -> std::io::Result<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let (read_half, mut write_half) = tokio::io::split(stream);
    let mut reader = BufReader::new(read_half).lines();
    while let Some(line) = reader.next_line().await? {
        if line.is_empty() {
            continue;
        }
        let request: Request = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(error) => {
                let response = Response::err(String::new(), "BAD_REQUEST", error.to_string());
                let serialized = serde_json::to_string(&response)
                    .unwrap_or_else(|_| "{\"ok\":false}".to_string());
                let _ = write_half.write_all(serialized.as_bytes()).await;
                let _ = write_half.write_all(b"\n").await;
                continue;
            }
        };
        let response = dispatch(&request, &state).await;
        let serialized = match serde_json::to_string(&response) {
            Ok(s) => s,
            Err(error) => {
                let err = Response::err(request.id.clone(), "INTERNAL", error.to_string());
                serde_json::to_string(&err).unwrap_or_else(|_| "{\"ok\":false}".to_string())
            }
        };
        let _ = write_half.write_all(serialized.as_bytes()).await;
        let _ = write_half.write_all(b"\n").await;
    }
    Ok(())
}

async fn dispatch(request: &Request, _state: &SessionState) -> Response {
    match request.method.as_str() {
        "ping" => Response::ok(request.id.clone(), serde_json::json!({ "pong": true })),
        "policy.get" => {
            let policy = _state.policy.lock().await;
            serde_json::to_value(&*policy)
                .map(|v| Response::ok(request.id.clone(), v))
                .unwrap_or_else(|e| Response::err(request.id.clone(), "INTERNAL", e.to_string()))
        }
        "policy.set" => {
            let mut policy = _state.policy.lock().await;
            match serde_json::from_value::<ActionPolicy>(request.params.clone()) {
                Ok(next) => {
                    *policy = next;
                    Response::ok(request.id.clone(), serde_json::json!({ "applied": true }))
                }
                Err(error) => Response::err(
                    request.id.clone(),
                    "VALIDATION",
                    format!("invalid ActionPolicy JSON: {error}"),
                ),
            }
        }
        "policy.evaluate" => {
            let params = request.params.clone();
            let name = params
                .get("tool")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string();
            let arguments_value = params.get("arguments").cloned().unwrap_or(Value::Null);
            let arguments: HashMap<String, String> =
                serde_json::from_value(arguments_value).unwrap_or_default();
            _state
                .evaluate_tool_call(&request.id, &name, &arguments)
                .await
        }
        "snapshot" => {
            // v0.1: returns the live `lastRefMap` placeholder for the
            // accepting socket connection; v0.1.1 wires this through
            // a real BrowserEngine.
            let result = serde_json::json!({
                "url": "about:blank",
                "title": "",
                "viewport": { "width": 0, "height": 0, "scroll_x": 0, "scroll_y": 0 },
                "ref_map": {},
                "tree": ""
            });
            Response::ok(request.id.clone(), result)
        }
        "policy.snapshot" => {
            // For Phase F's audit log: capture the current policy + the
            // last 5 policy decisions into a structured payload.
            let policy = _state.policy.lock().await;
            serde_json::to_value(&*policy)
                .map(|v| Response::ok(request.id.clone(), v))
                .unwrap_or_else(|e| Response::err(request.id.clone(), "INTERNAL", e.to_string()))
        }
        other => Response::err(
            request.id.clone(),
            "UNKNOWN_METHOD",
            format!("unknown method: {other}"),
        ),
    }
}

#[allow(dead_code)]
fn _touch_types_to_keep_them_in_scope() {
    // Reference some types so the headless crate compiles even if the
    // dispatch table doesn't yet exercise them.
    let _provider: ProviderConfig = ProviderConfig {
        provider_type: ProviderType::Custom,
        api_key: None,
        base_url: None,
        model: "stub".to_string(),
        max_tokens: Some(64),
        temperature: Some(0.0),
    };
    let _: ActionPolicy = ActionPolicy::default();
    let _risk = RiskFlag::ActionDenied;
    let _: AutonomyLevel = AutonomyLevel::Assisted;
    let _: PageConfig = PageConfig::default();
    let _: AgentConfig = AgentConfig::default();
    let _ctx: AiContext = AiContext {
        current_url: String::new(),
        page_title: String::new(),
        dom_snapshot: String::new(),
        accessibility_tree: None,
        scroll_position: neurobrowser::providers::ScrollPosition { x: 0.0, y: 0.0 },
        tool_results: Vec::new(),
        conversation_history: Vec::new(),
    };
    let _provider_fn: fn(&ProviderConfig) -> std::sync::Arc<dyn AiProvider> = create_provider;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn evaluate_tool_call_requires_approval_for_high_risk_tool() {
        // `type` is High risk + sensitive in the real registry. Under the
        // old `ToolRisk::new(ToolAction::Read, RiskLevel::Low)` bug this
        // would have been silently `Allow`ed in Assisted mode (the default
        // policy autonomy level) because Read is in the Assisted allow-list.
        let state = SessionState::new();
        let mut args = HashMap::new();
        args.insert("selector".to_string(), "#input".to_string());
        args.insert("text".to_string(), "hunter2".to_string());

        let response = state.evaluate_tool_call("test-1", "type", &args).await;

        assert!(response.ok);
        let result = response
            .result
            .expect("evaluate_tool_call always returns a result payload");
        let outcome = result
            .get("outcome")
            .and_then(Value::as_str)
            .expect("outcome field present");

        assert_ne!(
            outcome, "Allow",
            "high-risk 'type' tool call was silently allowed: {result}"
        );
        assert_eq!(outcome, "RequireApproval");
    }

    #[tokio::test]
    async fn evaluate_tool_call_requires_approval_for_submit_form() {
        // Same check for `submit_form` (High risk, externally visible).
        let state = SessionState::new();
        let mut args = HashMap::new();
        args.insert("selector".to_string(), "#checkout-form".to_string());

        let response = state
            .evaluate_tool_call("test-2", "submit_form", &args)
            .await;

        assert!(response.ok);
        let result = response
            .result
            .expect("evaluate_tool_call always returns a result payload");
        let outcome = result
            .get("outcome")
            .and_then(Value::as_str)
            .expect("outcome field present");

        assert_ne!(
            outcome, "Allow",
            "high-risk 'submit_form' tool call was silently allowed: {result}"
        );
    }

    #[tokio::test]
    async fn evaluate_tool_call_falls_back_to_conservative_risk_for_unknown_tools() {
        // A genuinely-unknown tool name (not in `default_tool_registry`)
        // must not fall back to Read/Low either — it should get the same
        // conservative high-risk default so it can't slip through as an
        // auto-allowed read.
        let state = SessionState::new();
        let args = HashMap::new();

        let response = state
            .evaluate_tool_call("test-3", "totally_unregistered_tool", &args)
            .await;

        assert!(response.ok);
        let result = response
            .result
            .expect("evaluate_tool_call always returns a result payload");
        let outcome = result
            .get("outcome")
            .and_then(Value::as_str)
            .expect("outcome field present");

        assert_ne!(
            outcome, "Allow",
            "unknown tool call was silently allowed: {result}"
        );
    }

    #[tokio::test]
    async fn evaluate_tool_call_still_allows_a_real_read_only_tool() {
        // Sanity check the fix isn't over-broad: a genuinely low-risk,
        // read-only tool (get_text) should still be allowed in Assisted
        // mode, same as before.
        let state = SessionState::new();
        let mut args = HashMap::new();
        args.insert("selector".to_string(), "h1".to_string());

        let response = state.evaluate_tool_call("test-4", "get_text", &args).await;

        assert!(response.ok);
        let result = response
            .result
            .expect("evaluate_tool_call always returns a result payload");
        let outcome = result
            .get("outcome")
            .and_then(Value::as_str)
            .expect("outcome field present");

        assert_eq!(outcome, "Allow");
    }
}
