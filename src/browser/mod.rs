use crate::tools::{
    BrowserInterface, BrowserTool, ElementInfo, FormInfo, FormInputInfo, ImageInfo, LinkInfo,
    PageSnapshot, PriceInfo, TableInfo, ToolAction, ToolArgumentDefinition, ToolDefinition,
    ToolRegistry, ToolRisk,
};
use async_trait::async_trait;
use regex_lite::Regex;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::{Arc, Mutex};

static PRICE_REGEX: OnceLock<Regex> = OnceLock::new();

fn get_price_regex() -> &'static Regex {
    PRICE_REGEX.get_or_init(|| Regex::new(r"\$[\d,]+(?:\.\d{1,2})?").expect("invalid price regex"))
}

/// Honest failure for an interactive action attempted on the static HTTP engine.
///
/// `BrowserEngine` fetches and parses HTML but renders no live DOM and executes
/// no JavaScript, so click/type/submit/scroll cannot actually happen. Returning
/// `Ok(())` would be a lie: the tool layer reports success for an action that
/// never occurred, and the agent then proceeds on a false premise (FA-2 "engine
/// honesty"). Instead we return an actionable error, distinguishing an invalid
/// selector, a selector that matches nothing, and a real element this engine
/// simply cannot act on — so the ReAct loop gets a signal it can adapt to.
fn static_interaction_error(html: &str, action: &str, selector: &str) -> String {
    match Selector::parse(selector) {
        Err(_) => format!("Cannot {action}: invalid CSS selector '{selector}'"),
        Ok(parsed) => {
            if Html::parse_document(html).select(&parsed).next().is_some() {
                format!(
                    "Cannot {action} '{selector}': the static HTTP engine renders no live DOM \
                     and executes no JavaScript, so interactive actions have no effect. Use the \
                     interactive runtime to {action}."
                )
            } else {
                format!(
                    "Cannot {action}: no element matches selector '{selector}' on the current page"
                )
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageConfig {
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub user_agent: String,
}

impl Default for PageConfig {
    fn default() -> Self {
        Self {
            viewport_width: 1280,
            viewport_height: 720,
            user_agent: "NeuroBrowser/0.1".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageState {
    pub url: String,
    pub title: String,
    pub html: String,
    pub text: String,
    pub scroll_x: f32,
    pub scroll_y: f32,
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub interactive_ready: bool,
}

pub struct BrowserEngine {
    config: PageConfig,
    state: Mutex<PageState>,
    http_client: reqwest::Client,
}

impl BrowserEngine {
    pub fn new(config: PageConfig) -> Self {
        let http_client = reqwest::Client::builder()
            .user_agent(config.user_agent.clone())
            .timeout(std::time::Duration::from_secs(30))
            // Every redirect hop is re-validated. Without this the guard below only
            // inspects the URL we were ASKED for, and a public host answering
            // `302 -> http://169.254.169.254/` is followed and its body returned as
            // page content.
            .redirect(crate::netguard::redirect_policy())
            // Filter blocked addresses inside the RESOLVER, not only in a pre-flight
            // check. A pre-flight resolve-then-judge is a TOCTOU: reqwest performs its
            // own second lookup to connect, and a name can answer public once and
            // loopback the next time. Judging inside the resolver makes the addresses
            // checked and the addresses connected to the same resolution.
            .dns_resolver(crate::netguard::guarded_resolver())
            .build()
            .expect("failed to create HTTP client");

        Self {
            state: Mutex::new(PageState {
                url: String::new(),
                title: String::new(),
                html: String::new(),
                text: String::new(),
                scroll_x: 0.0,
                scroll_y: 0.0,
                viewport_width: config.viewport_width,
                viewport_height: config.viewport_height,
                interactive_ready: false,
            }),
            http_client,
            config,
        }
    }
}

#[async_trait]
impl BrowserInterface for BrowserEngine {
    async fn navigate(&self, url: &str) -> Result<(), String> {
        // Shared boundary — the same check the Tauri runtime performs. Covers scheme,
        // literal addresses, every resolved address, and fails closed on parse or
        // resolution failure.
        if let Some(reason) = crate::netguard::blocked_reason(url) {
            tracing::warn!("Blocked navigation to {}: {}", url, reason);
            return Err(reason.to_string());
        }

        let response = self.http_client.get(url).send().await.map_err(|e| {
            tracing::error!("HTTP request failed for {}: {}", url, e);
            format!("Failed to fetch URL: {}", e)
        })?;

        if !response.status().is_success() {
            let status = response.status();
            tracing::error!("HTTP error {} for {}", status, url);
            return Err(format!("HTTP error: {}", status));
        }

        let html = response.text().await.map_err(|e| {
            tracing::error!("Failed to read response body for {}: {}", url, e);
            format!("Failed to read response: {}", e)
        })?;

        let snapshot = snapshot_from_html(
            url,
            &html,
            self.config.viewport_width,
            self.config.viewport_height,
            false,
        );

        let mut state = self.state.lock().map_err(|e| e.to_string())?;
        state.url = snapshot.url.clone();
        state.title = snapshot.title.clone();
        state.html = html;
        state.text = snapshot.text.clone().unwrap_or_default();
        state.scroll_x = snapshot.scroll_x;
        state.scroll_y = snapshot.scroll_y;
        state.viewport_width = snapshot.viewport_width;
        state.viewport_height = snapshot.viewport_height;
        state.interactive_ready = snapshot.interactive_ready;

        tracing::info!("Navigated to: {}", url);
        Ok(())
    }

    async fn query_selector(&self, selector: &str) -> Result<Vec<ElementInfo>, String> {
        let html = self.state.lock().map_err(|e| e.to_string())?.html.clone();
        query_selector_from_html(&html, selector)
    }

    async fn get_text(&self, selector: &str) -> Result<String, String> {
        let elements = self.query_selector(selector).await?;
        Ok(elements
            .iter()
            .map(|element| element.text.clone())
            .collect::<Vec<_>>()
            .join("\n"))
    }

    async fn click(&self, selector: &str) -> Result<(), String> {
        let html = self.state.lock().map_err(|e| e.to_string())?.html.clone();
        Err(static_interaction_error(&html, "click", selector))
    }

    async fn type_text(&self, selector: &str, _text: &str) -> Result<(), String> {
        // `_text` is the typed value; it is intentionally unused and never
        // logged, since tracing output flows to the log sink.
        let html = self.state.lock().map_err(|e| e.to_string())?.html.clone();
        Err(static_interaction_error(&html, "type into", selector))
    }

    async fn submit_form(&self, selector: &str) -> Result<(), String> {
        let html = self.state.lock().map_err(|e| e.to_string())?.html.clone();
        Err(static_interaction_error(&html, "submit", selector))
    }

    async fn scroll_to(&self, selector: &str) -> Result<(), String> {
        let html = self.state.lock().map_err(|e| e.to_string())?.html.clone();
        Err(static_interaction_error(&html, "scroll to", selector))
    }

    async fn scroll_by(&self, x: f32, y: f32) -> Result<(), String> {
        Err(format!(
            "Cannot scroll by ({x}, {y}): the static HTTP engine has no live viewport, so \
             scrolling has no effect. Use the interactive runtime to scroll."
        ))
    }

    async fn snapshot(&self) -> Result<PageSnapshot, String> {
        let state = self.state.lock().map_err(|e| e.to_string())?.clone();
        let mut snapshot = snapshot_from_html(
            &state.url,
            &state.html,
            state.viewport_width,
            state.viewport_height,
            state.interactive_ready,
        );
        snapshot.title = state.title;
        snapshot.scroll_x = state.scroll_x;
        snapshot.scroll_y = state.scroll_y;
        Ok(snapshot)
    }
}

pub fn default_tool_registry() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(NavigateTool));
    registry.register(Arc::new(WaitTool));
    registry.register(Arc::new(QueryDomTool));
    registry.register(Arc::new(GetTextTool));
    registry.register(Arc::new(GetLinksTool));
    registry.register(Arc::new(GetPricesTool));
    registry.register(Arc::new(GetTablesTool));
    registry.register(Arc::new(ClickTool));
    registry.register(Arc::new(TypeTool));
    registry.register(Arc::new(ScrollToTool));
    registry.register(Arc::new(ScrollByTool));
    registry.register(Arc::new(SubmitFormTool));
    registry.register(Arc::new(KeypressTool));
    registry.register(Arc::new(ScreenshotTool));
    registry.register(Arc::new(BackTool));
    registry.register(Arc::new(ForwardTool));
    registry.register(Arc::new(ReloadTool));
    registry
}

/// The 17 browser tools plus `search_personal_memory` and `inspect_active_page`.
///
/// `memory` is durable `neuro_memory::MemoryService` state. `policy` gates
/// `inspect_active_page` only. `ReActAgent::with_memory` builds this registry
/// with `CapturePolicy::default`.
pub fn default_tool_registry_with_memory(
    memory: Arc<neuro_memory::MemoryService>,
    policy: neuro_memory::CapturePolicy,
) -> ToolRegistry {
    let mut registry = default_tool_registry();
    crate::tools::memory_tools::register_memory_tools(&mut registry, memory, policy);
    registry
}

pub fn enrich_snapshot(snapshot: &mut PageSnapshot) {
    if !snapshot.prices.is_empty() {
        return;
    }

    let source_text = snapshot
        .text
        .as_deref()
        .or(snapshot.html.as_deref())
        .unwrap_or_default();

    snapshot.prices = extract_prices(source_text);
}

fn snapshot_from_html(
    url: &str,
    html: &str,
    viewport_width: u32,
    viewport_height: u32,
    interactive_ready: bool,
) -> PageSnapshot {
    let doc = Html::parse_document(html);
    let title = doc
        .select(&Selector::parse("title").expect("valid title selector"))
        .next()
        .map(|element| element.text().collect::<String>())
        .unwrap_or_default();

    let text = doc.root_element().text().collect::<Vec<_>>().join(" ");
    let mut snapshot = PageSnapshot {
        url: url.to_string(),
        title,
        html: Some(html.to_string()),
        text: Some(text),
        viewport_width,
        viewport_height,
        scroll_x: 0.0,
        scroll_y: 0.0,
        interactive_ready,
        links: extract_links(&doc),
        images: extract_images(&doc),
        forms: extract_forms(&doc),
        prices: vec![],
        tables: extract_tables(&doc),
    };
    enrich_snapshot(&mut snapshot);
    snapshot
}

fn query_selector_from_html(html: &str, selector: &str) -> Result<Vec<ElementInfo>, String> {
    let parsed_selector = match Selector::parse(selector) {
        Ok(selector) => selector,
        Err(_) => return Err(format!("Cannot query: invalid CSS selector '{selector}'")),
    };

    if html.is_empty() {
        return Ok(vec![]);
    }

    let document = Html::parse_document(html);
    Ok(document
        .select(&parsed_selector)
        .map(|element| ElementInfo {
            tag: element.value().name().to_string(),
            id: element.value().id().map(|value| value.to_string()),
            classes: element
                .value()
                .classes()
                .map(|value| value.to_string())
                .collect(),
            text: limit_text(&element.text().collect::<Vec<_>>().join(" "), 200),
            attributes: element
                .value()
                .attrs()
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect(),
            selector: selector.to_string(),
        })
        .collect())
}

fn extract_links(doc: &Html) -> Vec<LinkInfo> {
    let selector = Selector::parse("a[href]").expect("valid link selector");
    doc.select(&selector)
        .filter_map(|element| {
            let href = element.value().attr("href")?.to_string();
            Some(LinkInfo {
                href,
                text: limit_text(&element.text().collect::<Vec<_>>().join(" "), 160),
            })
        })
        .collect()
}

fn extract_images(doc: &Html) -> Vec<ImageInfo> {
    let selector = Selector::parse("img").expect("valid image selector");
    doc.select(&selector)
        .map(|element| ImageInfo {
            src: element.value().attr("src").unwrap_or_default().to_string(),
            alt: element.value().attr("alt").unwrap_or_default().to_string(),
            width: element
                .value()
                .attr("width")
                .and_then(|value| value.parse().ok()),
            height: element
                .value()
                .attr("height")
                .and_then(|value| value.parse().ok()),
        })
        .collect()
}

fn extract_forms(doc: &Html) -> Vec<FormInfo> {
    let form_selector = Selector::parse("form").expect("valid form selector");
    let input_selector =
        Selector::parse("input, textarea, select, button").expect("valid input selector");

    doc.select(&form_selector)
        .map(|form| {
            let inputs = form
                .select(&input_selector)
                .map(|input| FormInputInfo {
                    name: input.value().attr("name").unwrap_or_default().to_string(),
                    input_type: input
                        .value()
                        .attr("type")
                        .unwrap_or_else(|| input.value().name())
                        .to_string(),
                    value: input.value().attr("value").map(|value| value.to_string()),
                })
                .collect();

            FormInfo {
                action: form.value().attr("action").unwrap_or_default().to_string(),
                method: form.value().attr("method").unwrap_or("get").to_string(),
                inputs,
            }
        })
        .collect()
}

fn extract_tables(doc: &Html) -> Vec<TableInfo> {
    let table_selector = Selector::parse("table").expect("valid table selector");
    let row_selector = Selector::parse("tr").expect("valid row selector");
    let header_selector = Selector::parse("th").expect("valid header selector");
    let cell_selector = Selector::parse("td").expect("valid cell selector");

    doc.select(&table_selector)
        .map(|table| {
            let headers = table
                .select(&header_selector)
                .map(|header| limit_text(&header.text().collect::<Vec<_>>().join(" "), 120))
                .collect();

            let rows = table
                .select(&row_selector)
                .map(|row| {
                    row.select(&cell_selector)
                        .map(|cell| limit_text(&cell.text().collect::<Vec<_>>().join(" "), 120))
                        .collect::<Vec<_>>()
                })
                .filter(|row| !row.is_empty())
                .collect();

            TableInfo { headers, rows }
        })
        .collect()
}

fn extract_prices(source_text: &str) -> Vec<PriceInfo> {
    get_price_regex()
        .find_iter(source_text)
        .take(50)
        .map(|price_match| {
            let start = price_match.start().saturating_sub(32);
            let end = (price_match.end() + 32).min(source_text.len());
            PriceInfo {
                value: price_match.as_str().to_string(),
                currency: "USD".to_string(),
                context: limit_text(&source_text[start..end], 80),
            }
        })
        .collect()
}

fn limit_text(value: &str, max_len: usize) -> String {
    value.trim().chars().take(max_len).collect()
}

struct NavigateTool;

#[async_trait]
impl BrowserTool for NavigateTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "navigate",
            "Navigate the current page to a URL",
            ToolRisk::new(ToolAction::Navigate),
        )
        .with_arguments(vec![ToolArgumentDefinition::required(
            "url",
            "HTTP or HTTPS URL to open",
        )])
    }

    async fn execute(
        &self,
        args: HashMap<String, String>,
        browser: &dyn BrowserInterface,
    ) -> crate::tools::ToolResult {
        let url = args.get("url").cloned().unwrap_or_default();
        match browser.navigate(&url).await {
            Ok(()) => {
                let _ = browser.wait_for_navigation().await;
                crate::tools::ToolResult::success("navigate", format!("Navigated to {url}"))
            }
            Err(error) => crate::tools::ToolResult::error("navigate", error),
        }
    }
}

struct WaitTool;

#[async_trait]
impl BrowserTool for WaitTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "wait",
            "Wait for page navigation or dynamic page work to settle",
            ToolRisk::new(ToolAction::Wait),
        )
    }

    async fn execute(
        &self,
        _args: HashMap<String, String>,
        browser: &dyn BrowserInterface,
    ) -> crate::tools::ToolResult {
        match browser.wait_for_navigation().await {
            Ok(()) => crate::tools::ToolResult::success("wait", "Page is ready".to_string()),
            Err(error) => crate::tools::ToolResult::error("wait", error),
        }
    }
}

struct QueryDomTool;

#[async_trait]
impl BrowserTool for QueryDomTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "query_dom",
            "Query DOM elements by CSS selector",
            ToolRisk::new(ToolAction::Read),
        )
        .with_arguments(vec![ToolArgumentDefinition::required(
            "selector",
            "CSS selector to query",
        )])
    }

    async fn execute(
        &self,
        args: HashMap<String, String>,
        browser: &dyn BrowserInterface,
    ) -> crate::tools::ToolResult {
        let selector = args.get("selector").cloned().unwrap_or_default();
        match browser.query_selector(&selector).await {
            Ok(elements) => {
                let results: Vec<String> = elements
                    .iter()
                    .map(|element| {
                        format!(
                            "<{} class='{}'>{}</{}>",
                            element.tag,
                            element.classes.join(" "),
                            element.text,
                            element.tag
                        )
                    })
                    .collect();
                crate::tools::ToolResult::success(
                    "query_dom",
                    if results.is_empty() {
                        "No elements found".to_string()
                    } else {
                        results.join("\n")
                    },
                )
            }
            Err(error) => crate::tools::ToolResult::error("query_dom", error),
        }
    }
}

struct GetTextTool;

#[async_trait]
impl BrowserTool for GetTextTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "get_text",
            "Get text content of elements that match a selector",
            ToolRisk::new(ToolAction::Read),
        )
        .with_arguments(vec![ToolArgumentDefinition::required(
            "selector",
            "CSS selector to read",
        )])
    }

    async fn execute(
        &self,
        args: HashMap<String, String>,
        browser: &dyn BrowserInterface,
    ) -> crate::tools::ToolResult {
        let selector = args.get("selector").cloned().unwrap_or_default();
        match browser.get_text(&selector).await {
            Ok(text) => crate::tools::ToolResult::success("get_text", text),
            Err(error) => crate::tools::ToolResult::error("get_text", error),
        }
    }
}

struct GetLinksTool;

#[async_trait]
impl BrowserTool for GetLinksTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "get_links",
            "Get all links on the current page",
            ToolRisk::new(ToolAction::Read),
        )
    }

    async fn execute(
        &self,
        _args: HashMap<String, String>,
        browser: &dyn BrowserInterface,
    ) -> crate::tools::ToolResult {
        match browser.snapshot().await {
            Ok(snapshot) => {
                let links: Vec<String> = snapshot
                    .links
                    .iter()
                    .map(|link| format!("{} - {}", link.text, link.href))
                    .collect();
                crate::tools::ToolResult::success(
                    "get_links",
                    if links.is_empty() {
                        "No links found".to_string()
                    } else {
                        links.join("\n")
                    },
                )
            }
            Err(error) => crate::tools::ToolResult::error("get_links", error),
        }
    }
}

struct GetPricesTool;

#[async_trait]
impl BrowserTool for GetPricesTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "get_prices",
            "Extract price information from the current page",
            ToolRisk::new(ToolAction::Read),
        )
    }

    async fn execute(
        &self,
        _args: HashMap<String, String>,
        browser: &dyn BrowserInterface,
    ) -> crate::tools::ToolResult {
        match browser.snapshot().await {
            Ok(snapshot) => {
                let prices: Vec<String> = snapshot
                    .prices
                    .iter()
                    .map(|price| format!("{} {}", price.currency, price.value))
                    .collect();
                crate::tools::ToolResult::success(
                    "get_prices",
                    if prices.is_empty() {
                        "No prices found".to_string()
                    } else {
                        prices.join("\n")
                    },
                )
            }
            Err(error) => crate::tools::ToolResult::error("get_prices", error),
        }
    }
}

struct GetTablesTool;

#[async_trait]
impl BrowserTool for GetTablesTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "get_tables",
            "Table N: H headers, R rows per table",
            ToolRisk::new(ToolAction::Read),
        )
    }

    async fn execute(
        &self,
        _args: HashMap<String, String>,
        browser: &dyn BrowserInterface,
    ) -> crate::tools::ToolResult {
        match browser.snapshot().await {
            Ok(snapshot) => {
                let tables = snapshot
                    .tables
                    .iter()
                    .enumerate()
                    .map(|(index, table)| {
                        format!(
                            "Table {}: {} headers, {} rows",
                            index + 1,
                            table.headers.len(),
                            table.rows.len()
                        )
                    })
                    .collect::<Vec<_>>();
                crate::tools::ToolResult::success(
                    "get_tables",
                    if tables.is_empty() {
                        "No tables found".to_string()
                    } else {
                        tables.join("\n")
                    },
                )
            }
            Err(error) => crate::tools::ToolResult::error("get_tables", error),
        }
    }
}

struct ClickTool;

#[async_trait]
impl BrowserTool for ClickTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "click",
            "Click an element on the current page",
            ToolRisk::new(ToolAction::Click),
        )
        .with_arguments(vec![ToolArgumentDefinition::required(
            "selector",
            "CSS selector to click",
        )])
    }

    async fn execute(
        &self,
        args: HashMap<String, String>,
        browser: &dyn BrowserInterface,
    ) -> crate::tools::ToolResult {
        let selector = args.get("selector").cloned().unwrap_or_default();
        match browser.click(&selector).await {
            Ok(()) => {
                crate::tools::ToolResult::success("click", "Clicked successfully".to_string())
            }
            Err(error) => crate::tools::ToolResult::error("click", error),
        }
    }
}

struct TypeTool;

#[async_trait]
impl BrowserTool for TypeTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "type",
            "Type text into an input element",
            ToolRisk::new(ToolAction::Type).sensitive(true),
        )
        .with_arguments(vec![
            ToolArgumentDefinition::required("selector", "CSS selector to type into"),
            ToolArgumentDefinition::required("text", "Text to type"),
        ])
    }

    async fn execute(
        &self,
        args: HashMap<String, String>,
        browser: &dyn BrowserInterface,
    ) -> crate::tools::ToolResult {
        let selector = args.get("selector").cloned().unwrap_or_default();
        let text = args.get("text").cloned().unwrap_or_default();
        match browser.type_text(&selector, &text).await {
            // Never echo the raw typed value back into the result string,
            // since it flows unredacted into ToolCallResult, result_preview,
            // and stored session context. Report a length-based confirmation
            // instead.
            Ok(()) => crate::tools::ToolResult::success(
                "type",
                format!("Typed {} characters successfully", text.chars().count()),
            ),
            Err(error) => crate::tools::ToolResult::error("type", error),
        }
    }
}

struct ScrollToTool;

#[async_trait]
impl BrowserTool for ScrollToTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "scroll_to",
            "Scroll an element into view",
            ToolRisk::new(ToolAction::Scroll),
        )
        .with_arguments(vec![ToolArgumentDefinition::required(
            "selector",
            "CSS selector to scroll into view",
        )])
    }

    async fn execute(
        &self,
        args: HashMap<String, String>,
        browser: &dyn BrowserInterface,
    ) -> crate::tools::ToolResult {
        let selector = args.get("selector").cloned().unwrap_or_default();
        match browser.scroll_to(&selector).await {
            Ok(()) => {
                crate::tools::ToolResult::success("scroll_to", "Scrolled to element".to_string())
            }
            Err(error) => crate::tools::ToolResult::error("scroll_to", error),
        }
    }
}

struct ScrollByTool;

#[async_trait]
impl BrowserTool for ScrollByTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "scroll_by",
            "Scroll by pixel offset",
            ToolRisk::new(ToolAction::Scroll),
        )
        .with_arguments(vec![
            ToolArgumentDefinition::required("x", "Horizontal scroll delta in pixels"),
            ToolArgumentDefinition::required("y", "Vertical scroll delta in pixels"),
        ])
    }

    async fn execute(
        &self,
        args: HashMap<String, String>,
        browser: &dyn BrowserInterface,
    ) -> crate::tools::ToolResult {
        let x = args
            .get("x")
            .and_then(|value| value.parse().ok())
            .unwrap_or(0.0);
        let y = args
            .get("y")
            .and_then(|value| value.parse().ok())
            .unwrap_or(0.0);
        match browser.scroll_by(x, y).await {
            Ok(()) => {
                crate::tools::ToolResult::success("scroll_by", format!("Scrolled by {}, {}", x, y))
            }
            Err(error) => crate::tools::ToolResult::error("scroll_by", error),
        }
    }
}

struct SubmitFormTool;

#[async_trait]
impl BrowserTool for SubmitFormTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "submit_form",
            "Submit a form on the current page",
            ToolRisk::new(ToolAction::Submit).externally_visible(true),
        )
        .with_arguments(vec![ToolArgumentDefinition::required(
            "selector",
            "CSS selector for the form or element inside it",
        )])
    }

    async fn execute(
        &self,
        args: HashMap<String, String>,
        browser: &dyn BrowserInterface,
    ) -> crate::tools::ToolResult {
        let selector = args.get("selector").cloned().unwrap_or_default();
        match browser.submit_form(&selector).await {
            Ok(()) => crate::tools::ToolResult::success(
                "submit_form",
                "Form submitted successfully".to_string(),
            ),
            Err(error) => crate::tools::ToolResult::error("submit_form", error),
        }
    }
}

struct KeypressTool;

#[async_trait]
impl BrowserTool for KeypressTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "keypress",
            "Send a keypress to the active page element",
            ToolRisk::new(ToolAction::Keypress),
        )
        .with_arguments(vec![ToolArgumentDefinition::required(
            "key",
            "Keyboard key value, such as Enter or Escape",
        )])
    }

    async fn execute(
        &self,
        args: HashMap<String, String>,
        browser: &dyn BrowserInterface,
    ) -> crate::tools::ToolResult {
        let key = args.get("key").cloned().unwrap_or_default();
        match browser.keypress(&key).await {
            Ok(()) => crate::tools::ToolResult::success("keypress", format!("Pressed {key}")),
            Err(error) => crate::tools::ToolResult::error("keypress", error),
        }
    }
}

struct ScreenshotTool;

#[async_trait]
impl BrowserTool for ScreenshotTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "screenshot",
            "Registered; returns an error on both shipped backends",
            ToolRisk::new(ToolAction::Screenshot),
        )
    }

    async fn execute(
        &self,
        _args: HashMap<String, String>,
        browser: &dyn BrowserInterface,
    ) -> crate::tools::ToolResult {
        match browser.screenshot().await {
            Ok(value) => crate::tools::ToolResult::success("screenshot", value),
            Err(error) => crate::tools::ToolResult::error("screenshot", error),
        }
    }
}

struct BackTool;

#[async_trait]
impl BrowserTool for BackTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "back",
            "Navigate back in browser history",
            ToolRisk::new(ToolAction::Back),
        )
    }

    async fn execute(
        &self,
        _args: HashMap<String, String>,
        browser: &dyn BrowserInterface,
    ) -> crate::tools::ToolResult {
        match browser.browser_back().await {
            Ok(()) => crate::tools::ToolResult::success("back", "Navigated back".to_string()),
            Err(error) => crate::tools::ToolResult::error("back", error),
        }
    }
}

struct ForwardTool;

#[async_trait]
impl BrowserTool for ForwardTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "forward",
            "Navigate forward in browser history",
            ToolRisk::new(ToolAction::Forward),
        )
    }

    async fn execute(
        &self,
        _args: HashMap<String, String>,
        browser: &dyn BrowserInterface,
    ) -> crate::tools::ToolResult {
        match browser.browser_forward().await {
            Ok(()) => crate::tools::ToolResult::success("forward", "Navigated forward".to_string()),
            Err(error) => crate::tools::ToolResult::error("forward", error),
        }
    }
}

struct ReloadTool;

#[async_trait]
impl BrowserTool for ReloadTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "reload",
            "Reload the current page",
            ToolRisk::new(ToolAction::Reload),
        )
    }

    async fn execute(
        &self,
        _args: HashMap<String, String>,
        browser: &dyn BrowserInterface,
    ) -> crate::tools::ToolResult {
        match browser.browser_reload().await {
            Ok(()) => crate::tools::ToolResult::success("reload", "Reloaded page".to_string()),
            Err(error) => crate::tools::ToolResult::error("reload", error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enrich_snapshot_extracts_prices_from_text() {
        let mut snapshot = PageSnapshot {
            text: Some("Total today is $42.50 before tax".to_string()),
            ..PageSnapshot::default()
        };

        enrich_snapshot(&mut snapshot);

        assert_eq!(snapshot.prices.len(), 1);
        assert_eq!(snapshot.prices[0].value, "$42.50");
    }

    #[test]
    fn snapshot_from_html_collects_basic_metadata() {
        let snapshot = snapshot_from_html(
            "https://example.com",
            "<html><head><title>Example</title></head><body><a href='https://a'>Link</a><form action='/buy'><input name='email' /></form><table><tr><th>Name</th></tr><tr><td>Alpha</td></tr></table></body></html>",
            1200,
            800,
            true,
        );

        assert_eq!(snapshot.title, "Example");
        assert_eq!(snapshot.links.len(), 1);
        assert_eq!(snapshot.forms.len(), 1);
        assert_eq!(snapshot.tables.len(), 1);
        assert!(snapshot.interactive_ready);
    }

    #[test]
    fn default_registry_registers_seventeen_explicit_definitions() {
        let registry = default_tool_registry();
        let expected = [
            ("navigate", ToolRisk::new(ToolAction::Navigate)),
            ("wait", ToolRisk::new(ToolAction::Wait)),
            ("query_dom", ToolRisk::new(ToolAction::Read)),
            ("get_text", ToolRisk::new(ToolAction::Read)),
            ("get_links", ToolRisk::new(ToolAction::Read)),
            ("get_prices", ToolRisk::new(ToolAction::Read)),
            ("get_tables", ToolRisk::new(ToolAction::Read)),
            ("click", ToolRisk::new(ToolAction::Click)),
            ("type", ToolRisk::new(ToolAction::Type).sensitive(true)),
            ("scroll_to", ToolRisk::new(ToolAction::Scroll)),
            ("scroll_by", ToolRisk::new(ToolAction::Scroll)),
            (
                "submit_form",
                ToolRisk::new(ToolAction::Submit).externally_visible(true),
            ),
            ("keypress", ToolRisk::new(ToolAction::Keypress)),
            ("screenshot", ToolRisk::new(ToolAction::Screenshot)),
            ("back", ToolRisk::new(ToolAction::Back)),
            ("forward", ToolRisk::new(ToolAction::Forward)),
            ("reload", ToolRisk::new(ToolAction::Reload)),
        ];
        assert_eq!(expected.len(), 17);
        assert_eq!(registry.len(), expected.len());

        for (name, risk) in expected {
            let tool = registry
                .get(name)
                .unwrap_or_else(|| panic!("{name} must be registered"));
            let definition = tool.definition();
            assert_eq!(definition.name, name);
            assert!(
                !definition.description.is_empty(),
                "{name} must declare an explicit description"
            );
            assert_eq!(definition.risk, risk);
        }
    }

    /// Minimal `BrowserInterface` stub for tool-level unit tests.
    struct NoopBrowser;

    #[async_trait]
    impl BrowserInterface for NoopBrowser {
        async fn navigate(&self, _url: &str) -> Result<(), String> {
            Ok(())
        }
        async fn query_selector(&self, _selector: &str) -> Result<Vec<ElementInfo>, String> {
            Ok(Vec::new())
        }
        async fn get_text(&self, _selector: &str) -> Result<String, String> {
            Ok(String::new())
        }
        async fn click(&self, _selector: &str) -> Result<(), String> {
            Ok(())
        }
        async fn type_text(&self, _selector: &str, _text: &str) -> Result<(), String> {
            Ok(())
        }
        async fn submit_form(&self, _selector: &str) -> Result<(), String> {
            Ok(())
        }
        async fn scroll_to(&self, _selector: &str) -> Result<(), String> {
            Ok(())
        }
        async fn scroll_by(&self, _x: f32, _y: f32) -> Result<(), String> {
            Ok(())
        }
        async fn snapshot(&self) -> Result<PageSnapshot, String> {
            Ok(PageSnapshot::default())
        }
    }

    #[tokio::test]
    async fn type_tool_result_does_not_leak_typed_text() {
        let browser = NoopBrowser;
        let mut args = HashMap::new();
        args.insert("selector".to_string(), "#password".to_string());
        args.insert("text".to_string(), "super-secret-value-42".to_string());

        let result = TypeTool.execute(args, &browser).await;

        assert!(result.success);
        assert!(
            !result.result.contains("super-secret-value-42"),
            "type tool result leaked the raw sensitive text: {}",
            result.result
        );
    }

    #[test]
    fn static_interaction_error_reports_matched_element_as_uneffective() {
        // Element exists, but the static engine still cannot act on it: the
        // message must say so honestly rather than imply success.
        let html = r#"<html><body><button id="go">Go</button></body></html>"#;
        let message = static_interaction_error(html, "click", "#go");
        assert!(
            message.contains("no live DOM") && message.contains("click"),
            "matched-element error should explain the engine cannot act: {message}"
        );
        assert!(
            !message.contains("successfully"),
            "honest error must not imply success: {message}"
        );
    }

    #[test]
    fn static_interaction_error_reports_missing_element() {
        let html = r#"<html><body><button id="go">Go</button></body></html>"#;
        let message = static_interaction_error(html, "click", "#missing");
        assert!(
            message.contains("no element matches"),
            "unmatched selector should say so: {message}"
        );
    }

    #[test]
    fn static_interaction_error_reports_invalid_selector() {
        let message = static_interaction_error("<html></html>", "type into", ">>bad<<");
        assert!(
            message.contains("invalid CSS selector"),
            "invalid selector should be flagged distinctly: {message}"
        );
    }

    #[test]
    fn query_selector_from_html_rejects_invalid_css() {
        let error = query_selector_from_html("<html></html>", ">>bad<<")
            .expect_err("invalid CSS must not silently match nothing");
        assert_eq!(error, "Cannot query: invalid CSS selector '>>bad<<'");
        assert_eq!(
            error,
            static_interaction_error("<html></html>", "query", ">>bad<<")
        );
    }
}
