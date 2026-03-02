use serde::{Deserialize, Serialize};

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

#[derive(Debug)]
pub enum AgentError {
    Provider(String),
    Tool(String),
    Validation(String),
    Timeout(String),
    MaxIterationsReached,
}

impl std::fmt::Display for AgentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentError::Provider(msg) => write!(f, "Provider error: {}", msg),
            AgentError::Tool(msg) => write!(f, "Tool error: {}", msg),
            AgentError::Validation(msg) => write!(f, "Validation error: {}", msg),
            AgentError::Timeout(msg) => write!(f, "Timeout: {}", msg),
            AgentError::MaxIterationsReached => write!(f, "Max iterations reached"),
        }
    }
}

impl std::error::Error for AgentError {}

impl From<crate::providers::ProviderError> for AgentError {
    fn from(err: crate::providers::ProviderError) -> Self {
        AgentError::Provider(err.to_string())
    }
}

impl From<ToolError> for AgentError {
    fn from(err: ToolError) -> Self {
        AgentError::Tool(err.message)
    }
}

pub type AgentResult<T> = Result<T, AgentError>;
