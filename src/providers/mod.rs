use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("API request failed: {0}")]
    RequestFailed(String),
    #[error("Parse error: {0}")]
    ParseError(String),
    #[error("Authentication error: {0}")]
    AuthError(String),
    #[error("Rate limited")]
    RateLimited,
    #[error("Provider not configured: {0}")]
    NotConfigured(String),
}

pub type ProviderResult<T> = Result<T, ProviderError>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiResponse {
    pub content: String,
    pub reasoning: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub finish_reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub name: String,
    pub arguments: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiContext {
    pub current_url: String,
    pub page_title: String,
    pub dom_snapshot: String,
    pub accessibility_tree: Option<String>,
    pub scroll_position: ScrollPosition,
    pub tool_results: Vec<ToolResult>,
    pub conversation_history: Vec<Message>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScrollPosition {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_name: String,
    pub arguments: HashMap<String, String>,
    pub result: String,
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

#[async_trait]
pub trait AiProvider: Send + Sync {
    async fn complete(&self, prompt: &str, context: &AiContext) -> ProviderResult<AiResponse>;
    fn provider_name(&self) -> &str;
    fn is_configured(&self) -> bool;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub provider_type: ProviderType,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub model: String,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderType {
    Openai,
    Anthropic,
    Ollama,
    Custom,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            provider_type: ProviderType::Openai,
            api_key: None,
            base_url: None,
            model: "gpt-4o".to_string(),
            max_tokens: Some(4096),
            temperature: Some(0.3),
        }
    }
}

pub fn parse_tool_calls(content: &str) -> Vec<ToolCall> {
    let mut calls = Vec::new();
    
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with("Action:") {
            let action_part = line.strip_prefix("Action:").unwrap().trim();
            
            if let Some((name, args_str)) = action_part.split_once('(') {
                let name = name.trim();
                let args_str = args_str.trim_end_matches(')').trim();
                
                let arguments = parse_arguments(args_str);
                
                if !arguments.is_empty() {
                    calls.push(ToolCall {
                        name: name.to_string(),
                        arguments,
                    });
                }
            }
        }
    }
    
    calls
}

fn parse_arguments(args_str: &str) -> HashMap<String, String> {
    let mut arguments = HashMap::new();
    
    if args_str.is_empty() {
        return arguments;
    }
    
    let args = split_arguments(args_str);
    
    for arg in args {
        let arg = arg.trim();
        if arg.is_empty() {
            continue;
        }
        
        if let Some((key, value)) = arg.split_once('=') {
            let key = key.trim();
            let value = value.trim().trim_matches('"').trim_matches('\'');
            arguments.insert(key.to_string(), value.to_string());
        } else if let Some((key, value)) = arg.split_once(',') {
            let key = key.trim();
            let value = value.trim().trim_matches('"').trim_matches('\'');
            if !key.is_empty() && !value.is_empty() {
                arguments.insert(key.to_string(), value.to_string());
            }
        } else {
            arguments.insert("value".to_string(), arg.to_string());
        }
    }
    
    arguments
}

fn split_arguments(args_str: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut quote_char = ' ';
    let mut paren_depth = 0;
    
    for ch in args_str.chars() {
        match ch {
            '"' | '\'' if !in_quotes => {
                in_quotes = true;
                quote_char = ch;
                current.push(ch);
            }
            c if c == quote_char && in_quotes => {
                in_quotes = false;
                quote_char = ' ';
                current.push(ch);
            }
            ',' if !in_quotes && paren_depth == 0 => {
                args.push(current.trim().to_string());
                current.clear();
            }
            '(' | '[' | '{' if !in_quotes => {
                paren_depth += 1;
                current.push(ch);
            }
            ')' | ']' | '}' if !in_quotes && paren_depth > 0 => {
                paren_depth -= 1;
                current.push(ch);
            }
            _ => {
                current.push(ch);
            }
        }
    }
    
    if !current.trim().is_empty() {
        args.push(current.trim().to_string());
    }
    
    args
}

pub fn build_system_prompt(context: &AiContext) -> String {
    let mut prompt = String::from("You are an intelligent browser assistant. ");
    prompt.push_str(&format!("Current URL: {}\n", context.current_url));
    prompt.push_str(&format!("Page title: {}\n\n", context.page_title));
    
    if !context.tool_results.is_empty() {
        prompt.push_str("Recent tool results:\n");
        for result in &context.tool_results {
            prompt.push_str(&format!(
                "- {}: {}\n",
                result.tool_name,
                if result.success { &result.result } else { "Error" }
            ));
        }
        prompt.push('\n');
    }

    prompt.push_str("Available tools:\n");
    prompt.push_str("- query_dom(selector): Query DOM elements by CSS selector\n");
    prompt.push_str("- get_text(selector): Get text content of element\n");
    prompt.push_str("- click(selector): Click an element\n");
    prompt.push_str("- type(selector, text): Type text into input\n");
    prompt.push_str("- scroll_to(selector): Scroll element into view\n");
    prompt.push_str("- scroll_by(x, y): Scroll by pixels\n");
    prompt.push_str("- submit_form(selector): Submit a form\n");
    prompt.push_str("- get_links(): Get all links on page\n");
    prompt.push_str("- get_prices(): Extract price information\n");
    prompt.push_str("- get_tables(): Extract table data\n");
    prompt.push_str("- get_accessibility(): Get accessibility tree\n");
    
    prompt
}

pub mod openai;
pub mod anthropic;
pub mod ollama;

pub use openai::OpenAiProvider;
pub use anthropic::AnthropicProvider;
pub use ollama::OllamaProvider;

use std::sync::Arc;

pub fn create_provider(config: &ProviderConfig) -> Arc<dyn AiProvider> {
    match config.provider_type {
        ProviderType::Openai => Arc::new(OpenAiProvider::new(config.clone())),
        ProviderType::Anthropic => Arc::new(AnthropicProvider::new(config.clone())),
        ProviderType::Ollama => Arc::new(OllamaProvider::new(config.clone())),
        ProviderType::Custom => Arc::new(OpenAiProvider::new(config.clone())),
    }
}
