use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::{info_span, Instrument};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrelationContext {
    pub correlation_id: String,
    pub session_id: Option<String>,
    pub user_id: Option<String>,
    pub tenant_id: Option<String>,
}

impl CorrelationContext {
    pub fn new() -> Self {
        Self {
            correlation_id: Uuid::new_v4().to_string(),
            session_id: None,
            user_id: None,
            tenant_id: None,
        }
    }

    pub fn with_session(mut self, session_id: &str) -> Self {
        self.session_id = Some(session_id.to_string());
        self
    }

    pub fn with_user(mut self, user_id: &str) -> Self {
        self.user_id = Some(user_id.to_string());
        self
    }

    pub fn with_tenant(mut self, tenant_id: &str) -> Self {
        self.tenant_id = Some(tenant_id.to_string());
        self
    }
}

impl Default for CorrelationContext {
    fn default() -> Self {
        Self::new()
    }
}

pub fn llm_call_span(correlation_id: &str, model: &str) -> impl Instrument {
    info_span!(
        "llm.call",
        correlation_id = %correlation_id,
        model = %model,
        span_type = "llm"
    )
}

pub fn tool_call_span(correlation_id: &str, tool_name: &str) -> impl Instrument {
    info_span!(
        "tool.call",
        correlation_id = %correlation_id,
        tool.name = %tool_name,
        span_type = "tool"
    )
}

pub fn agent_iteration_span(correlation_id: &str, iteration: usize) -> impl Instrument {
    info_span!(
        "agent.iteration",
        correlation_id = %correlation_id,
        iteration = iteration,
        span_type = "agent"
    )
}

#[derive(Debug, Default)]
pub struct AgentMetrics {
    pub total_requests: AtomicU64,
    pub total_tokens: AtomicU64,
    pub total_tool_calls: AtomicU64,
    pub total_errors: AtomicU64,
    /// Per-tool-call counter map; keyed by tool name, accumulated into an
    /// atomic via an external `Mutex<HashMap>` would be heavier than needed
    /// here, so we keep a single coarse counter plus a side-table for names.
    pub tool_call_counts: std::sync::Mutex<std::collections::HashMap<String, u64>>,
}

impl AgentMetrics {
    pub fn record_request(&self) {
        self.total_requests.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_tokens(&self, count: u64) {
        self.total_tokens.fetch_add(count, Ordering::Relaxed);
    }

    pub fn record_tool_call(&self) {
        self.total_tool_calls.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_tool_call_named(&self, name: &str) {
        self.record_tool_call();
        if let Ok(mut counts) = self.tool_call_counts.lock() {
            *counts.entry(name.to_string()).or_insert(0) += 1;
        }
    }

    pub fn record_error(&self) {
        self.total_errors.fetch_add(1, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            total_requests: self.get_requests(),
            total_tokens: self.get_tokens(),
            total_tool_calls: self.get_tool_calls(),
            total_errors: self.get_errors(),
            per_tool: self
                .tool_call_counts
                .lock()
                .map(|c| c.clone())
                .unwrap_or_default(),
        }
    }

    pub fn get_requests(&self) -> u64 {
        self.total_requests.load(Ordering::Relaxed)
    }

    pub fn get_tokens(&self) -> u64 {
        self.total_tokens.load(Ordering::Relaxed)
    }

    pub fn get_tool_calls(&self) -> u64 {
        self.total_tool_calls.load(Ordering::Relaxed)
    }

    pub fn get_errors(&self) -> u64 {
        self.total_errors.load(Ordering::Relaxed)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSnapshot {
    pub total_requests: u64,
    pub total_tokens: u64,
    pub total_tool_calls: u64,
    pub total_errors: u64,
    pub per_tool: std::collections::HashMap<String, u64>,
}
