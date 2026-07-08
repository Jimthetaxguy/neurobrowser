pub mod memory;
pub mod policy;
pub mod streaming;

use crate::agent::policy::{
    ActionPolicy, AgentRunEvent, AgentRunResult, AgentRunStatus, PolicyOutcome,
};
use crate::providers::{
    create_provider, AiContext, AiProvider, ProviderConfig, ScrollPosition, ToolCall,
    ToolResult as AiToolResult,
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
    config: Mutex<AgentConfig>,
    provider: Mutex<Arc<dyn AiProvider + Send + Sync>>,
    tool_registry: Mutex<ToolRegistry>,
    state: Mutex<AgentState>,
}

impl ReActAgent {
    pub fn new(config: AgentConfig, provider: Arc<dyn AiProvider + Send + Sync>) -> Self {
        Self {
            config: Mutex::new(config.clone()),
            provider: Mutex::new(provider),
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

    pub fn set_provider_config(&self, provider_config: ProviderConfig) -> Result<(), String> {
        let mut config = self.config.lock().map_err(|e| e.to_string())?;
        config.provider_config = provider_config.clone();

        let mut provider = self.provider.lock().map_err(|e| e.to_string())?;
        *provider = create_provider(&provider_config);

        tracing::info!("Provider changed to: {:?}", provider_config.provider_type);
        Ok(())
    }

    pub async fn execute(
        &self,
        user_prompt: &str,
        browser: &dyn BrowserInterface,
    ) -> Result<String, String> {
        let run = self
            .execute_with_policy(user_prompt, browser, &ActionPolicy::default())
            .await?;
        match run.status {
            AgentRunStatus::Completed => Ok(run.final_response.unwrap_or_default()),
            AgentRunStatus::AwaitingApproval => {
                Ok("Action requires approval before continuing".to_string())
            }
            AgentRunStatus::Blocked => Ok("Action blocked by policy".to_string()),
            AgentRunStatus::Cancelled => Ok("Run cancelled".to_string()),
            AgentRunStatus::Failed => Err(run
                .final_response
                .unwrap_or_else(|| "Agent run failed".to_string())),
        }
    }

    pub async fn execute_with_policy(
        &self,
        user_prompt: &str,
        browser: &dyn BrowserInterface,
        policy: &ActionPolicy,
    ) -> Result<AgentRunResult, String> {
        let run_id = uuid::Uuid::new_v4().to_string();
        let page_info = browser.snapshot().await?;

        {
            let mut state = self.state.lock().map_err(|e| e.to_string())?;
            state.current_url = page_info.url.clone();
            state.page_title = page_info.title.clone();
            state.iterations = 0;
            state.tool_results.clear();
        }

        let mut current_prompt = user_prompt.to_string();

        let max_iterations = self
            .config
            .lock()
            .map_err(|e| e.to_string())?
            .max_iterations;
        let mut events = Vec::new();

        for iteration in 0..max_iterations {
            let context = self.build_context(browser).await?;
            let provider = self.provider.lock().map_err(|e| e.to_string())?.clone();

            let response = provider
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
                events.push(AgentRunEvent::RunDone {
                    run_id: run_id.clone(),
                    final_response: answer.clone(),
                    iterations: iteration + 1,
                });
                return Ok(AgentRunResult {
                    run_id,
                    status: AgentRunStatus::Completed,
                    final_response: Some(answer),
                    iterations: iteration + 1,
                    events,
                    pending_tool_call: None,
                    approval_id: None,
                });
            }

            for tool_call in &response.tool_calls {
                let snapshot = browser.snapshot().await?;
                let Some(tool) = self.get_tool(&tool_call.name)? else {
                    let decision = crate::agent::policy::PolicyDecision {
                        outcome: PolicyOutcome::Block,
                        reasons: vec![format!("Unknown tool '{}'", tool_call.name)],
                        risk_flags: vec![crate::agent::policy::RiskFlag::ActionDenied],
                        redacted_arguments: crate::agent::policy::redact_arguments(
                            &tool_call.arguments,
                        ),
                    };
                    events.push(AgentRunEvent::ToolCallBlocked {
                        run_id: run_id.clone(),
                        tool: tool_call.name.clone(),
                        decision,
                    });
                    return Ok(AgentRunResult {
                        run_id,
                        status: AgentRunStatus::Blocked,
                        final_response: Some(format!("Unknown tool '{}'", tool_call.name)),
                        iterations: iteration + 1,
                        events,
                        pending_tool_call: None,
                        approval_id: None,
                    });
                };

                let decision = policy.evaluate(
                    &tool_call.name,
                    &tool.definition().risk,
                    &tool_call.arguments,
                    &snapshot,
                );

                match decision.outcome {
                    PolicyOutcome::Block => {
                        events.push(AgentRunEvent::ToolCallBlocked {
                            run_id: run_id.clone(),
                            tool: tool_call.name.clone(),
                            decision,
                        });
                        return Ok(AgentRunResult {
                            run_id,
                            status: AgentRunStatus::Blocked,
                            final_response: Some("Tool call blocked by action policy".to_string()),
                            iterations: iteration + 1,
                            events,
                            pending_tool_call: None,
                            approval_id: None,
                        });
                    }
                    PolicyOutcome::RequireApproval => {
                        let approval_id = uuid::Uuid::new_v4().to_string();
                        events.push(AgentRunEvent::ApprovalRequested {
                            run_id: run_id.clone(),
                            approval_id: approval_id.clone(),
                            tool: tool_call.name.clone(),
                            decision,
                        });
                        return Ok(AgentRunResult {
                            run_id,
                            status: AgentRunStatus::AwaitingApproval,
                            final_response: Some(
                                "Approval required before executing browser action".to_string(),
                            ),
                            iterations: iteration + 1,
                            events,
                            pending_tool_call: Some(tool_call.clone()),
                            approval_id: Some(approval_id),
                        });
                    }
                    PolicyOutcome::Allow => {
                        events.push(AgentRunEvent::ToolCallStarted {
                            run_id: run_id.clone(),
                            tool: tool_call.name.clone(),
                            arguments: decision.redacted_arguments,
                        });
                    }
                }

                let result = self
                    .execute_tool_with_handle(tool_call, tool, browser)
                    .await;
                let success = result.is_ok();
                let result = match result {
                    Ok(value) => value,
                    Err(error) => format!("Error: {error}"),
                };
                events.push(AgentRunEvent::ToolCallResult {
                    run_id: run_id.clone(),
                    tool: tool_call.name.clone(),
                    result: result.clone(),
                    success,
                });

                let tool_result = AiToolResult {
                    tool_name: tool_call.name.clone(),
                    arguments: tool_call.arguments.clone(),
                    result: result.clone(),
                    success,
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

        Ok(AgentRunResult {
            run_id,
            status: AgentRunStatus::Failed,
            final_response: Some("Max iterations reached".to_string()),
            iterations: max_iterations,
            events,
            pending_tool_call: None,
            approval_id: None,
        })
    }

    pub async fn execute_approved_tool(
        &self,
        run_id: String,
        approval_id: String,
        tool_call: ToolCall,
        browser: &dyn BrowserInterface,
        approved: bool,
        message: Option<String>,
    ) -> Result<AgentRunResult, String> {
        let mut events = vec![AgentRunEvent::ApprovalResolved {
            run_id: run_id.clone(),
            approval_id,
            approved,
            message: message.unwrap_or_default(),
        }];

        if !approved {
            events.push(AgentRunEvent::RunCancelled {
                run_id: run_id.clone(),
                reason: "Approval denied".to_string(),
            });
            return Ok(AgentRunResult {
                run_id,
                status: AgentRunStatus::Cancelled,
                final_response: Some("Approval denied".to_string()),
                iterations: 0,
                events,
                pending_tool_call: None,
                approval_id: None,
            });
        }

        let Some(tool) = self.get_tool(&tool_call.name)? else {
            events.push(AgentRunEvent::ToolCallBlocked {
                run_id: run_id.clone(),
                tool: tool_call.name.clone(),
                decision: crate::agent::policy::PolicyDecision {
                    outcome: PolicyOutcome::Block,
                    reasons: vec![format!("Unknown tool '{}'", tool_call.name)],
                    risk_flags: vec![crate::agent::policy::RiskFlag::ActionDenied],
                    redacted_arguments: crate::agent::policy::redact_arguments(
                        &tool_call.arguments,
                    ),
                },
            });
            return Ok(AgentRunResult {
                run_id,
                status: AgentRunStatus::Blocked,
                final_response: Some(format!("Unknown tool '{}'", tool_call.name)),
                iterations: 0,
                events,
                pending_tool_call: None,
                approval_id: None,
            });
        };

        events.push(AgentRunEvent::ToolCallStarted {
            run_id: run_id.clone(),
            tool: tool_call.name.clone(),
            arguments: crate::agent::policy::redact_arguments(&tool_call.arguments),
        });
        let result = self
            .execute_tool_with_handle(&tool_call, tool, browser)
            .await
            .map_err(|error| format!("Error: {error}"));
        let success = result.is_ok();
        let result = result.unwrap_or_else(|error| error);
        events.push(AgentRunEvent::ToolCallResult {
            run_id: run_id.clone(),
            tool: tool_call.name.clone(),
            result: result.clone(),
            success,
        });

        {
            let mut state = self.state.lock().map_err(|e| e.to_string())?;
            state.tool_results.push(AiToolResult {
                tool_name: tool_call.name.clone(),
                arguments: tool_call.arguments.clone(),
                result: result.clone(),
                success,
            });
        }

        events.push(AgentRunEvent::RunDone {
            run_id: run_id.clone(),
            final_response: result.clone(),
            iterations: 1,
        });

        Ok(AgentRunResult {
            run_id,
            status: AgentRunStatus::Completed,
            final_response: Some(result),
            iterations: 1,
            events,
            pending_tool_call: None,
            approval_id: None,
        })
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

    fn get_tool(&self, name: &str) -> Result<Option<Arc<dyn BrowserTool>>, String> {
        let registry = self.tool_registry.lock().map_err(|e| e.to_string())?;
        Ok(registry.get(name))
    }

    async fn execute_tool_with_handle(
        &self,
        tool_call: &ToolCall,
        tool: Arc<dyn BrowserTool>,
        browser: &dyn BrowserInterface,
    ) -> Result<String, String> {
        let result = tool.execute(tool_call.arguments.clone(), browser).await;
        if result.success {
            Ok(result.result)
        } else {
            Err(result.result)
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
