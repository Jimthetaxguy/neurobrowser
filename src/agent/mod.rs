mod approval;
pub mod policy;

use crate::agent::policy::{
    ActionPolicy, AgentRunEvent, AgentRunResult, AgentRunStatus, PolicyOutcome,
};
use crate::providers::{
    create_provider, AiContext, AiProvider, ProviderConfig, ToolCall, ToolResult,
};
use crate::tools::{BrowserInterface, BrowserTool, ToolRegistry};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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
struct AgentState {
    pub current_url: String,
    pub page_title: String,
    pub tool_results: Vec<ToolResult>,
}

pub struct ReActAgent {
    config: Mutex<AgentConfig>,
    provider: Mutex<Arc<dyn AiProvider + Send + Sync>>,
    tool_registry: ToolRegistry,
    state: Mutex<AgentState>,
    /// True when `search_personal_memory` and `inspect_active_page` are registered.
    ///
    /// This flag follows [`neuro_memory::MemoryService`]. The shipped crate has
    /// no in-run `agent::memory` store.
    personal_memory: bool,
    pending_approvals: Mutex<HashMap<String, approval::PendingApproval>>,
}

impl ReActAgent {
    pub fn new(config: AgentConfig, provider: Arc<dyn AiProvider + Send + Sync>) -> Self {
        Self::with_memory(config, provider, None)
    }

    /// `new`, plus the two personal-memory tools when `memory` is set.
    ///
    /// `memory` is durable page memory ([`neuro_memory::MemoryService`]).
    /// `None` keeps the 22 browser tools. Inspect uses
    /// [`neuro_memory::CapturePolicy::default`].
    pub fn with_memory(
        config: AgentConfig,
        provider: Arc<dyn AiProvider + Send + Sync>,
        memory: Option<Arc<neuro_memory::MemoryService>>,
    ) -> Self {
        let personal_memory = memory.is_some();
        let tool_registry = match memory {
            Some(memory) => crate::browser::default_tool_registry_with_memory(
                memory,
                neuro_memory::CapturePolicy::default(),
            ),
            None => crate::browser::default_tool_registry(),
        };
        Self {
            config: Mutex::new(config.clone()),
            provider: Mutex::new(provider),
            tool_registry,
            state: Mutex::new(AgentState {
                current_url: String::new(),
                page_title: String::new(),
                tool_results: Vec::new(),
            }),
            personal_memory,
            pending_approvals: Mutex::new(HashMap::new()),
        }
    }

    /// Revoke every stored grant for a cancelled or closed run.
    pub fn cancel_pending_approval(&self, run_id: &str) -> Result<(), String> {
        self.pending_approvals
            .lock()
            .map_err(|e| e.to_string())?
            .retain(|_, grant| grant.run_id != run_id);
        Ok(())
    }

    /// Reviewed authority metadata for an unexpired proposal; never returns input values.
    pub fn approval_context(
        &self,
        approval_id: &str,
    ) -> Result<Option<crate::capability::ApprovalContext>, String> {
        let mut pending = self.pending_approvals.lock().map_err(|e| e.to_string())?;
        pending.retain(|_, grant| !grant.expired());
        pending
            .get(approval_id)
            .map(|grant| grant.reviewed.context())
            .transpose()
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
            state.current_url = page_info.url;
            state.page_title = page_info.title;
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
            if response.tool_calls.is_empty() {
                let answer = self.extract_final_answer(&response.content);
                events.push(AgentRunEvent::RunDone {
                    run_id: run_id.clone(),
                    final_response: answer.clone(),
                    iterations: iteration + 1,
                });
                return Ok(run_result(
                    run_id,
                    AgentRunStatus::Completed,
                    Some(answer),
                    iteration + 1,
                    events,
                ));
            }
            for call in &response.tool_calls {
                let mut outcome = self
                    .propose_call(run_id.clone(), call.clone(), browser, policy)
                    .await?;
                events.append(&mut outcome.events);
                if outcome.status != AgentRunStatus::Completed {
                    outcome.events = events;
                    outcome.iterations = iteration + 1;
                    return Ok(outcome);
                }
            }
        }
        Ok(run_result(
            run_id,
            AgentRunStatus::Failed,
            Some("Max iterations reached".into()),
            max_iterations,
            events,
        ))
    }

    /// One proposal shared by human clients and agents; never invokes an AI provider.
    pub async fn propose_tool_with_policy(
        &self,
        tool_call: ToolCall,
        browser: &dyn BrowserInterface,
        policy: &ActionPolicy,
    ) -> Result<AgentRunResult, String> {
        let mut result = self
            .propose_call(uuid::Uuid::new_v4().to_string(), tool_call, browser, policy)
            .await?;
        if result.status == AgentRunStatus::Completed {
            result.events.push(AgentRunEvent::RunDone {
                run_id: result.run_id.clone(),
                final_response: result.final_response.clone().unwrap_or_default(),
                iterations: 1,
            });
        }
        Ok(result)
    }

    async fn review_call(
        &self,
        call: &ToolCall,
        browser: &dyn BrowserInterface,
        policy: &ActionPolicy,
    ) -> Result<
        (
            Arc<dyn BrowserTool>,
            crate::agent::policy::PolicyDecision,
            approval::ReviewedState,
        ),
        String,
    > {
        let tool = self
            .get_tool(&call.name)
            .ok_or_else(|| format!("Unknown tool '{}'", call.name))?;
        let definition = tool.definition();
        let missing = definition.missing_required_arguments(&call.arguments);
        if !missing.is_empty() {
            return Err(format!(
                "missing required argument(s): {}",
                missing.join(", ")
            ));
        }
        let mut snapshot = browser.snapshot().await?;
        let (decision, reviewed) = if crate::capability::tools::target_action(&call.name).is_some()
        {
            let (command, _) =
                crate::capability::tools::parse_target_command(&call.name, &call.arguments)?;
            let (observation, target) =
                crate::capability::tools::review_target(browser, &command).await?;
            snapshot.url = observation.url.clone();
            snapshot.title = observation.title;
            snapshot.text = Some(observation.text);
            let decision = policy.evaluate_target(
                &call.name,
                &definition.risk,
                &call.arguments,
                &snapshot,
                &target,
            );
            let reviewed = approval::ReviewedState::Target {
                document: command.document,
                target,
                url: observation.url,
            };
            (decision, reviewed)
        } else {
            (
                policy.evaluate(&call.name, &definition.risk, &call.arguments, &snapshot),
                approval::ReviewedState::snapshot(&snapshot)?,
            )
        };
        Ok((tool, decision, reviewed))
    }

    async fn propose_call(
        &self,
        run_id: String,
        call: ToolCall,
        browser: &dyn BrowserInterface,
        policy: &ActionPolicy,
    ) -> Result<AgentRunResult, String> {
        let (tool, decision, reviewed) = match self.review_call(&call, browser, policy).await {
            Ok(review) => review,
            Err(error) if error.starts_with("missing required argument") => {
                let result = format!("Error: {error}");
                let events = vec![AgentRunEvent::ToolCallResult {
                    run_id: run_id.clone(),
                    tool: call.name.clone(),
                    result: result.clone(),
                    success: false,
                }];
                self.state
                    .lock()
                    .map_err(|e| e.to_string())?
                    .tool_results
                    .push(ToolResult {
                        tool_name: call.name,
                        result: result.clone(),
                        success: false,
                    });
                return Ok(run_result(
                    run_id,
                    AgentRunStatus::Completed,
                    Some(result),
                    1,
                    events,
                ));
            }
            Err(error) => return Ok(blocked(run_id, &call, error)),
        };
        match decision.outcome {
            PolicyOutcome::Block => {
                let events = vec![AgentRunEvent::ToolCallBlocked {
                    run_id: run_id.clone(),
                    tool: call.name,
                    decision,
                }];
                Ok(run_result(
                    run_id,
                    AgentRunStatus::Blocked,
                    Some("Tool call blocked by action policy".into()),
                    1,
                    events,
                ))
            }
            PolicyOutcome::RequireApproval => {
                let approval_id = uuid::Uuid::new_v4().to_string();
                {
                    let mut pending = self.pending_approvals.lock().map_err(|e| e.to_string())?;
                    pending.retain(|_, grant| !grant.expired());
                    if pending.len() >= 64 {
                        return Ok(blocked(
                            run_id,
                            &call,
                            "Pending approval capacity reached".into(),
                        ));
                    }
                    pending.insert(
                        approval_id.clone(),
                        approval::PendingApproval {
                            run_id: run_id.clone(),
                            call: call.clone(),
                            policy: policy.clone(),
                            reviewed,
                            expires_at: approval::PendingApproval::expiry(),
                        },
                    );
                }
                let events = vec![AgentRunEvent::ApprovalRequested {
                    run_id: run_id.clone(),
                    approval_id: approval_id.clone(),
                    tool: call.name.clone(),
                    decision,
                }];
                let mut result = run_result(
                    run_id,
                    AgentRunStatus::AwaitingApproval,
                    Some("Approval required before executing browser action".into()),
                    1,
                    events,
                );
                result.pending_tool_call = Some(call);
                result.approval_id = Some(approval_id);
                Ok(result)
            }
            PolicyOutcome::Allow => self.dispatch_call(run_id, &call, tool, browser).await,
        }
    }

    /// Compatibility entry point: authority still comes only from a stored proposal.
    pub async fn execute_approved_tool(
        &self,
        run_id: String,
        approval_id: String,
        tool_call: ToolCall,
        browser: &dyn BrowserInterface,
        approved: bool,
        message: Option<String>,
    ) -> Result<AgentRunResult, String> {
        let policy = self
            .pending_approvals
            .lock()
            .map_err(|e| e.to_string())?
            .get(&approval_id)
            .map(|grant| grant.policy.clone());
        let Some(policy) = policy else {
            return Ok(blocked(
                run_id,
                &tool_call,
                "Unknown or consumed approval ID".into(),
            ));
        };
        self.execute_approved_tool_with_policy(
            run_id,
            approval_id,
            tool_call,
            browser,
            approved,
            message,
            &policy,
        )
        .await
    }

    /// Revalidates the latest host policy and reviewed document before consuming authority.
    #[allow(clippy::too_many_arguments)]
    pub async fn execute_approved_tool_with_policy(
        &self,
        run_id: String,
        approval_id: String,
        tool_call: ToolCall,
        browser: &dyn BrowserInterface,
        approved: bool,
        message: Option<String>,
        policy: &ActionPolicy,
    ) -> Result<AgentRunResult, String> {
        let pending = self
            .pending_approvals
            .lock()
            .map_err(|e| e.to_string())?
            .remove(&approval_id);
        let Some(pending) = pending else {
            return Ok(blocked(
                run_id,
                &tool_call,
                "Unknown or consumed approval ID".into(),
            ));
        };
        if !pending.authorizes(&run_id, &tool_call, policy) {
            return Ok(blocked(
                run_id,
                &tool_call,
                "Approval does not match the exact call, run or current policy".into(),
            ));
        }
        let mut events = vec![AgentRunEvent::ApprovalResolved {
            run_id: run_id.clone(),
            approval_id,
            approved,
            message: message.unwrap_or_default(),
        }];
        if !approved {
            events.push(AgentRunEvent::RunCancelled {
                run_id: run_id.clone(),
                reason: "Approval denied".into(),
            });
            return Ok(run_result(
                run_id,
                AgentRunStatus::Cancelled,
                Some("Approval denied".into()),
                0,
                events,
            ));
        }
        let command = if crate::capability::tools::target_action(&tool_call.name).is_some() {
            Some(
                crate::capability::tools::parse_target_command(
                    &tool_call.name,
                    &tool_call.arguments,
                )?
                .0,
            )
        } else {
            None
        };
        if !pending
            .reviewed
            .matches(browser, command.as_ref())
            .await
            .unwrap_or(false)
        {
            return Ok(blocked(
                run_id,
                &tool_call,
                "Reviewed page or target changed; request a new approval".into(),
            ));
        }
        let (tool, decision, _) = match self.review_call(&tool_call, browser, policy).await {
            Ok(review) => review,
            Err(error) => return Ok(blocked(run_id, &tool_call, error)),
        };
        if decision.outcome == PolicyOutcome::Block {
            return Ok(blocked(
                run_id,
                &tool_call,
                "Current action policy prohibits this action".into(),
            ));
        }
        if pending.expired() {
            return Ok(blocked(
                run_id,
                &tool_call,
                "Approval expired during revalidation; request a new approval".into(),
            ));
        }
        let mut result = self
            .dispatch_call(run_id.clone(), &tool_call, tool, browser)
            .await?;
        events.append(&mut result.events);
        result.events = events;
        if result.status == AgentRunStatus::Completed {
            result.events.push(AgentRunEvent::RunDone {
                run_id,
                final_response: result.final_response.clone().unwrap_or_default(),
                iterations: 1,
            });
        }
        Ok(result)
    }

    async fn dispatch_call(
        &self,
        run_id: String,
        call: &ToolCall,
        tool: Arc<dyn BrowserTool>,
        browser: &dyn BrowserInterface,
    ) -> Result<AgentRunResult, String> {
        let mut events = vec![AgentRunEvent::ToolCallStarted {
            run_id: run_id.clone(),
            tool: call.name.clone(),
            arguments: crate::agent::policy::redact_arguments(&call.arguments),
        }];
        let scoped_action = crate::capability::tools::target_action(&call.name).is_some();
        let legacy_mutation = !scoped_action
            && !matches!(
                tool.definition().risk.action,
                crate::tools::ToolAction::Read
                    | crate::tools::ToolAction::Wait
                    | crate::tools::ToolAction::Screenshot
            );
        let output = tool.execute(call.arguments.clone(), browser).await;
        let receipt = if crate::capability::tools::target_action(&call.name).is_some() {
            serde_json::from_str::<crate::capability::ActionReceipt>(&output.result).ok()
        } else {
            None
        };
        let result = if output.success || receipt.is_some() {
            output.result
        } else {
            format!("Error: {}", output.result)
        };
        events.push(AgentRunEvent::ToolCallResult {
            run_id: run_id.clone(),
            tool: call.name.clone(),
            result: result.clone(),
            success: output.success,
        });
        self.state
            .lock()
            .map_err(|e| e.to_string())?
            .tool_results
            .push(ToolResult {
                tool_name: call.name.clone(),
                result: result.clone(),
                success: output.success,
            });
        // A dispatched action is never automatically retried when its outcome or proof is unresolved.
        if receipt.as_ref().is_some_and(|receipt| {
            receipt.dispatch == crate::capability::DispatchState::Unknown
                || (receipt.dispatch == crate::capability::DispatchState::Acknowledged
                    && (!receipt.page_ready
                        || matches!(
                            receipt.verification,
                            crate::capability::VerificationState::Unavailable
                                | crate::capability::VerificationState::Unsatisfied
                        )))
        }) {
            return Ok(run_result(
                run_id,
                AgentRunStatus::Failed,
                Some(result),
                1,
                events,
            ));
        }
        // Legacy mutation results lack typed uncertainty. Any error stops structurally,
        // so a provider cannot turn an ambiguous failure into an automatic replay.
        if legacy_mutation && !output.success {
            return Ok(run_result(
                run_id,
                AgentRunStatus::Failed,
                Some(
                    "Legacy action outcome unresolved; inspect the page before any further action"
                        .into(),
                ),
                1,
                events,
            ));
        }
        if legacy_mutation && browser.wait_for_navigation().await.is_err() {
            // Dispatch truth remains success in ToolCallResult; readiness is a separate fact.
            return Ok(run_result(run_id, AgentRunStatus::Failed, Some("Legacy action acknowledged; page readiness unconfirmed. Inspect before any further action".into()), 1, events));
        }
        // Refresh labels after a tool; transient loading failures retain the last known labels.
        let post_snapshot = match browser.snapshot().await {
            Ok(snapshot) => Some(snapshot),
            Err(_) => browser.snapshot().await.ok(),
        };
        if let Some(snapshot) = post_snapshot {
            let mut state = self.state.lock().map_err(|e| e.to_string())?;
            state.current_url = snapshot.url;
            state.page_title = snapshot.title;
        }
        Ok(run_result(
            run_id,
            AgentRunStatus::Completed,
            Some(result),
            1,
            events,
        ))
    }

    fn build_context(&self) -> Result<AiContext, String> {
        let state = self.state.lock().map_err(|e| e.to_string())?;
        Ok(AiContext {
            current_url: state.current_url.clone(),
            page_title: state.page_title.clone(),
            tool_results: state.tool_results.clone(),
            personal_memory: self.personal_memory,
        })
    }

    fn get_tool(&self, name: &str) -> Option<Arc<dyn BrowserTool>> {
        self.tool_registry.get(name)
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

fn run_result(
    run_id: String,
    status: AgentRunStatus,
    final_response: Option<String>,
    iterations: usize,
    events: Vec<AgentRunEvent>,
) -> AgentRunResult {
    AgentRunResult {
        run_id,
        status,
        final_response,
        iterations,
        events,
        pending_tool_call: None,
        approval_id: None,
    }
}
fn blocked(run_id: String, call: &ToolCall, reason: String) -> AgentRunResult {
    let decision = policy::PolicyDecision {
        outcome: PolicyOutcome::Block,
        reasons: vec![reason.clone()],
        risk_flags: vec![policy::RiskFlag::ActionDenied],
        redacted_arguments: policy::redact_arguments(&call.arguments),
    };
    let events = vec![AgentRunEvent::ToolCallBlocked {
        run_id: run_id.clone(),
        tool: call.name.clone(),
        decision,
    }];
    run_result(run_id, AgentRunStatus::Blocked, Some(reason), 0, events)
}

#[cfg(test)]
mod approval_context_tests {
    use super::*;
    use crate::tools::PageSnapshot;
    #[test]
    fn expired_grant_has_no_public_context_and_is_pruned() {
        let agent = ReActAgent::new(
            AgentConfig::default(),
            create_provider(&ProviderConfig::default()),
        );
        let call = ToolCall {
            name: "click".into(),
            arguments: HashMap::from([("selector".into(), "#next".into())]),
        };
        agent.pending_approvals.lock().unwrap().insert(
            "expired".into(),
            approval::PendingApproval {
                run_id: "run".into(),
                call,
                policy: ActionPolicy::default(),
                reviewed: approval::ReviewedState::snapshot(&PageSnapshot::default()).unwrap(),
                expires_at: std::time::Instant::now() - std::time::Duration::from_secs(1),
            },
        );
        assert!(agent.approval_context("expired").unwrap().is_none());
        assert!(agent.pending_approvals.lock().unwrap().is_empty());
    }
}
