use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolError {
    pub code: String,
    pub message: String,
    pub details: Option<serde_json::Value>,
    pub retryable: bool,
}

impl ToolError {
    pub fn new(code: &str, message: &str) -> Self {
        Self {
            code: code.to_string(),
            message: message.to_string(),
            details: None,
            retryable: false,
        }
    }

    pub fn timeout(msg: &str) -> Self {
        Self {
            code: "TIMEOUT".to_string(),
            message: msg.to_string(),
            details: None,
            retryable: true,
        }
    }

    pub fn not_found(msg: &str) -> Self {
        Self {
            code: "NOT_FOUND".to_string(),
            message: msg.to_string(),
            details: None,
            retryable: false,
        }
    }

    pub fn invalid_input(msg: &str) -> Self {
        Self {
            code: "INVALID_INPUT".to_string(),
            message: msg.to_string(),
            details: None,
            retryable: false,
        }
    }

    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }

    pub fn retryable(mut self, retryable: bool) -> Self {
        self.retryable = retryable;
        self
    }
}

impl From<ToolError> for String {
    fn from(err: ToolError) -> String {
        format!(
            "[{}] {} (retryable: {})",
            err.code, err.message, err.retryable
        )
    }
}

/// Agent-level errors. Migrated to thiserror derive in Ralph Plan Loop 50 (H15).
/// ToolError is intentionally a struct (not enum) — see CLAUDE.md.
#[derive(Debug, Error)]
pub enum AgentError {
    #[error("Provider error: {0}")]
    Provider(String),

    #[error("Tool error: {0}")]
    Tool(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Timeout: {0}")]
    Timeout(String),

    #[error("Max iterations reached")]
    MaxIterationsReached,
}

impl From<crate::providers::ProviderError> for AgentError {
    fn from(err: crate::providers::ProviderError) -> Self {
        Self::Provider(err.to_string())
    }
}

impl From<ToolError> for AgentError {
    fn from(err: ToolError) -> Self {
        Self::Tool(err.message)
    }
}

pub type AgentResult<T> = Result<T, AgentError>;
