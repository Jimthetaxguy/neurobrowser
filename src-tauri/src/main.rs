mod runtime;

use neuro_memory::{
    now_millis, CaptureDecision, CapturePolicy, CapturedPage, MemoryService, SearchExplain,
    SearchRequest, SearchResult,
};
use neurobrowser::{
    ActionPolicy, AgentConfig, AgentRunEvent, AgentRunResult, AgentRunStatus, BrowserInterface,
    PageSnapshot, PolicyDecision, ProviderConfig, ProviderType, SessionManager, ToolCall,
};
use runtime::{
    close_runtime_page, create_runtime_page, set_active_runtime_page, sync_runtime_viewport,
    BrowserRuntimeRegistry, BrowserViewport, RuntimeReportPayload, TauriBrowserRuntime,
};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use tauri::{AppHandle, Manager, State, WebviewWindow};

struct AppState {
    session_manager: SessionManager,
    runtimes: Arc<BrowserRuntimeRegistry>,
    /// Persistent page memory (`neuro_memory::MemoryService`), not in-run
    /// `agent::memory::AgentMemory`. Opened at `app_data_dir()/memory/`.
    memory: Arc<MemoryService>,
    action_policy: Mutex<ActionPolicy>,
    /// Caller-owned capture rules. `MemoryService::forget` tombstones a host
    /// here for this process. The service does not store the policy itself.
    capture_policy: tokio::sync::Mutex<CapturePolicy>,
    pending_approvals: Mutex<HashMap<String, PendingApproval>>,
}

struct PendingApproval {
    session_id: String,
    page_id: usize,
    tool_call: ToolCall,
    approval_id: String,
}

#[derive(Serialize)]
struct SnapshotResponse {
    url: String,
    title: String,
    link_count: usize,
    image_count: usize,
    form_count: usize,
    price_count: usize,
    table_count: usize,
}

#[derive(Serialize)]
struct AgentRunResponse {
    run_id: String,
    status: AgentRunStatus,
    final_response: Option<String>,
    events: Vec<AgentRunEventResponse>,
}

#[derive(Serialize)]
struct PolicyDecisionResponse {
    reasons: Vec<String>,
    redacted_arguments: HashMap<String, String>,
}

#[derive(Serialize)]
#[serde(tag = "type")]
enum AgentRunEventResponse {
    ToolCallStarted {
        tool: String,
    },
    ToolCallResult {
        tool: String,
        success: bool,
    },
    ToolCallBlocked {
        tool: String,
        decision: PolicyDecisionResponse,
    },
    ApprovalRequested {
        tool: String,
        decision: PolicyDecisionResponse,
    },
    ApprovalResolved {
        approved: bool,
    },
    RunCancelled,
    RunDone,
}

#[derive(Serialize)]
struct ProviderSelectionResult {
    provider: String,
    model: String,
    configured: bool,
}

fn provider_type_from_slug(provider: &str) -> Result<ProviderType, String> {
    match provider.trim().to_lowercase().as_str() {
        "openai" => Ok(ProviderType::Openai),
        "anthropic" => Ok(ProviderType::Anthropic),
        "ollama" => Ok(ProviderType::Ollama),
        "custom" => Ok(ProviderType::Custom),
        _ => Err(format!("Unknown provider: {provider}")),
    }
}

fn provider_slug(provider_type: &ProviderType) -> &'static str {
    match provider_type {
        ProviderType::Openai => "openai",
        ProviderType::Anthropic => "anthropic",
        ProviderType::Ollama => "ollama",
        ProviderType::Custom => "custom",
    }
}

fn provider_config_for(provider_type: ProviderType) -> ProviderConfig {
    match provider_type {
        ProviderType::Openai => ProviderConfig {
            provider_type: ProviderType::Openai,
            api_key: std::env::var("OPENAI_API_KEY").ok(),
            base_url: None,
            model: std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o".to_string()),
            max_tokens: Some(4096),
            temperature: Some(0.3),
        },
        ProviderType::Anthropic => ProviderConfig {
            provider_type: ProviderType::Anthropic,
            api_key: std::env::var("ANTHROPIC_API_KEY").ok(),
            base_url: None,
            model: std::env::var("ANTHROPIC_MODEL")
                .unwrap_or_else(|_| "claude-sonnet-5".to_string()),
            max_tokens: Some(4096),
            // Current Claude models (including the default above) reject an
            // explicit `temperature` with a 400 ("temperature is deprecated
            // for this model"). Leave unset by default; `build_request_body`
            // only sends the field when it is `Some`, so a caller who points
            // ANTHROPIC_MODEL at an older model that still accepts it can
            // opt back in without a code change here.
            temperature: None,
        },
        ProviderType::Ollama => ProviderConfig {
            provider_type: ProviderType::Ollama,
            api_key: None,
            base_url: std::env::var("OLLAMA_BASE_URL").ok(),
            model: std::env::var("OLLAMA_MODEL").unwrap_or_else(|_| "llama3.2".to_string()),
            max_tokens: Some(4096),
            temperature: Some(0.3),
        },
        ProviderType::Custom => ProviderConfig {
            provider_type: ProviderType::Custom,
            api_key: std::env::var("CUSTOM_PROVIDER_API_KEY")
                .ok()
                .or_else(|| std::env::var("OPENAI_API_KEY").ok()),
            base_url: std::env::var("CUSTOM_PROVIDER_BASE_URL").ok(),
            model: std::env::var("CUSTOM_PROVIDER_MODEL").unwrap_or_else(|_| "gpt-4o".to_string()),
            max_tokens: Some(4096),
            temperature: Some(0.3),
        },
    }
}

fn browser_for_page(
    app: AppHandle,
    state: &AppState,
    session_id: &str,
    page_id: usize,
) -> Result<(neurobrowser::PageHandle, TauriBrowserRuntime), String> {
    let page = state.session_manager.get_page(session_id, page_id)?;
    let browser = TauriBrowserRuntime::new(
        app,
        page.id,
        page.runtime_id.clone(),
        state.runtimes.clone(),
    );
    Ok((page, browser))
}

fn snapshot_response(snapshot: PageSnapshot) -> SnapshotResponse {
    SnapshotResponse {
        url: snapshot.url,
        title: snapshot.title,
        link_count: snapshot.links.len(),
        image_count: snapshot.images.len(),
        form_count: snapshot.forms.len(),
        price_count: snapshot.prices.len(),
        table_count: snapshot.tables.len(),
    }
}

fn policy_decision_response(decision: PolicyDecision) -> PolicyDecisionResponse {
    PolicyDecisionResponse {
        reasons: decision.reasons,
        redacted_arguments: decision.redacted_arguments,
    }
}

fn agent_run_event_response(event: AgentRunEvent) -> AgentRunEventResponse {
    match event {
        AgentRunEvent::ToolCallStarted { tool, .. } => {
            AgentRunEventResponse::ToolCallStarted { tool }
        }
        AgentRunEvent::ToolCallResult { tool, success, .. } => {
            AgentRunEventResponse::ToolCallResult { tool, success }
        }
        AgentRunEvent::ToolCallBlocked { tool, decision, .. } => {
            AgentRunEventResponse::ToolCallBlocked {
                tool,
                decision: policy_decision_response(decision),
            }
        }
        AgentRunEvent::ApprovalRequested { tool, decision, .. } => {
            AgentRunEventResponse::ApprovalRequested {
                tool,
                decision: policy_decision_response(decision),
            }
        }
        AgentRunEvent::ApprovalResolved { approved, .. } => {
            AgentRunEventResponse::ApprovalResolved { approved }
        }
        AgentRunEvent::RunCancelled { .. } => AgentRunEventResponse::RunCancelled,
        AgentRunEvent::RunDone { .. } => AgentRunEventResponse::RunDone,
    }
}

fn agent_run_response(result: AgentRunResult) -> AgentRunResponse {
    AgentRunResponse {
        run_id: result.run_id,
        status: result.status,
        final_response: result.final_response,
        events: result
            .events
            .into_iter()
            .map(agent_run_event_response)
            .collect(),
    }
}

#[tauri::command]
fn create_session(state: State<'_, AppState>) -> Result<String, String> {
    Ok(state.session_manager.create_session())
}

#[tauri::command]
async fn create_page(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<usize, String> {
    let page = state.session_manager.create_page(&session_id)?;
    let result = (|| -> Result<(), String> {
        let host_window = app
            .get_window(window.label())
            .ok_or_else(|| "Host window not found".to_string())?;
        create_runtime_page(
            &host_window,
            state.runtimes.clone(),
            page.id,
            &page.runtime_id,
        )?;
        set_active_runtime_page(&app, state.runtimes.as_ref(), page.id)?;
        Ok(())
    })();

    if let Err(error) = result {
        let _ = close_runtime_page(&app, state.runtimes.as_ref(), page.id);
        let _ = state.session_manager.close_page(&session_id, page.id);
        return Err(error);
    }

    Ok(page.id)
}

#[tauri::command]
fn set_active_page(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    page_id: usize,
) -> Result<(), String> {
    state
        .session_manager
        .set_active_page(&session_id, page_id)?;
    set_active_runtime_page(&app, state.runtimes.as_ref(), page_id)
}

#[tauri::command]
fn sync_browser_viewport(
    app: AppHandle,
    state: State<'_, AppState>,
    page_id: usize,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Result<(), String> {
    sync_runtime_viewport(
        &app,
        state.runtimes.as_ref(),
        page_id,
        BrowserViewport {
            x,
            y,
            width,
            height,
        },
    )
}

#[tauri::command]
async fn navigate(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    page_id: usize,
    url: String,
) -> Result<(), String> {
    // Host-side scheme/format + netguard check. UI does not preflight.
    let normalized_url = normalize_and_guard_url(url)?;
    let (_, browser) = browser_for_page(app, state.inner(), &session_id, page_id)?;
    browser.navigate(&normalized_url).await?;
    // Server-driven capture. Policy deny and capture errors stay off the
    // navigation result; the page load already succeeded.
    capture_after_navigate(state.inner(), &browser).await;
    Ok(())
}

#[tauri::command]
async fn get_page_snapshot(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    page_id: usize,
) -> Result<SnapshotResponse, String> {
    let (_, browser) = browser_for_page(app, state.inner(), &session_id, page_id)?;
    let snapshot = browser.snapshot().await?;
    Ok(snapshot_response(snapshot))
}

fn remember_pending_approval(
    state: &AppState,
    session_id: String,
    page_id: usize,
    result: &AgentRunResult,
) -> Result<(), String> {
    if result.status != AgentRunStatus::AwaitingApproval {
        return Ok(());
    }
    if let (Some(tool_call), Some(approval_id)) =
        (result.pending_tool_call.clone(), result.approval_id.clone())
    {
        state
            .pending_approvals
            .lock()
            .map_err(|e| e.to_string())?
            .insert(
                result.run_id.clone(),
                PendingApproval {
                    session_id,
                    page_id,
                    tool_call,
                    approval_id,
                },
            );
    }
    Ok(())
}

async fn execute_agent_run(
    app: AppHandle,
    state: &AppState,
    session_id: String,
    page_id: usize,
    prompt: &str,
) -> Result<AgentRunResult, String> {
    let (page, browser) = browser_for_page(app, state, &session_id, page_id)?;
    let policy = state
        .action_policy
        .lock()
        .map(|policy| policy.clone())
        .map_err(|e| e.to_string())?;
    let result = page
        .agent
        .execute_with_policy(prompt, &browser, &policy)
        .await?;
    remember_pending_approval(state, session_id, page_id, &result)?;
    Ok(result)
}

#[tauri::command]
fn get_action_policy(state: State<'_, AppState>) -> Result<ActionPolicy, String> {
    state
        .action_policy
        .lock()
        .map(|policy| policy.clone())
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn set_action_policy(
    state: State<'_, AppState>,
    policy: ActionPolicy,
) -> Result<ActionPolicy, String> {
    let mut current = state.action_policy.lock().map_err(|e| e.to_string())?;
    *current = policy.clone();
    Ok(policy)
}

#[tauri::command]
async fn start_agent_run(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    page_id: usize,
    prompt: String,
) -> Result<AgentRunResponse, String> {
    execute_agent_run(app, state.inner(), session_id, page_id, &prompt)
        .await
        .map(agent_run_response)
}

#[tauri::command]
async fn submit_approval(
    app: AppHandle,
    state: State<'_, AppState>,
    run_id: String,
    approved: bool,
) -> Result<AgentRunResponse, String> {
    let pending = state
        .pending_approvals
        .lock()
        .map_err(|e| e.to_string())?
        .remove(&run_id)
        .ok_or_else(|| format!("No pending approval for run {run_id}"))?;
    let (page, browser) =
        browser_for_page(app, state.inner(), &pending.session_id, pending.page_id)?;
    page.agent
        .execute_approved_tool(
            run_id,
            pending.approval_id,
            pending.tool_call,
            &browser,
            approved,
            None,
        )
        .await
        .map(agent_run_response)
}

#[tauri::command]
fn cancel_agent_run(state: State<'_, AppState>, run_id: String) -> Result<AgentRunResult, String> {
    let removed = state
        .pending_approvals
        .lock()
        .map_err(|e| e.to_string())?
        .remove(&run_id);
    let reason = if removed.is_some() {
        "Run cancelled by user"
    } else {
        "Run was not pending approval"
    }
    .to_string();
    Ok(AgentRunResult {
        run_id: run_id.clone(),
        status: AgentRunStatus::Cancelled,
        final_response: Some(reason.clone()),
        iterations: 0,
        events: vec![AgentRunEvent::RunCancelled { run_id, reason }],
        pending_tool_call: None,
        approval_id: None,
    })
}

#[tauri::command]
fn close_page(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    page_id: usize,
) -> Result<(), String> {
    state.session_manager.close_page(&session_id, page_id)?;
    close_runtime_page(&app, state.runtimes.as_ref(), page_id)
}

#[tauri::command]
async fn browser_reload(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    page_id: usize,
) -> Result<(), String> {
    let (_, browser) = browser_for_page(app, state.inner(), &session_id, page_id)?;
    browser.browser_reload().await
}

#[tauri::command]
async fn browser_back(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    page_id: usize,
) -> Result<(), String> {
    let (_, browser) = browser_for_page(app, state.inner(), &session_id, page_id)?;
    browser.browser_back().await
}

#[tauri::command]
async fn browser_forward(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    page_id: usize,
) -> Result<(), String> {
    let (_, browser) = browser_for_page(app, state.inner(), &session_id, page_id)?;
    browser.browser_forward().await
}

#[tauri::command]
fn browser_runtime_report(
    state: State<'_, AppState>,
    payload: RuntimeReportPayload,
) -> Result<(), String> {
    state.runtimes.page_runtime_id(payload.page_id)?;
    state
        .runtimes
        .resolve_request(&payload.request_id, payload.payload, payload.error)
}

fn normalize_and_guard_url(url: String) -> Result<String, String> {
    let normalized = if url.starts_with("http://") || url.starts_with("https://") {
        url
    } else if url.contains('.') && !url.contains(' ') {
        format!("https://{url}")
    } else {
        return Err("Invalid URL format".to_string());
    };

    if let Some(reason) = neurobrowser::netguard::blocked_reason(&normalized) {
        return Err(reason.to_string());
    }

    Ok(normalized)
}

const DEFAULT_MEMORY_SEARCH_LIMIT: usize = 8;

enum CaptureSkip {
    Denied(String),
    Failed(String),
}

impl std::fmt::Display for CaptureSkip {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Denied(reason) => write!(formatter, "capture denied: {reason}"),
            Self::Failed(message) => formatter.write_str(message),
        }
    }
}

fn captured_page_from_snapshot(snapshot: PageSnapshot) -> Result<CapturedPage, String> {
    Ok(CapturedPage {
        url: snapshot
            .url
            .parse()
            .map_err(|err| format!("invalid page url: {err}"))?,
        title: snapshot.title,
        html: snapshot.html.unwrap_or_default(),
        text: snapshot.text.unwrap_or_default(),
        content_hash: String::new(),
        captured_at: now_millis(),
    })
}

/// Store `snapshot` when [`CapturePolicy::evaluate`] allows its URL.
///
/// The policy lock is held across the write so a concurrent [`forget_memory`]
/// cannot tombstone the host and then lose the race to this put.
async fn capture_snapshot(state: &AppState, snapshot: PageSnapshot) -> Result<(), CaptureSkip> {
    let policy = state.capture_policy.lock().await;
    if let CaptureDecision::Deny { reason } = policy.evaluate(&snapshot.url) {
        return Err(CaptureSkip::Denied(reason));
    }
    let page = captured_page_from_snapshot(snapshot).map_err(CaptureSkip::Failed)?;
    state
        .memory
        .capture(page, &policy)
        .await
        .map_err(|err| CaptureSkip::Failed(err.to_string()))
}

async fn capture_after_navigate(state: &AppState, browser: &TauriBrowserRuntime) {
    let snapshot = match browser.snapshot().await {
        Ok(snapshot) => snapshot,
        Err(err) => {
            tracing::warn!("memory capture skipped; snapshot failed: {err}");
            return;
        }
    };
    match capture_snapshot(state, snapshot).await {
        Ok(()) => {}
        Err(err @ CaptureSkip::Denied(_)) => {
            tracing::info!("memory capture skipped: {err}");
        }
        Err(err) => {
            tracing::warn!("memory capture after navigate failed: {err}");
        }
    }
}

fn open_memory(app: &tauri::App) -> Result<Arc<MemoryService>, String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|err| err.to_string())?
        .join("memory");
    MemoryService::open(&data_dir)
        .map(Arc::new)
        .map_err(|err| err.to_string())
}

#[tauri::command]
async fn search_local_memory(
    state: State<'_, AppState>,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<SearchResult>, String> {
    let request = SearchRequest {
        query,
        limit: limit.unwrap_or(DEFAULT_MEMORY_SEARCH_LIMIT),
    };
    state
        .memory
        .search(request)
        .await
        .map_err(|err| err.to_string())
}

#[tauri::command]
async fn explain_memory_result(
    state: State<'_, AppState>,
    query: String,
    result: SearchResult,
    limit: Option<usize>,
) -> Result<SearchExplain, String> {
    let request = SearchRequest {
        query,
        limit: limit.unwrap_or(DEFAULT_MEMORY_SEARCH_LIMIT),
    };
    state
        .memory
        .explain(&request, &result)
        .await
        .map_err(|err| err.to_string())
}

#[tauri::command]
async fn forget_memory(state: State<'_, AppState>, page_url: String) -> Result<(), String> {
    let page_url: tauri::Url = page_url
        .parse()
        .map_err(|err| format!("invalid page url: {err}"))?;
    let mut policy = state.capture_policy.lock().await;
    state
        .memory
        .forget(&page_url, &mut policy)
        .await
        .map_err(|err| err.to_string())
}

#[tauri::command]
fn set_provider(
    state: State<'_, AppState>,
    provider: String,
) -> Result<ProviderSelectionResult, String> {
    let provider_type = provider_type_from_slug(&provider)?;
    let provider_config = provider_config_for(provider_type);
    let configured = matches!(provider_config.provider_type, ProviderType::Ollama)
        || provider_config.api_key.is_some();

    state
        .session_manager
        .set_provider_config(provider_config.clone())?;

    Ok(ProviderSelectionResult {
        provider: provider_slug(&provider_config.provider_type).to_string(),
        model: provider_config.model,
        configured,
    })
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("neurobrowser=info")),
        )
        .init();

    let agent_config = AgentConfig {
        max_iterations: 5,
        provider_config: provider_config_for(ProviderType::Openai),
    };

    let session_manager = SessionManager::new(agent_config);
    let runtimes = Arc::new(BrowserRuntimeRegistry::default());

    tauri::Builder::default()
        .setup(move |app| {
            let memory = open_memory(app).map_err(|err| -> Box<dyn std::error::Error> {
                Box::new(std::io::Error::other(err))
            })?;
            tracing::info!("memory data dir: {}", memory.data_dir().display());
            app.manage(AppState {
                session_manager,
                runtimes,
                memory,
                action_policy: Mutex::new(ActionPolicy::default()),
                capture_policy: tokio::sync::Mutex::new(CapturePolicy::default()),
                pending_approvals: Mutex::new(HashMap::new()),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            browser_back,
            browser_forward,
            browser_reload,
            browser_runtime_report,
            cancel_agent_run,
            close_page,
            create_page,
            create_session,
            explain_memory_result,
            forget_memory,
            get_action_policy,
            get_page_snapshot,
            navigate,
            search_local_memory,
            set_active_page,
            set_action_policy,
            set_provider,
            start_agent_run,
            submit_approval,
            sync_browser_viewport,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::{agent_run_response, AgentRunEvent, AgentRunResult, AgentRunStatus, ToolCall};
    use neurobrowser::{PolicyDecision, PolicyOutcome, RiskFlag};
    use std::collections::HashMap;

    #[test]
    fn agent_run_response_drops_unread_bodies() {
        let mut raw_arguments = HashMap::new();
        raw_arguments.insert("password".into(), "secret".into());
        raw_arguments.insert("selector".into(), "#pw".into());
        let mut redacted = HashMap::new();
        redacted.insert("password".into(), "[REDACTED]".into());
        redacted.insert("selector".into(), "#pw".into());
        let decision = PolicyDecision {
            outcome: PolicyOutcome::RequireApproval,
            reasons: vec!["Tool call contains sensitive input".into()],
            risk_flags: vec![RiskFlag::SensitiveArgument],
            redacted_arguments: redacted,
        };
        let result = AgentRunResult {
            run_id: "run-1".into(),
            status: AgentRunStatus::AwaitingApproval,
            final_response: Some("Approval required".into()),
            iterations: 3,
            events: vec![
                AgentRunEvent::ToolCallStarted {
                    run_id: "run-1".into(),
                    tool: "type_text".into(),
                    arguments: raw_arguments.clone(),
                },
                AgentRunEvent::ToolCallResult {
                    run_id: "run-1".into(),
                    tool: "get_text".into(),
                    result: "page body".into(),
                    success: true,
                },
                AgentRunEvent::ApprovalRequested {
                    run_id: "run-1".into(),
                    approval_id: "appr-1".into(),
                    tool: "type_text".into(),
                    decision,
                },
                AgentRunEvent::ApprovalResolved {
                    run_id: "run-1".into(),
                    approval_id: "appr-1".into(),
                    approved: false,
                    message: "unused".into(),
                },
                AgentRunEvent::RunDone {
                    run_id: "run-1".into(),
                    final_response: "done body".into(),
                    iterations: 3,
                },
                AgentRunEvent::RunCancelled {
                    run_id: "run-1".into(),
                    reason: "Approval denied".into(),
                },
            ],
            pending_tool_call: Some(ToolCall {
                name: "type_text".into(),
                arguments: raw_arguments,
            }),
            approval_id: Some("appr-1".into()),
        };

        let json = serde_json::to_value(agent_run_response(result)).expect("serialize");
        assert_eq!(json["run_id"], "run-1");
        assert_eq!(json["status"], "awaiting_approval");
        assert_eq!(json["final_response"], "Approval required");
        assert!(json.get("pending_tool_call").is_none());
        assert!(json.get("approval_id").is_none());
        assert!(json.get("iterations").is_none());
        assert!(!json.to_string().contains("secret"));
        assert!(!json.to_string().contains("page body"));
        assert!(!json.to_string().contains("done body"));

        let events = json["events"].as_array().expect("events");
        assert_eq!(events[0]["type"], "ToolCallStarted");
        assert_eq!(events[0]["tool"], "type_text");
        assert!(events[0].get("arguments").is_none());
        assert!(events[0].get("run_id").is_none());

        assert_eq!(events[1]["type"], "ToolCallResult");
        assert_eq!(events[1]["tool"], "get_text");
        assert_eq!(events[1]["success"], true);
        assert!(events[1].get("result").is_none());

        assert_eq!(events[2]["type"], "ApprovalRequested");
        assert_eq!(events[2]["tool"], "type_text");
        assert_eq!(
            events[2]["decision"]["reasons"][0],
            "Tool call contains sensitive input"
        );
        assert_eq!(
            events[2]["decision"]["redacted_arguments"]["password"],
            "[REDACTED]"
        );
        assert_eq!(
            events[2]["decision"]["redacted_arguments"]["selector"],
            "#pw"
        );
        assert!(events[2]["decision"].get("outcome").is_none());
        assert!(events[2]["decision"].get("risk_flags").is_none());
        assert!(events[2].get("approval_id").is_none());

        assert_eq!(events[3]["type"], "ApprovalResolved");
        assert_eq!(events[3]["approved"], false);
        assert!(events[3].get("message").is_none());
        assert!(events[3].get("approval_id").is_none());

        assert_eq!(events[4], serde_json::json!({ "type": "RunDone" }));
        assert_eq!(events[5], serde_json::json!({ "type": "RunCancelled" }));
    }
}
