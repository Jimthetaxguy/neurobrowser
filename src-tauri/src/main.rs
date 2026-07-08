#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod runtime;

use neurobrowser::{
    ActionPolicy, AgentConfig, AgentRunEvent, AgentRunResult, AgentRunStatus, BrowserInterface,
    PageConfig, PageSnapshot, ProviderConfig, ProviderType, SessionManager, ToolCall,
};
use runtime::{
    close_runtime_page, create_runtime_page, set_active_runtime_page, sync_runtime_viewport,
    wait_for_runtime_page, BrowserRuntimeRegistry, BrowserViewport, RuntimeReportPayload,
    TauriBrowserRuntime,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use tauri::{AppHandle, Manager, State, WebviewWindow};

struct AppState {
    session_manager: SessionManager,
    runtimes: Arc<BrowserRuntimeRegistry>,
    action_policy: Mutex<ActionPolicy>,
    pending_approvals: Mutex<HashMap<String, PendingApproval>>,
}

struct PendingApproval {
    session_id: String,
    page_id: usize,
    tool_call: ToolCall,
    approval_id: String,
}

#[derive(Serialize)]
struct AskResult {
    response: String,
    tools_used: Vec<String>,
    iterations: usize,
}

#[derive(Serialize)]
struct SnapshotResponse {
    url: String,
    title: String,
    html: String,
    text: String,
    link_count: usize,
    image_count: usize,
    form_count: usize,
    price_count: usize,
    table_count: usize,
    viewport_width: u32,
    viewport_height: u32,
    interactive_ready: bool,
}

#[derive(Serialize)]
struct SessionListItem {
    id: String,
    created_at: u64,
    page_count: usize,
}

#[derive(Serialize, Deserialize)]
struct ValidateUrlResult {
    valid: bool,
    normalized_url: String,
    error: Option<String>,
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
                .unwrap_or_else(|_| "claude-3-5-sonnet-latest".to_string()),
            max_tokens: Some(4096),
            temperature: Some(0.3),
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
        html: snapshot.html.unwrap_or_default(),
        text: snapshot.text.unwrap_or_default(),
        link_count: snapshot.links.len(),
        image_count: snapshot.images.len(),
        form_count: snapshot.forms.len(),
        price_count: snapshot.prices.len(),
        table_count: snapshot.tables.len(),
        viewport_width: snapshot.viewport_width,
        viewport_height: snapshot.viewport_height,
        interactive_ready: snapshot.interactive_ready,
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
    let (_, browser) = browser_for_page(app, state.inner(), &session_id, page_id)?;
    browser.navigate(&url).await
}

#[tauri::command]
async fn wait_for_page_ready(
    state: State<'_, AppState>,
    page_id: usize,
    timeout_ms: Option<u64>,
) -> Result<(), String> {
    wait_for_runtime_page(
        state.runtimes.as_ref(),
        page_id,
        timeout_ms.unwrap_or(8_000),
    )
    .await
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

#[tauri::command]
async fn get_page_info(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    page_id: usize,
) -> Result<SnapshotResponse, String> {
    get_page_snapshot(app, state, session_id, page_id).await
}

#[tauri::command]
async fn ask(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    page_id: usize,
    prompt: String,
) -> Result<AskResult, String> {
    let (page, browser) = browser_for_page(app, state.inner(), &session_id, page_id)?;
    let response = page.agent.execute(&prompt, &browser).await?;

    Ok(AskResult {
        response,
        tools_used: vec![],
        iterations: 1,
    })
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
) -> Result<AgentRunResult, String> {
    let (page, browser) = browser_for_page(app, state.inner(), &session_id, page_id)?;
    let policy = state
        .action_policy
        .lock()
        .map(|policy| policy.clone())
        .map_err(|e| e.to_string())?;

    let result = page
        .agent
        .execute_with_policy(&prompt, &browser, &policy)
        .await?;

    if result.status == AgentRunStatus::AwaitingApproval {
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
    }

    Ok(result)
}

#[tauri::command]
async fn submit_approval(
    app: AppHandle,
    state: State<'_, AppState>,
    run_id: String,
    approved: bool,
    message: Option<String>,
) -> Result<AgentRunResult, String> {
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
            message,
        )
        .await
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
fn list_workers(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Vec<neurobrowser::WorkerSummary>, String> {
    state
        .session_manager
        .list_workers(&session_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn get_worker(
    state: State<'_, AppState>,
    session_id: String,
    worker_id: String,
) -> Result<neurobrowser::WorkerSnapshot, String> {
    state
        .session_manager
        .get_worker(&session_id, &worker_id)
        .map_err(|e| e.to_string())
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
fn list_sessions(state: State<'_, AppState>) -> Result<Vec<SessionListItem>, String> {
    Ok(state
        .session_manager
        .list_sessions()
        .into_iter()
        .map(|session| SessionListItem {
            id: session.id,
            created_at: session.created_at,
            page_count: session.page_count,
        })
        .collect())
}

#[tauri::command]
fn browser_reload(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    page_id: usize,
) -> Result<(), String> {
    let (page, _) = browser_for_page(app.clone(), state.inner(), &session_id, page_id)?;
    state.runtimes.set_loading(page.id, true);
    app.get_webview(&page.runtime_id)
        .ok_or_else(|| "Runtime webview not found".to_string())?
        .reload()
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn browser_back(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    page_id: usize,
) -> Result<(), String> {
    let (page, _) = browser_for_page(app.clone(), state.inner(), &session_id, page_id)?;
    state.runtimes.set_loading(page.id, true);
    app.get_webview(&page.runtime_id)
        .ok_or_else(|| "Runtime webview not found".to_string())?
        .eval("history.back()")
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn browser_forward(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    page_id: usize,
) -> Result<(), String> {
    let (page, _) = browser_for_page(app.clone(), state.inner(), &session_id, page_id)?;
    state.runtimes.set_loading(page.id, true);
    app.get_webview(&page.runtime_id)
        .ok_or_else(|| "Runtime webview not found".to_string())?
        .eval("history.forward()")
        .map_err(|e| e.to_string())
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

#[tauri::command]
fn validate_url(url: String) -> ValidateUrlResult {
    let normalized = if url.starts_with("http://") || url.starts_with("https://") {
        url.clone()
    } else if url.contains('.') && !url.contains(' ') {
        format!("https://{}", url)
    } else {
        return ValidateUrlResult {
            valid: false,
            normalized_url: String::new(),
            error: Some("Invalid URL format".to_string()),
        };
    };

    if normalized.contains("javascript:") || normalized.contains("data:") {
        return ValidateUrlResult {
            valid: false,
            normalized_url: normalized,
            error: Some("Dangerous URL scheme blocked".to_string()),
        };
    }

    ValidateUrlResult {
        valid: true,
        normalized_url: normalized,
        error: None,
    }
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
        .with_env_filter("neurobrowser=info")
        .init();

    let browser_config = PageConfig::default();
    let agent_config = AgentConfig {
        max_iterations: 5,
        provider_config: provider_config_for(ProviderType::Openai),
    };

    let session_manager = SessionManager::new(browser_config, agent_config);
    let runtimes = Arc::new(BrowserRuntimeRegistry::default());

    tauri::Builder::default()
        .manage(AppState {
            session_manager,
            runtimes,
            action_policy: Mutex::new(ActionPolicy::default()),
            pending_approvals: Mutex::new(HashMap::new()),
        })
        .invoke_handler(tauri::generate_handler![
            ask,
            browser_back,
            browser_forward,
            browser_reload,
            browser_runtime_report,
            cancel_agent_run,
            close_page,
            create_page,
            create_session,
            get_action_policy,
            get_page_info,
            get_page_snapshot,
            get_worker,
            list_sessions,
            list_workers,
            navigate,
            set_active_page,
            set_action_policy,
            set_provider,
            start_agent_run,
            submit_approval,
            sync_browser_viewport,
            validate_url,
            wait_for_page_ready,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
