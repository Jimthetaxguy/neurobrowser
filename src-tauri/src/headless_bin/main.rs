//! NeuroBrowser — headless daemon: policy JSON-RPC + stub snapshot.
//!
//! External agents connect over a Unix domain socket and send newline-delimited
//! JSON-RPC-shaped requests. Dispatch never constructs a `BrowserEngine` or a
//! webview. The `snapshot` method returns a hardcoded `about:blank` stub.
//!
//! Methods: `auth`, `ping`, `policy.get`, `policy.set`, `policy.evaluate`,
//! `tools.list`, `snapshot` (stub).
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
//! The control socket is local but still an authorization boundary:
//!
//! * **Unix socket** — the file is `chmod 0600` (defense-in-depth) *and* every
//!   accepted connection is checked with `SO_PEERCRED`: a peer whose uid does
//!   not match the daemon's own uid is dropped **before any request is
//!   dispatched**. (macOS does not enforce socket-file permissions on
//!   `connect(2)`, so the peer-credential check is the real gate there.)
//! * **TCP fallback** — there is no peer-credential concept, so the connection
//!   must complete an `auth` handshake presenting a shared secret
//!   (`NEUROBROWSER_TOKEN`, or a freshly generated one printed at startup)
//!   before any other method is accepted.
//!
//! Each connection gets its **own** [`SessionState`] (its own [`ActionPolicy`]),
//! so a `policy.set` from one client can never mutate another client's policy.
//! The read-only tool registry is the only thing shared across connections.
//!
//! Per connection the negotiated policy tier is mapped to an allowed tool-name
//! set: `tools.list` advertises only the in-profile tools and
//! `policy.evaluate` rejects out-of-profile calls before they reach the policy
//! engine.
//!
//! Wire format (newline-delimited JSON over the socket):
//!
//! ```json
//! // (TCP only) authenticate first
//! { "id": "uuid", "method": "auth", "params": { "token": "..." } }
//! // request — `snapshot` ignores `params` and returns a hardcoded stub
//! { "id": "uuid", "method": "snapshot", "params": {} }
//! // response (on the next newline)
//! { "id": "uuid", "ok": true, "result": { ... } }
//! // or
//! { "id": "uuid", "ok": false, "error": { "code": "UNKNOWN_METHOD", "message": "..." } }
//! ```
//!
//! Error codes: `BAD_REQUEST`, `INTERNAL`, `VALIDATION`, `UNKNOWN_METHOD`,
//! `OUT_OF_PROFILE`, `UNAUTHENTICATED`.

#![cfg(feature = "headless")]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use neurobrowser::agent::policy::{ActionPolicy, AutonomyLevel};
use neurobrowser::browser::default_tool_registry;
use neurobrowser::tools::{PageSnapshot, ToolAction, ToolRegistry, ToolRisk};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
#[cfg(unix)]
use tokio::net::UnixListener;
use tokio::sync::Mutex;

/// Environment variable holding the shared secret required by the TCP fallback.
const TOKEN_ENV: &str = "NEUROBROWSER_TOKEN";

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

/// Process-wide, read-only context built once at startup and shared across all
/// connections. Deliberately holds **no** mutable per-client state.
struct Daemon {
    /// The real browser tool registry (navigate/click/type/submit_form/...).
    /// Built once and shared (read-only) via `Arc` — every connection resolves
    /// each tool's `ToolRisk` and tool-profile against this same registry.
    tool_registry: Arc<ToolRegistry>,
}

impl Daemon {
    fn new() -> Self {
        Self {
            tool_registry: Arc::new(default_tool_registry()),
        }
    }

    /// Mint a fresh [`SessionState`] for a newly accepted connection. Each one
    /// owns its own [`ActionPolicy`]; only the read-only registry is shared.
    fn new_session(&self) -> SessionState {
        SessionState::with_registry(self.tool_registry.clone())
    }
}

/// Per-**connection** state. A new one is constructed for every accepted
/// connection, so `policy.set` on one connection can never be observed by
/// another.
struct SessionState {
    /// Per-connection policy. Defaults to `Assisted` + no allow/deny lists.
    policy: Arc<Mutex<ActionPolicy>>,
    /// The real browser tool registry (shared, read-only) used to resolve each
    /// tool's actual `ToolRisk` and its tool-profile eligibility.
    tool_registry: Arc<ToolRegistry>,
}

impl SessionState {
    /// Standalone constructor (builds its own registry). Test-only: production
    /// connections are minted via [`Daemon::new_session`], which shares the
    /// process-wide read-only registry.
    #[cfg(test)]
    fn new() -> Self {
        Self::with_registry(Arc::new(default_tool_registry()))
    }

    /// Construct a session sharing the daemon's read-only registry but with a
    /// fresh, connection-scoped policy.
    fn with_registry(tool_registry: Arc<ToolRegistry>) -> Self {
        Self {
            policy: Arc::new(Mutex::new(ActionPolicy::default())),
            tool_registry,
        }
    }

    /// Is `name` visible/callable at this connection's current policy
    /// tier? A known tool is in-profile when its action is permitted by the
    /// tier; an unknown tool is allowed through (to the conservative-risk
    /// evaluation) at every tier except `ReadOnly`, where the surface is
    /// locked to the known read-only tools.
    async fn tool_in_profile(&self, name: &str) -> bool {
        let level = self.policy.lock().await.autonomy_level;
        match self.tool_registry.get(name) {
            Some(tool) => tier_allows_tool(level, tool.definition().risk.action),
            None => !matches!(level, AutonomyLevel::ReadOnly),
        }
    }

    /// The advertised tool manifest, filtered to the tools in-profile at
    /// this connection's current policy tier (denied tools omitted too).
    async fn tool_manifest(&self) -> Value {
        let (level, denied) = {
            let policy = self.policy.lock().await;
            (policy.autonomy_level, policy.denied_tools.clone())
        };
        let tools: Vec<Value> = self
            .tool_registry
            .definitions()
            .into_iter()
            .filter(|def| {
                tier_allows_tool(level, def.risk.action)
                    && !denied.iter().any(|t| t.eq_ignore_ascii_case(&def.name))
            })
            .map(|def| {
                serde_json::json!({
                    "name": def.name,
                    "description": def.description,
                    "action": format!("{:?}", def.risk.action),
                })
            })
            .collect();

        serde_json::json!({
            "tier": format!("{:?}", level),
            "tools": tools,
        })
    }

    async fn evaluate_tool_call(
        &self,
        id: &str,
        name: &str,
        args: &HashMap<String, String>,
    ) -> Response {
        let snapshot = PageSnapshot::default();

        // Registered tools use their real `ToolRisk`. Unknown names fall
        // back to Destructive so they are not treated as reads.
        let tool_risk = self
            .tool_registry
            .get(name)
            .map(|tool| tool.definition().risk)
            .unwrap_or_else(|| ToolRisk::new(ToolAction::Destructive));

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

/// Which tool actions a policy tier exposes. Mirrors the `ReadOnly`
/// allow-list in `ActionPolicy::evaluate` so the advertised surface and the
/// runtime decision agree.
fn tier_allows_tool(level: AutonomyLevel, action: ToolAction) -> bool {
    match level {
        AutonomyLevel::ReadOnly => matches!(
            action,
            ToolAction::Read | ToolAction::Wait | ToolAction::Scroll
        ),
        AutonomyLevel::Assisted | AutonomyLevel::HighAutonomy => true,
    }
}

/// Per-connection authentication gate.
///
/// * Unix connections arrive already authorized by the `SO_PEERCRED` same-uid
///   check performed at accept time ([`ConnAuth::peercred`]).
/// * TCP connections start unauthenticated and must present the shared secret
///   via an `auth` request before any other method is dispatched
///   ([`ConnAuth::token`]).
struct ConnAuth {
    authenticated: bool,
    /// `Some` only on the TCP path — the shared secret the client must present.
    expected_token: Option<Arc<String>>,
}

impl ConnAuth {
    /// Unix path: the connection is already authorized by peer credentials.
    fn peercred() -> Self {
        Self {
            authenticated: true,
            expected_token: None,
        }
    }

    /// TCP path: unauthenticated until the shared secret is presented.
    fn token(expected: Arc<String>) -> Self {
        Self {
            authenticated: false,
            expected_token: Some(expected),
        }
    }

    /// Handle an `auth` request. On the Unix path (`expected_token == None`)
    /// this is an idempotent no-op success. On the TCP path it flips
    /// `authenticated` iff the presented token matches.
    fn handle_auth(&mut self, request: &Request) -> Response {
        match &self.expected_token {
            None => {
                self.authenticated = true;
                Response::ok(
                    request.id.clone(),
                    serde_json::json!({ "authenticated": true }),
                )
            }
            Some(expected) => {
                let provided = request
                    .params
                    .get("token")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if tokens_match(provided, expected) {
                    self.authenticated = true;
                    Response::ok(
                        request.id.clone(),
                        serde_json::json!({ "authenticated": true }),
                    )
                } else {
                    Response::err(request.id.clone(), "UNAUTHENTICATED", "invalid token")
                }
            }
        }
    }
}

/// Constant-time-ish token comparison. Leaks length (acceptable for a local
/// shared secret) but compares contents without early-out to avoid a trivial
/// timing oracle.
fn tokens_match(provided: &str, expected: &str) -> bool {
    let provided = provided.as_bytes();
    let expected = expected.as_bytes();
    if provided.len() != expected.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in provided.iter().zip(expected.iter()) {
        diff |= a ^ b;
    }
    diff == 0
}

/// The daemon's own uid — the only uid authorized to talk to the Unix socket.
///
/// SAFETY: `getuid(2)` is always safe to call; it takes no arguments, never
/// fails, and has no preconditions. `libc::uid_t` is `u32` on supported targets.
#[cfg(unix)]
fn current_uid() -> u32 {
    unsafe { libc::getuid() }
}

/// SO_PEERCRED authorization predicate: a peer is authorized iff its uid is the
/// daemon's own uid.
fn uid_authorized(peer_uid: u32, daemon_uid: u32) -> bool {
    peer_uid == daemon_uid
}

/// Restrict the Unix control socket to the owning user (defense-in-depth in
/// addition to the peer-credential check).
fn restrict_socket_permissions(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(error) = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)) {
            tracing::warn!(?error, "failed to restrict control-socket permissions");
        }
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// Resolve the shared secret for the TCP fallback. Uses `NEUROBROWSER_TOKEN`
/// when set; otherwise generates one and prints it so a cooperating local
/// launcher can authenticate. The TCP path is never left unauthenticated.
fn resolve_tcp_token() -> Arc<String> {
    match std::env::var(TOKEN_ENV) {
        Ok(token) if !token.is_empty() => Arc::new(token),
        _ => {
            let token = uuid::Uuid::new_v4().to_string();
            println!("{TOKEN_ENV}={token}");
            Arc::new(token)
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
                tracing_subscriber::EnvFilter::new("neurobrowser=info,neurobrowser_headless=info")
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

    let daemon = Daemon::new();

    let listener = match UnixListener::bind(&socket_path) {
        Ok(listener) => listener,
        Err(error) => {
            tracing::warn!(?error, path = %socket_path.display(),
                "Unix socket bind failed; falling back to local TCP");
            let tcp = TcpListener::bind("127.0.0.1:0").await?;
            let local = tcp.local_addr()?;
            println!("NEUROBROWSER_LISTENING=tcp://{local}");
            let token = resolve_tcp_token();
            tokio::spawn(async move { serve_tcp(tcp, daemon, token).await });
            wait_for_signal().await;
            return Ok(());
        }
    };
    println!("NEUROBROWSER_LISTENING=unix://{}", socket_path.display());
    // Defense-in-depth: restrict the socket file to the owning user.
    // The authoritative gate is the per-connection SO_PEERCRED check in
    // `serve_unix` (macOS ignores socket-file perms on connect).
    restrict_socket_permissions(&socket_path);
    serve_unix(listener, daemon).await?;
    Ok(())
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

/// Accept loop for the Unix control socket. Every connection is authorized by
/// `SO_PEERCRED` (same-uid) **before** a handler is spawned, and each handler
/// gets its own [`SessionState`].
#[cfg(unix)]
async fn serve_unix(listener: UnixListener, daemon: Daemon) -> std::io::Result<()> {
    serve_unix_authorized(listener, daemon, current_uid()).await
}

/// Accept loop that authorizes peers against an explicit `authorized_uid`.
/// Production callers pass [`current_uid`]; tests inject a foreign uid to drive
/// the rejection path deterministically without a second OS user.
#[cfg(unix)]
async fn serve_unix_authorized(
    listener: UnixListener,
    daemon: Daemon,
    authorized_uid: u32,
) -> std::io::Result<()> {
    loop {
        let (stream, _addr) = match listener.accept().await {
            Ok(pair) => pair,
            Err(error) => {
                tracing::warn!(?error, "unix accept failed");
                continue;
            }
        };

        // SO_PEERCRED same-uid authorization — reject other users before any
        // request is read or dispatched.
        match stream.peer_cred() {
            Ok(cred) if uid_authorized(cred.uid(), authorized_uid) => {}
            Ok(cred) => {
                tracing::warn!(
                    peer_uid = cred.uid(),
                    authorized_uid,
                    "rejecting Unix connection from a different uid"
                );
                drop(stream);
                continue;
            }
            Err(error) => {
                tracing::warn!(?error, "peer-credential check failed; rejecting connection");
                drop(stream);
                continue;
            }
        }

        let session = daemon.new_session();
        tokio::spawn(async move {
            if let Err(error) = handle_connection(stream, session, ConnAuth::peercred()).await {
                tracing::warn!(?error, "connection closed");
            }
        });
    }
}

/// Accept loop for the TCP fallback. There is no peer-credential concept, so
/// each connection must complete the shared-secret `auth` handshake before any
/// other method is dispatched. Each handler gets its own [`SessionState`].
async fn serve_tcp(listener: TcpListener, daemon: Daemon, token: Arc<String>) {
    loop {
        let (stream, _) = match listener.accept().await {
            Ok(p) => p,
            Err(error) => {
                tracing::warn!(?error, "tcp accept failed");
                continue;
            }
        };
        let session = daemon.new_session();
        let token = token.clone();
        tokio::spawn(async move {
            if let Err(error) = handle_connection(stream, session, ConnAuth::token(token)).await {
                tracing::warn!(?error, "connection closed");
            }
        });
    }
}

async fn handle_connection<S>(
    stream: S,
    state: SessionState,
    mut auth: ConnAuth,
) -> std::io::Result<()>
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
                write_response(&mut write_half, &response).await?;
                continue;
            }
        };

        // Authentication gate — runs *before* any method is dispatched.
        if request.method == "auth" {
            let response = auth.handle_auth(&request);
            write_response(&mut write_half, &response).await?;
            // A failed token attempt (still unauthenticated) closes the
            // connection rather than allowing retries on the same socket.
            if !auth.authenticated {
                break;
            }
            continue;
        }
        if !auth.authenticated {
            let response = Response::err(
                request.id.clone(),
                "UNAUTHENTICATED",
                "authenticate with the `auth` method before sending requests",
            );
            write_response(&mut write_half, &response).await?;
            break;
        }

        let response = dispatch(&request, &state).await;
        write_response(&mut write_half, &response).await?;
    }
    Ok(())
}

/// Serialize and write a single JSON-line response.
async fn write_response<W>(writer: &mut W, response: &Response) -> std::io::Result<()>
where
    W: AsyncWriteExt + Unpin,
{
    let serialized =
        serde_json::to_string(response).unwrap_or_else(|_| "{\"ok\":false}".to_string());
    writer.write_all(serialized.as_bytes()).await?;
    writer.write_all(b"\n").await?;
    Ok(())
}

/// `arguments` must be a JSON object whose values are strings.
///
/// A missing key is an empty map. Null, a non-object, or a non-string value is
/// rejected. Substituting `{}` would drop `navigate`'s `url` and skip the
/// scheme and domain gates.
fn string_arguments(value: Option<&Value>) -> Result<HashMap<String, String>, &'static str> {
    let Some(value) = value else {
        return Ok(HashMap::new());
    };
    let Value::Object(map) = value else {
        return Err("arguments must be a JSON object whose values are strings");
    };
    let mut arguments = HashMap::with_capacity(map.len());
    for (key, value) in map {
        let Value::String(text) = value else {
            return Err("arguments must be a JSON object whose values are strings");
        };
        arguments.insert(key.clone(), text.clone());
    }
    Ok(arguments)
}

async fn dispatch(request: &Request, state: &SessionState) -> Response {
    match request.method.as_str() {
        "ping" => Response::ok(request.id.clone(), serde_json::json!({ "pong": true })),
        "policy.get" => {
            let policy = state.policy.lock().await;
            serde_json::to_value(&*policy)
                .map(|v| Response::ok(request.id.clone(), v))
                .unwrap_or_else(|e| Response::err(request.id.clone(), "INTERNAL", e.to_string()))
        }
        "policy.set" => {
            let mut policy = state.policy.lock().await;
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
        "tools.list" => {
            // Advertise only the tools in-profile at this connection's
            // current policy tier.
            let manifest = state.tool_manifest().await;
            Response::ok(request.id.clone(), manifest)
        }
        "policy.evaluate" => {
            let params = request.params.clone();
            let name = params
                .get("tool")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string();
            let arguments = match string_arguments(params.get("arguments")) {
                Ok(arguments) => arguments,
                Err(message) => {
                    return Response::err(request.id.clone(), "VALIDATION", message);
                }
            };

            // Reject out-of-profile tools before they reach the policy
            // engine — a lower-trust tier literally cannot exercise write tools.
            if !state.tool_in_profile(&name).await {
                Response::err(
                    request.id.clone(),
                    "OUT_OF_PROFILE",
                    format!("tool '{name}' is not available at the current policy tier"),
                )
            } else {
                state
                    .evaluate_tool_call(&request.id, &name, &arguments)
                    .await
            }
        }
        "snapshot" => {
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
    use tokio::io::{AsyncBufRead, Lines};
    use tokio::net::TcpStream;
    #[cfg(unix)]
    use tokio::net::UnixStream;

    // ---- pure predicates -------------------------------------------------

    #[test]
    fn uid_authorized_only_matches_same_uid() {
        assert!(uid_authorized(1000, 1000));
        assert!(!uid_authorized(1001, 1000));
        assert!(!uid_authorized(0, 1000));
    }

    #[test]
    fn tokens_match_is_exact_and_length_sensitive() {
        assert!(tokens_match("s3cr3t", "s3cr3t"));
        assert!(!tokens_match("s3cr3t", "s3cr3T"));
        assert!(!tokens_match("s3cr3t", "s3cr3"));
        assert!(!tokens_match("", "x"));
        assert!(tokens_match("", ""));
    }

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

    #[test]
    fn read_only_tier_hides_write_tools() {
        assert!(tier_allows_tool(AutonomyLevel::ReadOnly, ToolAction::Read));
        assert!(tier_allows_tool(AutonomyLevel::ReadOnly, ToolAction::Wait));
        assert!(tier_allows_tool(
            AutonomyLevel::ReadOnly,
            ToolAction::Scroll
        ));
        assert!(!tier_allows_tool(
            AutonomyLevel::ReadOnly,
            ToolAction::Navigate
        ));
        assert!(!tier_allows_tool(AutonomyLevel::ReadOnly, ToolAction::Type));
        assert!(!tier_allows_tool(
            AutonomyLevel::ReadOnly,
            ToolAction::Submit
        ));
        assert!(tier_allows_tool(AutonomyLevel::Assisted, ToolAction::Type));
        assert!(tier_allows_tool(
            AutonomyLevel::HighAutonomy,
            ToolAction::Destructive
        ));
    }

    // ---- test helpers ----------------------------------------------------

    #[cfg(unix)]
    fn unique_socket_path() -> PathBuf {
        std::env::temp_dir().join(format!("nb-authtest-{}.sock", uuid::Uuid::new_v4()))
    }

    async fn send<W: AsyncWriteExt + Unpin>(writer: &mut W, line: &str) {
        writer.write_all(line.as_bytes()).await.unwrap();
        writer.write_all(b"\n").await.unwrap();
    }

    async fn next_json<R: AsyncBufRead + Unpin>(lines: &mut Lines<R>) -> Option<Value> {
        lines
            .next_line()
            .await
            .unwrap()
            .map(|line| serde_json::from_str(&line).unwrap())
    }

    fn full_policy_params(autonomy: &str) -> Value {
        serde_json::json!({
            "autonomy_level": autonomy,
            "allowed_domains": [],
            "denied_domains": [],
            "denied_tools": [],
            "approval_required_tools": [],
            "block_prompt_injection": true,
        })
    }

    // ---- auth: TCP shared-secret token -----------------------------------

    #[tokio::test]
    async fn tcp_rejects_unauthenticated_request_before_dispatch() {
        let daemon = Daemon::new();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let token = Arc::new("test-token".to_string());
        tokio::spawn({
            let token = token.clone();
            async move { serve_tcp(listener, daemon, token).await }
        });

        let stream = TcpStream::connect(addr).await.unwrap();
        let (read_half, mut write_half) = stream.into_split();
        let mut lines = BufReader::new(read_half).lines();

        // A normal method before authenticating must be rejected up-front.
        send(
            &mut write_half,
            r#"{"id":"1","method":"policy.get","params":{}}"#,
        )
        .await;
        let response = next_json(&mut lines).await.expect("a response line");
        assert_eq!(response["ok"], Value::Bool(false));
        assert_eq!(response["error"]["code"], "UNAUTHENTICATED");
        // The connection is closed after rejection — no further lines.
        assert!(next_json(&mut lines).await.is_none());
    }

    #[tokio::test]
    async fn tcp_wrong_token_is_rejected_and_correct_token_authenticates() {
        let daemon = Daemon::new();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let token = Arc::new("test-token".to_string());
        tokio::spawn({
            let token = token.clone();
            async move { serve_tcp(listener, daemon, token).await }
        });

        // Wrong token → rejected, connection closed.
        {
            let stream = TcpStream::connect(addr).await.unwrap();
            let (read_half, mut write_half) = stream.into_split();
            let mut lines = BufReader::new(read_half).lines();
            send(
                &mut write_half,
                r#"{"id":"a","method":"auth","params":{"token":"nope"}}"#,
            )
            .await;
            let response = next_json(&mut lines).await.expect("a response line");
            assert_eq!(response["ok"], Value::Bool(false));
            assert_eq!(response["error"]["code"], "UNAUTHENTICATED");
            assert!(next_json(&mut lines).await.is_none());
        }

        // Correct token → authenticated, subsequent dispatch works.
        {
            let stream = TcpStream::connect(addr).await.unwrap();
            let (read_half, mut write_half) = stream.into_split();
            let mut lines = BufReader::new(read_half).lines();
            send(
                &mut write_half,
                r#"{"id":"b","method":"auth","params":{"token":"test-token"}}"#,
            )
            .await;
            let auth_response = next_json(&mut lines).await.expect("auth response");
            assert_eq!(auth_response["ok"], Value::Bool(true));
            assert_eq!(auth_response["result"]["authenticated"], Value::Bool(true));

            send(
                &mut write_half,
                r#"{"id":"c","method":"policy.get","params":{}}"#,
            )
            .await;
            let policy_response = next_json(&mut lines).await.expect("policy response");
            assert_eq!(policy_response["ok"], Value::Bool(true));
            assert_eq!(policy_response["result"]["autonomy_level"], "assisted");
        }
    }

    // ---- per-connection isolation: policy.set is connection-scoped -------

    #[cfg(unix)]
    #[tokio::test]
    async fn policy_set_on_one_connection_does_not_affect_another() {
        let daemon = Daemon::new();
        let path = unique_socket_path();
        let listener = UnixListener::bind(&path).unwrap();
        tokio::spawn({
            let daemon_path = path.clone();
            async move {
                let _ = serve_unix(listener, daemon).await;
                let _ = std::fs::remove_file(&daemon_path);
            }
        });

        // Two concurrent connections (same uid → both pass the peer-cred gate).
        let conn1 = UnixStream::connect(&path).await.unwrap();
        let (r1, mut w1) = conn1.into_split();
        let mut l1 = BufReader::new(r1).lines();
        let conn2 = UnixStream::connect(&path).await.unwrap();
        let (r2, mut w2) = conn2.into_split();
        let mut l2 = BufReader::new(r2).lines();

        // Connection 1 flips its policy to ReadOnly.
        let set = serde_json::json!({
            "id": "1",
            "method": "policy.set",
            "params": full_policy_params("read_only"),
        })
        .to_string();
        send(&mut w1, &set).await;
        let set_resp = next_json(&mut l1).await.expect("set response");
        assert_eq!(set_resp["ok"], Value::Bool(true));
        assert_eq!(set_resp["result"]["applied"], Value::Bool(true));

        // Connection 2, opened independently, is unaffected — still Assisted.
        send(&mut w2, r#"{"id":"2","method":"policy.get","params":{}}"#).await;
        let get_resp = next_json(&mut l2).await.expect("get response");
        assert_eq!(get_resp["ok"], Value::Bool(true));
        assert_eq!(
            get_resp["result"]["autonomy_level"], "assisted",
            "policy.set leaked across connections: {get_resp}"
        );

        // Sanity: connection 1 does observe its own change.
        send(&mut w1, r#"{"id":"3","method":"policy.get","params":{}}"#).await;
        let get1 = next_json(&mut l1).await.expect("get response");
        assert_eq!(get1["result"]["autonomy_level"], "read_only");

        let _ = std::fs::remove_file(&path);
    }

    // ---- auth: Unix SO_PEERCRED same-uid ---------------------------------

    #[cfg(unix)]
    #[tokio::test]
    async fn unix_rejects_mismatched_uid_before_dispatch() {
        // Drive the real accept -> peer_cred -> reject path end-to-end by
        // authorizing a uid this process does NOT have: every real connection
        // carries our uid, so all are rejected before any dispatch.
        let daemon = Daemon::new();
        let path = unique_socket_path();
        let listener = UnixListener::bind(&path).unwrap();
        let foreign_uid = current_uid().wrapping_add(1);
        tokio::spawn({
            let path = path.clone();
            async move {
                let _ = serve_unix_authorized(listener, daemon, foreign_uid).await;
                let _ = std::fs::remove_file(&path);
            }
        });

        let conn = UnixStream::connect(&path).await.unwrap();
        let (read_half, mut write_half) = conn.into_split();
        let mut lines = BufReader::new(read_half).lines();
        // Best-effort send — the server should already be dropping us. The
        // write may or may not land before the RST; either way there must be
        // no response.
        let _ = write_half
            .write_all(br#"{"id":"1","method":"ping","params":{}}"#)
            .await;
        let _ = write_half.write_all(b"\n").await;
        // Dropping the socket with unread bytes is a reset, not a clean EOF.
        // Either outcome means the peer was rejected before a response.
        match lines.next_line().await {
            Ok(None) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::BrokenPipe
                        | std::io::ErrorKind::ConnectionAborted
                        | std::io::ErrorKind::UnexpectedEof
                ) => {}
            Ok(Some(line)) => {
                panic!("mismatched-uid connection was not rejected before dispatch: {line}")
            }
            Err(error) => panic!("unexpected read error while expecting rejection: {error}"),
        }

        let _ = std::fs::remove_file(&path);
    }

    // ---- tool-profile scoping --------------------------------------------

    #[tokio::test]
    async fn tool_profile_tracks_the_policy_tier() {
        let state = SessionState::new();

        // Default Assisted: the full surface is in-profile.
        assert!(state.tool_in_profile("type").await);
        assert!(state.tool_in_profile("get_text").await);

        // Drop to ReadOnly: write tools disappear, read tools remain.
        {
            let mut policy = state.policy.lock().await;
            policy.autonomy_level = AutonomyLevel::ReadOnly;
        }
        assert!(!state.tool_in_profile("type").await);
        assert!(!state.tool_in_profile("submit_form").await);
        assert!(state.tool_in_profile("get_text").await);
        // Unknown tools are locked out entirely at ReadOnly.
        assert!(!state.tool_in_profile("totally_unregistered_tool").await);

        // The advertised manifest is filtered the same way.
        let manifest = state.tool_manifest().await;
        let names: Vec<String> = manifest["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect();
        assert!(names.iter().any(|n| n == "get_text"));
        assert!(!names.iter().any(|n| n == "type"));
        assert!(!names.iter().any(|n| n == "submit_form"));
    }

    #[tokio::test]
    async fn dispatch_rejects_out_of_profile_tool_before_evaluation() {
        let state = SessionState::new();
        {
            let mut policy = state.policy.lock().await;
            policy.autonomy_level = AutonomyLevel::ReadOnly;
        }
        let request = Request {
            id: "x".to_string(),
            method: "policy.evaluate".to_string(),
            params: serde_json::json!({ "tool": "type", "arguments": { "selector": "#i", "text": "hi" } }),
        };
        let response = dispatch(&request, &state).await;
        assert!(!response.ok);
        assert_eq!(response.error.unwrap().code, "OUT_OF_PROFILE");
    }

    #[tokio::test]
    async fn evaluate_tool_call_requires_approval_for_high_risk_tool() {
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

    fn evaluate_request(id: &str, params: Value) -> Request {
        Request {
            id: id.to_string(),
            method: "policy.evaluate".to_string(),
            params,
        }
    }

    #[tokio::test]
    async fn policy_evaluate_rejects_arguments_that_are_not_a_string_map() {
        let state = SessionState::new();
        let cases = [
            serde_json::json!({ "tool": "navigate", "arguments": { "url": 1 } }),
            serde_json::json!({ "tool": "navigate", "arguments": { "url": ["https://evil.example"] } }),
            serde_json::json!({ "tool": "navigate", "arguments": { "url": { "href": "javascript:alert(1)" } } }),
            serde_json::json!({ "tool": "navigate", "arguments": { "url": true } }),
            serde_json::json!({ "tool": "navigate", "arguments": { "url": null } }),
            serde_json::json!({ "tool": "navigate", "arguments": { "url": "https://example.com", "n": 2 } }),
            serde_json::json!({ "tool": "navigate", "arguments": 1 }),
            serde_json::json!({ "tool": "navigate", "arguments": ["https://evil.example"] }),
            serde_json::json!({ "tool": "navigate", "arguments": "https://evil.example" }),
            serde_json::json!({ "tool": "navigate", "arguments": null }),
        ];

        for params in cases {
            let response = dispatch(&evaluate_request("bad", params.clone()), &state).await;
            assert!(!response.ok, "{params}");
            assert!(response.result.is_none(), "{params}");
            let error = response.error.expect("validation error");
            assert_eq!(error.code, "VALIDATION", "{params}: {error:?}");
            assert!(
                error
                    .message
                    .contains("JSON object whose values are strings"),
                "{params}: {}",
                error.message
            );
        }
    }

    #[tokio::test]
    async fn policy_evaluate_keeps_string_argument_values() {
        let state = SessionState::new();

        let blocked = dispatch(
            &evaluate_request(
                "js",
                serde_json::json!({
                    "tool": "navigate",
                    "arguments": { "url": "javascript:alert(1)" }
                }),
            ),
            &state,
        )
        .await;
        assert!(blocked.ok, "{blocked:?}");
        assert_eq!(blocked.result.unwrap()["outcome"], "Block");

        let allowed = dispatch(
            &evaluate_request(
                "read",
                serde_json::json!({
                    "tool": "get_text",
                    "arguments": { "selector": "h1" }
                }),
            ),
            &state,
        )
        .await;
        assert!(allowed.ok, "{allowed:?}");
        assert_eq!(allowed.result.unwrap()["outcome"], "Allow");

        let omitted = dispatch(
            &evaluate_request("omit", serde_json::json!({ "tool": "get_text" })),
            &state,
        )
        .await;
        assert!(omitted.ok, "{omitted:?}");
        assert_eq!(omitted.result.unwrap()["outcome"], "Allow");
    }
}
