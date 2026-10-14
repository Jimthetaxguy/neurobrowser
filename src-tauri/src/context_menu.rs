//! Right-click menu for a page webview.
//!
//! The page reports what was under the pointer. This module decides the
//! items and the action. A page cannot name a destination file: Save Image
//! only keeps an http(s) address, and the download handler chooses the path.

use serde::Serialize;
use serde_json::Value;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::{Manager, Runtime};

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long a native right-click still counts. A later report is ignored.
pub const NATIVE_CLICK_WINDOW: Duration = Duration::from_millis(400);

/// Minimum time between menus on the same tab.
pub const MENU_COOLDOWN: Duration = Duration::from_millis(500);

const BOUND_MENUS: usize = 8;

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

struct ShownMenu {
    menu_id: u64,
    page_id: usize,
}

struct MenuBook {
    clicks: HashMap<usize, Instant>,
    last_shown: HashMap<usize, Instant>,
    shown: Option<ShownMenu>,
    bound: Vec<(u64, ContextHit)>,
    next_id: u64,
}

impl Default for MenuBook {
    fn default() -> Self {
        Self {
            clicks: HashMap::new(),
            last_shown: HashMap::new(),
            shown: None,
            bound: Vec::new(),
            next_id: 0,
        }
    }
}

pub struct ContextMenuSession {
    book: Mutex<MenuBook>,
}

impl Default for ContextMenuSession {
    fn default() -> Self {
        Self {
            book: Mutex::new(MenuBook::default()),
        }
    }
}

impl ContextMenuSession {
    /// A native right-click on this tab, recorded outside the page script.
    pub fn note_native_click(&self, page_id: usize) {
        self.note_native_click_at(page_id, Instant::now());
    }

    fn note_native_click_at(&self, page_id: usize, now: Instant) {
        self.lock().clicks.insert(page_id, now);
    }

    /// Open a menu for this report, or ignore it.
    ///
    /// Ignored when no native right-click landed on this tab in the last few
    /// hundred milliseconds, when the tab is not the visible one, when a menu
    /// is already showing, or when this tab showed a menu too recently.
    /// The returned id is embedded in the item ids. A later report cannot
    /// change what those items do.
    pub fn try_open(
        &self,
        page_id: usize,
        active_page: Option<usize>,
        hit: ContextHit,
    ) -> Option<u64> {
        self.try_open_at(page_id, active_page, hit, Instant::now())
    }

    fn try_open_at(
        &self,
        page_id: usize,
        active_page: Option<usize>,
        hit: ContextHit,
        now: Instant,
    ) -> Option<u64> {
        let mut book = self.lock();
        if book.shown.is_some() || active_page != Some(page_id) {
            return None;
        }
        let fresh = book
            .clicks
            .get(&page_id)
            .is_some_and(|clicked| now.saturating_duration_since(*clicked) <= NATIVE_CLICK_WINDOW);
        if !fresh {
            book.clicks.remove(&page_id);
            return None;
        }
        if book
            .last_shown
            .get(&page_id)
            .is_some_and(|shown| now.saturating_duration_since(*shown) < MENU_COOLDOWN)
        {
            return None;
        }
        book.clicks.remove(&page_id);
        book.next_id = book.next_id.saturating_add(1);
        let menu_id = book.next_id;
        book.last_shown.insert(page_id, now);
        book.shown = Some(ShownMenu { menu_id, page_id });
        book.bound.push((menu_id, hit));
        if book.bound.len() > BOUND_MENUS {
            book.bound.remove(0);
        }
        Some(menu_id)
    }

    /// The native menu has closed. A new report may be considered.
    pub fn finish_showing(&self, menu_id: u64) {
        let mut book = self.lock();
        if book
            .shown
            .as_ref()
            .is_some_and(|shown| shown.menu_id == menu_id)
        {
            book.shown = None;
        }
    }

    pub fn forget_page(&self, page_id: usize) {
        let mut book = self.lock();
        book.clicks.remove(&page_id);
        book.last_shown.remove(&page_id);
        if book
            .shown
            .as_ref()
            .is_some_and(|shown| shown.page_id == page_id)
        {
            book.shown = None;
        }
    }

    /// The action bound to this item when its menu was built.
    /// `None` when the id is not one of our items, or that menu was never opened.
    pub fn command(&self, item_id: &str) -> Option<ContextCommand> {
        let (menu_id, action) = parse_context_item(item_id)?;
        let book = self.lock();
        let hit = book
            .bound
            .iter()
            .find(|(id, _)| *id == menu_id)
            .map(|(_, hit)| hit)?;
        command_for(action, hit)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, MenuBook> {
        self.book.lock().unwrap_or_else(|error| error.into_inner())
    }
}

pub fn is_context_item(id: &str) -> bool {
    parse_context_item(id).is_some()
}

/// `ctx-7-open-link` -> `(7, "open-link")`. The number is the menu that was
/// showing when the item was created. A page cannot invent a matching target.
pub fn parse_context_item(id: &str) -> Option<(u64, &str)> {
    let rest = id.strip_prefix("ctx-")?;
    let (id_text, action) = rest.split_once('-')?;
    if id_text.is_empty() || !id_text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let menu_id: u64 = id_text.parse().ok()?;
    if menu_id == 0 || id_text != menu_id.to_string() {
        return None;
    }
    is_action(action).then_some((menu_id, action))
}

pub fn item_id(menu_id: u64, action: &str) -> String {
    format!("ctx-{menu_id}-{action}")
}

fn is_action(action: &str) -> bool {
    matches!(
        action,
        "open-link"
            | "copy-link"
            | "open-image"
            | "save-image"
            | "copy-image"
            | "copy-selection"
            | "search-selection"
            | "back"
            | "forward"
            | "reload"
    )
}

/// What the shell should do for one menu id. `None` when the hit has nothing
/// for that item. The command never carries a filesystem path.
pub fn command_for(id: &str, hit: &ContextHit) -> Option<ContextCommand> {
    match id {
        "open-link" => Some(ContextCommand::OpenTab {
            page_id: hit.page_id,
            url: allowed_http_url(hit.link_url.as_deref()?)?,
        }),
        "copy-link" => Some(ContextCommand::Copy {
            text: allowed_http_url(hit.link_url.as_deref()?)?,
        }),
        "open-image" => Some(ContextCommand::OpenTab {
            page_id: hit.page_id,
            url: allowed_http_url(hit.image_url.as_deref()?)?,
        }),
        "save-image" => Some(ContextCommand::SaveImage {
            page_id: hit.page_id,
            url: allowed_http_url(hit.image_url.as_deref()?)?,
        }),
        "copy-image" => Some(ContextCommand::Copy {
            text: allowed_http_url(hit.image_url.as_deref()?)?,
        }),
        "copy-selection" => Some(ContextCommand::Copy {
            text: hit.selection.clone()?,
        }),
        "search-selection" => Some(ContextCommand::OpenTab {
            page_id: hit.page_id,
            url: duckduckgo_search_url(hit.selection.as_deref()?),
        }),
        "back" => Some(ContextCommand::Back),
        "forward" => Some(ContextCommand::Forward),
        "reload" => Some(ContextCommand::Reload),
        _ => None,
    }
}

pub fn menu_model(hit: &ContextHit) -> ContextMenuModel {
    let mut content = Vec::new();
    if hit.link_url.is_some() {
        content.push(entry("open-link", "Open Link in New Tab"));
        content.push(entry("copy-link", "Copy Link Address"));
    }
    if hit.image_url.is_some() {
        content.push(entry("open-image", "Open Image in New Tab"));
        content.push(entry("save-image", "Save Image"));
        content.push(entry("copy-image", "Copy Image Address"));
    }
    if let Some(selection) = &hit.selection {
        content.push(entry("copy-selection", "Copy"));
        content.push(entry("search-selection", search_label(selection).as_str()));
    }
    ContextMenuModel {
        content,
        page: vec![
            entry("back", "Back"),
            entry("forward", "Forward"),
            entry("reload", "Reload"),
        ],
    }
}

pub fn build_menu<R: Runtime, M: Manager<R>>(
    manager: &M,
    menu_id: u64,
    hit: &ContextHit,
) -> tauri::Result<Menu<R>> {
    let model = menu_model(hit);
    let menu = Menu::new(manager)?;
    for item in &model.content {
        let entry = MenuItem::with_id(
            manager,
            item_id(menu_id, item.id),
            &item.label,
            true,
            None::<&str>,
        )?;
        menu.append(&entry)?;
    }
    if !model.content.is_empty() {
        let separator = PredefinedMenuItem::separator(manager)?;
        menu.append(&separator)?;
    }
    for item in &model.page {
        let entry = MenuItem::with_id(
            manager,
            item_id(menu_id, item.id),
            &item.label,
            true,
            None::<&str>,
        )?;
        menu.append(&entry)?;
    }
    Ok(menu)
}

/// Keep a page-supplied point inside the webview. Values are CSS pixels.
pub fn clamp_menu_point(x: f64, y: f64, width: f64, height: f64) -> (f64, f64) {
    let width = if width.is_finite() && width > 0.0 {
        width
    } else {
        0.0
    };
    let height = if height.is_finite() && height > 0.0 {
        height
    } else {
        0.0
    };
    let x = if x.is_finite() {
        x.clamp(0.0, width)
    } else {
        0.0
    };
    let y = if y.is_finite() {
        y.clamp(0.0, height)
    } else {
        0.0
    };
    (x, y)
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
                "open-link",
                "copy-link",
                "open-image",
                "save-image",
                "copy-image",
                "copy-selection",
                "search-selection",
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
        let save = command_for("save-image", &hit).expect("save");
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
        assert!(command_for("open-link", &hit).is_none());
        assert!(command_for("save-image", &hit).is_none());
        let loopback = hit_from(json!({ "imageUrl": "http://127.0.0.1/pic.png" }));
        assert!(loopback.image_url.is_none());
        assert!(command_for("save-image", &loopback).is_none());
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
        match command_for("search-selection", &hit).expect("search") {
            ContextCommand::OpenTab { page_id, url } => {
                assert_eq!(page_id, 4);
                assert_eq!(url, "https://duckduckgo.com/?q=caf%C3%A9");
            }
            other => panic!("expected a tab, got {other:?}"),
        }
        assert_eq!(command_for("back", &hit), Some(ContextCommand::Back));
        assert_eq!(command_for("forward", &hit), Some(ContextCommand::Forward));
        assert_eq!(command_for("reload", &hit), Some(ContextCommand::Reload));
        assert!(!is_context_item("reload"));
        assert!(!is_context_item("ctx-reload"));
        assert!(is_context_item("ctx-1-reload"));
        let long = "word ".repeat(20);
        let label = search_label(long.trim());
        assert!(label.ends_with('…') || label.contains('…'));
        assert!(label.starts_with("Search DuckDuckGo for \""));
    }

    fn sample_hit(url: &str) -> ContextHit {
        ContextHit {
            page_id: 1,
            link_url: Some(url.into()),
            image_url: None,
            selection: None,
            x: 10.0,
            y: 12.0,
        }
    }

    #[test]
    fn a_report_without_a_native_right_click_does_not_open_a_menu() {
        let session = ContextMenuSession::default();
        let now = Instant::now();
        assert!(session
            .try_open_at(1, Some(1), sample_hit("https://8.8.8.8/a"), now)
            .is_none());
        session.note_native_click_at(1, now);
        assert!(session
            .try_open_at(
                1,
                Some(1),
                sample_hit("https://8.8.8.8/a"),
                now + NATIVE_CLICK_WINDOW + Duration::from_millis(1)
            )
            .is_none());
    }

    #[test]
    fn a_fresh_native_click_opens_only_the_active_tab_and_then_waits() {
        let session = ContextMenuSession::default();
        let now = Instant::now();
        session.note_native_click_at(2, now);
        assert!(session
            .try_open_at(2, Some(1), sample_hit("https://8.8.8.8/bg"), now)
            .is_none());
        assert!(session
            .try_open_at(2, None, sample_hit("https://8.8.8.8/bg"), now)
            .is_none());
        let menu_id = session
            .try_open_at(2, Some(2), sample_hit("https://8.8.8.8/ok"), now)
            .expect("active tab");
        session.finish_showing(menu_id);
        session.note_native_click_at(2, now + Duration::from_millis(10));
        assert!(session
            .try_open_at(
                2,
                Some(2),
                sample_hit("https://8.8.8.8/again"),
                now + Duration::from_millis(10)
            )
            .is_none());
        let later = now + MENU_COOLDOWN;
        session.note_native_click_at(2, later);
        assert!(session
            .try_open_at(2, Some(2), sample_hit("https://8.8.8.8/later"), later)
            .is_some());
    }

    #[test]
    fn a_report_while_a_menu_is_open_cannot_replace_its_target() {
        let session = ContextMenuSession::default();
        let now = Instant::now();
        session.note_native_click_at(1, now);
        let menu_id = session
            .try_open_at(1, Some(1), sample_hit("https://8.8.8.8/first"), now)
            .expect("menu");
        session.note_native_click_at(1, now);
        assert!(session
            .try_open_at(1, Some(1), sample_hit("https://8.8.8.8/swapped"), now)
            .is_none());
        match session
            .command(&item_id(menu_id, "open-link"))
            .expect("bound")
        {
            ContextCommand::OpenTab { url, .. } => assert_eq!(url, "https://8.8.8.8/first"),
            other => panic!("expected the original tab, got {other:?}"),
        }
        assert!(session
            .command(&item_id(menu_id + 1, "open-link"))
            .is_none());
        session.finish_showing(menu_id);
        let later = now + MENU_COOLDOWN;
        session.note_native_click_at(1, later);
        let next = session
            .try_open_at(1, Some(1), sample_hit("https://8.8.8.8/second"), later)
            .expect("next menu");
        assert_ne!(next, menu_id);
        match session
            .command(&item_id(menu_id, "open-link"))
            .expect("old")
        {
            ContextCommand::OpenTab { url, .. } => assert_eq!(url, "https://8.8.8.8/first"),
            other => panic!("expected the first target, got {other:?}"),
        }
        match session.command(&item_id(next, "open-link")).expect("new") {
            ContextCommand::OpenTab { url, .. } => assert_eq!(url, "https://8.8.8.8/second"),
            other => panic!("expected the second target, got {other:?}"),
        }
    }

    #[test]
    fn menu_coordinates_stay_inside_the_page_webview() {
        assert_eq!(clamp_menu_point(12.0, 4.0, 800.0, 600.0), (12.0, 4.0));
        assert_eq!(clamp_menu_point(-40.0, -1.0, 800.0, 600.0), (0.0, 0.0));
        assert_eq!(
            clamp_menu_point(10_000.0, 900.0, 800.0, 600.0),
            (800.0, 600.0)
        );
        assert_eq!(
            clamp_menu_point(f64::NAN, f64::INFINITY, 800.0, 600.0),
            (0.0, 0.0)
        );
        assert_eq!(clamp_menu_point(3.0, 4.0, 0.0, -5.0), (0.0, 0.0));
    }
}
