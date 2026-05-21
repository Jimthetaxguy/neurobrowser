//! Error Type Tests — ToolError builder pattern + AgentError conversions
//!
//! Source: Ralph Plan Loop 51 (C2 — neurobrowser greenfield tests)
//! Tests the error architecture: ToolError (struct), AgentError (thiserror enum),
//! ProviderError → AgentError conversions.

use neurobrowser::tools::errors::{AgentError, AgentResult, ToolError};

// ─── ToolError Builder Pattern ──────────────────────────────────────

#[test]
fn tool_error_new_sets_code_and_message() {
    let err = ToolError::new("TEST_CODE", "test message");
    assert_eq!(err.code, "TEST_CODE");
    assert_eq!(err.message, "test message");
    assert_eq!(err.details, None);
    assert!(!err.retryable);
}

#[test]
fn tool_error_timeout_is_retryable() {
    let err = ToolError::timeout("operation timed out");
    assert_eq!(err.code, "TIMEOUT");
    assert!(err.retryable);
}

#[test]
fn tool_error_not_found_is_not_retryable() {
    let err = ToolError::not_found("resource missing");
    assert_eq!(err.code, "NOT_FOUND");
    assert!(!err.retryable);
}

#[test]
fn tool_error_invalid_input_is_not_retryable() {
    let err = ToolError::invalid_input("bad query");
    assert_eq!(err.code, "INVALID_INPUT");
    assert!(!err.retryable);
}

#[test]
fn tool_error_with_details_adds_json() {
    let err = ToolError::new("ERR", "msg").with_details(serde_json::json!({"field": "value"}));
    assert!(err.details.is_some());
    assert_eq!(err.details.unwrap()["field"], "value");
}

#[test]
fn tool_error_retryable_builder_overrides() {
    let err = ToolError::not_found("gone").retryable(true);
    assert!(err.retryable); // Overridden from default false
}

#[test]
fn tool_error_to_string_format() {
    let err = ToolError::new("E001", "something broke");
    let s: String = err.into();
    assert!(s.contains("E001"));
    assert!(s.contains("something broke"));
    assert!(s.contains("retryable: false"));
}

// ─── AgentError Display (thiserror derive) ──────────────────────────

#[test]
fn agent_error_provider_display() {
    let err = AgentError::Provider("API key invalid".into());
    assert_eq!(format!("{err}"), "Provider error: API key invalid");
}

#[test]
fn agent_error_tool_display() {
    let err = AgentError::Tool("tool failed".into());
    assert_eq!(format!("{err}"), "Tool error: tool failed");
}

#[test]
fn agent_error_validation_display() {
    let err = AgentError::Validation("invalid param".into());
    assert_eq!(format!("{err}"), "Validation error: invalid param");
}

#[test]
fn agent_error_timeout_display() {
    let err = AgentError::Timeout("30s exceeded".into());
    assert_eq!(format!("{err}"), "Timeout: 30s exceeded");
}

#[test]
fn agent_error_max_iterations_display() {
    let err = AgentError::MaxIterationsReached;
    assert_eq!(format!("{err}"), "Max iterations reached");
}

// ─── Error Conversions ──────────────────────────────────────────────

#[test]
fn tool_error_converts_to_agent_error() {
    let tool_err = ToolError::new("PARSE", "JSON parse failed");
    let agent_err: AgentError = tool_err.into();
    match agent_err {
        AgentError::Tool(msg) => assert_eq!(msg, "JSON parse failed"),
        _ => panic!("Expected AgentError::Tool"),
    }
}

#[test]
fn agent_result_ok_unwraps() {
    let result: AgentResult<i32> = Ok(42);
    assert!(matches!(result, Ok(42)));
}

#[test]
fn agent_result_err_propagates() {
    let result: AgentResult<i32> = Err(AgentError::Timeout("slow".into()));
    assert!(result.is_err());
}

// ─── Serialization (ToolError is serde-enabled) ─────────────────────

#[test]
fn tool_error_serializes_to_json() {
    let err = ToolError::new("E001", "test")
        .with_details(serde_json::json!({"key": "val"}))
        .retryable(true);
    let json = serde_json::to_string(&err).unwrap();
    assert!(json.contains("\"code\":\"E001\""));
    assert!(json.contains("\"retryable\":true"));
}

#[test]
fn tool_error_deserializes_from_json() {
    let json = r#"{"code":"TIMEOUT","message":"slow","details":null,"retryable":true}"#;
    let err: ToolError = serde_json::from_str(json).unwrap();
    assert_eq!(err.code, "TIMEOUT");
    assert!(err.retryable);
}
