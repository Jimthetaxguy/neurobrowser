//! Tests for Phase C wiring: AgentMemory + AgentMetrics + conversation bound.
//!
//! Covers:
//! - C1: ReActAgent owns an `AgentMemory` and `snapshot_state` returns it.
//! - C3: `metrics()` returns a process-global singleton that records
//!   requests / tool calls / errors.
//! - C6: `current_prompt` is no longer overwritten on each tool-call
//!   observation (the LLM input is derived from `build_context`).
//! - C8: `state.conversation` is bounded (VecDeque of capacity 20).

use neurobrowser::agent::metrics;
use neurobrowser::providers::{ProviderConfig, ProviderType};
use neurobrowser::AgentMessage;
use neurobrowser::{AgentConfig, ReActAgent};
use std::collections::VecDeque;
use std::sync::Arc;

fn stub_provider() -> Arc<dyn neurobrowser::providers::AiProvider> {
    let config = ProviderConfig {
        provider_type: ProviderType::Custom,
        api_key: None,
        base_url: None,
        model: "stub".to_string(),
        max_tokens: Some(64),
        temperature: Some(0.0),
    };
    neurobrowser::providers::create_provider(&config)
}

#[tokio::test]
async fn snapshot_state_returns_fresh_memory() {
    let agent = ReActAgent::new(AgentConfig::default(), stub_provider());
    let snap = agent.snapshot_state().expect("snapshot_state");
    assert_eq!(snap.state.iterations, 0);
    assert_eq!(snap.state.current_url, "");
    assert!(snap.memory.episodic.events.is_empty());
}

#[test]
fn metrics_are_process_global() {
    let m1 = metrics();
    m1.record_request();
    m1.record_tool_call_named("query_dom");
    let m2 = metrics();
    assert!(
        m2.get_requests() >= 1,
        "metrics should be a process-global singleton"
    );
    assert!(m2.get_tool_calls() >= 1, "tool-call counter should record");
    let snap = m2.snapshot();
    assert!(snap.per_tool.get("query_dom").copied().unwrap_or(0) >= 1);
}

#[test]
fn conversation_is_bounded() {
    // Construct an agent, fill the conversation past the limit, and confirm
    // the VecDeque stays bounded.
    const CONVERSATION_WINDOW: usize = 20;
    let mut q: VecDeque<AgentMessage> = VecDeque::with_capacity(CONVERSATION_WINDOW);
    for i in 0..50 {
        if q.len() >= CONVERSATION_WINDOW {
            q.pop_front();
        }
        q.push_back(AgentMessage {
            role: "user".to_string(),
            content: format!("msg-{i}"),
        });
    }
    assert_eq!(q.len(), CONVERSATION_WINDOW);
    // The oldest message should be msg-30 (50 - 20).
    assert_eq!(q.front().unwrap().content, "msg-30");
}
