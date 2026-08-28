use crate::providers::{
    build_system_prompt, parse_tool_calls, resolve_endpoint, AiContext, AiProvider, AiResponse,
    ProviderConfig, ProviderError, ProviderResult,
};
use async_trait::async_trait;
use reqwest::Client;
use std::time::Duration;

pub struct AnthropicProvider {
    config: ProviderConfig,
    client: Client,
}

impl AnthropicProvider {
    pub fn new(config: ProviderConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| Client::new());

        Self { config, client }
    }

    /// Build the single user-message content, folding in any tool results.
    fn build_user_content(&self, prompt: &str, context: &AiContext) -> String {
        if context.tool_results.is_empty() {
            prompt.to_string()
        } else {
            let tool_results_str = context
                .tool_results
                .iter()
                .map(|r| format!("{}: {}", r.tool_name, r.result))
                .collect::<Vec<_>>()
                .join("\n");
            format!("{prompt}\n\nTool results:\n{tool_results_str}")
        }
    }

    /// Build the JSON body for the Messages API.
    ///
    /// The system prompt MUST go in the top-level `system` field — the Anthropic
    /// Messages API rejects a `{"role": "system"}` entry inside `messages` with a
    /// 400. `messages` therefore carries only `user`/`assistant` turns.
    fn build_request_body(&self, prompt: &str, context: &AiContext) -> serde_json::Value {
        serde_json::json!({
            "model": self.config.model,
            "system": build_system_prompt(context),
            "messages": [
                { "role": "user", "content": self.build_user_content(prompt, context) }
            ],
            "max_tokens": self.config.max_tokens.unwrap_or(4096),
            "temperature": self.config.temperature.unwrap_or(0.3),
        })
    }

    /// Normalize Anthropic's `stop_reason` into the provider-agnostic vocabulary
    /// the agent loop expects. The loop terminates on `finish_reason == "stop"`,
    /// which Anthropic never emits verbatim (it uses `end_turn`/`stop_sequence`).
    fn normalize_finish_reason(stop_reason: &str) -> String {
        match stop_reason {
            "end_turn" | "stop_sequence" => "stop",
            "max_tokens" => "length",
            other => other,
        }
        .to_string()
    }

    /// Concatenate the text of every `text`-typed content block in a Messages API
    /// response. A response may carry multiple blocks (e.g. `thinking` + `text`),
    /// so indexing `content[0]` alone can silently drop the real answer.
    fn extract_text(json: &serde_json::Value) -> String {
        json["content"]
            .as_array()
            .map(|blocks| {
                blocks
                    .iter()
                    .filter_map(|b| {
                        if b["type"] == "text" {
                            b["text"].as_str()
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default()
    }
}

#[async_trait]
impl AiProvider for AnthropicProvider {
    async fn complete(&self, prompt: &str, context: &AiContext) -> ProviderResult<AiResponse> {
        let api_key =
            self.config.api_key.as_ref().ok_or_else(|| {
                ProviderError::NotConfigured("Anthropic API key not set".to_string())
            })?;

        let body = self.build_request_body(prompt, context);

        let endpoint = resolve_endpoint(
            self.config.base_url.as_deref(),
            "https://api.anthropic.com",
            "/v1/messages",
        );

        let response = self
            .client
            .post(endpoint)
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::RequestFailed(e.to_string()))?;

        if response.status() == 429 {
            return Err(ProviderError::RateLimited);
        }

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(ProviderError::RequestFailed(format!(
                "Status {status}: {text}"
            )));
        }

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| ProviderError::ParseError(e.to_string()))?;

        let content = Self::extract_text(&json);
        let finish_reason =
            Self::normalize_finish_reason(json["stop_reason"].as_str().unwrap_or("end_turn"));
        let tool_calls = parse_tool_calls(&content);

        Ok(AiResponse {
            content,
            reasoning: None,
            tool_calls,
            finish_reason,
        })
    }

    fn provider_name(&self) -> &str {
        "anthropic"
    }

    fn is_configured(&self) -> bool {
        self.config.api_key.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::ScrollPosition;

    fn ctx() -> AiContext {
        AiContext {
            current_url: String::new(),
            page_title: String::new(),
            dom_snapshot: String::new(),
            accessibility_tree: None,
            scroll_position: ScrollPosition { x: 0.0, y: 0.0 },
            tool_results: Vec::new(),
            conversation_history: Vec::new(),
        }
    }

    fn provider() -> AnthropicProvider {
        let config = ProviderConfig {
            model: "claude-3-5-sonnet".to_string(),
            ..Default::default()
        };
        AnthropicProvider::new(config)
    }

    #[test]
    fn request_body_puts_system_at_top_level_not_in_messages() {
        let body = provider().build_request_body("do the thing", &ctx());

        // system prompt is a top-level field, not a message role
        assert!(
            body["system"].as_str().is_some_and(|s| !s.is_empty()),
            "system prompt must be a non-empty top-level field"
        );

        let messages = body["messages"].as_array().expect("messages is an array");
        assert_eq!(messages.len(), 1, "only the user turn should be sent");
        assert_eq!(messages[0]["role"], "user");
        assert!(
            messages.iter().all(|m| m["role"] != "system"),
            "no message may carry the system role (the Messages API rejects it)"
        );
    }

    #[test]
    fn finish_reason_is_normalized_to_shared_vocabulary() {
        assert_eq!(
            AnthropicProvider::normalize_finish_reason("end_turn"),
            "stop"
        );
        assert_eq!(
            AnthropicProvider::normalize_finish_reason("stop_sequence"),
            "stop"
        );
        assert_eq!(
            AnthropicProvider::normalize_finish_reason("max_tokens"),
            "length"
        );
        // an unknown/other reason (e.g. tool_use) passes through unchanged
        assert_eq!(
            AnthropicProvider::normalize_finish_reason("tool_use"),
            "tool_use"
        );
    }

    #[test]
    fn extract_text_concatenates_all_text_blocks() {
        let json = serde_json::json!({
            "content": [
                { "type": "thinking", "thinking": "hmm" },
                { "type": "text", "text": "hello" },
                { "type": "text", "text": " world" }
            ]
        });
        assert_eq!(AnthropicProvider::extract_text(&json), "hello world");
    }
}
