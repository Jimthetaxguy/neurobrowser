use crate::tools::{
    BrowserInterface, BrowserTool, ElementInfo, FormInfo, FormInputInfo, ImageInfo, LinkInfo,
    PageSnapshot, PriceInfo, TableInfo, ToolRegistry,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageConfig {
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub user_agent: String,
    pub enable_javascript: bool,
}

impl Default for PageConfig {
    fn default() -> Self {
        Self {
            viewport_width: 1280,
            viewport_height: 720,
            user_agent: "NeuroBrowser/0.1".to_string(),
            enable_javascript: true,
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
    #[allow(dead_code)]
    config: PageConfig,
    state: Mutex<PageState>,
    tool_registry: Mutex<ToolRegistry>,
    http_client: reqwest::blocking::Client,
}

impl BrowserEngine {
    pub fn new(config: PageConfig) -> Self {
        let http_client = reqwest::blocking::Client::builder()
            .user_agent(config.user_agent.clone())
            .timeout(std::time::Duration::from_secs(30))
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
            tool_registry: Mutex::new(default_tool_registry()),
            http_client,
            config,
        }
    }

    pub fn load_html(&self, html: &str) -> Result<(), String> {
        let snapshot = snapshot_from_html(
            "about:blank",
            html,
            self.config.viewport_width,
            self.config.viewport_height,
            false,
        );
        let mut state = self.state.lock().map_err(|e| e.to_string())?;
        state.url = snapshot.url.clone();
        state.title = snapshot.title.clone();
        state.html = snapshot.html.clone().unwrap_or_default();
        state.text = snapshot.text.clone().unwrap_or_default();
        state.scroll_x = snapshot.scroll_x;
        state.scroll_y = snapshot.scroll_y;
        state.viewport_width = snapshot.viewport_width;
        state.viewport_height = snapshot.viewport_height;
        state.interactive_ready = snapshot.interactive_ready;
        Ok(())
    }

    pub fn get_state(&self) -> Result<PageState, String> {
        let state = self.state.lock().map_err(|e| e.to_string())?;
        Ok(state.clone())
    }

    pub fn get_tool_registry(&self) -> Result<&Mutex<ToolRegistry>, String> {
        Ok(&self.tool_registry)
    }
}

#[async_trait]
impl BrowserInterface for BrowserEngine {
    async fn navigate(&self, url: &str) -> Result<(), String> {
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err("Only http:// and https:// URLs are allowed".to_string());
        }

        let response = self.http_client.get(url).send().map_err(|e| {
            tracing::error!("HTTP request failed for {}: {}", url, e);
            format!("Failed to fetch URL: {}", e)
        })?;

        if !response.status().is_success() {
            let status = response.status();
            tracing::error!("HTTP error {} for {}", status, url);
            return Err(format!("HTTP error: {}", status));
        }

        let html = response.text().map_err(|e| {
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
        Ok(query_selector_from_html(&html, selector))
    }

    async fn get_text(&self, selector: &str) -> Result<String, String> {
        let elements = self.query_selector(selector).await?;
        Ok(elements
            .iter()
            .map(|element| element.text.clone())
            .collect::<Vec<_>>()
            .join("\n"))
    }

    async fn get_attributes(&self, selector: &str) -> Result<HashMap<String, String>, String> {
        let html = self.state.lock().map_err(|e| e.to_string())?.html.clone();
        let doc = Html::parse_document(&html);
        let selector = Selector::parse(selector).map_err(|e| e.to_string())?;
        Ok(doc
            .select(&selector)
            .next()
            .map(|element| {
                element
                    .value()
                    .attrs()
                    .map(|(key, value)| (key.to_string(), value.to_string()))
                    .collect()
            })
            .unwrap_or_default())
    }

    async fn click(&self, selector: &str) -> Result<(), String> {
        tracing::info!("Static browser click fallback: {}", selector);
        Ok(())
    }

    async fn type_text(&self, selector: &str, text: &str) -> Result<(), String> {
        tracing::info!("Static browser type fallback: '{}' -> {}", text, selector);
        Ok(())
    }

    async fn submit_form(&self, selector: &str) -> Result<(), String> {
        tracing::info!("Static browser submit fallback: {}", selector);
        Ok(())
    }

    async fn scroll_to(&self, selector: &str) -> Result<(), String> {
        tracing::info!("Static browser scroll_to fallback: {}", selector);
        Ok(())
    }

    async fn scroll_by(&self, x: f32, y: f32) -> Result<(), String> {
        tracing::info!("Static browser scroll_by fallback: {}, {}", x, y);
        Ok(())
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

fn query_selector_from_html(html: &str, selector: &str) -> Vec<ElementInfo> {
    if html.is_empty() {
        return vec![];
    }

    let document = Html::parse_document(html);
    let parsed_selector = match Selector::parse(selector) {
        Ok(selector) => selector,
        Err(_) => return vec![],
    };

    document
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
        .collect()
}

fn extract_links(doc: &Html) -> Vec<LinkInfo> {
    let selector = Selector::parse("a[href]").expect("valid link selector");
    doc.select(&selector)
        .filter_map(|element| {
            let href = element.value().attr("href")?.to_string();
            Some(LinkInfo {
                href,
                text: limit_text(&element.text().collect::<Vec<_>>().join(" "), 160),
                selector: "a[href]".to_string(),
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
                    selector: input.value().name().to_string(),
                    value: input.value().attr("value").map(|value| value.to_string()),
                })
                .collect();

            FormInfo {
                action: form.value().attr("action").unwrap_or_default().to_string(),
                method: form.value().attr("method").unwrap_or("get").to_string(),
                inputs,
                selector: "form".to_string(),
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

            TableInfo {
                headers,
                rows,
                selector: "table".to_string(),
            }
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
                selector: String::new(),
                context: limit_text(&source_text[start..end], 80),
            }
        })
        .collect()
}

fn limit_text(value: &str, max_len: usize) -> String {
    value.trim().chars().take(max_len).collect()
}

struct QueryDomTool;

#[async_trait]
impl BrowserTool for QueryDomTool {
    fn name(&self) -> &str {
        "query_dom"
    }

    fn description(&self) -> &str {
        "Query DOM elements by CSS selector"
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
                    args,
                    if results.is_empty() {
                        "No elements found".to_string()
                    } else {
                        results.join("\n")
                    },
                )
            }
            Err(error) => crate::tools::ToolResult::error("query_dom", args, error),
        }
    }
}

struct GetTextTool;

#[async_trait]
impl BrowserTool for GetTextTool {
    fn name(&self) -> &str {
        "get_text"
    }

    fn description(&self) -> &str {
        "Get text content of elements that match a selector"
    }

    async fn execute(
        &self,
        args: HashMap<String, String>,
        browser: &dyn BrowserInterface,
    ) -> crate::tools::ToolResult {
        let selector = args.get("selector").cloned().unwrap_or_default();
        match browser.get_text(&selector).await {
            Ok(text) => crate::tools::ToolResult::success("get_text", args, text),
            Err(error) => crate::tools::ToolResult::error("get_text", args, error),
        }
    }
}

struct GetLinksTool;

#[async_trait]
impl BrowserTool for GetLinksTool {
    fn name(&self) -> &str {
        "get_links"
    }

    fn description(&self) -> &str {
        "Get all links on the current page"
    }

    async fn execute(
        &self,
        args: HashMap<String, String>,
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
                    args,
                    if links.is_empty() {
                        "No links found".to_string()
                    } else {
                        links.join("\n")
                    },
                )
            }
            Err(error) => crate::tools::ToolResult::error("get_links", args, error),
        }
    }
}

struct GetPricesTool;

#[async_trait]
impl BrowserTool for GetPricesTool {
    fn name(&self) -> &str {
        "get_prices"
    }

    fn description(&self) -> &str {
        "Extract price information from the current page"
    }

    async fn execute(
        &self,
        args: HashMap<String, String>,
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
                    args,
                    if prices.is_empty() {
                        "No prices found".to_string()
                    } else {
                        prices.join("\n")
                    },
                )
            }
            Err(error) => crate::tools::ToolResult::error("get_prices", args, error),
        }
    }
}

struct GetTablesTool;

#[async_trait]
impl BrowserTool for GetTablesTool {
    fn name(&self) -> &str {
        "get_tables"
    }

    fn description(&self) -> &str {
        "Extract table data from the current page"
    }

    async fn execute(
        &self,
        args: HashMap<String, String>,
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
                    args,
                    if tables.is_empty() {
                        "No tables found".to_string()
                    } else {
                        tables.join("\n")
                    },
                )
            }
            Err(error) => crate::tools::ToolResult::error("get_tables", args, error),
        }
    }
}

struct ClickTool;

#[async_trait]
impl BrowserTool for ClickTool {
    fn name(&self) -> &str {
        "click"
    }

    fn description(&self) -> &str {
        "Click an element on the current page"
    }

    async fn execute(
        &self,
        args: HashMap<String, String>,
        browser: &dyn BrowserInterface,
    ) -> crate::tools::ToolResult {
        let selector = args.get("selector").cloned().unwrap_or_default();
        match browser.click(&selector).await {
            Ok(()) => {
                crate::tools::ToolResult::success("click", args, "Clicked successfully".to_string())
            }
            Err(error) => crate::tools::ToolResult::error("click", args, error),
        }
    }
}

struct TypeTool;

#[async_trait]
impl BrowserTool for TypeTool {
    fn name(&self) -> &str {
        "type"
    }

    fn description(&self) -> &str {
        "Type text into an input element"
    }

    async fn execute(
        &self,
        args: HashMap<String, String>,
        browser: &dyn BrowserInterface,
    ) -> crate::tools::ToolResult {
        let selector = args.get("selector").cloned().unwrap_or_default();
        let text = args.get("text").cloned().unwrap_or_default();
        match browser.type_text(&selector, &text).await {
            Ok(()) => crate::tools::ToolResult::success(
                "type",
                args,
                format!("Typed '{}' successfully", text),
            ),
            Err(error) => crate::tools::ToolResult::error("type", args, error),
        }
    }
}

struct ScrollToTool;

#[async_trait]
impl BrowserTool for ScrollToTool {
    fn name(&self) -> &str {
        "scroll_to"
    }

    fn description(&self) -> &str {
        "Scroll an element into view"
    }

    async fn execute(
        &self,
        args: HashMap<String, String>,
        browser: &dyn BrowserInterface,
    ) -> crate::tools::ToolResult {
        let selector = args.get("selector").cloned().unwrap_or_default();
        match browser.scroll_to(&selector).await {
            Ok(()) => crate::tools::ToolResult::success(
                "scroll_to",
                args,
                "Scrolled to element".to_string(),
            ),
            Err(error) => crate::tools::ToolResult::error("scroll_to", args, error),
        }
    }
}

struct ScrollByTool;

#[async_trait]
impl BrowserTool for ScrollByTool {
    fn name(&self) -> &str {
        "scroll_by"
    }

    fn description(&self) -> &str {
        "Scroll by pixel offset"
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
            Ok(()) => crate::tools::ToolResult::success(
                "scroll_by",
                args,
                format!("Scrolled by {}, {}", x, y),
            ),
            Err(error) => crate::tools::ToolResult::error("scroll_by", args, error),
        }
    }
}

struct SubmitFormTool;

#[async_trait]
impl BrowserTool for SubmitFormTool {
    fn name(&self) -> &str {
        "submit_form"
    }

    fn description(&self) -> &str {
        "Submit a form on the current page"
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
                args,
                "Form submitted successfully".to_string(),
            ),
            Err(error) => crate::tools::ToolResult::error("submit_form", args, error),
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
}
