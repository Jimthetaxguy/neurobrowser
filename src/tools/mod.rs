use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

pub mod contracts;
pub mod errors;

pub use contracts::{
    RiskLevel, StructuredToolCall, ToolAction, ToolArgumentDefinition, ToolDefinition, ToolRisk,
};
pub use errors::{AgentError, AgentResult, ToolError};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_name: String,
    pub arguments: HashMap<String, String>,
    pub result: String,
    pub success: bool,
}

impl ToolResult {
    pub fn success(tool_name: &str, arguments: HashMap<String, String>, result: String) -> Self {
        Self {
            tool_name: tool_name.to_string(),
            arguments,
            result,
            success: true,
        }
    }

    pub fn error(tool_name: &str, arguments: HashMap<String, String>, error: String) -> Self {
        Self {
            tool_name: tool_name.to_string(),
            arguments,
            result: error,
            success: false,
        }
    }
}

#[async_trait]
pub trait BrowserTool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            self.name(),
            self.description(),
            ToolRisk::new(ToolAction::Read, RiskLevel::Low),
        )
    }
    async fn execute(
        &self,
        args: HashMap<String, String>,
        browser: &dyn BrowserInterface,
    ) -> ToolResult;
}

#[async_trait]
pub trait BrowserInterface: Send + Sync {
    async fn navigate(&self, url: &str) -> Result<(), String>;
    async fn query_selector(&self, selector: &str) -> Result<Vec<ElementInfo>, String>;
    async fn get_text(&self, selector: &str) -> Result<String, String>;
    async fn get_attributes(&self, selector: &str) -> Result<HashMap<String, String>, String>;
    async fn click(&self, selector: &str) -> Result<(), String>;
    async fn type_text(&self, selector: &str, text: &str) -> Result<(), String>;
    async fn submit_form(&self, selector: &str) -> Result<(), String>;
    async fn scroll_to(&self, selector: &str) -> Result<(), String>;
    async fn scroll_by(&self, x: f32, y: f32) -> Result<(), String>;
    async fn snapshot(&self) -> Result<PageSnapshot, String>;
    async fn keypress(&self, key: &str) -> Result<(), String> {
        Err(format!("keypress is not supported by this browser: {key}"))
    }

    async fn screenshot(&self) -> Result<String, String> {
        Err("screenshot is not supported by this browser".to_string())
    }

    async fn browser_back(&self) -> Result<(), String> {
        Err("back navigation is not supported by this browser".to_string())
    }

    async fn browser_forward(&self) -> Result<(), String> {
        Err("forward navigation is not supported by this browser".to_string())
    }

    async fn browser_reload(&self) -> Result<(), String> {
        Err("reload is not supported by this browser".to_string())
    }

    async fn wait_for_navigation(&self) -> Result<(), String> {
        Ok(())
    }

    async fn accessibility_tree(&self) -> Result<Option<String>, String> {
        Ok(None)
    }

    async fn get_page_info(&self) -> Result<PageSnapshot, String> {
        self.snapshot().await
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PageSnapshot {
    pub url: String,
    pub title: String,
    pub html: Option<String>,
    pub text: Option<String>,
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub scroll_x: f32,
    pub scroll_y: f32,
    pub interactive_ready: bool,
    pub links: Vec<LinkInfo>,
    pub images: Vec<ImageInfo>,
    pub forms: Vec<FormInfo>,
    pub prices: Vec<PriceInfo>,
    pub tables: Vec<TableInfo>,
}

pub type PageInfo = PageSnapshot;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElementInfo {
    pub tag: String,
    pub id: Option<String>,
    pub classes: Vec<String>,
    pub text: String,
    pub attributes: HashMap<String, String>,
    pub selector: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkInfo {
    pub href: String,
    pub text: String,
    pub selector: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageInfo {
    pub src: String,
    pub alt: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormInfo {
    pub action: String,
    pub method: String,
    pub inputs: Vec<FormInputInfo>,
    pub selector: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormInputInfo {
    pub name: String,
    pub input_type: String,
    pub selector: String,
    pub value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceInfo {
    pub value: String,
    pub currency: String,
    pub selector: String,
    pub context: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableInfo {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub selector: String,
}

pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn BrowserTool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    pub fn register(&mut self, tool: Arc<dyn BrowserTool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn BrowserTool>> {
        self.tools.get(name).cloned()
    }

    pub fn list(&self) -> Vec<(&str, &str)> {
        self.tools
            .iter()
            .map(|(name, tool)| (name.as_str(), tool.description()))
            .collect()
    }

    pub fn definitions(&self) -> Vec<ToolDefinition> {
        self.tools.values().map(|tool| tool.definition()).collect()
    }

    pub fn names(&self) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
