use crate::tools::{AgentError, BrowserInterface};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentStatus {
    Idle,
    Thinking,
    Searching,
    ExecutingTool,
    Writing,
    WaitingForInput,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum StreamEvent {
    Token {
        text: String,
    },
    ToolCallStart {
        tool: String,
        arguments: serde_json::Value,
    },
    ToolCallResult {
        tool: String,
        result: String,
        success: bool,
    },
    ToolCallBlocked {
        run_id: String,
        tool: String,
        reasons: Vec<String>,
    },
    ApprovalRequested {
        run_id: String,
        approval_id: String,
        tool: String,
        arguments: HashMap<String, String>,
        reasons: Vec<String>,
    },
    ApprovalResolved {
        run_id: String,
        approval_id: String,
        approved: bool,
    },
    RunCancelled {
        run_id: String,
        reason: String,
    },
    Status(AgentStatus),
    Error {
        code: String,
        message: String,
    },
    Done {
        final_response: String,
        iterations: usize,
    },
}

#[async_trait]
pub trait StreamingAgent: Send + Sync {
    async fn execute_stream(
        &self,
        prompt: &str,
        browser: Arc<dyn BrowserInterface>,
        sender: mpsc::Sender<StreamEvent>,
    ) -> Result<String, AgentError>;
}

/// Execute a streaming agent with a timeout.
/// Returns the agent result or AgentError::Timeout if the deadline is exceeded.
/// Sends StreamEvent::Error before returning on timeout.
pub async fn execute_with_timeout(
    agent: &dyn StreamingAgent,
    prompt: &str,
    browser: Arc<dyn BrowserInterface>,
    sender: mpsc::Sender<StreamEvent>,
    timeout_secs: u64,
) -> Result<String, AgentError> {
    match tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
        agent.execute_stream(prompt, browser, sender.clone()),
    )
    .await
    {
        Ok(result) => result,
        Err(_elapsed) => {
            let _ = sender
                .send(StreamEvent::Error {
                    code: "TIMEOUT".to_string(),
                    message: format!("Agent execution exceeded {timeout_secs}s deadline"),
                })
                .await;
            Err(AgentError::Timeout(format!(
                "Agent execution exceeded {timeout_secs}s deadline"
            )))
        }
    }
}
