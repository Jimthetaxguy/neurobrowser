pub mod policy;

use crate::agent::policy::{
    ActionPolicy, AgentRunEvent, AgentRunResult, AgentRunStatus, PolicyOutcome,
};
use crate::providers::{
    create_provider, AiContext, AiProvider, ProviderConfig, ToolCall, ToolResult,
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
    pub tool_results: Vec<ToolResult>,
    pub iterations: usize,
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
                iterations: 0,
            }),
        }
    }

    pub fn set_provider_config(&self, provider_config: ProviderConfig) -> Result<(), String> {
        let mut config = self.config.lock().map_err(|e| e.to_string())?;
        config.provider_config = provider_config.clone();

        let mut provider = self.provider.lock().map_err(|e| e.to_string())?;
        *provider = create_provider(&provider_config);

        tracing::info!("Provider changed to: {:?}", provider_config.provider_type);
        Ok(())
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

        let max_iterations = self
            .config
            .lock()
            .map_err(|e| e.to_string())?
            .max_iterations;
        let mut events = Vec::new();

        for iteration in 0..max_iterations {
            let context = self.build_context()?;
            let provider = self.provider.lock().map_err(|e| e.to_string())?.clone();

            let response = provider
                .complete(user_prompt, &context)
                .await
                .map_err(|e| e.to_string())?;

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

                let tool_result = ToolResult {
                    tool_name: tool_call.name.clone(),
                    arguments: tool_call.arguments.clone(),
                    result: result.clone(),
                    success,
                };

                // Re-snapshot AFTER the tool ran so a navigating tool updates the
                // url/title the model sees on the next iteration. The pre-execution
                // `snapshot` (used above for policy evaluation) is stale here after
                // a navigate. Taken outside the state lock to avoid holding it
                // across `.await`.
                // A post-tool snapshot may legitimately fail transiently: on the
                // desktop runtime it can land while the old document is unloading.
                // Propagating that with `?` failed the ENTIRE otherwise-successful run
                // over a timing artifact. Retry once, then degrade to keeping the
                // previous url/title rather than discarding the run's work — the tool
                // already succeeded, and a stale label is a smaller lie than a failed
                // run that actually did its job.
                let post_snapshot = match browser.snapshot().await {
                    Ok(snapshot) => Some(snapshot),
                    Err(first_err) => {
                        tracing::debug!(
                            error = %first_err,
                            "post-tool snapshot failed; retrying once"
                        );
                        match browser.snapshot().await {
                            Ok(snapshot) => Some(snapshot),
                            Err(second_err) => {
                                tracing::warn!(
                                    error = %second_err,
                                    "post-tool snapshot failed twice; keeping previous \
                                     url/title for this iteration"
                                );
                                None
                            }
                        }
                    }
                };
                {
                    let mut state = self.state.lock().map_err(|e| e.to_string())?;
                    state.tool_results.push(tool_result);
                    if let Some(snapshot) = post_snapshot {
                        state.current_url = snapshot.url;
                        state.page_title = snapshot.title;
                    }
                    state.iterations = iteration + 1;
                }
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
            state.tool_results.push(ToolResult {
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

    fn build_context(&self) -> Result<AiContext, String> {
        let state = self.state.lock().map_err(|e| e.to_string())?;
        Ok(AiContext {
            current_url: state.current_url.clone(),
            page_title: state.page_title.clone(),
            tool_results: state.tool_results.clone(),
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
