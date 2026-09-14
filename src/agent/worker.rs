use crate::agent::policy::{ActionPolicy, AgentRunResult};
use serde::{Deserialize, Serialize};

/// Lightweight summary used by the React sidebar / headless CLI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerSummary {
    pub worker_id: String,
    pub session_id: String,
    pub name: String,
    pub status: WorkerStatus,
    pub last_iteration: usize,
    pub last_tool: Option<String>,
    pub last_url: Option<String>,
    pub last_update_ms: u64,
}

/// Detailed snapshot of a worker's state, returned by `get_worker`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerSnapshot {
    pub summary: WorkerSummary,
    pub goal: String,
    pub policy: ActionPolicy,
    pub last_run: Option<AgentRunResult>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkerStatus {
    Idle,
    Running,
    AwaitingApproval,
    Blocked,
    Cancelled,
    Completed,
    Failed,
}
