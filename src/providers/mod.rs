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
    #[error("Rate limited")]
    RateLimited,
    #[error("Provider not configured: {0}")]
    NotConfigured(String),
}

pub type ProviderResult<T> = Result<T, ProviderError>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiResponse {
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
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
    pub tool_results: Vec<ToolResult>,
    /// When true, the system prompt lists `search_personal_memory` and
    /// `inspect_active_page`. Set by `ReActAgent::with_memory`.
    ///
    /// Those tools read `neuro_memory::MemoryService`. They do not read an
    /// in-run agent log.
    #[serde(default)]
    pub personal_memory: bool,
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

/// Parse `ToolCall: {json}` lines and legacy `Action: tool(args)` lines.
///
/// A recognized call is kept even when it omits a required argument.
/// `ReActAgent` checks it against the tool that would run it and reports the
/// failure to the model. Dropping it here would leave `tool_calls` empty,
/// which the agent treats as a final answer.
///
/// A legacy line must end with `)`. A truncated `back(` or
/// `navigate(https://ex` is not a call. A legacy call with no arguments, such
/// as `wait()`, counts only when it names a known tool.
pub fn parse_tool_calls(content: &str) -> Vec<ToolCall> {
    let mut calls = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.starts_with("ToolCall:") {
            let json_part = line.strip_prefix("ToolCall:").unwrap().trim();
            if let Some(call) = parse_structured_tool_call(json_part) {
                calls.push(call);
            }
            continue;
        }

        if line.starts_with("Action:") {
            let action_part = line.strip_prefix("Action:").unwrap().trim();

            if let Some((name, args_str)) = action_part
                .split_once('(')
                .filter(|(_, args_str)| args_str.ends_with(')'))
            {
                let name = name.trim();
                let args_str = args_str.trim_end_matches(')').trim();

                let arguments = parse_arguments(name, args_str);

                if !arguments.is_empty() || is_known_tool(name) {
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

fn parse_structured_tool_call(json_part: &str) -> Option<ToolCall> {
    #[derive(Deserialize)]
    struct RawToolCall {
        name: String,
        /// Absent or `null` means no arguments, so `{"name":"navigate"}` is
        /// kept for the agent to report instead of failing to parse.
        arguments: Option<HashMap<String, serde_json::Value>>,
    }

    let parsed: RawToolCall = serde_json::from_str(json_part).ok()?;
    let arguments = parsed
        .arguments
        .unwrap_or_default()
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

fn parse_arguments(tool_name: &str, args_str: &str) -> HashMap<String, String> {
    let mut arguments = HashMap::new();

    if args_str.is_empty() {
        return arguments;
    }

    // `split_arguments` already splits `args_str` on top-level commas, so by
    // the time we get here each `arg` is a single token — there is never an
    // embedded ',' left to split on. Positional (unlabeled) args are mapped
    // by index to the tool's real, ordered parameter names (as declared on
    // `BrowserTool::definition()`, e.g. `type` -> ["selector", "text"]) so
    // multi-arg positional calls like `type(#input, hello)` land on the same
    // keys `TypeTool::execute` reads, instead of every positional arg
    // overwriting a single "value" key.
    let args = split_arguments(args_str);
    let positional_names = positional_argument_names(tool_name);
    let mut positional_index = 0usize;

    for arg in args {
        let arg = arg.trim();
        if arg.is_empty() {
            continue;
        }

        if let Some((key, value)) = arg.split_once('=') {
            let key = key.trim();
            let value = value.trim().trim_matches('"').trim_matches('\'');
            arguments.insert(key.to_string(), value.to_string());
        } else {
            let value = arg.trim_matches('"').trim_matches('\'');
            if !value.is_empty() {
                let key = positional_names
                    .get(positional_index)
                    .cloned()
                    .unwrap_or_else(|| format!("value{}", positional_index + 1));
                arguments.insert(key, value.to_string());
            }
            positional_index += 1;
        }
    }

    arguments
}

/// Looks up the real, ordered argument names for `tool_name` from the
/// browser tool registry (the same registry `ReActAgent` dispatches
/// through) so legacy positional `Action: tool(a, b)` calls feed the same
/// keys the tool's `execute()` reads. Falls back to an empty list for
/// unknown tool names, in which case positional args get distinct
/// `value1`, `value2`, ... keys rather than overwriting each other.
fn positional_argument_names(tool_name: &str) -> Vec<String> {
    if let Some(names) = crate::tools::memory_tools::positional_argument_names(tool_name) {
        return names;
    }
    browser_tool_registry()
        .get(tool_name)
        .map(|tool| {
            tool.definition()
                .arguments
                .into_iter()
                .map(|argument| argument.name)
                .collect()
        })
        .unwrap_or_default()
}

/// True for the 17 browser tools and the two memory tools.
fn is_known_tool(tool_name: &str) -> bool {
    crate::tools::memory_tools::positional_argument_names(tool_name).is_some()
        || browser_tool_registry().get(tool_name).is_some()
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

/// The 17 browser tools, built once. The legacy positional parser and the
/// prompt's tool catalog both read this registry.
fn browser_tool_registry() -> &'static crate::tools::ToolRegistry {
    static REGISTRY: std::sync::OnceLock<crate::tools::ToolRegistry> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(crate::browser::default_tool_registry)
}

/// One catalog entry: the tool, then one line per argument.
fn push_tool_definition(prompt: &mut String, definition: &crate::tools::ToolDefinition) {
    prompt.push_str(&format!(
        "- {}: {}\n",
        definition.name, definition.description
    ));
    for argument in &definition.arguments {
        let required = if argument.required { " (required)" } else { "" };
        prompt.push_str(&format!(
            "  - {}{required}: {}\n",
            argument.name, argument.description
        ));
    }
}

pub fn build_system_prompt(context: &AiContext) -> String {
    let mut prompt = String::from("You are an intelligent browser assistant. ");
    prompt.push_str(&format!("Current URL: {}\n", context.current_url));
    prompt.push_str(&format!("Page title: {}\n\n", context.page_title));

    if !context.tool_results.is_empty() {
        prompt.push_str("Recent tool results:\n");
        for result in &context.tool_results {
            if result.success {
                prompt.push_str(&format!("- {}: {}\n", result.tool_name, result.result));
            } else {
                // Keep the reason so the model can correct the call. The agent
                // stores failures as "Error: <reason>"; tools return the bare reason.
                let reason = result
                    .result
                    .strip_prefix("Error: ")
                    .unwrap_or(&result.result);
                prompt.push_str(&format!("- {}: Error: {reason}\n", result.tool_name));
            }
        }
        prompt.push('\n');
    }

    prompt.push_str("Use structured browser tool calls when an action is needed:\n");
    prompt.push_str("ToolCall: {\"name\":\"tool_name\",\"arguments\":{\"key\":\"value\"}}\n\n");
    prompt.push_str("Available tools:\n");
    for definition in browser_tool_registry().definitions() {
        push_tool_definition(&mut prompt, &definition);
    }
    if context.personal_memory {
        for definition in crate::tools::memory_tools::definitions() {
            push_tool_definition(&mut prompt, &definition);
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
mod http;
pub mod ollama;
pub mod openai;

pub(crate) use http::client_for_origin;

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
    use super::{build_system_prompt, parse_tool_calls, resolve_endpoint, AiContext, ToolResult};
    use crate::tools::ToolDefinition;

    fn empty_context() -> AiContext {
        AiContext {
            current_url: "https://example.com".to_string(),
            page_title: "Example".to_string(),
            tool_results: Vec::new(),
            personal_memory: false,
        }
    }

    fn assert_lists(prompt: &str, definition: &ToolDefinition) {
        assert!(
            prompt.contains(&format!(
                "- {}: {}\n",
                definition.name, definition.description
            )),
            "missing tool {}",
            definition.name
        );
        for argument in &definition.arguments {
            let required = if argument.required { " (required)" } else { "" };
            assert!(
                prompt.contains(&format!(
                    "  - {}{required}: {}\n",
                    argument.name, argument.description
                )),
                "missing argument {} of {}",
                argument.name,
                definition.name
            );
        }
    }

    #[test]
    fn system_prompt_is_driven_by_the_tool_registry() {
        let prompt = build_system_prompt(&empty_context());
        assert!(prompt.contains("ToolCall:"));
        assert!(!prompt.contains("Action:"));
        // The hand-maintained catalog is gone.
        assert!(!prompt.contains("navigate(url)"));
        assert!(!prompt.contains("Navigate to an HTTP(S) URL"));

        let browser = crate::browser::default_tool_registry().definitions();
        assert_eq!(browser.len(), 17);
        for definition in &browser {
            assert_lists(&prompt, definition);
        }
        for definition in crate::tools::memory_tools::definitions() {
            assert!(
                !prompt.contains(&definition.name),
                "{} listed without personal memory",
                definition.name
            );
        }

        let with_memory = build_system_prompt(&AiContext {
            personal_memory: true,
            ..empty_context()
        });
        for definition in browser
            .iter()
            .chain(&crate::tools::memory_tools::definitions())
        {
            assert_lists(&with_memory, definition);
        }
    }

    #[test]
    fn failed_tool_results_reach_the_prompt_with_their_reason() {
        let context = AiContext {
            tool_results: vec![
                ToolResult::success("get_text", "hello".to_string()),
                // Shape the agent stores after a failed or invalid call.
                ToolResult::error(
                    "navigate",
                    "Error: missing required argument(s): url".to_string(),
                ),
                // Shape a tool returns directly.
                ToolResult::error("click", "no element matches '#go'".to_string()),
            ],
            ..empty_context()
        };
        let prompt = build_system_prompt(&context);
        assert!(prompt.contains("- get_text: hello\n"), "{prompt}");
        assert!(
            prompt.contains("- navigate: Error: missing required argument(s): url\n"),
            "{prompt}"
        );
        assert!(
            prompt.contains("- click: Error: no element matches '#go'\n"),
            "{prompt}"
        );
        assert!(!prompt.contains("Error: Error:"), "{prompt}");
    }

    #[test]
    fn parse_tool_calls_keeps_calls_with_missing_required_arguments() {
        // The agent reports these to the model. Dropping them would end the run.
        let calls = parse_tool_calls(r#"ToolCall: {"name":"navigate","arguments":{}}"#);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "navigate");
        assert!(calls[0].arguments.is_empty());

        let calls = parse_tool_calls(r#"ToolCall: {"name":"navigate","arguments":{"url":"   "}}"#);
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].arguments.get("url").map(String::as_str),
            Some("   ")
        );

        // An omitted or null `arguments` object is an empty one.
        for line in [
            r#"ToolCall: {"name":"navigate"}"#,
            r#"ToolCall: {"name":"navigate","arguments":null}"#,
        ] {
            let calls = parse_tool_calls(line);
            assert_eq!(calls.len(), 1, "{line}");
            assert_eq!(calls[0].name, "navigate");
            assert!(calls[0].arguments.is_empty(), "{line}");
        }
    }

    #[test]
    fn parse_tool_calls_keeps_registered_zero_argument_legacy_actions() {
        for name in [
            "wait",
            "inspect_active_page",
            "get_links",
            "get_prices",
            "get_tables",
            "screenshot",
            "back",
            "forward",
            "reload",
        ] {
            let calls = parse_tool_calls(&format!("Action: {name}()"));
            assert_eq!(calls.len(), 1, "{name} was dropped");
            assert_eq!(calls[0].name, name);
            assert!(
                calls[0].arguments.is_empty(),
                "{name} should keep an empty argument map"
            );
        }

        assert!(
            parse_tool_calls("Action: ()").is_empty(),
            "an empty tool name is not a call"
        );
        assert!(
            parse_tool_calls("Action: unknown_tool()").is_empty(),
            "an unknown empty-argument tool should not become a call"
        );
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
