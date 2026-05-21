use crate::providers::ToolCall;
use crate::tools::{PageSnapshot, ToolAction, ToolRisk};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutonomyLevel {
    ReadOnly,
    Assisted,
    HighAutonomy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyOutcome {
    Allow,
    RequireApproval,
    Block,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskFlag {
    ActionDenied,
    DomainDenied,
    DomainNotAllowed,
    ExternalNavigation,
    SensitiveArgument,
    PromptInjection,
    RequiresApproval,
    HighImpactAction,
    ReadOnlyMode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionPolicy {
    pub autonomy_level: AutonomyLevel,
    pub allowed_domains: Vec<String>,
    pub denied_domains: Vec<String>,
    pub denied_tools: Vec<String>,
    pub approval_required_tools: Vec<String>,
    pub block_prompt_injection: bool,
}

impl Default for ActionPolicy {
    fn default() -> Self {
        Self {
            autonomy_level: AutonomyLevel::Assisted,
            allowed_domains: Vec::new(),
            denied_domains: Vec::new(),
            denied_tools: Vec::new(),
            approval_required_tools: Vec::new(),
            block_prompt_injection: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyDecision {
    pub outcome: PolicyOutcome,
    pub reasons: Vec<String>,
    pub risk_flags: Vec<RiskFlag>,
    pub redacted_arguments: HashMap<String, String>,
}

impl PolicyDecision {
    fn allow(redacted_arguments: HashMap<String, String>) -> Self {
        Self {
            outcome: PolicyOutcome::Allow,
            reasons: Vec::new(),
            risk_flags: Vec::new(),
            redacted_arguments,
        }
    }

    fn with_outcome(
        outcome: PolicyOutcome,
        reasons: Vec<String>,
        risk_flags: Vec<RiskFlag>,
        redacted_arguments: HashMap<String, String>,
    ) -> Self {
        Self {
            outcome,
            reasons,
            risk_flags,
            redacted_arguments,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRunStatus {
    Completed,
    AwaitingApproval,
    Blocked,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AgentRunEvent {
    ToolCallStarted {
        run_id: String,
        tool: String,
        arguments: HashMap<String, String>,
    },
    ToolCallResult {
        run_id: String,
        tool: String,
        result: String,
        success: bool,
    },
    ToolCallBlocked {
        run_id: String,
        tool: String,
        decision: PolicyDecision,
    },
    ApprovalRequested {
        run_id: String,
        approval_id: String,
        tool: String,
        decision: PolicyDecision,
    },
    ApprovalResolved {
        run_id: String,
        approval_id: String,
        approved: bool,
        message: String,
    },
    RunCancelled {
        run_id: String,
        reason: String,
    },
    RunDone {
        run_id: String,
        final_response: String,
        iterations: usize,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRunResult {
    pub run_id: String,
    pub status: AgentRunStatus,
    pub final_response: Option<String>,
    pub iterations: usize,
    pub events: Vec<AgentRunEvent>,
    pub pending_tool_call: Option<ToolCall>,
    pub approval_id: Option<String>,
}

impl ActionPolicy {
    pub fn evaluate(
        &self,
        tool_name: &str,
        tool_risk: &ToolRisk,
        arguments: &HashMap<String, String>,
        snapshot: &PageSnapshot,
    ) -> PolicyDecision {
        let redacted_arguments = redact_arguments(arguments);
        let mut reasons = Vec::new();
        let mut flags = Vec::new();

        if self
            .denied_tools
            .iter()
            .any(|denied| denied.eq_ignore_ascii_case(tool_name))
        {
            reasons.push(format!("Tool '{tool_name}' is denied by policy"));
            flags.push(RiskFlag::ActionDenied);
            return PolicyDecision::with_outcome(
                PolicyOutcome::Block,
                reasons,
                flags,
                redacted_arguments,
            );
        }

        if self.block_prompt_injection && snapshot_contains_prompt_injection(snapshot) {
            reasons.push("Page content contains prompt-injection-like instructions".to_string());
            flags.push(RiskFlag::PromptInjection);
            return PolicyDecision::with_outcome(
                PolicyOutcome::Block,
                reasons,
                flags,
                redacted_arguments,
            );
        }

        if let Some(domain) = target_domain(tool_name, arguments, snapshot) {
            if self
                .denied_domains
                .iter()
                .any(|denied| domain_matches(&domain, denied))
            {
                reasons.push(format!("Domain '{domain}' is denied by policy"));
                flags.push(RiskFlag::DomainDenied);
                return PolicyDecision::with_outcome(
                    PolicyOutcome::Block,
                    reasons,
                    flags,
                    redacted_arguments,
                );
            }

            if !self.allowed_domains.is_empty()
                && !self
                    .allowed_domains
                    .iter()
                    .any(|allowed| domain_matches(&domain, allowed))
            {
                reasons.push(format!("Domain '{domain}' is not in the allowlist"));
                flags.push(RiskFlag::DomainNotAllowed);
                return PolicyDecision::with_outcome(
                    PolicyOutcome::Block,
                    reasons,
                    flags,
                    redacted_arguments,
                );
            }

            if tool_risk.action == ToolAction::Navigate && is_cross_domain(&domain, snapshot) {
                flags.push(RiskFlag::ExternalNavigation);
            }
        }

        if contains_sensitive_argument(arguments) || tool_risk.sensitive {
            reasons.push("Tool call contains sensitive input".to_string());
            flags.push(RiskFlag::SensitiveArgument);
            return PolicyDecision::with_outcome(
                PolicyOutcome::RequireApproval,
                reasons,
                flags,
                redacted_arguments,
            );
        }

        if self
            .approval_required_tools
            .iter()
            .any(|required| required.eq_ignore_ascii_case(tool_name))
        {
            reasons.push(format!("Tool '{tool_name}' requires approval by policy"));
            flags.push(RiskFlag::RequiresApproval);
            return PolicyDecision::with_outcome(
                PolicyOutcome::RequireApproval,
                reasons,
                flags,
                redacted_arguments,
            );
        }

        match self.autonomy_level {
            AutonomyLevel::ReadOnly => match tool_risk.action {
                ToolAction::Read | ToolAction::Wait | ToolAction::Scroll | ToolAction::Navigate => {
                    PolicyDecision::allow(redacted_arguments)
                }
                _ => {
                    reasons.push("Read-only mode blocks this action".to_string());
                    flags.push(RiskFlag::ReadOnlyMode);
                    PolicyDecision::with_outcome(
                        PolicyOutcome::Block,
                        reasons,
                        flags,
                        redacted_arguments,
                    )
                }
            },
            AutonomyLevel::Assisted => match tool_risk.action {
                ToolAction::Read | ToolAction::Wait | ToolAction::Scroll => {
                    PolicyDecision::allow(redacted_arguments)
                }
                ToolAction::Navigate if !flags.contains(&RiskFlag::ExternalNavigation) => {
                    PolicyDecision::allow(redacted_arguments)
                }
                _ => {
                    reasons.push("Assisted mode requires approval for this action".to_string());
                    flags.push(RiskFlag::RequiresApproval);
                    if tool_risk.externally_visible {
                        flags.push(RiskFlag::HighImpactAction);
                    }
                    PolicyDecision::with_outcome(
                        PolicyOutcome::RequireApproval,
                        reasons,
                        flags,
                        redacted_arguments,
                    )
                }
            },
            AutonomyLevel::HighAutonomy => match tool_risk.action {
                ToolAction::Submit
                | ToolAction::Purchase
                | ToolAction::Auth
                | ToolAction::Upload
                | ToolAction::Message
                | ToolAction::Destructive => {
                    reasons.push("High-impact action requires approval".to_string());
                    flags.push(RiskFlag::HighImpactAction);
                    PolicyDecision::with_outcome(
                        PolicyOutcome::RequireApproval,
                        reasons,
                        flags,
                        redacted_arguments,
                    )
                }
                _ => PolicyDecision::allow(redacted_arguments),
            },
        }
    }
}

pub fn redact_arguments(arguments: &HashMap<String, String>) -> HashMap<String, String> {
    arguments
        .iter()
        .map(|(key, value)| {
            if is_sensitive_key(key) {
                (key.clone(), "[REDACTED]".to_string())
            } else {
                (key.clone(), value.clone())
            }
        })
        .collect()
}

fn contains_sensitive_argument(arguments: &HashMap<String, String>) -> bool {
    arguments.keys().any(|key| is_sensitive_key(key))
}

fn is_sensitive_key(key: &str) -> bool {
    let key = key.to_lowercase();
    [
        "password", "passcode", "token", "secret", "api_key", "apikey", "ssn", "social", "credit",
        "card", "cvv", "otp", "auth",
    ]
    .iter()
    .any(|needle| key.contains(needle))
}

fn snapshot_contains_prompt_injection(snapshot: &PageSnapshot) -> bool {
    let text = snapshot
        .text
        .as_deref()
        .or(snapshot.html.as_deref())
        .unwrap_or_default()
        .to_lowercase();
    [
        "ignore previous instructions",
        "ignore all previous instructions",
        "disregard previous instructions",
        "system prompt",
        "developer message",
        "reveal your instructions",
        "exfiltrate",
        "run this tool",
        "without asking the user",
    ]
    .iter()
    .any(|pattern| text.contains(pattern))
}

fn target_domain(
    tool_name: &str,
    arguments: &HashMap<String, String>,
    snapshot: &PageSnapshot,
) -> Option<String> {
    let url = if tool_name == "navigate" {
        arguments.get("url").map(String::as_str)
    } else {
        Some(snapshot.url.as_str())
    }?;
    url::Url::parse(url)
        .ok()
        .and_then(|url| url.host_str().map(|host| host.to_lowercase()))
}

fn is_cross_domain(candidate_domain: &str, snapshot: &PageSnapshot) -> bool {
    let Some(current_domain) = target_domain("current", &HashMap::new(), snapshot) else {
        return false;
    };
    current_domain != candidate_domain
}

fn domain_matches(domain: &str, rule: &str) -> bool {
    let rule = rule.trim().trim_start_matches('.').to_lowercase();
    domain == rule || domain.ends_with(&format!(".{rule}"))
}
