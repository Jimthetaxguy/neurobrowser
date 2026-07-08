use crate::providers::{
    build_system_prompt, parse_tool_calls, AiContext, AiProvider, AiResponse, ProviderConfig,
    ProviderError, ProviderResult,
};
use async_trait::async_trait;
use reqwest::Client;
use std::time::Duration;

pub struct OpenAiProvider {
    config: ProviderConfig,
    client: Client,
}

impl OpenAiProvider {
    pub fn new(config: ProviderConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| Client::new());

        Self { config, client }
    }
}

#[async_trait]
impl AiProvider for OpenAiProvider {
    async fn complete(&self, prompt: &str, context: &AiContext) -> ProviderResult<AiResponse> {
        let api_key =
            self.config.api_key.as_ref().ok_or_else(|| {
                ProviderError::NotConfigured("OpenAI API key not set".to_string())
            })?;

        let system_prompt = build_system_prompt(context);

        let messages: Vec<serde_json::Value> = vec![
            serde_json::json!({
                "role": "system",
                "content": system_prompt
            }),
            serde_json::json!({
                "role": "user",
                "content": prompt
            }),
        ];

        let body = serde_json::json!({
            "model": self.config.model,
            "messages": messages,
            "temperature": self.config.temperature.unwrap_or(0.3),
            "max_tokens": self.config.max_tokens.unwrap_or(4096),
        });

        let response = self
            .client
            .post("https://api.openai.com/v1/chat/completions")
            .bearer_auth(api_key)
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

        let choices = json["choices"].as_array().ok_or_else(|| {
            ProviderError::ParseError("Response missing 'choices' array".to_string())
        })?;

        if choices.is_empty() {
            return Err(ProviderError::ParseError(
                "API returned empty choices array".to_string(),
            ));
        }

        let content = choices[0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();

        let finish_reason = choices[0]["finish_reason"]
            .as_str()
            .unwrap_or("stop")
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
        "openai"
    }

    fn is_configured(&self) -> bool {
        self.config.api_key.is_some()
    }
}
