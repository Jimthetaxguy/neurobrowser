mod downloads;
mod runtime;

use downloads::DownloadBook;
use neuro_memory::{
    now_millis, CaptureDecision, CapturePolicy, CapturedPage, MemoryService, SearchExplain,
    SearchRequest, SearchResult,
};
use neurobrowser::{
    should_record_visit, ActionPolicy, ActionReceipt, AgentConfig, AgentRunEvent, AgentRunResult,
    AgentRunStatus, ApprovalContext, BrowserInterface, HistoryEntry, HistoryStore,
    ObservationLimits, PageObservation, PageSnapshot, PolicyDecision, ProviderConfig, ProviderType,
    ReActAgent, SessionManager, ToolCall,
};
use runtime::{
    blocked_popup_payload, close_runtime_page, create_runtime_page, emit_page_new_window,
    hide_unbrowsed_runtime_page, page_id_from_runtime_label, reveal_runtime_page,
    set_active_runtime_page, sync_runtime_viewport, BrowserRuntimeRegistry, BrowserViewport,
    RuntimeReportPayload, TauriBrowserRuntime, POPUP_BLOCKED_REQUEST,
};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::SystemTime;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Emitter, EventTarget, Manager, Runtime, State, WebviewWindow};

struct AppState {
    session_manager: SessionManager,
    runtimes: Arc<BrowserRuntimeRegistry>,
    /// Persistent page memory (`neuro_memory::MemoryService`). Opened at
    /// `app_data_dir()/memory/`.
    memory: Arc<MemoryService>,
    /// A read lease orders each governed operation against policy updates.
    /// The UI receives acknowledgment only after a write lease installs the policy.
    action_policy: tokio::sync::RwLock<ActionPolicy>,
    /// Caller-owned capture rules. `MemoryService::forget` tombstones a host
    /// here for this process. The service does not store the policy itself.
    capture_policy: tokio::sync::Mutex<CapturePolicy>,
    /// Browsing history (`history.json`), distinct from page memory.
    history: Arc<HistoryStore>,
    /// Files this process has accepted into the user's Downloads folder.
    downloads: Arc<DownloadBook>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    approval_context: Option<ApprovalContext>,
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
        #[serde(skip_serializing_if = "Option::is_none")]
        receipt: Option<ActionReceipt>,
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
        AgentRunEvent::ToolCallResult {
            tool,
            success,
            result,
            ..
        } => {
            let receipt = if matches!(
                tool.as_str(),
                "click_target" | "type_target" | "submit_target" | "scroll_target"
            ) {
                serde_json::from_str::<ActionReceipt>(&result).ok()
            } else {
                None
            };
            AgentRunEventResponse::ToolCallResult {
                tool,
                success,
                receipt,
            }
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
        approval_context: None,
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

fn contextual_agent_run_response(
    result: AgentRunResult,
    agent: &ReActAgent,
) -> Result<AgentRunResponse, String> {
    let context = result
        .approval_id
        .as_deref()
        .map(|id| agent.approval_context(id))
        .transpose()?
        .flatten();
    let mut response = agent_run_response(result);
    response.approval_context = context;
    Ok(response)
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
    activate: Option<bool>,
) -> Result<usize, String> {
    let page = state.session_manager.create_page(&session_id)?;
    // A background tab must not cover the page that opened it.
    let activate = activate.unwrap_or(true);
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
        if activate {
            set_active_runtime_page(&app, state.runtimes.as_ref(), page.id)?;
        }
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
    state.session_manager.get_page(&session_id, page_id)?;
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
    let _operation = state
        .session_manager
        .lock_page_operation(&session_id, page_id)
        .await?;
    // Host-side scheme/format + netguard check. UI does not preflight.
    let normalized_url = normalize_and_guard_url(url)?;
    let (_, browser) = browser_for_page(app.clone(), state.inner(), &session_id, page_id)?;
    // The new-tab page is the shell. Show the page webview once a real
    // navigation starts, and put it away again if that navigation never leaves
    // about:blank.
    reveal_runtime_page(&app, state.runtimes.as_ref(), page_id)?;
    if let Err(error) = browser.navigate(&normalized_url).await {
        if let Err(hide_error) = hide_unbrowsed_runtime_page(&app, state.runtimes.as_ref(), page_id)
        {
            tracing::warn!("failed to hide the blank page after navigation failed: {hide_error}");
        }
        return Err(error);
    }
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

/// Human and agent clients receive the same bounded, provenance-bearing evidence.
#[tauri::command]
async fn get_page_observation(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    page_id: usize,
) -> Result<PageObservation, String> {
    let _operation = state
        .session_manager
        .lock_page_operation(&session_id, page_id)
        .await?;
    let (_, browser) = browser_for_page(app, state.inner(), &session_id, page_id)?;
    browser.observe(ObservationLimits::default()).await
}

/// A single explicit proposal uses the agent's policy/approval route without a model.
#[tauri::command]
async fn execute_browser_tool(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    page_id: usize,
    tool_call: ToolCall,
) -> Result<AgentRunResponse, String> {
    let _operation = state
        .session_manager
        .lock_page_operation(&session_id, page_id)
        .await?;
    let (page, browser) = browser_for_page(app, state.inner(), &session_id, page_id)?;
    let policy = state.action_policy.read().await;
    let result = page
        .agent
        .propose_tool_with_policy(tool_call, &browser, &policy)
        .await?;
    remember_pending_approval(state.inner(), session_id, page_id, &result)?;
    contextual_agent_run_response(result, &page.agent)
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
) -> Result<AgentRunResponse, String> {
    let _operation = state
        .session_manager
        .lock_page_operation(&session_id, page_id)
        .await?;
    let (page, browser) = browser_for_page(app, state, &session_id, page_id)?;
    let policy = state.action_policy.read().await;
    let result = page
        .agent
        .execute_with_policy(prompt, &browser, &policy)
        .await?;
    remember_pending_approval(state, session_id, page_id, &result)?;
    contextual_agent_run_response(result, &page.agent)
}

#[tauri::command]
async fn get_action_policy(state: State<'_, AppState>) -> Result<ActionPolicy, String> {
    Ok(state.action_policy.read().await.clone())
}

#[tauri::command]
async fn set_action_policy(
    state: State<'_, AppState>,
    policy: ActionPolicy,
) -> Result<ActionPolicy, String> {
    let mut current = state.action_policy.write().await;
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
    execute_agent_run(app, state.inner(), session_id, page_id, &prompt).await
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
    let _operation = state
        .session_manager
        .lock_page_operation(&pending.session_id, pending.page_id)
        .await?;
    let (page, browser) =
        browser_for_page(app, state.inner(), &pending.session_id, pending.page_id)?;
    let policy = state.action_policy.read().await;
    page.agent
        .execute_approved_tool_with_policy(
            run_id,
            pending.approval_id,
            pending.tool_call,
            &browser,
            approved,
            None,
            &policy,
        )
        .await
        .map(agent_run_response)
}

#[tauri::command]
fn cancel_agent_run(
    state: State<'_, AppState>,
    run_id: String,
) -> Result<AgentRunResponse, String> {
    let removed = state
        .pending_approvals
        .lock()
        .map_err(|e| e.to_string())?
        .remove(&run_id);
    if let Some(pending) = &removed {
        if let Ok(page) = state
            .session_manager
            .get_page(&pending.session_id, pending.page_id)
        {
            page.agent.cancel_pending_approval(&run_id)?;
        }
    }
    let reason = if removed.is_some() {
        "Run cancelled by user"
    } else {
        "Run was not pending approval"
    }
    .to_string();
    Ok(agent_run_response(AgentRunResult {
        run_id: run_id.clone(),
        status: AgentRunStatus::Cancelled,
        final_response: Some(reason.clone()),
        iterations: 0,
        events: vec![AgentRunEvent::RunCancelled { run_id, reason }],
        pending_tool_call: None,
        approval_id: None,
    }))
}

#[tauri::command]
fn close_page(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    page_id: usize,
) -> Result<(), String> {
    state.session_manager.close_page(&session_id, page_id)?;
    state
        .pending_approvals
        .lock()
        .map_err(|e| e.to_string())?
        .retain(|_, pending| pending.session_id != session_id || pending.page_id != page_id);
    close_runtime_page(&app, state.runtimes.as_ref(), page_id)
}

#[tauri::command]
async fn browser_reload(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    page_id: usize,
) -> Result<(), String> {
    let _operation = state
        .session_manager
        .lock_page_operation(&session_id, page_id)
        .await?;
    let (_, browser) = browser_for_page(app, state.inner(), &session_id, page_id)?;
    browser.browser_reload().await?;
    browser.wait_for_navigation().await.map_err(|error| {
        format!(
            "Reload dispatched; page readiness is unconfirmed: {error}. \
             Do not repeat the reload automatically; inspect the page."
        )
    })
}

#[tauri::command]
fn open_download(state: State<'_, AppState>, id: String) -> Result<(), String> {
    downloads::open_download_file(&state.downloads, &id)
}

#[tauri::command]
fn show_download_in_folder(state: State<'_, AppState>, id: String) -> Result<(), String> {
    downloads::show_download_in_folder(&state.downloads, &id)
}

#[tauri::command]
fn stop_page_load(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    page_id: usize,
) -> Result<(), String> {
    // Navigate and reload hold the page lock until the load finishes. Stop has
    // to run without that lock, or it would wait on the load it is cancelling.
    let (_, browser) = browser_for_page(app, state.inner(), &session_id, page_id)?;
    browser.stop_page_load()
}

#[tauri::command]
async fn browser_back(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    page_id: usize,
) -> Result<(), String> {
    let _operation = state
        .session_manager
        .lock_page_operation(&session_id, page_id)
        .await?;
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
    let _operation = state
        .session_manager
        .lock_page_operation(&session_id, page_id)
        .await?;
    let (_, browser) = browser_for_page(app, state.inner(), &session_id, page_id)?;
    browser.browser_forward().await
}

#[tauri::command]
fn browser_runtime_report(
    webview: tauri::Webview,
    state: State<'_, AppState>,
    payload: RuntimeReportPayload,
) -> Result<(), String> {
    if payload.request_id == POPUP_BLOCKED_REQUEST {
        let page_id = page_id_from_runtime_label(webview.label())
            .ok_or("Runtime report caller does not own this page")?;
        if state.runtimes.page_runtime_id(page_id).ok().as_deref() != Some(webview.label()) {
            return Err("Runtime report caller does not own this page".into());
        }
        if let Some(event) = blocked_popup_payload(webview.label(), payload.payload.as_ref()) {
            emit_page_new_window(&webview, &event);
        }
        return Ok(());
    }
    state.runtimes.page_runtime_id(payload.page_id)?;
    state.runtimes.resolve_request(
        payload.page_id,
        webview.label(),
        &payload.request_id,
        payload.payload,
        payload.error,
    )
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

/// How many history rows the address bar and the new-tab page ask for.
const BROWSING_HISTORY_LIMIT: usize = 8;

/// One history row sent to the shell. `last_visited` stays in the file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct BrowsingHistoryHit {
    url: String,
    title: String,
    visit_count: u64,
}

fn browsing_history_hit(entry: HistoryEntry) -> BrowsingHistoryHit {
    BrowsingHistoryHit {
        url: entry.url,
        title: entry.title,
        visit_count: entry.visit_count,
    }
}

/// Visit count, then recency, then URL.
///
/// [`HistoryStore::search`] orders by recency first and truncates. This reads
/// every match, then ranks the way the address bar and new-tab page do.
fn browsing_history_hits(
    store: &HistoryStore,
    query: &str,
    limit: usize,
) -> Vec<BrowsingHistoryHit> {
    let limit = limit.min(BROWSING_HISTORY_LIMIT);
    if limit == 0 {
        return Vec::new();
    }
    let mut matches = store.search(query.trim(), usize::MAX);
    matches.sort_by(|left, right| {
        right
            .visit_count
            .cmp(&left.visit_count)
            .then_with(|| right.last_visited.cmp(&left.last_visited))
            .then_with(|| left.url.cmp(&right.url))
    });
    matches.truncate(limit);
    matches.into_iter().map(browsing_history_hit).collect()
}

/// Record a finished http(s) visit. An empty title keeps a title already stored
/// for that URL so a load that finishes before the document title does not
/// wipe it. Errors are logged; a history write never fails the page load.
fn record_browsing_visit(store: &HistoryStore, url: &str, title: &str) {
    if !should_record_visit(url, false) {
        return;
    }
    let title_to_store = if title.trim().is_empty() {
        store
            .get(url)
            .map(|entry| entry.title)
            .filter(|existing| !existing.is_empty())
            .unwrap_or_default()
    } else {
        title.to_string()
    };
    if let Err(error) = store.record_visit(url, &title_to_store, SystemTime::now()) {
        tracing::warn!("browsing history was not recorded: {error}");
    }
}

fn open_history(app: &tauri::App) -> Result<Arc<HistoryStore>, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|err| err.to_string())?
        .join("history");
    HistoryStore::open(dir)
        .map(Arc::new)
        .map_err(|err| err.to_string())
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
fn search_browsing_history(
    state: State<'_, AppState>,
    query: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<BrowsingHistoryHit>, String> {
    Ok(browsing_history_hits(
        &state.history,
        query.as_deref().unwrap_or(""),
        limit.unwrap_or(BROWSING_HISTORY_LIMIT),
    ))
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

/// Emitted to the main control webview when a browser menu item is chosen.
/// Edit-role items are not emitted; the OS applies them to the focused webview.
pub const MENU_COMMAND_EVENT: &str = "menu-command";

#[derive(Clone, Copy, PartialEq, Eq)]
enum MenuSection {
    File,
    View,
    History,
}

struct MenuShortcut {
    section: MenuSection,
    id: &'static str,
    label: &'static str,
    accelerator: &'static str,
    enabled: bool,
}

// Reopen Closed Tab (Cmd/Ctrl+Shift+T) is omitted: the shell has no closed-tab stack.
const BROWSER_MENU_ITEMS: &[MenuShortcut] = &[
    MenuShortcut {
        section: MenuSection::File,
        id: "new-tab",
        label: "New Tab",
        accelerator: "CmdOrCtrl+T",
        enabled: true,
    },
    MenuShortcut {
        section: MenuSection::File,
        id: "close-tab",
        label: "Close Tab",
        accelerator: "CmdOrCtrl+W",
        enabled: true,
    },
    MenuShortcut {
        section: MenuSection::File,
        id: "focus-address",
        label: "Focus Address Bar",
        accelerator: "CmdOrCtrl+L",
        enabled: true,
    },
    MenuShortcut {
        section: MenuSection::View,
        id: "reload",
        label: "Reload",
        accelerator: "CmdOrCtrl+R",
        enabled: true,
    },
    MenuShortcut {
        section: MenuSection::View,
        id: "stop",
        label: "Stop",
        accelerator: "Escape",
        enabled: false,
    },
    MenuShortcut {
        section: MenuSection::View,
        id: "next-tab",
        label: "Next Tab",
        accelerator: "Ctrl+Tab",
        enabled: true,
    },
    MenuShortcut {
        section: MenuSection::View,
        id: "previous-tab",
        label: "Previous Tab",
        accelerator: "Ctrl+Shift+Tab",
        enabled: true,
    },
    MenuShortcut {
        section: MenuSection::View,
        id: "select-tab-1",
        label: "Tab 1",
        accelerator: "CmdOrCtrl+1",
        enabled: true,
    },
    MenuShortcut {
        section: MenuSection::View,
        id: "select-tab-2",
        label: "Tab 2",
        accelerator: "CmdOrCtrl+2",
        enabled: true,
    },
    MenuShortcut {
        section: MenuSection::View,
        id: "select-tab-3",
        label: "Tab 3",
        accelerator: "CmdOrCtrl+3",
        enabled: true,
    },
    MenuShortcut {
        section: MenuSection::View,
        id: "select-tab-4",
        label: "Tab 4",
        accelerator: "CmdOrCtrl+4",
        enabled: true,
    },
    MenuShortcut {
        section: MenuSection::View,
        id: "select-tab-5",
        label: "Tab 5",
        accelerator: "CmdOrCtrl+5",
        enabled: true,
    },
    MenuShortcut {
        section: MenuSection::View,
        id: "select-tab-6",
        label: "Tab 6",
        accelerator: "CmdOrCtrl+6",
        enabled: true,
    },
    MenuShortcut {
        section: MenuSection::View,
        id: "select-tab-7",
        label: "Tab 7",
        accelerator: "CmdOrCtrl+7",
        enabled: true,
    },
    MenuShortcut {
        section: MenuSection::View,
        id: "select-tab-8",
        label: "Tab 8",
        accelerator: "CmdOrCtrl+8",
        enabled: true,
    },
    MenuShortcut {
        section: MenuSection::View,
        id: "select-tab-9",
        label: "Last Tab",
        accelerator: "CmdOrCtrl+9",
        enabled: true,
    },
    MenuShortcut {
        section: MenuSection::History,
        id: "back",
        label: "Back",
        accelerator: "CmdOrCtrl+[",
        enabled: true,
    },
    MenuShortcut {
        section: MenuSection::History,
        id: "forward",
        label: "Forward",
        accelerator: "CmdOrCtrl+]",
        enabled: true,
    },
    MenuShortcut {
        section: MenuSection::History,
        id: "back-alt",
        label: "Back",
        accelerator: "Alt+Left",
        enabled: true,
    },
    MenuShortcut {
        section: MenuSection::History,
        id: "forward-alt",
        label: "Forward",
        accelerator: "Alt+Right",
        enabled: true,
    },
];

fn menu_command(id: &str) -> Option<&'static str> {
    match id {
        "new-tab" => Some("new-tab"),
        "close-tab" => Some("close-tab"),
        "focus-address" => Some("focus-address"),
        "reload" => Some("reload"),
        "stop" => Some("stop"),
        "back" | "back-alt" => Some("back"),
        "forward" | "forward-alt" => Some("forward"),
        "next-tab" => Some("next-tab"),
        "previous-tab" => Some("previous-tab"),
        "select-tab-1" => Some("select-tab-1"),
        "select-tab-2" => Some("select-tab-2"),
        "select-tab-3" => Some("select-tab-3"),
        "select-tab-4" => Some("select-tab-4"),
        "select-tab-5" => Some("select-tab-5"),
        "select-tab-6" => Some("select-tab-6"),
        "select-tab-7" => Some("select-tab-7"),
        "select-tab-8" => Some("select-tab-8"),
        "select-tab-9" => Some("select-tab-9"),
        _ => None,
    }
}

fn emit_menu_command(app: &AppHandle, command: &str) {
    if let Err(error) = app.emit_to(
        EventTarget::labeled(runtime::MAIN_WEBVIEW_LABEL),
        MENU_COMMAND_EVENT,
        command,
    ) {
        tracing::warn!("failed to emit {MENU_COMMAND_EVENT} ({command}): {error}");
    }
}

fn section_items<R: Runtime, M: Manager<R>>(
    manager: &M,
    section: MenuSection,
) -> tauri::Result<Vec<MenuItem<R>>> {
    let mut items = Vec::new();
    for shortcut in BROWSER_MENU_ITEMS {
        if shortcut.section != section {
            continue;
        }
        items.push(MenuItem::with_id(
            manager,
            shortcut.id,
            shortcut.label,
            shortcut.enabled,
            Some(shortcut.accelerator),
        )?);
    }
    Ok(items)
}

fn append_with_separator_before<R: Runtime, M: Manager<R>>(
    submenu: &Submenu<R>,
    manager: &M,
    items: &[MenuItem<R>],
    separator_before: &[&str],
) -> tauri::Result<()> {
    for item in items {
        let id: &str = item.id().as_ref();
        if separator_before.contains(&id) {
            submenu.append(&PredefinedMenuItem::separator(manager)?)?;
        }
        submenu.append(item)?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn macos_app_submenu<R: Runtime, M: Manager<R>>(manager: &M) -> tauri::Result<Submenu<R>> {
    let about = PredefinedMenuItem::about(manager, None, None)?;
    let services = PredefinedMenuItem::services(manager, None)?;
    let hide = PredefinedMenuItem::hide(manager, None)?;
    let hide_others = PredefinedMenuItem::hide_others(manager, None)?;
    let show_all = PredefinedMenuItem::show_all(manager, None)?;
    let quit = PredefinedMenuItem::quit(manager, None)?;
    let after_about = PredefinedMenuItem::separator(manager)?;
    let after_services = PredefinedMenuItem::separator(manager)?;
    let before_quit = PredefinedMenuItem::separator(manager)?;
    Submenu::with_items(
        manager,
        "NeuroBrowser",
        true,
        &[
            &about,
            &after_about,
            &services,
            &after_services,
            &hide,
            &hide_others,
            &show_all,
            &before_quit,
            &quit,
        ],
    )
}

fn browser_menu<R: Runtime, M: Manager<R>>(manager: &M) -> tauri::Result<(Menu<R>, MenuItem<R>)> {
    let file_items = section_items(manager, MenuSection::File)?;
    let view_items = section_items(manager, MenuSection::View)?;
    let history_items = section_items(manager, MenuSection::History)?;
    let stop = view_items
        .iter()
        .find(|item| item.id().as_ref() == "stop")
        .cloned()
        .expect("View menu includes Stop");

    let file = Submenu::new(manager, "File", true)?;
    append_with_separator_before(&file, manager, &file_items, &["focus-address"])?;

    // Predefined edit items target the focused webview, including a page.
    let undo = PredefinedMenuItem::undo(manager, None)?;
    let redo = PredefinedMenuItem::redo(manager, None)?;
    let cut = PredefinedMenuItem::cut(manager, None)?;
    let copy = PredefinedMenuItem::copy(manager, None)?;
    let paste = PredefinedMenuItem::paste(manager, None)?;
    let select_all = PredefinedMenuItem::select_all(manager, None)?;
    let edit_history_sep = PredefinedMenuItem::separator(manager)?;
    let edit_select_sep = PredefinedMenuItem::separator(manager)?;
    let edit = Submenu::with_items(
        manager,
        "Edit",
        true,
        &[
            &undo,
            &redo,
            &edit_history_sep,
            &cut,
            &copy,
            &paste,
            &edit_select_sep,
            &select_all,
        ],
    )?;

    let view = Submenu::new(manager, "View", true)?;
    append_with_separator_before(&view, manager, &view_items, &["next-tab", "select-tab-1"])?;

    let history = Submenu::new(manager, "History", true)?;
    append_with_separator_before(&history, manager, &history_items, &["back-alt"])?;

    let menu = Menu::new(manager)?;
    #[cfg(target_os = "macos")]
    menu.append(&macos_app_submenu(manager)?)?;
    menu.append(&file)?;
    menu.append(&edit)?;
    menu.append(&view)?;
    menu.append(&history)?;
    Ok((menu, stop))
}

fn install_app_menu(app: &tauri::App, runtimes: &BrowserRuntimeRegistry) -> tauri::Result<()> {
    let handle = app.handle();
    let (menu, stop) = browser_menu(handle)?;
    let app_handle = handle.clone();
    let stop_item = stop.clone();
    runtimes.set_active_loading_hook(std::sync::Arc::new(move |loading| {
        let stop_item = stop_item.clone();
        let app_handle = app_handle.clone();
        if let Err(error) = app_handle.run_on_main_thread(move || {
            if let Err(error) = stop_item.set_enabled(loading) {
                tracing::warn!("failed to update Stop menu item: {error}");
            }
        }) {
            tracing::warn!("failed to schedule Stop menu update: {error}");
        }
    }));
    handle.set_menu(menu)?;
    handle.on_menu_event(|app, event| {
        if let Some(command) = menu_command(event.id().as_ref()) {
            emit_menu_command(app, command);
        }
    });
    Ok(())
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
            install_app_menu(app, runtimes.as_ref()).map_err(
                |err| -> Box<dyn std::error::Error> {
                    Box::new(std::io::Error::other(err.to_string()))
                },
            )?;
            let memory = open_memory(app).map_err(|err| -> Box<dyn std::error::Error> {
                Box::new(std::io::Error::other(err))
            })?;
            let history = open_history(app).map_err(|err| -> Box<dyn std::error::Error> {
                Box::new(std::io::Error::other(err))
            })?;
            tracing::info!("memory data dir: {}", memory.data_dir().display());
            app.manage(AppState {
                session_manager,
                runtimes,
                memory,
                action_policy: tokio::sync::RwLock::new(ActionPolicy::default()),
                capture_policy: tokio::sync::Mutex::new(CapturePolicy::default()),
                history,
                downloads: Arc::new(DownloadBook::default()),
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
            get_page_observation,
            execute_browser_tool,
            navigate,
            open_download,
            search_browsing_history,
            show_download_in_folder,
            search_local_memory,
            set_active_page,
            set_action_policy,
            set_provider,
            start_agent_run,
            stop_page_load,
            submit_approval,
            sync_browser_viewport,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::{
        agent_run_response, menu_command, AgentRunEvent, AgentRunResult, AgentRunStatus, ToolCall,
        BROWSER_MENU_ITEMS,
    };
    use neurobrowser::{PolicyDecision, PolicyOutcome, RiskFlag};
    use std::collections::HashMap;
    use std::collections::HashSet;
    use std::str::FromStr;

    #[test]
    fn browser_menu_accelerators_parse_and_skip_edit_roles() {
        let mut ids = HashSet::new();
        let mut accelerators = HashSet::new();
        for shortcut in BROWSER_MENU_ITEMS {
            assert!(ids.insert(shortcut.id), "duplicate id {}", shortcut.id);
            assert!(
                accelerators.insert(shortcut.accelerator),
                "duplicate accelerator {}",
                shortcut.accelerator
            );
            muda::accelerator::Accelerator::from_str(shortcut.accelerator).unwrap_or_else(
                |error| panic!("{} failed to parse: {error}", shortcut.accelerator),
            );
            let command = menu_command(shortcut.id).unwrap_or_else(|| {
                panic!("{} is not forwarded to the control webview", shortcut.id)
            });
            if shortcut.id == "back-alt" {
                assert_eq!(command, "back");
            } else if shortcut.id == "forward-alt" {
                assert_eq!(command, "forward");
            } else {
                assert_eq!(command, shortcut.id);
            }
        }
        assert!(
            !accelerators
                .iter()
                .any(|item| *item == "CmdOrCtrl+Shift+T" || *item == "Ctrl+Shift+T"),
            "reopen closed tab is not a menu accelerator"
        );
        assert_eq!(menu_command("reopen-closed-tab"), None);
        for edit in [
            "undo",
            "redo",
            "cut",
            "copy",
            "paste",
            "select_all",
            "separator",
        ] {
            assert_eq!(
                menu_command(edit),
                None,
                "{edit} stays on the focused webview"
            );
        }
        let stop = BROWSER_MENU_ITEMS
            .iter()
            .find(|item| item.id == "stop")
            .expect("stop item");
        assert_eq!(stop.accelerator, "Escape");
        assert!(!stop.enabled);
        assert_eq!(menu_command("select-tab-9"), Some("select-tab-9"));
    }

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

    #[test]
    fn browsing_history_records_visits_and_ranks_by_count_then_recency() {
        let dir = std::env::temp_dir().join(format!(
            "neurobrowser-history-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let store = neurobrowser::HistoryStore::open(&dir).expect("open history");
        let start = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        let gap = neurobrowser::history::VISIT_COLLAPSE_WINDOW + std::time::Duration::from_secs(1);

        super::record_browsing_visit(&store, "about:blank", "Blank");
        super::record_browsing_visit(&store, "ftp://example.com/file", "File");
        assert!(store.recent(10).is_empty());

        super::record_browsing_visit(&store, "https://example.com/docs", "Docs");
        super::record_browsing_visit(&store, "https://example.com/docs", "");
        let kept = store.get("https://example.com/docs").expect("visit");
        assert_eq!(kept.title, "Docs");
        assert_eq!(kept.visit_count, 1);

        store
            .record_visit("https://example.com/popular", "Popular", start)
            .expect("popular first");
        store
            .record_visit("https://example.com/popular", "Popular", start + gap)
            .expect("popular second");
        store
            .record_visit(
                "https://example.com/newer",
                "Newer notes",
                start + gap + gap,
            )
            .expect("newer");
        store
            .record_visit("https://example.com/other", "Other", start + gap)
            .expect("other");

        let ranked = super::browsing_history_hits(&store, "example.com", 100);
        let urls: Vec<_> = ranked.iter().map(|hit| hit.url.as_str()).collect();
        assert_eq!(
            urls,
            [
                "https://example.com/popular",
                "https://example.com/docs",
                "https://example.com/newer",
                "https://example.com/other",
            ]
        );
        assert!(ranked.iter().all(|hit| hit.visit_count >= 1));
        assert_eq!(ranked[0].visit_count, 2);
        assert_eq!(ranked[1].visit_count, 1);

        let title_match = super::browsing_history_hits(&store, "NOTES", 8);
        assert_eq!(title_match.len(), 1);
        assert_eq!(title_match[0].url, "https://example.com/newer");

        let capped = super::browsing_history_hits(&store, "", 8);
        assert!(capped.len() <= 8);
        assert_eq!(capped[0].url, "https://example.com/popular");
        let json = serde_json::to_value(&capped).expect("serialize hits");
        assert!(json[0].get("visitCount").is_some());
        assert!(json[0].get("visit_count").is_none());
        assert!(json[0].get("last_visited").is_none());

        assert!(super::browsing_history_hits(&store, "missing", 8).is_empty());
        assert!(super::browsing_history_hits(&store, "example", 0).is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
