//! Right-click menu for a page webview.
//!
//! The page reports what was under the pointer. This module decides the
//! items and the action. A page cannot name a destination file: Save Image
//! only keeps an http(s) address, and the download handler chooses the path.

use serde::Serialize;
use serde_json::Value;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::{Manager, Runtime};

use std::sync::Mutex;

/// Report id the page script uses for a trusted context menu.
/// It is not a pending runtime request id.
pub const CONTEXT_MENU_REQUEST: &str = "context-menu";

/// Main-webview event. Payload is `{ text }` and nothing else.
pub const CONTEXT_COPY_EVENT: &str = "context-copy";

const URL_LIMIT: usize = 16_384;
const SELECTION_LIMIT: usize = 2_000;
const LABEL_LIMIT: usize = 40;

#[derive(Clone, Debug, PartialEq)]
pub struct ContextHit {
    pub page_id: usize,
    pub link_url: Option<String>,
    pub image_url: Option<String>,
    pub selection: Option<String>,
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContextEntry {
    pub id: &'static str,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContextMenuModel {
    pub content: Vec<ContextEntry>,
    pub page: Vec<ContextEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContextCommand {
    OpenTab { page_id: usize, url: String },
    Copy { text: String },
    SaveImage { page_id: usize, url: String },
    Back,
    Forward,
    Reload,
}

#[derive(Clone, Serialize)]
pub struct ContextCopy<'a> {
    pub text: &'a str,
}

pub struct ContextMenuSession {
    hit: Mutex<Option<ContextHit>>,
    shown: Mutex<Option<Box<dyn std::any::Any + Send>>>,
}

impl Default for ContextMenuSession {
    fn default() -> Self {
        Self {
            hit: Mutex::new(None),
            shown: Mutex::new(None),
        }
    }
}

impl ContextMenuSession {
    pub fn store(&self, hit: ContextHit) {
        *self.lock_hit() = Some(hit);
    }

    pub fn current(&self) -> Option<ContextHit> {
        self.lock_hit().clone()
    }

    /// Keep the native menu alive until the next right-click replaces it.
    pub fn keep_shown<T: Send + 'static>(&self, menu: T) {
        *self.shown.lock().unwrap_or_else(|error| error.into_inner()) = Some(Box::new(menu));
    }

    fn lock_hit(&self) -> std::sync::MutexGuard<'_, Option<ContextHit>> {
        self.hit.lock().unwrap_or_else(|error| error.into_inner())
    }
}

pub fn is_context_item(id: &str) -> bool {
    matches!(
        id,
        "ctx-open-link"
            | "ctx-copy-link"
            | "ctx-open-image"
            | "ctx-save-image"
            | "ctx-copy-image"
            | "ctx-copy-selection"
            | "ctx-search-selection"
            | "ctx-back"
            | "ctx-forward"
            | "ctx-reload"
    )
}

/// What the shell should do for one menu id. `None` when the hit has nothing
/// for that item. The command never carries a filesystem path.
pub fn command_for(id: &str, hit: &ContextHit) -> Option<ContextCommand> {
    match id {
        "ctx-open-link" => Some(ContextCommand::OpenTab {
            page_id: hit.page_id,
            url: allowed_http_url(hit.link_url.as_deref()?)?,
        }),
        "ctx-copy-link" => Some(ContextCommand::Copy {
            text: allowed_http_url(hit.link_url.as_deref()?)?,
        }),
        "ctx-open-image" => Some(ContextCommand::OpenTab {
            page_id: hit.page_id,
            url: allowed_http_url(hit.image_url.as_deref()?)?,
        }),
        "ctx-save-image" => Some(ContextCommand::SaveImage {
            page_id: hit.page_id,
            url: allowed_http_url(hit.image_url.as_deref()?)?,
        }),
        "ctx-copy-image" => Some(ContextCommand::Copy {
            text: allowed_http_url(hit.image_url.as_deref()?)?,
        }),
        "ctx-copy-selection" => Some(ContextCommand::Copy {
            text: hit.selection.clone()?,
        }),
        "ctx-search-selection" => Some(ContextCommand::OpenTab {
            page_id: hit.page_id,
            url: duckduckgo_search_url(hit.selection.as_deref()?),
        }),
        "ctx-back" => Some(ContextCommand::Back),
        "ctx-forward" => Some(ContextCommand::Forward),
        "ctx-reload" => Some(ContextCommand::Reload),
        _ => None,
    }
}

pub fn menu_model(hit: &ContextHit) -> ContextMenuModel {
    let mut content = Vec::new();
    if hit.link_url.is_some() {
        content.push(entry("ctx-open-link", "Open Link in New Tab"));
        content.push(entry("ctx-copy-link", "Copy Link Address"));
    }
    if hit.image_url.is_some() {
        content.push(entry("ctx-open-image", "Open Image in New Tab"));
        content.push(entry("ctx-save-image", "Save Image"));
        content.push(entry("ctx-copy-image", "Copy Image Address"));
    }
    if let Some(selection) = &hit.selection {
        content.push(entry("ctx-copy-selection", "Copy"));
        content.push(entry(
            "ctx-search-selection",
            search_label(selection).as_str(),
        ));
    }
    ContextMenuModel {
        content,
        page: vec![
            entry("ctx-back", "Back"),
            entry("ctx-forward", "Forward"),
            entry("ctx-reload", "Reload"),
        ],
    }
}

pub fn build_menu<R: Runtime, M: Manager<R>>(
    manager: &M,
    hit: &ContextHit,
) -> tauri::Result<Menu<R>> {
    let model = menu_model(hit);
    let menu = Menu::new(manager)?;
    for item in &model.content {
        let entry = MenuItem::with_id(manager, item.id, &item.label, true, None::<&str>)?;
        menu.append(&entry)?;
    }
    if !model.content.is_empty() {
        let separator = PredefinedMenuItem::separator(manager)?;
        menu.append(&separator)?;
    }
    for item in &model.page {
        let entry = MenuItem::with_id(manager, item.id, &item.label, true, None::<&str>)?;
        menu.append(&entry)?;
    }
    Ok(menu)
}

/// Expression the page webview evaluates. One JSON string, the image address.
/// There is no path argument.
pub fn save_image_expression(url: &str) -> String {
    let url_json = serde_json::to_string(url).unwrap_or_else(|_| "\"\"".to_string());
    format!(
        "(() => {{ const save = window.__NEUROBROWSER_SAVE_IMAGE__; if (typeof save === \"function\") save({url_json}); }})()"
    )
}

pub fn duckduckgo_search_url(query: &str) -> String {
    format!("https://duckduckgo.com/?q={}", encode_uri_component(query))
}

/// Read a context-menu report. `page_url` is the webview's real address.
/// `path`, `destination`, and `filename` on the report are ignored.
pub fn context_hit_from_report(page_url: &str, body: Option<&Value>) -> ContextHit {
    let body = body.filter(|value| value.is_object());
    let link_url = body
        .and_then(|value| value.get("linkUrl"))
        .and_then(|value| value.as_str())
        .and_then(|value| resolve_http_url(page_url, value));
    let image_url = body
        .and_then(|value| value.get("imageUrl"))
        .and_then(|value| value.as_str())
        .and_then(|value| resolve_http_url(page_url, value));
    let selection = body
        .and_then(|value| value.get("selection"))
        .and_then(|value| value.as_str())
        .and_then(selection_text);
    let x = coordinate(
        body.and_then(|value| value.get("x"))
            .and_then(|value| value.as_f64()),
    );
    let y = coordinate(
        body.and_then(|value| value.get("y"))
            .and_then(|value| value.as_f64()),
    );
    ContextHit {
        page_id: 0,
        link_url,
        image_url,
        selection,
        x,
        y,
    }
}

fn entry(id: &'static str, label: &str) -> ContextEntry {
    ContextEntry {
        id,
        label: label.to_string(),
    }
}

fn search_label(selection: &str) -> String {
    let mut preview = String::new();
    let mut pending_space = false;
    for ch in selection.chars() {
        if ch.is_whitespace() {
            if !preview.is_empty() {
                pending_space = true;
            }
            continue;
        }
        if pending_space {
            preview.push(' ');
            pending_space = false;
        }
        if preview.chars().count() >= LABEL_LIMIT {
            preview.push('…');
            break;
        }
        preview.push(ch);
    }
    format!("Search DuckDuckGo for \"{preview}\"")
}

fn selection_text(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let text: String = trimmed.chars().take(SELECTION_LIMIT).collect();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn coordinate(value: Option<f64>) -> f64 {
    match value {
        Some(value) if value.is_finite() && (0.0..=100_000.0).contains(&value) => value,
        _ => 0.0,
    }
}

fn resolve_http_url(page_url: &str, raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() || raw.len() > URL_LIMIT {
        return None;
    }
    let absolute = if let Ok(parsed) = tauri::Url::parse(raw) {
        if parsed.scheme() != "http" && parsed.scheme() != "https" {
            return None;
        }
        parsed.to_string()
    } else {
        let base = tauri::Url::parse(page_url).ok()?;
        base.join(raw).ok()?.to_string()
    };
    allowed_http_url(&absolute)
}

fn allowed_http_url(url: &str) -> Option<String> {
    let url = url.trim();
    if url.is_empty() || url.len() > URL_LIMIT {
        return None;
    }
    if neurobrowser::netguard::blocked_reason(url).is_some() {
        return None;
    }
    Some(url.to_string())
}

/// `encodeURIComponent`: every byte except the unescaped set becomes `%HH`.
fn encode_uri_component(text: &str) -> String {
    const UNESCAPED: &[u8] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_.!~*'()";
    let mut encoded = String::new();
    for byte in text.as_bytes() {
        if UNESCAPED.contains(byte) {
            encoded.push(*byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn hit_from(body: Value) -> ContextHit {
        context_hit_from_report("https://8.8.8.8/dir/page", Some(&body))
    }

    #[test]
    fn a_page_only_menu_is_back_forward_and_reload() {
        let hit = context_hit_from_report("https://8.8.8.8/", None);
        let model = menu_model(&hit);
        assert!(model.content.is_empty());
        assert_eq!(
            model
                .page
                .iter()
                .map(|item| item.label.as_str())
                .collect::<Vec<_>>(),
            vec!["Back", "Forward", "Reload"]
        );
        let labels = model
            .page
            .iter()
            .chain(model.content.iter())
            .map(|item| item.label.as_str())
            .collect::<Vec<_>>();
        assert!(!labels
            .iter()
            .any(|label| label.to_ascii_lowercase().contains("source")));
    }

    #[test]
    fn links_images_and_selection_each_add_their_items() {
        let hit = hit_from(json!({
            "linkUrl": "https://8.8.8.8/next",
            "imageUrl": "/pic.png",
            "selection": "cats & dogs",
            "x": 12.5,
            "y": 40
        }));
        assert_eq!(hit.link_url.as_deref(), Some("https://8.8.8.8/next"));
        assert_eq!(hit.image_url.as_deref(), Some("https://8.8.8.8/pic.png"));
        assert_eq!(hit.selection.as_deref(), Some("cats & dogs"));
        assert_eq!(hit.x, 12.5);
        assert_eq!(hit.y, 40.0);
        let model = menu_model(&hit);
        assert_eq!(
            model.content.iter().map(|item| item.id).collect::<Vec<_>>(),
            vec![
                "ctx-open-link",
                "ctx-copy-link",
                "ctx-open-image",
                "ctx-save-image",
                "ctx-copy-image",
                "ctx-copy-selection",
                "ctx-search-selection",
            ]
        );
        assert_eq!(
            model.content.last().map(|item| item.label.as_str()),
            Some("Search DuckDuckGo for \"cats & dogs\"")
        );
    }

    #[test]
    fn a_reported_path_is_not_part_of_the_hit_or_the_save() {
        let hit = hit_from(json!({
            "linkUrl": "https://8.8.8.8/next",
            "imageUrl": "https://8.8.8.8/pic.png",
            "selection": "cats",
            "path": "/tmp/evil",
            "destination": "/tmp/evil",
            "filename": "../../etc/passwd",
            "savePath": "/tmp/evil"
        }));
        let rendered = format!("{hit:?}");
        assert!(!rendered.contains("/tmp/evil"));
        assert!(!rendered.contains("passwd"));
        assert!(hit.link_url.is_some());
        let save = command_for("ctx-save-image", &hit).expect("save");
        let rendered = format!("{save:?}");
        assert!(rendered.contains("https://8.8.8.8/pic.png"));
        assert!(!rendered.contains("/tmp"));
        assert!(!rendered.contains("passwd"));
        match save {
            ContextCommand::SaveImage { url, .. } => {
                assert_eq!(url, "https://8.8.8.8/pic.png");
            }
            other => panic!("expected save, got {other:?}"),
        }
        let expression = save_image_expression("https://8.8.8.8/pic.png\");alert(1)");
        assert!(expression.contains("__NEUROBROWSER_SAVE_IMAGE__"));
        assert!(expression.contains(r#"https://8.8.8.8/pic.png\");alert(1)"#));
        assert!(!expression.contains("/tmp"));
        assert_eq!(
            duckduckgo_search_url("cats & dogs"),
            "https://duckduckgo.com/?q=cats%20%26%20dogs"
        );
    }

    #[test]
    fn blocked_and_non_http_addresses_do_not_become_menu_items() {
        let hit = hit_from(json!({
            "linkUrl": "javascript:alert(1)",
            "imageUrl": "file:///tmp/pic.png",
            "selection": "   "
        }));
        assert!(hit.link_url.is_none());
        assert!(hit.image_url.is_none());
        assert!(hit.selection.is_none());
        assert!(command_for("ctx-open-link", &hit).is_none());
        assert!(command_for("ctx-save-image", &hit).is_none());
        let loopback = hit_from(json!({ "imageUrl": "http://127.0.0.1/pic.png" }));
        assert!(loopback.image_url.is_none());
        assert!(command_for("ctx-save-image", &loopback).is_none());
    }

    #[test]
    fn search_opens_a_tab_and_page_items_stay_browser_commands() {
        let hit = ContextHit {
            page_id: 4,
            link_url: None,
            image_url: None,
            selection: Some("café".into()),
            x: 0.0,
            y: 0.0,
        };
        match command_for("ctx-search-selection", &hit).expect("search") {
            ContextCommand::OpenTab { page_id, url } => {
                assert_eq!(page_id, 4);
                assert_eq!(url, "https://duckduckgo.com/?q=caf%C3%A9");
            }
            other => panic!("expected a tab, got {other:?}"),
        }
        assert_eq!(command_for("ctx-back", &hit), Some(ContextCommand::Back));
        assert_eq!(
            command_for("ctx-forward", &hit),
            Some(ContextCommand::Forward)
        );
        assert_eq!(
            command_for("ctx-reload", &hit),
            Some(ContextCommand::Reload)
        );
        assert!(!is_context_item("reload"));
        assert!(is_context_item("ctx-reload"));
        let long = "word ".repeat(20);
        let label = search_label(long.trim());
        assert!(label.ends_with('…') || label.contains('…'));
        assert!(label.starts_with("Search DuckDuckGo for \""));
    }
}
