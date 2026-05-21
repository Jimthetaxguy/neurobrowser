//! Streaming Tests — StreamEvent serialization + AgentStatus enum
//!
//! Source: Ralph Plan Loop 51 (C2 — neurobrowser greenfield tests)
//! Tests the serde-tagged StreamEvent JSON format used for IPC/SSE streaming.

use neurobrowser::agent::streaming::{AgentStatus, StreamEvent};

// ─── AgentStatus Enum ───────────────────────────────────────────────

#[test]
fn agent_status_idle_is_default_representation() {
    let status = AgentStatus::Idle;
    let json = serde_json::to_string(&status).unwrap();
    assert_eq!(json, "\"Idle\"");
}

#[test]
fn agent_status_round_trips_all_variants() {
    let variants = [
        AgentStatus::Idle,
        AgentStatus::Thinking,
        AgentStatus::Searching,
        AgentStatus::ExecutingTool,
        AgentStatus::Writing,
        AgentStatus::WaitingForInput,
    ];
    for status in &variants {
        let json = serde_json::to_string(status).unwrap();
        let parsed: AgentStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(*status, parsed);
    }
}

// ─── StreamEvent Tagged Serialization ───────────────────────────────

#[test]
fn stream_event_token_serializes_with_tag() {
    let event = StreamEvent::Token {
        text: "Hello".into(),
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("\"type\":\"Token\""));
    assert!(json.contains("\"text\":\"Hello\""));
}

#[test]
fn stream_event_tool_call_start_serializes() {
    let event = StreamEvent::ToolCallStart {
        tool: "search".into(),
        arguments: serde_json::json!({"query": "rust"}),
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("\"type\":\"ToolCallStart\""));
    assert!(json.contains("\"tool\":\"search\""));
}

#[test]
fn stream_event_tool_call_result_captures_success() {
    let event = StreamEvent::ToolCallResult {
        tool: "fetch".into(),
        result: "200 OK".into(),
        success: true,
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("\"success\":true"));
}

#[test]
fn stream_event_tool_call_result_captures_failure() {
    let event = StreamEvent::ToolCallResult {
        tool: "fetch".into(),
        result: "timeout".into(),
        success: false,
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("\"success\":false"));
}

#[test]
fn stream_event_error_serializes() {
    let event = StreamEvent::Error {
        code: "PROVIDER_DOWN".into(),
        message: "API unavailable".into(),
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("\"type\":\"Error\""));
    assert!(json.contains("\"code\":\"PROVIDER_DOWN\""));
}

#[test]
fn stream_event_done_captures_iterations() {
    let event = StreamEvent::Done {
        final_response: "Analysis complete.".into(),
        iterations: 3,
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("\"type\":\"Done\""));
    assert!(json.contains("\"iterations\":3"));
}

#[test]
fn stream_event_status_serializes() {
    let event = StreamEvent::Status(AgentStatus::Thinking);
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("\"type\":\"Status\""));
    assert!(json.contains("Thinking"));
}

#[test]
fn stream_event_round_trips_token() {
    let event = StreamEvent::Token {
        text: "round-trip test".into(),
    };
    let json = serde_json::to_string(&event).unwrap();
    let parsed: StreamEvent = serde_json::from_str(&json).unwrap();
    match parsed {
        StreamEvent::Token { text } => assert_eq!(text, "round-trip test"),
        _ => panic!("Expected Token variant"),
    }
}

// ─── Timeout Wrapper ────────────────────────────────────────────────

#[test]
fn stream_event_timeout_error_serializes_correctly() {
    // Verify the timeout error event format matches what execute_with_timeout sends
    let event = StreamEvent::Error {
        code: "TIMEOUT".to_string(),
        message: "Agent execution exceeded 30s deadline".to_string(),
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("\"type\":\"Error\""));
    assert!(json.contains("TIMEOUT"));
    assert!(json.contains("30s deadline"));
}
