use crate::providers::{
    build_system_prompt, parse_tool_calls, AiContext, AiProvider, AiResponse, ProviderConfig,
    ProviderError, ProviderResult,
};
use async_trait::async_trait;
use reqwest::Client;
use std::time::Duration;

pub struct OllamaProvider {
    config: ProviderConfig,
    client: Client,
    base_url: String,
}

impl OllamaProvider {
    pub fn new(config: ProviderConfig) -> Self {
        let base_url = config
            .base_url
            .clone()
            .unwrap_or_else(|| "http://localhost:11434".to_string());

        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| Client::new());

        Self {
            config,
            client,
            base_url,
        }
    }

    fn build_prompt(&self, prompt: &str, context: &AiContext) -> String {
        format!("{}\nUser request: {prompt}", build_system_prompt(context))
    }
}

#[async_trait]
impl AiProvider for OllamaProvider {
    async fn complete(&self, prompt: &str, context: &AiContext) -> ProviderResult<AiResponse> {
        let full_prompt = self.build_prompt(prompt, context);

        let body = serde_json::json!({
            "model": self.config.model,
            "prompt": full_prompt,
            "stream": false,
            "options": {
                "temperature": self.config.temperature.unwrap_or(0.3),
                "num_predict": self.config.max_tokens.unwrap_or(4096),
            }
        });

        let response = self
            .client
            .post(format!("{}/api/generate", self.base_url))
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

        let content = json["response"].as_str().unwrap_or("").to_string();

        let finish_reason = if json["done"].as_bool().unwrap_or(true) {
            "stop".to_string()
        } else {
            "length".to_string()
        };

        let tool_calls = parse_tool_calls(&content);

        Ok(AiResponse {
            content,
            reasoning: None,
            tool_calls,
            finish_reason,
        })
    }

    fn provider_name(&self) -> &str {
        "ollama"
    }
}
