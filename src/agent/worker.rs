//! Phase E — Worker model (tabs-as-workers).
//!
//! Each `PageHandle` in a session can be promoted to a long-running
//! `ReActAgent` worker with its own `ActionPolicy`, `AgentMemory`, and goal
//! string. The `SessionManager` carries a worker registry alongside the
//! page list, and workers can read from sibling workers via
//! `cross_worker_observations`.
//!
//! This file introduces the **types** — `WorkerSpec`, `WorkerHandle`,
//! `WorkerSummary`, `WorkerSnapshot`, `WorkerMessage` — and provides the
//! glue that `SessionManager` invokes in Phase E1/E2/E4. The fan-out
//! command (E6) lives in the headless daemon and ships with v0.2.

use crate::agent::memory::AgentEvent;
use crate::agent::policy::{ActionPolicy, AgentRunEvent, AgentRunResult};
use crate::agent::ReActAgent;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

/// What a caller wants a worker to do.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerSpec {
    /// Human-readable name (e.g. "PriceComparator", "FormFiller").
    pub name: String,
    /// The goal the worker is trying to satisfy (free-form natural language
    /// prompt — same shape as `ReActAgent::execute`'s user_prompt).
    pub goal: String,
    /// Per-worker action policy. Independent from the session default —
    /// a ReadOnly worker cannot mutate even if the session is HighAutonomy,
    /// and vice-versa.
    pub policy: ActionPolicy,
    /// Maximum agent iterations to spend on a single turn before yielding.
    pub max_iterations: usize,
    /// Optional pin to a specific tab (page_id) on the session. When `None`,
    /// the worker can be reassigned by the session manager to any tab.
    pub pinned_page_id: Option<usize>,
}

impl Default for WorkerSpec {
    fn default() -> Self {
        Self {
            name: "Worker".to_string(),
            goal: String::new(),
            policy: ActionPolicy::default(),
            max_iterations: 5,
            pinned_page_id: None,
        }
    }
}

/// Internal handle to a live worker. Not serialized across processes.
#[derive(Clone)]
pub struct WorkerHandle {
    pub worker_id: String,
    pub session_id: String,
    pub spec: WorkerSpec,
    pub agent: Arc<ReActAgent>,
    pub status: WorkerStatus,
}

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

/// A message sent between workers. The `to` and `from` are worker_ids.
/// `kind` discriminates how the recipient should treat the payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerMessage {
    pub from: String,
    pub to: String,
    pub kind: WorkerMessageKind,
    pub payload: serde_json::Value,
    pub sent_at_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerMessageKind {
    /// A short note ("the price on http://x is $42"). Free-form.
    Observation,
    /// "I'm done with my piece; please continue with your piece."
    Handoff,
    /// Tell the other worker to stop. Sent on cancel.
    Cancellation,
}

impl WorkerMessage {
    pub fn now(kind: WorkerMessageKind, from: &str, to: &str, payload: serde_json::Value) -> Self {
        Self {
            from: from.to_string(),
            to: to.to_string(),
            kind,
            payload,
            sent_at_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
        }
    }
}

impl WorkerHandle {
    pub fn new(session_id: String, spec: WorkerSpec, agent: Arc<ReActAgent>) -> Self {
        Self {
            worker_id: Uuid::new_v4().to_string(),
            session_id,
            spec,
            agent,
            status: WorkerStatus::Idle,
        }
    }

    pub fn summary(&self, last_run: Option<&AgentRunResult>) -> WorkerSummary {
        let last_iteration = last_run.map(|r| r.iterations).unwrap_or(0);
        let last_tool = last_run.and_then(|r| {
            r.events.iter().find_map(|e| match e {
                AgentRunEvent::ToolCallResult { tool, .. } => Some(tool.clone()),
                AgentRunEvent::ToolCallStarted { tool, .. } => Some(tool.clone()),
                _ => None,
            })
        });
        WorkerSummary {
            worker_id: self.worker_id.clone(),
            session_id: self.session_id.clone(),
            name: self.spec.name.clone(),
            status: self.status,
            last_iteration,
            last_tool,
            last_url: self
                .agent
                .snapshot_state()
                .ok()
                .and_then(|s| (!s.state.current_url.is_empty()).then_some(s.state.current_url)),
            last_update_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
        }
    }

    pub fn snapshot(&self, last_run: Option<&AgentRunResult>) -> WorkerSnapshot {
        let summary = self.summary(last_run);
        WorkerSnapshot {
            summary,
            goal: self.spec.goal.clone(),
            policy: self.spec.policy.clone(),
            last_run: last_run.cloned(),
        }
    }
}

/// A slice of shared observations across all workers in a session.
/// Workers write observations after each `execute_tool` (Phase E2 hook).
/// Other workers can `latest_n(n)` this slice to read what their siblings
/// have discovered.
#[derive(Debug, Default)]
pub struct CrossWorkerObservations {
    entries: Vec<AgentEvent>,
}

impl CrossWorkerObservations {
    pub fn push(&mut self, event: AgentEvent) {
        self.entries.push(event);
        // Bound to a sliding window; matches `state.conversation`'s 20.
        const OBSERVATION_WINDOW: usize = 200;
        if self.entries.len() > OBSERVATION_WINDOW {
            let drop = self.entries.len() - OBSERVATION_WINDOW;
            self.entries.drain(0..drop);
        }
    }

    pub fn latest_n(&self, n: usize) -> Vec<&AgentEvent> {
        let start = self.entries.len().saturating_sub(n);
        self.entries[start..].iter().collect()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
