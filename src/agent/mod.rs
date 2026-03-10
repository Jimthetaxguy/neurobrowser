pub mod memory;
pub mod streaming;

use crate::providers::{
    AiContext, AiProvider, ProviderConfig, ScrollPosition, ToolCall, ToolResult as AiToolResult,
};
use crate::tools::{BrowserInterface, BrowserTool, ToolRegistry};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub max_iterations: usize,
    pub provider_config: ProviderConfig,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_iterations: 5,
            provider_config: ProviderConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentState {
    pub current_url: String,
    pub page_title: String,
    pub tool_results: Vec<AiToolResult>,
    pub conversation: Vec<AgentMessage>,
    pub iterations: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMessage {
    pub role: String,
    pub content: String,
}

pub struct ReActAgent {
    config: AgentConfig,
    provider: Arc<dyn AiProvider + Send + Sync>,
    tool_registry: Mutex<ToolRegistry>,
    state: Mutex<AgentState>,
}

impl ReActAgent {
    pub fn new(config: AgentConfig, provider: Arc<dyn AiProvider + Send + Sync>) -> Self {
        Self {
            config: config.clone(),
            provider,
            tool_registry: Mutex::new(crate::browser::default_tool_registry()),
            state: Mutex::new(AgentState {
                current_url: String::new(),
                page_title: String::new(),
                tool_results: Vec::new(),
                conversation: Vec::new(),
                iterations: 0,
            }),
        }
    }

    pub fn with_tools(self, tools: ToolRegistry) -> Self {
        *self.tool_registry.lock().unwrap() = tools;
        self
    }

    pub fn set_provider(&self, provider_type: crate::providers::ProviderType) {
        // This would require a mutable provider - for now, we just log the change
        // A full implementation would swap out the provider instance
        tracing::info!("Provider change requested to: {:?}", provider_type);
    }

    pub async fn execute(
        &self,
        user_prompt: &str,
        browser: &dyn BrowserInterface,
    ) -> Result<String, String> {
        let page_info = browser.snapshot().await?;

        {
            let mut state = self.state.lock().map_err(|e| e.to_string())?;
            state.current_url = page_info.url.clone();
            state.page_title = page_info.title.clone();
            state.iterations = 0;
            state.tool_results.clear();
        }

        let mut current_prompt = user_prompt.to_string();

        for iteration in 0..self.config.max_iterations {
            let context = self.build_context(browser).await?;

            let response = self
                .provider
                .complete(&current_prompt, &context)
                .await
                .map_err(|e| e.to_string())?;

            {
                let mut state = self.state.lock().map_err(|e| e.to_string())?;
                state.conversation.push(AgentMessage {
                    role: "assistant".to_string(),
                    content: response.content.clone(),
                });
            }

            if response.finish_reason == "stop" || response.tool_calls.is_empty() {
                let answer = self.extract_final_answer(&response.content);
                return Ok(answer);
            }

            for tool_call in &response.tool_calls {
                let result = self.execute_tool(tool_call, browser).await?;

                let tool_result = AiToolResult {
                    tool_name: tool_call.name.clone(),
                    arguments: tool_call.arguments.clone(),
                    result: result.clone(),
                    success: !result.starts_with("Error:"),
                };

                {
                    let mut state = self.state.lock().map_err(|e| e.to_string())?;
                    state.tool_results.push(tool_result);
                }

                current_prompt = format!("Observation: {}", result);
            }

            {
                let mut state = self.state.lock().map_err(|e| e.to_string())?;
                state.iterations = iteration + 1;
            }
        }

        Err("Max iterations reached".to_string())
    }

    async fn build_context(&self, browser: &dyn BrowserInterface) -> Result<AiContext, String> {
        let page_info = browser.snapshot().await?;

        let (current_url, page_title, tool_results, conversation_history) = {
            let state = self.state.lock().map_err(|e| e.to_string())?;
            (
                state.current_url.clone(),
                state.page_title.clone(),
                state.tool_results.clone(),
                state
                    .conversation
                    .iter()
                    .map(|message| crate::providers::Message {
                        role: message.role.clone(),
                        content: message.content.clone(),
                    })
                    .collect::<Vec<_>>(),
            )
        };

        let dom_snapshot = format!(
            "URL: {}\nTitle: {}\nInteractive: {}\nLinks: {}\nImages: {}\nForms: {}",
            page_info.url,
            page_info.title,
            page_info.interactive_ready,
            page_info.links.len(),
            page_info.images.len(),
            page_info.forms.len()
        );

        Ok(AiContext {
            current_url,
            page_title,
            dom_snapshot,
            accessibility_tree: browser.accessibility_tree().await?,
            scroll_position: ScrollPosition {
                x: page_info.scroll_x,
                y: page_info.scroll_y,
            },
            tool_results,
            conversation_history,
        })
    }

    async fn execute_tool(
        &self,
        tool_call: &ToolCall,
        browser: &dyn BrowserInterface,
    ) -> Result<String, String> {
        // Get tool from registry BEFORE await to avoid holding lock across await point
        let tool: Option<Arc<dyn BrowserTool>> = {
            let registry = self.tool_registry.lock().map_err(|e| e.to_string())?;
            registry.get(&tool_call.name)
        };

        if let Some(tool) = tool {
            let result = tool.execute(tool_call.arguments.clone(), browser).await;
            if result.success {
                Ok(result.result)
            } else {
                Err(result.result)
            }
        } else {
            Err(format!("Unknown tool '{}'", tool_call.name))
        }
    }

    fn extract_final_answer(&self, content: &str) -> String {
        for line in content.lines() {
            let line = line.trim();
            if line.starts_with("Final Answer:") {
                return line
                    .strip_prefix("Final Answer:")
                    .unwrap()
                    .trim()
                    .to_string();
            }
        }
        content.to_string()
    }
}
