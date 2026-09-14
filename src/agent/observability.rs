use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Default)]
pub struct AgentMetrics {
    pub total_requests: AtomicU64,
    pub total_tool_calls: AtomicU64,
    pub total_errors: AtomicU64,
    pub tool_call_counts: std::sync::Mutex<std::collections::HashMap<String, u64>>,
}

impl AgentMetrics {
    pub fn record_request(&self) {
        self.total_requests.fetch_add(1, Ordering::Relaxed);
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
    pub total_tool_calls: u64,
    pub total_errors: u64,
    pub per_tool: std::collections::HashMap<String, u64>,
}
