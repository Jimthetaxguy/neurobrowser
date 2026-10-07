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
//! ## Authorization (SG2 → full authz)
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
//! set (NB-12): `tools.list` advertises only the in-profile tools and
//! `policy.evaluate` rejects out-of-profile calls before they reach the policy
//! engine.
//!
//! Wire format (newline-delimited JSON over the socket):
//!
//! ```json
//! // (TCP only) authenticate first
//! { "id": "uuid", "method": "auth", "params": { "token": "..." } }
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
use std::path::{Path, PathBuf};
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
/// another. (Previously a single process-wide instance was cloned into every
/// handler, and the `Arc<Mutex<ActionPolicy>>` clone shared one policy across
/// all clients — the authorization gap this module now closes.)
struct SessionState {
    /// Per-connection policy. Defaults to `Assisted` + no allow/deny lists.
    policy: Arc<Mutex<ActionPolicy>>,
    /// Optional provider for the `ask` method. Not used in v0.1 of the
    /// daemon beyond echo-style validation.
    #[allow(dead_code)]
    agent: Arc<Mutex<Option<Arc<ReActAgent>>>>,
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
            agent: Arc::new(Mutex::new(None)),
            tool_registry,
        }
    }

    /// NB-12: is `name` visible/callable at this connection's current policy
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

    /// NB-12: the advertised tool manifest, filtered to the tools in-profile at
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
                    "level": format!("{:?}", def.risk.level),
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

/// NB-12: which tool actions a policy tier exposes. Mirrors the `ReadOnly`
/// allow-list in `ActionPolicy::evaluate` so the advertised surface and the
/// runtime decision agree.
fn tier_allows_tool(level: AutonomyLevel, action: ToolAction) -> bool {
    match level {
        AutonomyLevel::ReadOnly => matches!(
            action,
            ToolAction::Read | ToolAction::Wait | ToolAction::Scroll | ToolAction::Navigate
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

    let daemon = Daemon::new();

    match UnixListener::bind(&socket_path) {
        Ok(listener) => {
            println!("NEUROBROWSER_LISTENING=unix://{}", socket_path.display());
            // Defense-in-depth: restrict the socket file to the owning user.
            // The authoritative gate is the per-connection SO_PEERCRED check in
            // `serve_unix` (macOS ignores socket-file perms on connect).
            restrict_socket_permissions(&socket_path);
            serve_unix(listener, daemon).await?;
            Ok(())
        }
        Err(error) => {
            tracing::warn!(?error, path = %socket_path.display(),
                "Unix socket bind failed; falling back to local TCP");
            let tcp = TcpListener::bind("127.0.0.1:0").await?;
            let local = tcp.local_addr()?;
            println!("NEUROBROWSER_LISTENING=tcp://{local}");
            let token = resolve_tcp_token();
            tokio::spawn(async move { serve_tcp(tcp, daemon, token).await });
            wait_for_signal().await;
            Ok(())
        }
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

/// Accept loop for the Unix control socket. Every connection is authorized by
/// `SO_PEERCRED` (same-uid) **before** a handler is spawned, and each handler
/// gets its own [`SessionState`].
async fn serve_unix(listener: UnixListener, daemon: Daemon) -> std::io::Result<()> {
    serve_unix_authorized(listener, daemon, current_uid()).await
}

/// Accept loop that authorizes peers against an explicit `authorized_uid`.
/// Production callers pass [`current_uid`]; tests inject a foreign uid to drive
/// the rejection path deterministically without a second OS user.
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
            // NB-12: advertise only the tools in-profile at this connection's
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
            let arguments_value = params.get("arguments").cloned().unwrap_or(Value::Null);
            let arguments: HashMap<String, String> =
                serde_json::from_value(arguments_value).unwrap_or_default();

            // NB-12: reject out-of-profile tools before they reach the policy
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
            let policy = state.policy.lock().await;
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
    use tokio::io::{AsyncBufRead, Lines};
    use tokio::net::{TcpStream, UnixStream};

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

    #[test]
    fn read_only_tier_hides_write_tools() {
        assert!(tier_allows_tool(AutonomyLevel::ReadOnly, ToolAction::Read));
        assert!(tier_allows_tool(
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
        assert!(
            next_json(&mut lines).await.is_none(),
            "mismatched-uid connection was not rejected before dispatch"
        );

        let _ = std::fs::remove_file(&path);
    }

    // ---- NB-12: tool-profile scoping -------------------------------------

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

    // ---- existing behaviour (unchanged) ----------------------------------

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
