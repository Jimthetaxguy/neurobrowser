//! Approval authority stays inside the agent; public IDs are lookup keys, not grants.
use super::policy::ActionPolicy;
use crate::capability::{ApprovalContext, DocumentStamp, ObservedTarget, TargetCommand};
use crate::providers::ToolCall;
use crate::tools::{BrowserInterface, PageSnapshot};
use std::time::{Duration, Instant};

#[derive(Clone)]
pub(super) enum ReviewedState {
    Snapshot(String),
    Target {
        document: DocumentStamp,
        target: ObservedTarget,
        url: String,
    },
}
impl ReviewedState {
    pub(super) fn snapshot(snapshot: &PageSnapshot) -> Result<Self, String> {
        serde_json::to_string(snapshot)
            .map(Self::Snapshot)
            .map_err(|error| error.to_string())
    }
    pub(super) fn context(&self) -> Result<ApprovalContext, String> {
        match self {
            Self::Snapshot(json) => {
                let snapshot: PageSnapshot =
                    serde_json::from_str(json).map_err(|error| error.to_string())?;
                Ok(ApprovalContext {
                    url: snapshot.url,
                    document: None,
                    target: None,
                })
            }
            Self::Target {
                document,
                target,
                url,
            } => Ok(ApprovalContext {
                url: url.clone(),
                document: Some(document.clone()),
                target: Some(target.clone()),
            }),
        }
    }

    pub(super) async fn matches(
        &self,
        browser: &dyn BrowserInterface,
        command: Option<&TargetCommand>,
    ) -> Result<bool, String> {
        match self {
            Self::Snapshot(reviewed) => Ok(serde_json::to_string(&browser.snapshot().await?)
                .map_err(|error| error.to_string())?
                == *reviewed),
            Self::Target {
                document,
                target,
                url,
            } => {
                let command = command.ok_or("Approval target command missing")?;
                let (observation, current_target) =
                    crate::capability::tools::review_target(browser, command).await?;
                Ok(observation.document.as_ref() == Some(document)
                    && current_target == *target
                    && observation.url == *url)
            }
        }
    }
}
#[derive(Clone)]
pub(super) struct PendingApproval {
    pub run_id: String,
    pub call: ToolCall,
    pub policy: ActionPolicy,
    pub reviewed: ReviewedState,
    pub expires_at: Instant,
}
impl PendingApproval {
    pub(super) fn expiry() -> Instant {
        Instant::now() + Duration::from_secs(5 * 60)
    }
    pub(super) fn expired(&self) -> bool {
        Instant::now() >= self.expires_at
    }
    pub(super) fn authorizes(&self, run_id: &str, call: &ToolCall, policy: &ActionPolicy) -> bool {
        !self.expired()
            && self.run_id == run_id
            && self.call.name == call.name
            && self.call.arguments == call.arguments
            && self.policy == *policy
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    #[test]
    fn expired_grant_cannot_authorize_without_waiting() {
        let call = ToolCall {
            name: "submit_form".into(),
            arguments: HashMap::new(),
        };
        let policy = ActionPolicy::default();
        let grant = PendingApproval {
            run_id: "run".into(),
            call: call.clone(),
            policy: policy.clone(),
            reviewed: ReviewedState::Snapshot("{}".into()),
            expires_at: Instant::now() - Duration::from_secs(1),
        };
        assert!(grant.expired());
        assert!(!grant.authorizes("run", &call, &policy));
    }
    #[test]
    fn legacy_approval_context_projects_only_url() {
        let snapshot = PageSnapshot {
            url: "https://example.com".into(),
            html: Some("secret input value".into()),
            text: Some("private document".into()),
            ..PageSnapshot::default()
        };
        let context = ReviewedState::snapshot(&snapshot)
            .unwrap()
            .context()
            .unwrap();
        let json = serde_json::to_string(&context).unwrap();
        assert_eq!(context.url, "https://example.com");
        assert!(context.target.is_none() && context.document.is_none());
        assert!(!json.contains("secret") && !json.contains("private"));
    }
}
