use crate::tools::AgentError;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
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
        sender: mpsc::Sender<StreamEvent>,
    ) -> Result<String, AgentError>;
}
