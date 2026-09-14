//! Tests for AgentMemory + AgentMetrics.

use neurobrowser::agent::metrics;
use neurobrowser::providers::{ProviderConfig, ProviderType};
use neurobrowser::{AgentConfig, ReActAgent};
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
