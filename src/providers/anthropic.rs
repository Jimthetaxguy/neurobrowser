use crate::providers::{
    build_system_prompt, parse_tool_calls, AiContext, AiProvider, AiResponse, ProviderConfig,
    ProviderError, ProviderResult,
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

    fn build_messages(&self, prompt: &str, context: &AiContext) -> Vec<serde_json::Value> {
        let system_prompt = build_system_prompt(context);

        // If there are tool results, include them in the user message for context
        if context.tool_results.is_empty() {
            vec![
                serde_json::json!({
                    "role": "system",
                    "content": system_prompt
                }),
                serde_json::json!({
                    "role": "user",
                    "content": prompt
                }),
            ]
        } else {
            // Include tool results in the prompt
            let tool_results_str = context
                .tool_results
                .iter()
                .map(|r| format!("{}: {}", r.tool_name, r.result))
                .collect::<Vec<_>>()
                .join("\n");

            vec![
                serde_json::json!({
                    "role": "system",
                    "content": system_prompt
                }),
                serde_json::json!({
                    "role": "user",
                    "content": format!("{}\n\nTool results:\n{}", prompt, tool_results_str)
                }),
            ]
        }
    }
}

#[async_trait]
impl AiProvider for AnthropicProvider {
    async fn complete(&self, prompt: &str, context: &AiContext) -> ProviderResult<AiResponse> {
        let api_key =
            self.config.api_key.as_ref().ok_or_else(|| {
                ProviderError::NotConfigured("Anthropic API key not set".to_string())
            })?;

        let messages = self.build_messages(prompt, context);

        let body = serde_json::json!({
            "model": self.config.model,
            "messages": messages,
            "max_tokens": self.config.max_tokens.unwrap_or(4096),
            "temperature": self.config.temperature.unwrap_or(0.3),
        });

        let response = self
            .client
            .post("https://api.anthropic.com/v1/messages")
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
                "Status {}: {}",
                status, text
            )));
        }

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| ProviderError::ParseError(e.to_string()))?;

        let content = json["content"][0]["text"]
            .as_str()
            .unwrap_or("")
            .to_string();

        let finish_reason = json["stop_reason"]
            .as_str()
            .unwrap_or("end_turn")
            .to_string();

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
