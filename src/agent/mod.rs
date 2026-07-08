pub mod memory;
pub mod observability;
pub mod policy;
pub mod streaming;
pub mod worker;

use crate::agent::memory::{AgentEvent, AgentMemory, EpisodicMemory, StateMemory};
use crate::agent::observability::AgentMetrics;
use crate::agent::policy::{
    ActionPolicy, AgentRunEvent, AgentRunResult, AgentRunStatus, PolicyOutcome,
};
use crate::agent::streaming::{AgentStatus, StreamEvent, StreamingAgent};
use crate::providers::{
    create_provider, AiContext, AiProvider, Message, ProviderConfig, ScrollPosition, ToolCall,
    ToolResult as AiToolResult,
};
use crate::tools::{AgentError, BrowserInterface, BrowserTool, ToolRegistry};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, OnceLock};

const CONVERSATION_WINDOW: usize = 20;

static GLOBAL_METRICS: OnceLock<AgentMetrics> = OnceLock::new();

pub fn metrics() -> &'static AgentMetrics {
    GLOBAL_METRICS.get_or_init(AgentMetrics::default)
}

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
    pub conversation: VecDeque<AgentMessage>,
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
    memory: Mutex<AgentMemory>,
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
                conversation: VecDeque::with_capacity(CONVERSATION_WINDOW),
                iterations: 0,
            }),
            memory: Mutex::new(AgentMemory {
                episodic: EpisodicMemory::default(),
                semantic: Default::default(),
                state: StateMemory::default(),
            }),
        }
    }

    pub fn snapshot_state(&self) -> Result<AgentSnapshot, String> {
        let state = self.state.lock().map_err(|e| e.to_string())?.clone();
        let memory = self.memory.lock().map_err(|e| e.to_string())?.clone();
        Ok(AgentSnapshot { state, memory })
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
            state.conversation.clear();
        }

        // Seed the conversation with the user's intent. Subsequent iterations
        // derive the LLM input from build_context (system + history + tool_results);
        // the original user_prompt is preserved as the first user message.
        self.push_conversation_bounded(AgentMessage {
            role: "user".to_string(),
            content: user_prompt.to_string(),
        })?;

        let max_iterations = self
            .config
            .lock()
            .map_err(|e| e.to_string())?
            .max_iterations;
        let mut events = Vec::new();
        metrics().record_request();

        for iteration in 0..max_iterations {
            let context = self.build_context(browser).await?;
            let provider = self.provider.lock().map_err(|e| e.to_string())?.clone();

            // The LLM always sees the user's original prompt plus the
            // accumulated tool results / conversation. We no longer overwrite
            // current_prompt each iteration (the previous design's
            // `current_prompt = format!("Observation: {result}")` discarded
            // prior observations when a single response carried multiple
            // tool calls).
            let response = provider
                .complete(user_prompt, &context)
                .await
                .map_err(|e| {
                    self.record_error_metric();
                    e.to_string()
                })?;

            self.push_conversation_bounded(AgentMessage {
                role: "assistant".to_string(),
                content: response.content.clone(),
            })?;
            self.push_episodic(AgentEvent::LlmCall {
                run_id: run_id.clone(),
                model: provider.provider_name().to_string(),
                iteration,
                content_preview: response.content.chars().take(200).collect(),
                timestamp: AgentEvent::now("LlmCall"),
            })?;

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
                    self.record_error_metric();
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
                        self.record_error_metric();
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
                self.record_tool_call_metrics(&tool_call.name);
                self.push_episodic(AgentEvent::ToolCall {
                    run_id: run_id.clone(),
                    tool: tool_call.name.clone(),
                    arguments: tool_call.arguments.clone(),
                    success,
                    result_preview: result.chars().take(200).collect(),
                    timestamp: AgentEvent::now("ToolCall"),
                })?;

                let tool_result = AiToolResult {
                    tool_name: tool_call.name.clone(),
                    arguments: tool_call.arguments.clone(),
                    result: result.clone(),
                    success,
                };

                {
                    let mut state = self.state.lock().map_err(|e| e.to_string())?;
                    state.tool_results.push(tool_result);
                    // Reflect the latest navigation state from the browser so
                    // build_context sees fresh url/title/scroll on the next
                    // iteration without needing a full re-snapshot.
                    state.current_url = snapshot.url.clone();
                    state.page_title = snapshot.title.clone();
                    state.iterations = iteration + 1;
                }
            }
        }

        self.record_error_metric();
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
                    .map(|message| Message {
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

    fn push_conversation_bounded(&self, message: AgentMessage) -> Result<(), String> {
        let mut state = self.state.lock().map_err(|e| e.to_string())?;
        if state.conversation.len() >= CONVERSATION_WINDOW {
            state.conversation.pop_front();
        }
        state.conversation.push_back(message);
        Ok(())
    }

    fn record_tool_call_metrics(&self, tool_name: &str) {
        metrics().record_tool_call_named(tool_name);
    }

    fn record_error_metric(&self) {
        metrics().record_error();
    }

    fn push_episodic(&self, event: AgentEvent) -> Result<(), String> {
        let mut memory = self.memory.lock().map_err(|e| e.to_string())?;
        memory.episodic.push(event);
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSnapshot {
    pub state: AgentState,
    pub memory: AgentMemory,
}

#[async_trait::async_trait]
impl StreamingAgent for ReActAgent {
    async fn execute_stream(
        &self,
        prompt: &str,
        browser: Arc<dyn BrowserInterface>,
        sender: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> Result<String, AgentError> {
        // The current execute_with_policy builds a complete event log; we
        // run it once with the default policy and forward the resulting
        // events through the streaming channel. This satisfies the trait
        // contract; full incremental streaming is tracked for a follow-up.
        let _ = sender
            .send(StreamEvent::Status(AgentStatus::Thinking))
            .await;

        match self
            .execute_with_policy(prompt, browser.as_ref(), &ActionPolicy::default())
            .await
        {
            Ok(run) => {
                for event in run.events {
                    let mapped = map_run_event_to_stream(event);
                    if let Some(mapped) = mapped {
                        if sender.send(mapped).await.is_err() {
                            // Receiver dropped — best-effort stop.
                            break;
                        }
                    }
                }
                let final_answer = run
                    .final_response
                    .clone()
                    .unwrap_or_else(|| "Run completed without a final response".to_string());

                match run.status {
                    AgentRunStatus::Completed => {
                        let _ = sender
                            .send(StreamEvent::Done {
                                final_response: final_answer.clone(),
                                iterations: run.iterations,
                            })
                            .await;
                        Ok(final_answer)
                    }
                    AgentRunStatus::AwaitingApproval => {
                        // Don't emit Done; the caller is expected to listen
                        // for ApprovalRequested and either submit_approval or
                        // cancel_agent_run.
                        Ok("AwaitingApproval".to_string())
                    }
                    AgentRunStatus::Blocked => {
                        let _ = sender.send(StreamEvent::Status(AgentStatus::Idle)).await;
                        Ok("Blocked".to_string())
                    }
                    AgentRunStatus::Cancelled => {
                        let _ = sender.send(StreamEvent::Status(AgentStatus::Idle)).await;
                        Ok("Cancelled".to_string())
                    }
                    AgentRunStatus::Failed => {
                        let _ = sender
                            .send(StreamEvent::Error {
                                code: "FAILED".to_string(),
                                message: final_answer.clone(),
                            })
                            .await;
                        Err(AgentError::Validation(final_answer))
                    }
                }
            }
            Err(error) => {
                let _ = sender
                    .send(StreamEvent::Error {
                        code: "EXECUTION_ERROR".to_string(),
                        message: error.clone(),
                    })
                    .await;
                Err(AgentError::Validation(error))
            }
        }
    }
}

fn map_run_event_to_stream(event: AgentRunEvent) -> Option<StreamEvent> {
    use AgentRunEvent::*;
    match event {
        ToolCallStarted { tool, .. } => Some(StreamEvent::ToolCallStart {
            tool,
            arguments: serde_json::Value::Null,
        }),
        ToolCallResult {
            tool,
            result,
            success,
            ..
        } => Some(StreamEvent::ToolCallResult {
            tool,
            result,
            success,
        }),
        ToolCallBlocked { tool, decision, .. } => Some(StreamEvent::ToolCallBlocked {
            run_id: String::new(),
            tool,
            reasons: decision.reasons,
        }),
        ApprovalRequested {
            tool,
            approval_id,
            decision,
            ..
        } => {
            let arguments = decision.redacted_arguments;
            Some(StreamEvent::ApprovalRequested {
                run_id: String::new(),
                approval_id,
                tool,
                arguments,
                reasons: decision.reasons,
            })
        }
        ApprovalResolved {
            approval_id,
            approved,
            ..
        } => Some(StreamEvent::ApprovalResolved {
            run_id: String::new(),
            approval_id,
            approved,
        }),
        RunCancelled { reason, .. } => Some(StreamEvent::RunCancelled {
            run_id: String::new(),
            reason,
        }),
        RunDone { .. } => None, // mapped at the call site
    }
}
