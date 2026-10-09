use async_trait::async_trait;
use neurobrowser::capability::{
    ActionReceipt, DispatchState, DocumentStamp, ObservationLimits, ObservedTarget,
    PageObservation, RuntimeCapabilities, TargetCommand, TargetDispatchError, VerificationState,
};
use neurobrowser::providers::ProviderResult;
use neurobrowser::{
    ActionPolicy, AgentConfig, AgentRunStatus, AiContext, AiProvider, AiResponse, AutonomyLevel,
    BrowserInterface, ElementInfo, PageSnapshot, ReActAgent, ToolCall,
};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

struct Browser {
    observation: Mutex<PageObservation>,
    dispatched: Mutex<usize>,
    dispatch_error: Option<DispatchState>,
    legacy_dispatch_error: bool,
    readiness_error: bool,
    observation_error_after_dispatch: bool,
}
impl Browser {
    fn new() -> Self {
        Self {
            observation: Mutex::new(PageObservation {
                schema_version: 1,
                document: Some(DocumentStamp {
                    runtime_id: "runtime".into(),
                    document_id: "document".into(),
                    revision: 1,
                }),
                url: "https://current.example".into(),
                title: "Current".into(),
                text: "Ready".into(),
                links: vec![],
                tables: vec![],
                targets: vec![ObservedTarget {
                    id: "target".into(),
                    role: "button".into(),
                    label: "Continue".into(),
                    tag: "button".into(),
                    disabled: false,
                    sensitive: false,
                    destination: None,
                }],
                omissions: vec![],
                capabilities: RuntimeCapabilities::desktop(),
                collected_at_ms: 0,
            }),
            dispatched: Mutex::new(0),
            dispatch_error: None,
            legacy_dispatch_error: false,
            readiness_error: false,
            observation_error_after_dispatch: false,
        }
    }
    fn call(&self, name: &str) -> ToolCall {
        let document =
            serde_json::to_string(self.observation.lock().unwrap().document.as_ref().unwrap())
                .unwrap();
        let mut arguments = HashMap::from([
            ("document".into(), document),
            ("target_id".into(), "target".into()),
        ]);
        if name == "type_target" {
            arguments.insert("text".into(), "private input".into());
        }
        ToolCall {
            name: name.into(),
            arguments,
        }
    }
    fn dispatch_count(&self) -> usize {
        *self.dispatched.lock().unwrap()
    }
}
#[async_trait]
impl BrowserInterface for Browser {
    fn capabilities(&self) -> RuntimeCapabilities {
        RuntimeCapabilities::desktop()
    }
    async fn observe(&self, _limits: ObservationLimits) -> Result<PageObservation, String> {
        if self.observation_error_after_dispatch && self.dispatch_count() > 0 {
            return Err("Observation unavailable".into());
        }
        Ok(self.observation.lock().unwrap().clone())
    }
    async fn dispatch_target(&self, _command: &TargetCommand) -> Result<(), TargetDispatchError> {
        *self.dispatched.lock().unwrap() += 1;
        match self.dispatch_error {
            Some(state) => Err(TargetDispatchError {
                state,
                message: "private input MUST NOT appear in receipts".into(),
            }),
            None => Ok(()),
        }
    }
    async fn snapshot(&self) -> Result<PageSnapshot, String> {
        let observation = self.observation.lock().unwrap();
        Ok(PageSnapshot {
            url: "https://snapshot.example".into(),
            title: observation.title.clone(),
            text: Some(observation.text.clone()),
            interactive_ready: true,
            ..PageSnapshot::default()
        })
    }
    async fn wait_for_navigation(&self) -> Result<(), String> {
        if self.readiness_error {
            Err("Loading timed out".into())
        } else {
            Ok(())
        }
    }
    async fn navigate(&self, _url: &str) -> Result<(), String> {
        *self.dispatched.lock().unwrap() += 1;
        if self.legacy_dispatch_error {
            Err("Transport disconnected".into())
        } else {
            Ok(())
        }
    }
    async fn query_selector(&self, _selector: &str) -> Result<Vec<ElementInfo>, String> {
        Ok(vec![])
    }
    async fn get_text(&self, _selector: &str) -> Result<String, String> {
        Ok("Ready".into())
    }
    async fn click(&self, _selector: &str) -> Result<(), String> {
        *self.dispatched.lock().unwrap() += 1;
        Ok(())
    }
    async fn type_text(&self, _selector: &str, _text: &str) -> Result<(), String> {
        *self.dispatched.lock().unwrap() += 1;
        Ok(())
    }
    async fn submit_form(&self, _selector: &str) -> Result<(), String> {
        *self.dispatched.lock().unwrap() += 1;
        if self.legacy_dispatch_error {
            Err("Transport disconnected".into())
        } else {
            Ok(())
        }
    }
    async fn scroll_to(&self, _selector: &str) -> Result<(), String> {
        Ok(())
    }
    async fn scroll_by(&self, _x: f32, _y: f32) -> Result<(), String> {
        Ok(())
    }
}
struct Provider {
    calls: Mutex<usize>,
    responses: Mutex<VecDeque<AiResponse>>,
}
#[async_trait]
impl AiProvider for Provider {
    async fn complete(&self, _prompt: &str, _context: &AiContext) -> ProviderResult<AiResponse> {
        *self.calls.lock().unwrap() += 1;
        Ok(self
            .responses
            .lock()
            .unwrap()
            .pop_front()
            .expect("Unexpected extra provider iteration"))
    }
}
fn agent() -> ReActAgent {
    ReActAgent::new(
        AgentConfig::default(),
        Arc::new(Provider {
            calls: Mutex::new(0),
            responses: Mutex::new(VecDeque::new()),
        }),
    )
}
fn policy() -> ActionPolicy {
    ActionPolicy {
        autonomy_level: AutonomyLevel::HighAutonomy,
        ..ActionPolicy::default()
    }
}

#[tokio::test]
async fn forged_approval_never_dispatches() {
    let browser = Browser::new();
    let result = agent()
        .execute_approved_tool(
            "invented".into(),
            "forged".into(),
            browser.call("click_target"),
            &browser,
            true,
            None,
        )
        .await
        .unwrap();
    assert_eq!(result.status, AgentRunStatus::Blocked);
    assert_eq!(browser.dispatch_count(), 0);
}

#[tokio::test]
async fn exact_approval_dispatches_once_and_excludes_input() {
    let browser = Browser::new();
    let agent = agent();
    let call = browser.call("type_target");
    let policy = policy();
    let pending = agent
        .propose_tool_with_policy(call.clone(), &browser, &policy)
        .await
        .unwrap();
    assert_eq!(pending.status, AgentRunStatus::AwaitingApproval);
    assert!(!serde_json::to_string(&pending.events)
        .unwrap()
        .contains("private input"));
    let run = pending.run_id;
    let approval = pending.approval_id.unwrap();
    let result = agent
        .execute_approved_tool_with_policy(
            run.clone(),
            approval.clone(),
            call.clone(),
            &browser,
            true,
            None,
            &policy,
        )
        .await
        .unwrap();
    assert_eq!(result.status, AgentRunStatus::Completed);
    let receipt: ActionReceipt =
        serde_json::from_str(result.final_response.as_ref().unwrap()).unwrap();
    assert_eq!(receipt.dispatch, DispatchState::Acknowledged);
    assert!(!result.final_response.unwrap().contains("private input"));
    let duplicate = agent
        .execute_approved_tool(run, approval, call, &browser, true, None)
        .await
        .unwrap();
    assert_eq!(duplicate.status, AgentRunStatus::Blocked);
    assert_eq!(browser.dispatch_count(), 1);
}

#[tokio::test]
async fn altered_call_policy_run_and_reviewed_target_invalidate_approval() {
    for alteration in ["call", "policy", "run", "stamp", "target", "url", "cancel"] {
        let browser = Browser::new();
        let agent = agent();
        let mut call = browser.call("click_target");
        let mut policy = policy();
        let pending = agent
            .propose_tool_with_policy(call.clone(), &browser, &policy)
            .await
            .unwrap();
        let mut run = pending.run_id;
        let approval = pending.approval_id.unwrap();
        match alteration {
            "call" => {
                call.arguments.insert("target_id".into(), "other".into());
            }
            "policy" => policy.autonomy_level = AutonomyLevel::ReadOnly,
            "run" => run = "different-run".into(),
            "stamp" => {
                browser
                    .observation
                    .lock()
                    .unwrap()
                    .document
                    .as_mut()
                    .unwrap()
                    .revision += 1
            }
            "target" => browser.observation.lock().unwrap().targets[0].label = "Delete".into(),
            "url" => browser.observation.lock().unwrap().url = "https://other.example".into(),
            "cancel" => agent.cancel_pending_approval(&run).unwrap(),
            _ => unreachable!(),
        }
        let result = agent
            .execute_approved_tool_with_policy(run, approval, call, &browser, true, None, &policy)
            .await
            .unwrap();
        assert_eq!(result.status, AgentRunStatus::Blocked, "{alteration}");
        assert_eq!(browser.dispatch_count(), 0, "{alteration}");
    }
}

#[tokio::test]
async fn legacy_selector_approval_revalidates_snapshot() {
    let browser = Browser::new();
    let agent = agent();
    let call = ToolCall {
        name: "click".into(),
        arguments: HashMap::from([("selector".into(), "#continue".into())]),
    };
    let pending = agent
        .propose_tool_with_policy(call.clone(), &browser, &ActionPolicy::default())
        .await
        .unwrap();
    browser.observation.lock().unwrap().title = "Different page".into();
    let result = agent
        .execute_approved_tool(
            pending.run_id,
            pending.approval_id.unwrap(),
            call,
            &browser,
            true,
            None,
        )
        .await
        .unwrap();
    assert_eq!(result.status, AgentRunStatus::Blocked);
    assert_eq!(browser.dispatch_count(), 0);
}

#[tokio::test]
async fn native_observed_url_is_used_for_policy_not_snapshot_url() {
    let browser = Browser::new();
    let result = agent()
        .propose_tool_with_policy(
            browser.call("click_target"),
            &browser,
            &ActionPolicy {
                denied_domains: vec!["current.example".into()],
                ..policy()
            },
        )
        .await
        .unwrap();
    assert_eq!(result.status, AgentRunStatus::Blocked);
    assert_eq!(browser.dispatch_count(), 0);
}

#[tokio::test]
async fn stale_unknown_disabled_and_unsupported_targets_never_dispatch() {
    for alteration in ["stamp", "target", "disabled", "unsupported", "native_url"] {
        let browser = Browser::new();
        let call = browser.call("click_target");
        {
            let mut observation = browser.observation.lock().unwrap();
            match alteration {
                "stamp" => observation.document.as_mut().unwrap().revision += 1,
                "target" => observation.targets.clear(),
                "disabled" => observation.targets[0].disabled = true,
                "unsupported" => observation.capabilities.scoped_targets = false,
                "native_url" => observation.capabilities.native_url = false,
                _ => unreachable!(),
            }
        }
        let result = agent()
            .propose_tool_with_policy(call, &browser, &policy())
            .await
            .unwrap();
        assert_eq!(result.status, AgentRunStatus::Blocked, "{alteration}");
        assert_eq!(browser.dispatch_count(), 0);
    }
}

#[tokio::test]
async fn unknown_dispatch_halts_remaining_calls_and_provider_iterations() {
    let mut browser = Browser::new();
    browser.dispatch_error = Some(DispatchState::Unknown);
    let provider = Arc::new(Provider {
        calls: Mutex::new(0),
        responses: Mutex::new(VecDeque::from([AiResponse {
            content: String::new(),
            tool_calls: vec![browser.call("scroll_target"), browser.call("scroll_target")],
        }])),
    });
    let agent = ReActAgent::new(AgentConfig::default(), provider.clone());
    let result = agent
        .execute_with_policy("Scroll twice", &browser, &policy())
        .await
        .unwrap();
    assert_eq!(result.status, AgentRunStatus::Failed);
    assert_eq!(browser.dispatch_count(), 1);
    assert_eq!(*provider.calls.lock().unwrap(), 1);
    let receipt: ActionReceipt =
        serde_json::from_str(result.final_response.as_ref().unwrap()).unwrap();
    assert_eq!(receipt.dispatch, DispatchState::Unknown);
    assert!(!receipt.message.contains("private input"));
}

#[tokio::test]
async fn readiness_and_postcondition_failures_preserve_acknowledged_dispatch() {
    for failure in ["readiness", "predicate", "observation"] {
        let mut browser = Browser::new();
        let mut call = browser.call("scroll_target");
        if failure == "readiness" {
            browser.readiness_error = true;
        } else {
            call.arguments.insert(
                "postcondition".into(),
                r#"{"type":"text_contains","text":"Order confirmed"}"#.into(),
            );
        }
        if failure == "observation" {
            browser.observation_error_after_dispatch = true;
        }
        let result = agent()
            .propose_tool_with_policy(call, &browser, &policy())
            .await
            .unwrap();
        assert_eq!(result.status, AgentRunStatus::Failed, "{failure}");
        let receipt: ActionReceipt =
            serde_json::from_str(result.final_response.as_ref().unwrap()).unwrap();
        assert_eq!(receipt.dispatch, DispatchState::Acknowledged);
        assert_eq!(browser.dispatch_count(), 1);
        if failure == "predicate" {
            assert_eq!(receipt.verification, VerificationState::Unsatisfied);
        }
        if failure == "observation" {
            assert_eq!(receipt.verification, VerificationState::Unavailable);
        }
        if failure == "readiness" {
            assert!(!receipt.page_ready);
        }
    }
}

#[tokio::test]
async fn predicate_success_is_document_evidence() {
    let browser = Browser::new();
    let mut call = browser.call("scroll_target");
    call.arguments.insert(
        "postcondition".into(),
        r#"{"type":"url_equals","url":"https://current.example"}"#.into(),
    );
    let result = agent()
        .propose_tool_with_policy(call, &browser, &policy())
        .await
        .unwrap();
    assert_eq!(result.status, AgentRunStatus::Completed);
    let receipt: ActionReceipt =
        serde_json::from_str(result.final_response.as_ref().unwrap()).unwrap();
    assert_eq!(receipt.verification, VerificationState::Satisfied);
    assert!(receipt
        .message
        .contains("does not certify a business transaction"));
}

#[tokio::test]
async fn private_target_destination_cannot_be_proposed_for_approval() {
    let browser = Browser::new();
    browser.observation.lock().unwrap().targets[0].destination =
        Some("http://127.0.0.1/admin".into());
    let result = agent()
        .propose_tool_with_policy(browser.call("click_target"), &browser, &policy())
        .await
        .unwrap();
    assert_eq!(result.status, AgentRunStatus::Blocked);
    assert_eq!(browser.dispatch_count(), 0);
}

#[tokio::test]
async fn malformed_scoped_arguments_never_reach_approval_or_dispatch() {
    for alteration in ["unknown", "oversized_stamp", "empty_identity", "predicate"] {
        let browser = Browser::new();
        let mut call = browser.call("click_target");
        match alteration {
            "unknown" => {
                call.arguments
                    .insert("selector".into(), "#different".into());
            }
            "oversized_stamp" => {
                call.arguments.insert("document".into(), "x".repeat(1025));
            }
            "empty_identity" => {
                call.arguments.insert(
                    "document".into(),
                    r#"{"runtime_id":"","document_id":"document","revision":1}"#.into(),
                );
            }
            "predicate" => {
                call.arguments.insert(
                    "postcondition".into(),
                    r#"{"type":"text_contains","text":""}"#.into(),
                );
            }
            _ => unreachable!(),
        }
        let result = agent()
            .propose_tool_with_policy(call, &browser, &policy())
            .await
            .unwrap();
        assert_eq!(result.status, AgentRunStatus::Blocked, "{alteration}");
        assert!(result.approval_id.is_none());
        assert_eq!(browser.dispatch_count(), 0);
    }
}

#[tokio::test]
async fn explicit_dispatch_rejection_receipt_is_not_unknown_and_omits_input() {
    use neurobrowser::capability::{tools::TargetTool, TargetAction};
    use neurobrowser::tools::BrowserTool;
    let mut browser = Browser::new();
    browser.dispatch_error = Some(DispatchState::NotDispatched);
    let call = browser.call("type_target");
    let output = TargetTool::new(TargetAction::Type)
        .execute(call.arguments, &browser)
        .await;
    assert!(!output.success);
    let receipt: ActionReceipt = serde_json::from_str(&output.result).unwrap();
    assert_eq!(receipt.dispatch, DispatchState::NotDispatched);
    assert!(!output.result.contains("private input"));
}

#[tokio::test]
async fn readonly_scoped_type_prohibition_cannot_be_approved() {
    let browser = Browser::new();
    let result = agent()
        .propose_tool_with_policy(
            browser.call("type_target"),
            &browser,
            &ActionPolicy {
                autonomy_level: AutonomyLevel::ReadOnly,
                approval_required_tools: vec!["type_target".into()],
                ..policy()
            },
        )
        .await
        .unwrap();
    assert_eq!(result.status, AgentRunStatus::Blocked);
    assert!(result.approval_id.is_none());
    assert_eq!(browser.dispatch_count(), 0);
}

#[tokio::test]
async fn invalid_observation_authority_cannot_be_used_or_returned_as_evidence() {
    use neurobrowser::capability::tools::ObservePageTool;
    use neurobrowser::tools::BrowserTool;
    let browser = Browser::new();
    let call = browser.call("click_target");
    browser.observation.lock().unwrap().schema_version = 99;
    let result = agent()
        .propose_tool_with_policy(call, &browser, &policy())
        .await
        .unwrap();
    assert_eq!(result.status, AgentRunStatus::Blocked);
    assert_eq!(browser.dispatch_count(), 0);
    let observation = ObservePageTool.execute(HashMap::new(), &browser).await;
    assert!(!observation.success);
}

#[tokio::test]
async fn legacy_mutation_error_halts_batch_before_next_call_or_provider_turn() {
    let mut browser = Browser::new();
    browser.legacy_dispatch_error = true;
    let call = ToolCall {
        name: "navigate".into(),
        arguments: HashMap::from([("url".into(), "https://example.com/next".into())]),
    };
    let provider = Arc::new(Provider {
        calls: Mutex::new(0),
        responses: Mutex::new(VecDeque::from([AiResponse {
            content: String::new(),
            tool_calls: vec![call.clone(), call],
        }])),
    });
    let agent = ReActAgent::new(AgentConfig::default(), provider.clone());
    let result = agent
        .execute_with_policy("Navigate", &browser, &policy())
        .await
        .unwrap();
    assert_eq!(result.status, AgentRunStatus::Failed);
    assert_eq!(browser.dispatch_count(), 1);
    assert_eq!(*provider.calls.lock().unwrap(), 1);
}

#[tokio::test]
async fn legacy_submit_batch_has_no_replay_after_failed_approved_dispatch_or_readiness() {
    for failure in ["dispatch", "readiness"] {
        let mut browser = Browser::new();
        browser.legacy_dispatch_error = failure == "dispatch";
        browser.readiness_error = failure == "readiness";
        let call = ToolCall {
            name: "submit_form".into(),
            arguments: HashMap::from([("selector".into(), "#checkout".into())]),
        };
        let provider = Arc::new(Provider {
            calls: Mutex::new(0),
            responses: Mutex::new(VecDeque::from([AiResponse {
                content: String::new(),
                tool_calls: vec![call.clone(), call.clone()],
            }])),
        });
        let agent = ReActAgent::new(AgentConfig::default(), provider.clone());
        let pending = agent
            .execute_with_policy("Submit checkout", &browser, &policy())
            .await
            .unwrap();
        assert_eq!(pending.status, AgentRunStatus::AwaitingApproval);
        let result = agent
            .execute_approved_tool_with_policy(
                pending.run_id,
                pending.approval_id.unwrap(),
                call,
                &browser,
                true,
                None,
                &policy(),
            )
            .await
            .unwrap();
        assert_eq!(result.status, AgentRunStatus::Failed, "{failure}");
        assert_eq!(browser.dispatch_count(), 1);
        assert_eq!(*provider.calls.lock().unwrap(), 1);
        let success = result
            .events
            .iter()
            .find_map(|event| match event {
                neurobrowser::AgentRunEvent::ToolCallResult { success, .. } => Some(*success),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            success,
            failure == "readiness",
            "Dispatch truth must survive readiness failure"
        );
    }
}

#[tokio::test]
async fn scroll_and_type_do_not_govern_an_unused_destination() {
    let browser = Browser::new();
    browser.observation.lock().unwrap().targets[0].destination =
        Some("http://127.0.0.1/private".into());
    let agent = agent();
    let scroll = agent
        .propose_tool_with_policy(
            browser.call("scroll_target"),
            &browser,
            &ActionPolicy {
                autonomy_level: AutonomyLevel::ReadOnly,
                ..policy()
            },
        )
        .await
        .unwrap();
    assert_eq!(scroll.status, AgentRunStatus::Completed);
    let typing = agent
        .propose_tool_with_policy(browser.call("type_target"), &browser, &policy())
        .await
        .unwrap();
    assert_eq!(typing.status, AgentRunStatus::AwaitingApproval);
    assert_eq!(browser.dispatch_count(), 1);
}

#[tokio::test]
async fn pending_context_exposes_reviewed_target_without_input_values() {
    let browser = Browser::new();
    let agent = agent();
    let pending = agent
        .propose_tool_with_policy(browser.call("type_target"), &browser, &policy())
        .await
        .unwrap();
    let context = agent
        .approval_context(pending.approval_id.as_ref().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(context.url, "https://current.example");
    assert_eq!(context.target.unwrap().label, "Continue");
    assert_eq!(context.document.unwrap().revision, 1);
    let context = agent
        .approval_context(pending.approval_id.as_ref().unwrap())
        .unwrap()
        .unwrap();
    assert!(!serde_json::to_string(&context)
        .unwrap()
        .contains("private input"));
    agent.cancel_pending_approval(&pending.run_id).unwrap();
    assert!(agent
        .approval_context(pending.approval_id.as_ref().unwrap())
        .unwrap()
        .is_none());
}
