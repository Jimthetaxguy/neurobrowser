use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

pub use crate::tools::ToolResult;

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
pub struct Message {
    pub role: String,
    pub content: String,
}

#[async_trait]
pub trait AiProvider: Send + Sync {
    async fn complete(&self, prompt: &str, context: &AiContext) -> ProviderResult<AiResponse>;
    fn provider_name(&self) -> &str;
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
        if line.starts_with("ToolCall:") {
            let json_part = line.strip_prefix("ToolCall:").unwrap().trim();
            if let Some(call) = parse_structured_tool_call(json_part) {
                if missing_required_arguments(&call).is_empty() {
                    calls.push(call);
                }
            }
        }
    }

    calls
}

fn parse_structured_tool_call(json_part: &str) -> Option<ToolCall> {
    #[derive(Deserialize)]
    struct RawToolCall {
        name: String,
        arguments: HashMap<String, serde_json::Value>,
    }

    let parsed: RawToolCall = serde_json::from_str(json_part).ok()?;
    let arguments = parsed
        .arguments
        .into_iter()
        .map(|(key, value)| {
            let value = match value {
                serde_json::Value::String(value) => value,
                other => other.to_string(),
            };
            (key, value)
        })
        .collect();
    Some(ToolCall {
        name: parsed.name,
        arguments,
    })
}

fn missing_required_arguments(call: &ToolCall) -> Vec<String> {
    let Some(tool) = crate::browser::default_tool_registry().get(&call.name) else {
        return Vec::new();
    };
    tool.definition()
        .arguments
        .into_iter()
        .filter(|argument| argument.required)
        .filter(|argument| {
            call.arguments
                .get(&argument.name)
                .map(String::as_str)
                .unwrap_or("")
                .trim()
                .is_empty()
        })
        .map(|argument| argument.name)
        .collect()
}

fn format_argument_line(argument: &crate::tools::ToolArgumentDefinition) -> String {
    let mut flags = Vec::new();
    if argument.required {
        flags.push("required");
    }
    if argument.sensitive {
        flags.push("sensitive");
    }
    let flags = if flags.is_empty() {
        String::new()
    } else {
        format!(" ({})", flags.join(", "))
    };
    format!("  - {}{}: {}\n", argument.name, flags, argument.description)
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
                if result.success {
                    &result.result
                } else {
                    "Error"
                }
            ));
        }
        prompt.push('\n');
    }

    prompt.push_str("Use structured browser tool calls when an action is needed:\n");
    prompt.push_str("ToolCall: {\"name\":\"tool_name\",\"arguments\":{\"key\":\"value\"}}\n\n");
    prompt.push_str("Available tools:\n");
    for definition in crate::browser::default_tool_registry().definitions() {
        prompt.push_str(&format!(
            "- {}: {}\n",
            definition.name, definition.description
        ));
        for argument in &definition.arguments {
            prompt.push_str(&format_argument_line(argument));
        }
    }

    prompt
}

/// Resolve a provider request URL, honoring an optional `base_url` override.
///
/// `base_url` (fed from `CUSTOM_PROVIDER_BASE_URL`) is treated as the API
/// **origin** — `scheme://host[:port]` — and the provider's fixed `path` is
/// appended, so callers can point OpenAI/Anthropic at Azure, a corporate
/// gateway, or a local proxy. An empty/whitespace override falls back to
/// `default_origin`, and a trailing slash on the override is trimmed to avoid a
/// doubled `//`.
pub(crate) fn resolve_endpoint(base_url: Option<&str>, default_origin: &str, path: &str) -> String {
    let origin = base_url
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.trim_end_matches('/'))
        .unwrap_or(default_origin);
    format!("{origin}{path}")
}

pub mod anthropic;
pub mod ollama;
pub mod openai;

pub use anthropic::AnthropicProvider;
pub use ollama::OllamaProvider;
pub use openai::OpenAiProvider;

use std::sync::Arc;

pub fn create_provider(config: &ProviderConfig) -> Arc<dyn AiProvider> {
    match config.provider_type {
        ProviderType::Openai => Arc::new(OpenAiProvider::new(config.clone())),
        ProviderType::Anthropic => Arc::new(AnthropicProvider::new(config.clone())),
        ProviderType::Ollama => Arc::new(OllamaProvider::new(config.clone())),
        ProviderType::Custom => Arc::new(OpenAiProvider::new(config.clone())),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_system_prompt, parse_tool_calls, resolve_endpoint, AiContext, ScrollPosition,
    };

    fn empty_context() -> AiContext {
        AiContext {
            current_url: "https://example.com".to_string(),
            page_title: "Example".to_string(),
            dom_snapshot: String::new(),
            accessibility_tree: None,
            scroll_position: ScrollPosition { x: 0.0, y: 0.0 },
            tool_results: Vec::new(),
            conversation_history: Vec::new(),
        }
    }

    #[test]
    fn system_prompt_is_driven_by_the_tool_registry() {
        let prompt = build_system_prompt(&empty_context());
        assert!(prompt.contains("ToolCall:"));
        assert!(!prompt.contains("Action:"));
        assert!(!prompt.contains("Capture the current page if supported"));
        assert!(!prompt.contains("Navigate to an HTTP(S) URL"));
        for definition in crate::browser::default_tool_registry().definitions() {
            assert!(
                prompt.contains(&format!(
                    "- {}: {}",
                    definition.name, definition.description
                )),
                "missing registry tool {}",
                definition.name
            );
            for argument in definition.arguments {
                assert!(
                    prompt.contains(&argument.description),
                    "missing argument description for {}",
                    argument.name
                );
                if argument.required {
                    assert!(
                        prompt.contains(&format!("{} (required", argument.name)),
                        "required flag missing for {}",
                        argument.name
                    );
                }
                if argument.sensitive {
                    assert!(
                        prompt.contains(&format!("{} (required, sensitive)", argument.name)),
                        "sensitive flag missing for {}",
                        argument.name
                    );
                }
            }
        }
    }

    #[test]
    fn parse_tool_calls_rejects_missing_required_arguments() {
        let calls = parse_tool_calls(r#"ToolCall: {"name":"navigate","arguments":{}}"#);
        assert!(calls.is_empty());

        let calls = parse_tool_calls(r#"ToolCall: {"name":"navigate","arguments":{"url":"   "}}"#);
        assert!(calls.is_empty());

        let calls = parse_tool_calls(r#"ToolCall: {"name":"wait","arguments":{}}"#);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "wait");
    }

    #[test]
    fn parse_tool_calls_ignores_legacy_action_syntax() {
        let calls = parse_tool_calls("Action: click(selector=\"#go\")");
        assert!(calls.is_empty());
    }

    #[test]
    fn resolve_endpoint_uses_default_origin_when_unset() {
        assert_eq!(
            resolve_endpoint(None, "https://api.openai.com", "/v1/chat/completions"),
            "https://api.openai.com/v1/chat/completions"
        );
    }

    #[test]
    fn resolve_endpoint_overrides_origin() {
        assert_eq!(
            resolve_endpoint(
                Some("https://gateway.example.com"),
                "https://api.openai.com",
                "/v1/chat/completions"
            ),
            "https://gateway.example.com/v1/chat/completions"
        );
    }

    #[test]
    fn resolve_endpoint_trims_trailing_slash_and_ignores_blank() {
        assert_eq!(
            resolve_endpoint(
                Some("https://proxy.local/"),
                "https://api.anthropic.com",
                "/v1/messages"
            ),
            "https://proxy.local/v1/messages"
        );
        // Empty / whitespace-only override falls back to the default origin.
        assert_eq!(
            resolve_endpoint(Some("   "), "https://api.anthropic.com", "/v1/messages"),
            "https://api.anthropic.com/v1/messages"
        );
    }
}
