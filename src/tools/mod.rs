use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use async_trait::async_trait;

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
    async fn execute(&self, args: HashMap<String, String>, browser: &dyn BrowserInterface) -> ToolResult;
}

pub trait BrowserInterface: Send + Sync {
    fn query_selector(&self, selector: &str) -> Vec<ElementInfo>;
    fn get_text(&self, selector: &str) -> String;
    fn get_attributes(&self, selector: &str) -> HashMap<String, String>;
    fn click(&self, selector: &str) -> Result<(), String>;
    fn type_text(&self, selector: &str, text: &str) -> Result<(), String>;
    fn submit_form(&self, selector: &str) -> Result<(), String>;
    fn scroll_to(&self, selector: &str) -> Result<(), String>;
    fn scroll_by(&self, x: f32, y: f32) -> Result<(), String>;
    fn get_page_info(&self) -> PageInfo;
}

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
pub struct PageInfo {
    pub url: String,
    pub title: String,
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub scroll_x: f32,
    pub scroll_y: f32,
    pub links: Vec<LinkInfo>,
    pub images: Vec<ImageInfo>,
    pub forms: Vec<FormInfo>,
    pub prices: Vec<PriceInfo>,
    pub tables: Vec<TableInfo>,
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
    tools: std::collections::HashMap<String, Arc<dyn BrowserTool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: std::collections::HashMap::new(),
        }
    }

    pub fn register(&mut self, tool: Arc<dyn BrowserTool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn BrowserTool>> {
        self.tools.get(name).cloned()
    }

    pub fn list(&self) -> Vec<(&str, &str)> {
        self.tools.iter()
            .map(|(name, tool)| (name.as_str(), tool.description()))
            .collect()
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
