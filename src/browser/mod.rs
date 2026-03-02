use crate::tools::{BrowserInterface, PageInfo, LinkInfo, ImageInfo, FormInfo, PriceInfo, ElementInfo, ToolRegistry, BrowserTool};
use regex_lite::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use async_trait::async_trait;
use scraper::{Html, Selector};

static PRICE_REGEX: OnceLock<Regex> = OnceLock::new();

fn get_price_regex() -> &'static Regex {
    PRICE_REGEX.get_or_init(|| Regex::new(r"\$[\d,]+\.?\d*").expect("Invalid price regex"))
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
            enable_javascript: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageState {
    pub url: String,
    pub title: String,
    pub html: String,
    pub scroll_x: f32,
    pub scroll_y: f32,
    pub viewport_width: u32,
    pub viewport_height: u32,
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

        let http_client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            config: config.clone(),
            state: Mutex::new(PageState {
                url: String::new(),
                title: String::new(),
                html: String::new(),
                scroll_x: 0.0,
                scroll_y: 0.0,
                viewport_width: config.viewport_width,
                viewport_height: config.viewport_height,
            }),
            tool_registry: Mutex::new(registry),
            http_client,
        }
    }

    pub fn navigate(&self, url: &str) -> Result<(), String> {
        // Validate URL to prevent SSRF attacks
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err("Only http:// and https:// URLs are allowed".to_string());
        }

        let response = self.http_client.get(url).send().map_err(|e| {
            tracing::error!("HTTP request failed for {}: {}", url, e);
            format!("Failed to fetch URL: {}", e)
        })?;

        if !response.status().is_success() {
            let status = response.status();
            let error_msg = format!("HTTP error: {}", status);
            tracing::error!("HTTP error {} for {}", status, url);
            return Err(error_msg);
        }

        let html = response.text().map_err(|e| {
            tracing::error!("Failed to read response body for {}: {}", url, e);
            format!("Failed to read response: {}", e)
        })?;

        let doc = Html::parse_document(&html);
        let title = doc.select(&Selector::parse("title").unwrap())
            .next()
            .map(|el| el.text().collect::<String>())
            .unwrap_or_default();
        
        let mut state = self.state.lock().map_err(|e| e.to_string())?;
        state.url = url.to_string();
        state.title = title;
        state.html = html;
        
        tracing::info!("Navigated to: {}", url);
        Ok(())
    }

    pub fn load_html(&self, html: &str) -> Result<(), String> {
        let doc = Html::parse_document(html);
        
        let title = doc.select(&Selector::parse("title").unwrap())
            .next()
            .map(|el| el.text().collect::<String>())
            .unwrap_or_default();
        
        let mut state = self.state.lock().map_err(|e| e.to_string())?;
        state.url = "about:blank".to_string();
        state.title = title;
        state.html = html.to_string();
        
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

impl BrowserInterface for BrowserEngine {
    fn query_selector(&self, selector: &str) -> Vec<ElementInfo> {
        let html = match self.state.lock() {
            Ok(s) => s.html.clone(),
            Err(_) => return vec![],
        };
        
        if html.is_empty() {
            return vec![];
        }
        
        let doc = Html::parse_document(&html);
        
        match Selector::parse(selector) {
            Ok(sel) => {
                doc.select(&sel).map(|el| {
                    let tag = el.value().name().to_string();
                    let id = el.value().id().map(|s| s.to_string());
                    let classes: Vec<String> = el.value().classes().map(|s| s.to_string()).collect();
                    let text: String = el.text().collect::<Vec<_>>().join(" ").chars().take(200).collect();
                    
                    ElementInfo {
                        tag,
                        id,
                        classes,
                        text,
                        attributes: HashMap::new(),
                        selector: selector.to_string(),
                    }
                }).collect()
            }
            Err(_) => vec![],
        }
    }

    fn get_text(&self, selector: &str) -> String {
        let elements = self.query_selector(selector);
        elements.iter().map(|e| e.text.clone()).collect::<Vec<_>>().join("\n")
    }

    fn get_attributes(&self, selector: &str) -> HashMap<String, String> {
        let html = match self.state.lock() {
            Ok(s) => s.html.clone(),
            Err(_) => return HashMap::new(),
        };
        
        if html.is_empty() {
            return HashMap::new();
        }

        let doc = Html::parse_document(&html);

        match Selector::parse(selector) {
            Ok(sel) => {
                if let Some(el) = doc.select(&sel).next() {
                    el.value().attrs().map(|(k, v)| (k.to_string(), v.to_string())).collect()
                } else {
                    HashMap::new()
                }
            }
            Err(_) => HashMap::new(),
        }
    }

    fn click(&self, selector: &str) -> Result<(), String> {
        tracing::info!("Click: {}", selector);
        Ok(())
    }

    fn type_text(&self, selector: &str, text: &str) -> Result<(), String> {
        tracing::info!("Type '{}' into: {}", text, selector);
        Ok(())
    }

    fn submit_form(&self, selector: &str) -> Result<(), String> {
        tracing::info!("Submit form: {}", selector);
        Ok(())
    }

    fn scroll_to(&self, selector: &str) -> Result<(), String> {
        tracing::info!("Scroll to: {}", selector);
        Ok(())
    }

    fn scroll_by(&self, x: f32, y: f32) -> Result<(), String> {
        tracing::info!("Scroll by: {}, {}", x, y);
        Ok(())
    }

    fn get_page_info(&self) -> PageInfo {
        let state = match self.state.lock() {
            Ok(s) => s,
            Err(_) => return PageInfo::default(),
        };

        let html = state.html.clone();
        
        let mut links = vec![];
        let mut images = vec![];
        let mut forms = vec![];
        let mut prices = vec![];
        
        if !html.is_empty() {
            let doc = Html::parse_document(&html);
            
            // Extract links
            if let Ok(sel) = Selector::parse("a[href]") {
                for el in doc.select(&sel) {
                    let href = el.value().attr("href").unwrap_or("").to_string();
                    let text: String = el.text().collect::<Vec<_>>().join(" ").chars().take(100).collect();
                    if !href.is_empty() {
                        links.push(LinkInfo { href, text, selector: "a".to_string() });
                    }
                }
            }
            
            // Extract images
            if let Ok(sel) = Selector::parse("img") {
                for el in doc.select(&sel) {
                    let src = el.value().attr("src").unwrap_or("").to_string();
                    let alt = el.value().attr("alt").unwrap_or("").to_string();
                    images.push(ImageInfo { src, alt, width: None, height: None });
                }
            }
            
            // Extract forms
            if let Ok(sel) = Selector::parse("form") {
                for el in doc.select(&sel) {
                    let action = el.value().attr("action").unwrap_or("").to_string();
                    let method = el.value().attr("method").unwrap_or("get").to_string();
                    forms.push(FormInfo {
                        action,
                        method,
                        inputs: vec![],
                        selector: "form".to_string(),
                    });
                }
            }
            
            // Extract prices
            if let Ok(sel) = Selector::parse("*") {
                for el in doc.select(&sel) {
                    let text: String = el.text().collect::<Vec<_>>().join(" ");
                    if text.contains('$') || text.contains("USD") || text.contains("price") {
                        if let Some(captures) = get_price_regex().find(&text) {
                            prices.push(PriceInfo {
                                value: captures.as_str().to_string(),
                                currency: "USD".to_string(),
                                selector: "".to_string(),
                                context: text.chars().take(50).collect(),
                            });
                        }
                    }
                }
            }
        }

        PageInfo {
            url: state.url.clone(),
            title: state.title.clone(),
            viewport_width: state.viewport_width,
            viewport_height: state.viewport_height,
            scroll_x: state.scroll_x,
            scroll_y: state.scroll_y,
            links,
            images,
            forms,
            prices,
            tables: vec![],
        }
    }
}

impl Default for PageInfo {
    fn default() -> Self {
        Self {
            url: "".to_string(),
            title: "".to_string(),
            viewport_width: 1280,
            viewport_height: 720,
            scroll_x: 0.0,
            scroll_y: 0.0,
            links: vec![],
            images: vec![],
            forms: vec![],
            prices: vec![],
            tables: vec![],
        }
    }
}

// Tool implementations
struct QueryDomTool;

#[async_trait]
impl BrowserTool for QueryDomTool {
    fn name(&self) -> &str { "query_dom" }
    fn description(&self) -> &str { "Query DOM elements by CSS selector" }
    
    async fn execute(&self, args: HashMap<String, String>, browser: &dyn BrowserInterface) -> crate::tools::ToolResult {
        let selector = args.get("selector").cloned().unwrap_or_default();
        let elements = browser.query_selector(&selector);
        
        let results: Vec<String> = elements.iter()
            .map(|e| format!("<{} class='{}'>{}</{}>", 
                e.tag, 
                e.classes.join(" "), 
                e.text, 
                e.tag))
            .collect();
        
        crate::tools::ToolResult::success(
            "query_dom",
            args,
            if results.is_empty() {
                "No elements found".to_string()
            } else {
                results.join("\n")
            }
        )
    }
}

struct GetTextTool;

#[async_trait]
impl BrowserTool for GetTextTool {
    fn name(&self) -> &str { "get_text" }
    fn description(&self) -> &str { "Get text content of element" }
    
    async fn execute(&self, args: HashMap<String, String>, browser: &dyn BrowserInterface) -> crate::tools::ToolResult {
        let selector = args.get("selector").cloned().unwrap_or_default();
        let text = browser.get_text(&selector);
        
        crate::tools::ToolResult::success("get_text", args, text)
    }
}

struct GetLinksTool;

#[async_trait]
impl BrowserTool for GetLinksTool {
    fn name(&self) -> &str { "get_links" }
    fn description(&self) -> &str { "Get all links on the page" }
    
    async fn execute(&self, args: HashMap<String, String>, browser: &dyn BrowserInterface) -> crate::tools::ToolResult {
        let page_info = browser.get_page_info();
        let links: Vec<String> = page_info.links.iter()
            .map(|l| format!("{} - {}", l.text, l.href))
            .collect();
        
        crate::tools::ToolResult::success(
            "get_links", 
            args, 
            if links.is_empty() {
                "No links found".to_string()
            } else {
                links.join("\n")
            }
        )
    }
}

struct GetPricesTool;

#[async_trait]
impl BrowserTool for GetPricesTool {
    fn name(&self) -> &str { "get_prices" }
    fn description(&self) -> &str { "Extract price information from the page" }
    
    async fn execute(&self, args: HashMap<String, String>, browser: &dyn BrowserInterface) -> crate::tools::ToolResult {
        let page_info = browser.get_page_info();
        let prices: Vec<String> = page_info.prices.iter()
            .map(|p| format!("{} {}", p.currency, p.value))
            .collect();
        
        crate::tools::ToolResult::success(
            "get_prices", 
            args, 
            if prices.is_empty() {
                "No prices found".to_string()
            } else {
                prices.join("\n")
            }
        )
    }
}

struct GetTablesTool;

#[async_trait]
impl BrowserTool for GetTablesTool {
    fn name(&self) -> &str { "get_tables" }
    fn description(&self) -> &str { "Extract table data from the page" }
    
    async fn execute(&self, args: HashMap<String, String>, _browser: &dyn BrowserInterface) -> crate::tools::ToolResult {
        crate::tools::ToolResult::success("get_tables", args, "Table extraction not yet implemented".to_string())
    }
}

struct ClickTool;

#[async_trait]
impl BrowserTool for ClickTool {
    fn name(&self) -> &str { "click" }
    fn description(&self) -> &str { "Click an element on the page" }
    
    async fn execute(&self, args: HashMap<String, String>, browser: &dyn BrowserInterface) -> crate::tools::ToolResult {
        let selector = args.get("selector").cloned().unwrap_or_default();
        
        match browser.click(&selector) {
            Ok(_) => crate::tools::ToolResult::success("click", args, "Clicked successfully".to_string()),
            Err(e) => crate::tools::ToolResult::error("click", args, e),
        }
    }
}

struct TypeTool;

#[async_trait]
impl BrowserTool for TypeTool {
    fn name(&self) -> &str { "type" }
    fn description(&self) -> &str { "Type text into an input element" }
    
    async fn execute(&self, args: HashMap<String, String>, browser: &dyn BrowserInterface) -> crate::tools::ToolResult {
        let selector = args.get("selector").cloned().unwrap_or_default();
        let text = args.get("text").cloned().unwrap_or_default();
        
        match browser.type_text(&selector, &text) {
            Ok(_) => crate::tools::ToolResult::success("type", args, format!("Typed '{}' successfully", text)),
            Err(e) => crate::tools::ToolResult::error("type", args, e),
        }
    }
}

struct ScrollToTool;

#[async_trait]
impl BrowserTool for ScrollToTool {
    fn name(&self) -> &str { "scroll_to" }
    fn description(&self) -> &str { "Scroll element into view" }
    
    async fn execute(&self, args: HashMap<String, String>, browser: &dyn BrowserInterface) -> crate::tools::ToolResult {
        let selector = args.get("selector").cloned().unwrap_or_default();
        
        match browser.scroll_to(&selector) {
            Ok(_) => crate::tools::ToolResult::success("scroll_to", args, "Scrolled to element".to_string()),
            Err(e) => crate::tools::ToolResult::error("scroll_to", args, e),
        }
    }
}

struct ScrollByTool;

#[async_trait]
impl BrowserTool for ScrollByTool {
    fn name(&self) -> &str { "scroll_by" }
    fn description(&self) -> &str { "Scroll by pixel offset" }
    
    async fn execute(&self, args: HashMap<String, String>, browser: &dyn BrowserInterface) -> crate::tools::ToolResult {
        let x: f32 = args.get("x").and_then(|v| v.parse().ok()).unwrap_or(0.0);
        let y: f32 = args.get("y").and_then(|v| v.parse().ok()).unwrap_or(0.0);
        
        match browser.scroll_by(x, y) {
            Ok(_) => crate::tools::ToolResult::success("scroll_by", args, format!("Scrolled by {}, {}", x, y)),
            Err(e) => crate::tools::ToolResult::error("scroll_by", args, e),
        }
    }
}

struct SubmitFormTool;

#[async_trait]
impl BrowserTool for SubmitFormTool {
    fn name(&self) -> &str { "submit_form" }
    fn description(&self) -> &str { "Submit a form" }
    
    async fn execute(&self, args: HashMap<String, String>, browser: &dyn BrowserInterface) -> crate::tools::ToolResult {
        let selector = args.get("selector").cloned().unwrap_or_default();
        
        match browser.submit_form(&selector) {
            Ok(_) => crate::tools::ToolResult::success("submit_form", args, "Form submitted".to_string()),
            Err(e) => crate::tools::ToolResult::error("submit_form", args, e),
        }
    }
}
