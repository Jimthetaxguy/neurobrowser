use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AgentEvent {
    LlmCall {
        run_id: String,
        model: String,
        iteration: usize,
        content_preview: String,
        timestamp: u64,
    },
    ToolCall {
        run_id: String,
        tool: String,
        arguments: std::collections::HashMap<String, String>,
        success: bool,
        result_preview: String,
        timestamp: u64,
    },
}

impl AgentEvent {
    pub fn now(kind: &str) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or_else(|_| {
                tracing::warn!("clock before unix epoch; emitting 0 for {kind} timestamp");
                0
            })
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct EpisodicMemory {
    pub events: Vec<AgentEvent>,
}

impl EpisodicMemory {
    pub fn push(&mut self, event: AgentEvent) {
        self.events.push(event);
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct AgentMemory {
    pub episodic: EpisodicMemory,
}
