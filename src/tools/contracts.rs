use async_trait::async_trait;
use schemars::schema::Schema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimit {
    pub requests_per_minute: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Capability {
    Browser,
    Network,
    FileSystem,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub version: String,
    pub description: String,
    pub input_schema: Schema,
    pub output_schema: Schema,
    pub rate_limit: Option<RateLimit>,
    pub required_capability: Option<Capability>,
}

impl ToolDefinition {
    pub fn new(
        name: String,
        version: String,
        description: String,
        input_schema: Schema,
        output_schema: Schema,
    ) -> Self {
        Self {
            name,
            version,
            description,
            input_schema,
            output_schema,
            rate_limit: None,
            required_capability: None,
        }
    }

    pub fn with_rate_limit(mut self, rate_limit: RateLimit) -> Self {
        self.rate_limit = Some(rate_limit);
        self
    }

    pub fn with_capability(mut self, capability: Capability) -> Self {
        self.required_capability = Some(capability);
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub tool_name: String,
    pub tool_version: String,
    pub arguments: serde_json::Value,
    pub idempotency_key: Option<String>,
    pub correlation_id: String,
}

impl ToolCall {
    pub fn new(
        tool_name: String,
        tool_version: String,
        arguments: serde_json::Value,
        correlation_id: String,
    ) -> Self {
        Self {
            tool_name,
            tool_version,
            arguments,
            idempotency_key: None,
            correlation_id,
        }
    }

    pub fn with_idempotency_key(mut self, key: String) -> Self {
        self.idempotency_key = Some(key);
        self
    }
}

pub trait ToolSchema: Send + Sync {
    fn definition(&self) -> ToolDefinition;
}

#[async_trait]
pub trait BrowserTool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn definition(&self) -> ToolDefinition;
    async fn execute(&self, args: serde_json::Value, browser: &dyn BrowserInterface) -> ToolResult;
}

pub struct ToolResult {
    pub tool_name: String,
    pub arguments: serde_json::Value,
    pub result: serde_json::Value,
    pub success: bool,
}

impl ToolResult {
    pub fn success(
        tool_name: &str,
        arguments: serde_json::Value,
        result: serde_json::Value,
    ) -> Self {
        Self {
            tool_name: tool_name.to_string(),
            arguments,
            result,
            success: true,
        }
    }

    pub fn error(tool_name: &str, arguments: serde_json::Value, error: serde_json::Value) -> Self {
        Self {
            tool_name: tool_name.to_string(),
            arguments,
            result: error,
            success: false,
        }
    }
}

pub trait BrowserInterface: Send + Sync {
    fn query_selector(&self, selector: &str) -> Vec<ElementInfo>;
    fn get_text(&self, selector: &str) -> String;
    fn get_attributes(&self, selector: &str) -> std::collections::HashMap<String, String>;
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
    pub attributes: std::collections::HashMap<String, String>,
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
