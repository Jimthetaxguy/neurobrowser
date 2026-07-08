use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AgentEvent {
    UserMessage {
        content: String,
        timestamp: u64,
    },
    AssistantMessage {
        content: String,
        timestamp: u64,
    },
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
    ToolResult {
        tool: String,
        result: String,
        success: bool,
        timestamp: u64,
    },
    Decision {
        reasoning: String,
        timestamp: u64,
    },
    Navigation {
        url: String,
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

    pub fn iter(&self) -> impl Iterator<Item = &AgentEvent> {
        self.events.iter()
    }

    pub fn get_tool_history(&self) -> Vec<&AgentEvent> {
        self.events
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    AgentEvent::ToolCall { .. } | AgentEvent::ToolResult { .. }
                )
            })
            .collect()
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct SemanticMemory {
    pub items: Vec<SemanticItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticItem {
    pub id: String,
    pub content: String,
    pub embedding: Option<Vec<f32>>,
    pub source_event_id: Option<String>,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateMemory {
    pub current_url: Option<String>,
    pub page_title: Option<String>,
    pub scroll_position: (f32, f32),
    pub last_updated: u64,
}

impl Default for StateMemory {
    fn default() -> Self {
        Self {
            current_url: None,
            page_title: None,
            scroll_position: (0.0, 0.0),
            last_updated: 0,
        }
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct AgentMemory {
    pub episodic: EpisodicMemory,
    pub semantic: SemanticMemory,
    pub state: StateMemory,
}
