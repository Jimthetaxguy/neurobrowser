#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use neurobrowser::{
    AgentConfig, BrowserInterface, PageConfig, ProviderConfig, ProviderType, SessionManager,
};
use serde::{Deserialize, Serialize};
use tauri::State;

struct AppState {
    session_manager: SessionManager,
}

#[derive(Serialize, Deserialize)]
struct NavigateRequest {
    session_id: String,
    url: String,
}

#[derive(Serialize, Deserialize)]
struct AskRequest {
    session_id: String,
    page_id: usize,
    prompt: String,
}

#[derive(Serialize, Deserialize)]
struct PageInfo {
    url: String,
    title: String,
    html: String,
    link_count: usize,
    image_count: usize,
    form_count: usize,
    price_count: usize,
    viewport_width: u32,
    viewport_height: u32,
}

#[tauri::command]
fn create_session(state: State<'_, AppState>) -> Result<String, String> {
    let session_id = state.session_manager.create_session();
    Ok(session_id)
}

#[tauri::command]
fn create_page(state: State<'_, AppState>, session_id: String) -> Result<usize, String> {
    let page = state.session_manager.create_page(&session_id)?;
    Ok(page.id)
}

#[tauri::command]
fn navigate(
    state: State<'_, AppState>,
    session_id: String,
    page_id: usize,
    url: String,
) -> Result<(), String> {
    let page = state.session_manager.get_page(&session_id, page_id)?;
    page.browser.navigate(&url)
}

#[tauri::command]
async fn ask(
    state: State<'_, AppState>,
    session_id: String,
    page_id: usize,
    prompt: String,
) -> Result<String, String> {
    let page = state.session_manager.get_page(&session_id, page_id)?;
    page.agent.execute(&prompt, page.browser.as_ref()).await
}

#[tauri::command]
fn get_page_info(
    state: State<'_, AppState>,
    session_id: String,
    page_id: usize,
) -> Result<PageInfo, String> {
    let page = state.session_manager.get_page(&session_id, page_id)?;
    let page_state = page.browser.get_state()?;
    let page_info = page.browser.get_page_info();

    Ok(PageInfo {
        url: page_state.url,
        title: page_state.title,
        html: page_state.html,
        link_count: page_info.links.len(),
        image_count: page_info.images.len(),
        form_count: page_info.forms.len(),
        price_count: page_info.prices.len(),
        viewport_width: page_state.viewport_width,
        viewport_height: page_state.viewport_height,
    })
}

#[tauri::command]
fn list_sessions(state: State<'_, AppState>) -> Result<Vec<SessionListItem>, String> {
    let sessions = state.session_manager.list_sessions();
    Ok(sessions
        .into_iter()
        .map(|s| SessionListItem {
            id: s.id,
            created_at: s.created_at,
            page_count: s.page_count,
        })
        .collect())
}

#[derive(Serialize, Deserialize)]
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

#[tauri::command]
fn validate_url(url: String) -> ValidateUrlResult {
    // Basic URL validation
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

    // Check for suspicious patterns
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

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter("neurobrowser=info")
        .init();

    let browser_config = PageConfig::default();
    let agent_config = AgentConfig {
        max_iterations: 5,
        provider_config: ProviderConfig {
            provider_type: ProviderType::Openai,
            api_key: std::env::var("OPENAI_API_KEY").ok(),
            base_url: None,
            model: "gpt-4o".to_string(),
            max_tokens: Some(4096),
            temperature: Some(0.3),
        },
    };

    let session_manager = SessionManager::new(browser_config, agent_config);

    tauri::Builder::default()
        .manage(AppState { session_manager })
        .invoke_handler(tauri::generate_handler![
            create_session,
            create_page,
            navigate,
            ask,
            get_page_info,
            list_sessions,
            validate_url,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
