use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
