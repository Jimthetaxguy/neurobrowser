//! NeuroBrowser — headless daemon: policy JSON-RPC + stub snapshot.
//!
//! External agents connect over a Unix domain socket and send newline-delimited
//! JSON-RPC-shaped requests. Dispatch never constructs a `BrowserEngine` or a
//! webview. The `snapshot` method returns a hardcoded `about:blank` stub.
//!
//! Methods: `ping`, `policy.get`, `policy.set`, `policy.evaluate`,
//! `snapshot` (stub).
//!
//! Socket path, first match: `--socket PATH`, else `$NEUROBROWSER_SOCKET`,
//! else a per-pid file in the temp directory. `--help` prints usage. Unknown
//! arguments are rejected (they are not ignored).
//!
//! The listener is Unix-only. Non-Unix targets compile (`--help` works) but
//! refuse to listen. If the Unix socket cannot be bound, the process falls
//! back to `127.0.0.1:0` and prints `NEUROBROWSER_LISTENING=tcp://…`. That
//! fallback is not a non-Unix transport.
//!
//! Wire format (newline-delimited JSON over the socket):
//!
//! ```json
//! // request — `snapshot` ignores `params` and returns a hardcoded stub
//! { "id": "uuid", "method": "snapshot", "params": {} }
//! // response (on the next newline)
//! { "id": "uuid", "ok": true, "result": { ... } }
//! // or
//! { "id": "uuid", "ok": false, "error": { "code": "UNKNOWN_METHOD", "message": "..." } }
//! ```
//!
//! Error codes: `BAD_REQUEST`, `INTERNAL`, `VALIDATION`, `UNKNOWN_METHOD`.

#![cfg(feature = "headless")]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use neurobrowser::agent::policy::ActionPolicy;
use neurobrowser::browser::default_tool_registry;
use neurobrowser::tools::{PageSnapshot, RiskLevel, ToolAction, ToolRegistry, ToolRisk};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
#[cfg(unix)]
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
    /// Browser tool registry used to resolve each tool's real `ToolRisk`.
    /// `ToolRegistry::default()` is empty and would silently defeat that lookup.
    tool_registry: Arc<ToolRegistry>,
}

impl SessionState {
    fn new() -> Self {
        Self {
            policy: Arc::new(Mutex::new(ActionPolicy::default())),
            tool_registry: Arc::new(default_tool_registry()),
        }
    }

    async fn evaluate_tool_call(
        &self,
        id: &str,
        name: &str,
        args: &HashMap<String, String>,
    ) -> Response {
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

        // Registered tools use their real `ToolRisk`. Unknown names fall
        // back to Destructive/Critical so they are not treated as reads.
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

#[derive(Debug)]
struct Cli {
    socket: Option<PathBuf>,
}

#[derive(Debug)]
enum CliError {
    Help,
    Message(String),
}

fn print_help() {
    println!(
        "neurobrowser-headless — policy JSON-RPC + stub snapshot\n\
         \n\
         Usage: neurobrowser-headless [--socket PATH]\n\
         \n\
         --socket PATH   Unix socket path (overrides NEUROBROWSER_SOCKET)\n\
         --help          Print this help and exit\n\
         \n\
         If --socket is omitted, the path is $NEUROBROWSER_SOCKET or a per-pid\n\
         file in the temp directory. This process does not construct a\n\
         BrowserEngine and does not drive a webview."
    );
}

fn parse_args<I>(args: I) -> Result<Cli, CliError>
where
    I: IntoIterator<Item = String>,
{
    let mut socket = None;
    let mut iter = args.into_iter();
    let _exe = iter.next();
    while let Some(arg) = iter.next() {
        if arg == "-h" || arg == "--help" {
            return Err(CliError::Help);
        }
        if arg == "--socket" {
            let path = iter
                .next()
                .ok_or_else(|| CliError::Message("--socket requires a path".into()))?;
            if path.is_empty() || path.starts_with('-') {
                return Err(CliError::Message(
                    "--socket requires a path; use --socket=PATH for a dash-prefixed path".into(),
                ));
            }
            socket = Some(PathBuf::from(path));
            continue;
        }
        if let Some(path) = arg.strip_prefix("--socket=") {
            if path.is_empty() {
                return Err(CliError::Message("--socket requires a path".into()));
            }
            socket = Some(PathBuf::from(path));
            continue;
        }
        return Err(CliError::Message(format!("unknown argument: {arg}")));
    }
    Ok(Cli { socket })
}

fn resolve_socket_path(cli: &Cli) -> PathBuf {
    if let Some(path) = &cli.socket {
        return path.clone();
    }
    std::env::var("NEUROBROWSER_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let mut p = std::env::temp_dir();
            p.push(format!("neurobrowser-{}.sock", std::process::id()));
            p
        })
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                tracing_subscriber::EnvFilter::new("neurobrowser=info,headless=info")
            }),
        )
        .init();

    let cli = match parse_args(std::env::args()) {
        Ok(cli) => cli,
        Err(CliError::Help) => {
            print_help();
            return Ok(());
        }
        Err(CliError::Message(message)) => {
            eprintln!("neurobrowser-headless: {message}");
            eprintln!("Try --help for usage.");
            std::process::exit(2);
        }
    };

    let socket_path = resolve_socket_path(&cli);

    #[cfg(unix)]
    return listen_unix(socket_path).await;

    #[cfg(not(unix))]
    {
        let _ = socket_path;
        eprintln!(
            "neurobrowser-headless listens on a Unix domain socket and is not supported on this target."
        );
        std::process::exit(1);
    }
}

#[cfg(unix)]
async fn listen_unix(socket_path: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = socket_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
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

    // Restrict the control socket to the owning user. This is defense-in-depth;
    // full per-connection peer-credential authz (SO_PEERCRED same-uid + per-client
    // session state) is tracked as a follow-up.
    use std::os::unix::fs::PermissionsExt;
    if let Err(error) =
        std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o600))
    {
        tracing::warn!(?error, "failed to restrict control-socket permissions");
    }

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

#[cfg(unix)]
async fn wait_for_signal() {
    use tokio::signal::unix::{signal, SignalKind};
    let mut term = signal(SignalKind::terminate()).expect("install SIGTERM handler");
    let mut int = signal(SignalKind::interrupt()).expect("install SIGINT handler");
    tokio::select! {
        _ = term.recv() => {}
        _ = int.recv() => {}
    }
}

#[cfg(unix)]
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
            // Stub: no BrowserEngine, no live page.
            let result = serde_json::json!({
                "url": "about:blank",
                "title": "",
                "viewport": { "width": 0, "height": 0, "scroll_x": 0, "scroll_y": 0 },
                "tree": ""
            });
            Response::ok(request.id.clone(), result)
        }
        other => Response::err(
            request.id.clone(),
            "UNKNOWN_METHOD",
            format!("unknown method: {other}"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(argv: &[&str]) -> Vec<String> {
        std::iter::once("neurobrowser-headless")
            .chain(argv.iter().copied())
            .map(String::from)
            .collect()
    }

    #[test]
    fn parse_args_accepts_socket() {
        let cli = parse_args(args(&["--socket", "/tmp/nb.sock"])).unwrap();
        assert_eq!(
            cli.socket.as_deref(),
            Some(std::path::Path::new("/tmp/nb.sock"))
        );
    }

    #[test]
    fn parse_args_accepts_socket_equals() {
        let cli = parse_args(args(&["--socket=/tmp/nb.sock"])).unwrap();
        assert_eq!(
            cli.socket.as_deref(),
            Some(std::path::Path::new("/tmp/nb.sock"))
        );
    }

    #[test]
    fn parse_args_help() {
        assert!(matches!(parse_args(args(&["--help"])), Err(CliError::Help)));
        assert!(matches!(parse_args(args(&["-h"])), Err(CliError::Help)));
    }

    #[test]
    fn parse_args_rejects_unknown() {
        match parse_args(args(&["--tauri"])) {
            Err(CliError::Message(message)) => {
                assert!(message.contains("unknown argument: --tauri"), "{message}");
            }
            other => panic!("expected unknown-argument error, got {other:?}"),
        }
    }

    #[test]
    fn parse_args_socket_requires_path() {
        match parse_args(args(&["--socket"])) {
            Err(CliError::Message(message)) => {
                assert!(message.contains("--socket requires a path"), "{message}");
            }
            other => panic!("expected missing-path error, got {other:?}"),
        }
    }

    #[test]
    fn parse_args_rejects_options_in_place_of_socket_path() {
        for option in [
            "--help",
            "-h",
            "--tauri",
            "--socket",
            "--socket=/tmp/other.sock",
        ] {
            match parse_args(args(&["--socket", option])) {
                Err(CliError::Message(message)) => {
                    assert!(
                        message.contains("--socket requires a path"),
                        "{option}: {message}"
                    );
                }
                other => panic!("expected missing-path error for {option}, got {other:?}"),
            }
        }
    }

    #[test]
    fn parse_args_accepts_explicit_dash_prefixed_socket_path() {
        let cli = parse_args(args(&["--socket=--help"])).unwrap();
        assert_eq!(cli.socket.as_deref(), Some(std::path::Path::new("--help")));
    }

    #[test]
    fn resolve_socket_path_prefers_flag_over_env() {
        let cli = Cli {
            socket: Some(PathBuf::from("/tmp/from-flag.sock")),
        };
        assert_eq!(
            resolve_socket_path(&cli),
            PathBuf::from("/tmp/from-flag.sock")
        );
    }

    #[tokio::test]
    async fn evaluate_tool_call_requires_approval_for_high_risk_tool() {
        // `type` is High risk + sensitive; Assisted mode must not auto-allow it.
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
        // Unregistered names get the conservative high-risk default.
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
        // `get_text` is low-risk and remains allowed in Assisted mode.
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
