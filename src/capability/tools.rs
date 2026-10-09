//! Observation and document-scoped action tools for the shared registry.
use super::{
    ActionReceipt, DispatchState, DocumentStamp, ObservationLimits, ObservedTarget,
    PageObservation, TargetAction, TargetCommand, VerificationState, CAPABILITY_SCHEMA_VERSION,
};
use crate::tools::{
    BrowserInterface, BrowserTool, ToolAction, ToolArgumentDefinition, ToolDefinition, ToolResult,
    ToolRisk,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Postcondition {
    UrlEquals { url: String },
    TextContains { text: String },
}

pub fn target_action(name: &str) -> Option<TargetAction> {
    match name {
        "click_target" => Some(TargetAction::Click),
        "type_target" => Some(TargetAction::Type),
        "submit_target" => Some(TargetAction::Submit),
        "scroll_target" => Some(TargetAction::Scroll),
        _ => None,
    }
}
pub fn parse_target_command(
    name: &str,
    args: &HashMap<String, String>,
) -> Result<(TargetCommand, Option<Postcondition>), String> {
    let action = target_action(name).ok_or("Unknown scoped action")?;
    if args.iter().any(|(key, value)| {
        !matches!(key.as_str(), "document" | "target_id" | "postcondition")
            && !(key == "text" && action == TargetAction::Type)
            || key.len() + value.len() > 20_000
    }) || args
        .iter()
        .map(|(key, value)| key.len() + value.len())
        .sum::<usize>()
        > 28_000
    {
        return Err("Unknown or oversized scoped action argument".into());
    }

    let document_json = args
        .get("document")
        .filter(|json| json.len() <= 1024)
        .ok_or("Missing or oversized document stamp")?;
    let document: DocumentStamp =
        serde_json::from_str(document_json).map_err(|_| "Invalid document stamp")?;
    if document.runtime_id.is_empty()
        || document.runtime_id.len() > 200
        || document.document_id.is_empty()
        || document.document_id.len() > 200
    {
        return Err("Invalid document identity".into());
    }
    let predicate = postcondition(args)?;
    let target_id = args
        .get("target_id")
        .filter(|value| !value.is_empty() && value.len() <= 200)
        .cloned()
        .ok_or("Invalid target ID")?;
    let text = if action == TargetAction::Type {
        Some(
            args.get("text")
                .filter(|text| text.len() <= 16_384)
                .cloned()
                .ok_or("Missing or oversized input text")?,
        )
    } else {
        None
    };
    Ok((
        TargetCommand {
            document,
            target_id,
            action,
            text,
        },
        predicate,
    ))
}
fn postcondition(args: &HashMap<String, String>) -> Result<Option<Postcondition>, String> {
    args.get("postcondition")
        .map(|json| {
            if json.len() > 8192 {
                return Err("Oversized postcondition".into());
            }
            let predicate: Postcondition =
                serde_json::from_str(json).map_err(|_| "Invalid postcondition")?;
            match &predicate {
                Postcondition::UrlEquals { url } if url.is_empty() => {
                    Err("Empty URL predicate".into())
                }
                Postcondition::TextContains { text } if text.is_empty() => {
                    Err("Empty text predicate".into())
                }
                _ => Ok(predicate),
            }
        })
        .transpose()
}
/// Resolve only from fresh host evidence; the caller cannot supply destination metadata.
pub async fn review_target(
    browser: &dyn BrowserInterface,
    command: &TargetCommand,
) -> Result<(PageObservation, ObservedTarget), String> {
    let observation = browser.observe(ObservationLimits::default()).await?;
    observation.validate()?;
    if !observation.capabilities.scoped_targets
        || !observation.capabilities.interaction
        || !observation.capabilities.native_url
    {
        return Err("Runtime does not support trusted scoped interaction".into());
    }
    if observation.document.as_ref() != Some(&command.document) {
        return Err("Document changed; observe again before acting".into());
    }
    let target = observation
        .targets
        .iter()
        .find(|target| target.id == command.target_id)
        .cloned()
        .ok_or("Target is unavailable in the current observation")?;
    if target.disabled {
        return Err("Target is disabled".into());
    }
    if let Some(destination) = target
        .destination
        .as_ref()
        .filter(|_| matches!(command.action, TargetAction::Click | TargetAction::Submit))
    {
        // The dispatched href/form action is governed by the same boundary as navigation.
        if let Some(reason) = crate::netguard::blocked_reason(destination) {
            return Err(format!("Target destination blocked: {reason}"));
        }
    }
    Ok((observation, target))
}

pub struct ObservePageTool;
#[async_trait]
impl BrowserTool for ObservePageTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new("observe_page", "Read bounded page evidence, runtime capabilities and document-scoped target IDs. Evidence excludes field values, raw HTML and frames.", ToolRisk::new(ToolAction::Read))
    }
    async fn execute(
        &self,
        _args: HashMap<String, String>,
        browser: &dyn BrowserInterface,
    ) -> ToolResult {
        match browser
            .observe(ObservationLimits::default())
            .await
            .and_then(|observation| {
                observation.validate()?;
                serde_json::to_string(&observation).map_err(|error| error.to_string())
            }) {
            Ok(json) => ToolResult::success("observe_page", json),
            Err(error) => ToolResult::error("observe_page", error),
        }
    }
}
pub struct TargetTool {
    action: TargetAction,
}
impl TargetTool {
    pub fn new(action: TargetAction) -> Self {
        Self { action }
    }
    fn name(&self) -> &'static str {
        match self.action {
            TargetAction::Click => "click_target",
            TargetAction::Type => "type_target",
            TargetAction::Submit => "submit_target",
            TargetAction::Scroll => "scroll_target",
        }
    }
}
#[async_trait]
impl BrowserTool for TargetTool {
    fn definition(&self) -> ToolDefinition {
        let risk = match self.action {
            TargetAction::Click => ToolRisk::new(ToolAction::Click).externally_visible(true),
            TargetAction::Type => ToolRisk::new(ToolAction::Type).sensitive(true),
            TargetAction::Submit => ToolRisk::new(ToolAction::Submit).externally_visible(true),
            TargetAction::Scroll => ToolRisk::new(ToolAction::Scroll),
        };
        let mut arguments = vec![
            ToolArgumentDefinition::required("document", "JSON document stamp from observe_page"),
            ToolArgumentDefinition::required(
                "target_id",
                "Target ID from that document observation",
            ),
        ];
        if self.action == TargetAction::Type {
            arguments.push(ToolArgumentDefinition::required(
                "text",
                "Input text; excluded from action receipts",
            ));
        }
        arguments.push(ToolArgumentDefinition { name: "postcondition".into(), required: false, description: "Optional JSON predicate: {\"type\":\"url_equals\",\"url\":\"...\"} or {\"type\":\"text_contains\",\"text\":\"...\"}. Proves observed document evidence only, not transaction success.".into() });
        ToolDefinition::new(self.name(), "Act on a document-scoped target. Receipt separates dispatch, page readiness and observed predicate verification; an uncertain dispatch must not be retried automatically.", risk).with_arguments(arguments)
    }
    async fn execute(
        &self,
        args: HashMap<String, String>,
        browser: &dyn BrowserInterface,
    ) -> ToolResult {
        let (command, predicate) = match parse_target_command(self.name(), &args) {
            Ok(parsed) => parsed,
            Err(error) => return ToolResult::error(self.name(), error),
        };
        let mut receipt = ActionReceipt {
            schema_version: CAPABILITY_SCHEMA_VERSION,
            receipt_id: uuid::Uuid::new_v4().to_string(),
            document: command.document.clone(),
            target_id: command.target_id.clone(),
            action: command.action,
            dispatch: DispatchState::NotDispatched,
            page_ready: false,
            verification: VerificationState::NotRequested,
            message: "Action not dispatched".into(),
        };
        if let Err(error) = review_target(browser, &command).await {
            receipt.message = error;
            return receipt_result(self.name(), receipt);
        }
        match browser.dispatch_target(&command).await {
            Err(error) => {
                receipt.dispatch = error.state;
                // Native errors may include submitted values; return a fixed diagnostic.
                receipt.message = match error.state {
                    DispatchState::NotDispatched => {
                        "Runtime rejected the scoped action; observe again before retrying"
                    }
                    _ => "Dispatch outcome unknown; inspect the page before any further action",
                }
                .into();
                if predicate.is_some() {
                    receipt.verification = VerificationState::Unavailable;
                }
                return receipt_result(self.name(), receipt);
            }
            Ok(()) => receipt.dispatch = DispatchState::Acknowledged,
        }
        receipt.page_ready = browser.wait_for_navigation().await.is_ok();
        receipt.verification = match predicate {
            None => VerificationState::NotRequested,
            Some(predicate) => match browser.observe(ObservationLimits::default()).await {
                Err(_) => VerificationState::Unavailable,
                Ok(observation) if observation.validate().is_err() => {
                    VerificationState::Unavailable
                }
                Ok(observation) => {
                    let satisfied = match predicate {
                        Postcondition::UrlEquals { url } => observation.url == url,
                        Postcondition::TextContains { text } => observation.text.contains(&text),
                    };
                    if satisfied {
                        VerificationState::Satisfied
                    } else {
                        VerificationState::Unsatisfied
                    }
                }
            },
        };
        receipt.message = if !receipt.page_ready { "Dispatch acknowledged; page readiness unavailable. Inspect before any further action." } else if receipt.verification == VerificationState::Unsatisfied { "Dispatch acknowledged; requested document predicate was not observed." } else if receipt.verification == VerificationState::Unavailable { "Dispatch acknowledged; requested document predicate could not be checked." } else { "Dispatch acknowledged; requested evidence collected. This does not certify a business transaction." }.into();
        receipt_result(self.name(), receipt)
    }
}
fn receipt_result(name: &str, receipt: ActionReceipt) -> ToolResult {
    let success = receipt.dispatch == DispatchState::Acknowledged
        && receipt.page_ready
        && matches!(
            receipt.verification,
            VerificationState::NotRequested | VerificationState::Satisfied
        );
    ToolResult {
        tool_name: name.into(),
        result: serde_json::to_string(&receipt).expect("receipt is serializable"),
        success,
    }
}
