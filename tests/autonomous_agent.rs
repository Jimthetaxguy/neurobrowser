use async_trait::async_trait;
use neurobrowser::providers::ProviderResult;
use neurobrowser::{
    ActionPolicy, AgentConfig, AgentRunEvent, AgentRunStatus, AiContext, AiProvider, AiResponse,
    AutonomyLevel, BrowserInterface, ElementInfo, PageSnapshot, ToolCall,
};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

struct TestBrowser {
    snapshot: PageSnapshot,
}

impl TestBrowser {
    fn new(url: &str, text: &str) -> Self {
        Self {
            snapshot: PageSnapshot {
                url: url.to_string(),
                title: "Test Page".to_string(),
                text: Some(text.to_string()),
                html: Some(format!("<html><body><main>{text}</main></body></html>")),
                viewport_width: 1280,
                viewport_height: 720,
                interactive_ready: true,
                ..PageSnapshot::default()
            },
        }
    }
}

#[async_trait]
impl BrowserInterface for TestBrowser {
    async fn navigate(&self, _url: &str) -> Result<(), String> {
        Ok(())
    }

    async fn query_selector(&self, selector: &str) -> Result<Vec<ElementInfo>, String> {
        Ok(vec![ElementInfo {
            tag: "main".to_string(),
            id: None,
            classes: Vec::new(),
            text: self.snapshot.text.clone().unwrap_or_default(),
            attributes: HashMap::new(),
            selector: selector.to_string(),
        }])
    }

    async fn get_text(&self, _selector: &str) -> Result<String, String> {
        Ok(self.snapshot.text.clone().unwrap_or_default())
    }

    async fn click(&self, _selector: &str) -> Result<(), String> {
        Ok(())
    }

    async fn type_text(&self, _selector: &str, _text: &str) -> Result<(), String> {
        Ok(())
    }

    async fn submit_form(&self, _selector: &str) -> Result<(), String> {
        Ok(())
    }

    async fn scroll_to(&self, _selector: &str) -> Result<(), String> {
        Ok(())
    }

    async fn scroll_by(&self, _x: f32, _y: f32) -> Result<(), String> {
        Ok(())
    }

    async fn snapshot(&self) -> Result<PageSnapshot, String> {
        Ok(self.snapshot.clone())
    }
}

struct FakeProvider {
    responses: Mutex<VecDeque<AiResponse>>,
}

impl FakeProvider {
    fn new(responses: Vec<AiResponse>) -> Self {
        Self {
            responses: Mutex::new(responses.into()),
        }
    }
}

#[async_trait]
impl AiProvider for FakeProvider {
    async fn complete(&self, _prompt: &str, _context: &AiContext) -> ProviderResult<AiResponse> {
        Ok(self
            .responses
            .lock()
            .unwrap()
            .pop_front()
            .expect("fake provider response"))
    }
}

fn response(content: &str, tool_calls: Vec<ToolCall>) -> AiResponse {
    AiResponse {
        content: content.to_string(),
        tool_calls,
    }
}

/// Build an `AiResponse` the way the three real providers do: parse
/// `ToolCall: {json}` from ordinary completion text. The agent dispatches
/// those calls and returns only when `tool_calls` is empty.
fn real_shaped_response(content: &str) -> AiResponse {
    AiResponse {
        content: content.to_string(),
        tool_calls: neurobrowser::providers::parse_tool_calls(content),
    }
}

fn call(name: &str, args: &[(&str, &str)]) -> ToolCall {
    ToolCall {
        name: name.to_string(),
        arguments: args
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect::<HashMap<_, _>>(),
    }
}

/// A browser whose current URL changes when `navigate` is called, so we can
/// verify the agent surfaces the POST-navigation URL to the model.
struct MutBrowser {
    snapshot: Mutex<PageSnapshot>,
    failures_after_navigate: usize,
    remaining_snapshot_failures: Mutex<usize>,
}

impl MutBrowser {
    fn new(url: &str) -> Self {
        Self {
            snapshot: Mutex::new(PageSnapshot {
                url: url.to_string(),
                title: "Test Page".to_string(),
                text: Some("ready".to_string()),
                html: Some("<html><body>ready</body></html>".to_string()),
                viewport_width: 1280,
                viewport_height: 720,
                interactive_ready: true,
                ..PageSnapshot::default()
            }),
            failures_after_navigate: 0,
            remaining_snapshot_failures: Mutex::new(0),
        }
    }

    fn with_snapshot_failures(mut self, count: usize) -> Self {
        self.failures_after_navigate = count;
        self
    }
}

#[async_trait]
impl BrowserInterface for MutBrowser {
    async fn navigate(&self, url: &str) -> Result<(), String> {
        self.snapshot.lock().unwrap().url = url.to_string();
        *self.remaining_snapshot_failures.lock().unwrap() = self.failures_after_navigate;
        Ok(())
    }
    async fn query_selector(&self, _selector: &str) -> Result<Vec<ElementInfo>, String> {
        Ok(Vec::new())
    }
    async fn get_text(&self, _selector: &str) -> Result<String, String> {
        Ok("ready".to_string())
    }
    async fn click(&self, _selector: &str) -> Result<(), String> {
        Ok(())
    }
    async fn type_text(&self, _selector: &str, _text: &str) -> Result<(), String> {
        Ok(())
    }
    async fn submit_form(&self, _selector: &str) -> Result<(), String> {
        Ok(())
    }
    async fn scroll_to(&self, _selector: &str) -> Result<(), String> {
        Ok(())
    }
    async fn scroll_by(&self, _x: f32, _y: f32) -> Result<(), String> {
        Ok(())
    }
    async fn snapshot(&self) -> Result<PageSnapshot, String> {
        let mut failures = self.remaining_snapshot_failures.lock().unwrap();
        if *failures > 0 {
            *failures -= 1;
            return Err("document is unloading".to_string());
        }
        Ok(self.snapshot.lock().unwrap().clone())
    }
}

/// A provider that records the `current_url` it is handed on each model call.
struct RecordingProvider {
    responses: Mutex<VecDeque<AiResponse>>,
    seen_urls: Arc<Mutex<Vec<String>>>,
}

impl RecordingProvider {
    fn new(responses: Vec<AiResponse>, seen_urls: Arc<Mutex<Vec<String>>>) -> Self {
        Self {
            responses: Mutex::new(responses.into()),
            seen_urls,
        }
    }
}

#[async_trait]
impl AiProvider for RecordingProvider {
    async fn complete(&self, _prompt: &str, context: &AiContext) -> ProviderResult<AiResponse> {
        self.seen_urls
            .lock()
            .unwrap()
            .push(context.current_url.clone());
        Ok(self
            .responses
            .lock()
            .unwrap()
            .pop_front()
            .expect("recording provider response"))
    }
}

/// A provider that records every `AiContext` it is handed.
struct ContextRecordingProvider {
    responses: Mutex<VecDeque<AiResponse>>,
    contexts: Arc<Mutex<Vec<AiContext>>>,
}

#[async_trait]
impl AiProvider for ContextRecordingProvider {
    async fn complete(&self, _prompt: &str, context: &AiContext) -> ProviderResult<AiResponse> {
        self.contexts.lock().unwrap().push(context.clone());
        Ok(self
            .responses
            .lock()
            .unwrap()
            .pop_front()
            .expect("context recording provider response"))
    }
}

/// Run two turns of `invalid`, a `navigate` call without its required `url`,
/// and check that the run fails closed and the model is told why.
async fn assert_missing_url_reaches_model(invalid: &str) {
    let contexts = Arc::new(Mutex::new(Vec::new()));
    let provider = Arc::new(ContextRecordingProvider {
        responses: Mutex::new(
            vec![real_shaped_response(invalid), real_shaped_response(invalid)].into(),
        ),
        contexts: contexts.clone(),
    });
    let browser = MutBrowser::new("https://before.example");
    let config = AgentConfig {
        max_iterations: 2,
        ..AgentConfig::default()
    };
    let agent = neurobrowser::ReActAgent::new(config, provider);

    let run = agent
        .execute_with_policy("open the page", &browser, &ActionPolicy::default())
        .await
        .unwrap();

    assert_ne!(run.status, AgentRunStatus::Completed);
    assert_eq!(run.status, AgentRunStatus::Failed);
    assert_eq!(
        run.final_response.as_deref(),
        Some("Max iterations reached")
    );
    assert!(!run
        .events
        .iter()
        .any(|event| matches!(event, AgentRunEvent::RunDone { .. })));

    // Fail closed: the call is reported, never started, and never navigates.
    assert!(!run
        .events
        .iter()
        .any(|event| matches!(event, AgentRunEvent::ToolCallStarted { .. })));
    let failures: Vec<&str> = run
        .events
        .iter()
        .filter_map(|event| match event {
            AgentRunEvent::ToolCallResult {
                tool,
                result,
                success: false,
                ..
            } if tool == "navigate" => Some(result.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        failures,
        vec!["Error: missing required argument(s): url"; 2],
        "{:?}",
        run.events
    );
    assert_eq!(
        browser.snapshot().await.unwrap().url,
        "https://before.example"
    );

    // The model's next turn carries the failure and the reason.
    let contexts = contexts.lock().unwrap();
    assert_eq!(contexts.len(), 2, "the model must be asked again");
    let feedback = &contexts[1];
    assert!(
        feedback
            .tool_results
            .iter()
            .any(|result| result.tool_name == "navigate" && !result.success),
        "{:?}",
        feedback.tool_results
    );
    let prompt = neurobrowser::providers::build_system_prompt(feedback);
    assert!(
        prompt.contains("- navigate: Error: missing required argument(s): url\n"),
        "{prompt}"
    );
}

#[tokio::test]
async fn invalid_tool_call_reports_error_to_model_instead_of_completing() {
    // `navigate` is a real tool, but this call omits its required `url`.
    assert_missing_url_reaches_model(r#"ToolCall: {"name":"navigate","arguments":{}}"#).await;
}

#[tokio::test]
async fn argument_less_legacy_call_reports_error_to_model_instead_of_completing() {
    // Same guarantee for the legacy syntax: `navigate()` has no arguments.
    assert_missing_url_reaches_model("Action: navigate()").await;
}

#[tokio::test]
async fn argument_less_legacy_wait_passes_policy_and_runs() {
    // `wait` has no required arguments, so `wait()` is a complete call.
    assert!(neurobrowser::default_tool_registry()
        .get("wait")
        .expect("wait is registered")
        .definition()
        .arguments
        .iter()
        .all(|argument| !argument.required));
    let browser = TestBrowser::new("https://form.example", "ready");
    let provider = Arc::new(FakeProvider::new(vec![
        real_shaped_response("Action: wait()"),
        real_shaped_response("Final Answer: ready"),
    ]));
    let agent = neurobrowser::ReActAgent::new(AgentConfig::default(), provider);

    let run = agent
        .execute_with_policy("wait for the page", &browser, &ActionPolicy::default())
        .await
        .unwrap();

    // `ToolCallStarted` is emitted only after the policy allows the call.
    assert!(
        run.events.iter().any(
            |event| matches!(event, AgentRunEvent::ToolCallStarted { tool, .. } if tool == "wait")
        ),
        "{:?}",
        run.events
    );
    assert!(
        run.events.iter().any(|event| matches!(
            event,
            AgentRunEvent::ToolCallResult { tool, result, success: true, .. }
                if tool == "wait" && result == "Page is ready"
        )),
        "{:?}",
        run.events
    );
    assert_eq!(run.status, AgentRunStatus::Completed);
    assert_eq!(run.final_response.as_deref(), Some("ready"));
    assert_eq!(run.iterations, 2);
}

#[tokio::test]
async fn post_navigation_url_reaches_model_next_iteration() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let browser = MutBrowser::new("https://before.example");
    let provider = Arc::new(RecordingProvider::new(
        vec![
            response(
                "go",
                vec![call("navigate", &[("url", "https://after.example")])],
            ),
            response("Final Answer: done", vec![]),
        ],
        seen.clone(),
    ));
    let agent = neurobrowser::ReActAgent::new(AgentConfig::default(), provider);
    // HighAutonomy so the cross-domain navigate runs without an approval gate.
    let policy = ActionPolicy {
        autonomy_level: AutonomyLevel::HighAutonomy,
        ..ActionPolicy::default()
    };

    let run = agent
        .execute_with_policy("navigate then finish", &browser, &policy)
        .await
        .unwrap();

    assert_eq!(run.status, AgentRunStatus::Completed);
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 2, "expected two model calls");
    assert_eq!(seen[1], "https://after.example");
}

#[tokio::test]
async fn deterministic_provider_runs_read_tool_loop() {
    let browser = TestBrowser::new("https://invoice.example", "Invoice total is $42.00");
    let provider = Arc::new(FakeProvider::new(vec![
        response(
            "Need text.\nToolCall: {\"name\":\"get_text\",\"arguments\":{\"selector\":\"main\"}}",
            vec![call("get_text", &[("selector", "main")])],
        ),
        response("Final Answer: Invoice total is $42.00", vec![]),
    ]));
    let agent = neurobrowser::ReActAgent::new(AgentConfig::default(), provider);

    let run = agent
        .execute_with_policy("Find the invoice total", &browser, &ActionPolicy::default())
        .await
        .unwrap();

    assert_eq!(run.status, AgentRunStatus::Completed);
    assert_eq!(
        run.final_response.as_deref(),
        Some("Invoice total is $42.00")
    );
    assert!(run
        .events
        .iter()
        .any(|event| matches!(event, AgentRunEvent::ToolCallResult { tool, success: true, .. } if tool == "get_text")));
}

#[tokio::test]
async fn real_shaped_toolcall_json_dispatches_despite_provider_stop() {
    // Tool calls live in ordinary completion text as `ToolCall: {json}`.
    // The agent dispatches parsed `tool_calls` and returns only when that
    // list is empty.
    let tool_turn =
        "Need text.\nToolCall: {\"name\":\"get_text\",\"arguments\":{\"selector\":\"main\"}}";
    let parsed = neurobrowser::providers::parse_tool_calls(tool_turn);
    assert_eq!(parsed.len(), 1, "fixture must parse like a real provider");
    assert_eq!(parsed[0].name, "get_text");

    let browser = TestBrowser::new("https://invoice.example", "Invoice total is $42.00");
    let provider = Arc::new(FakeProvider::new(vec![
        real_shaped_response(tool_turn),
        real_shaped_response("Final Answer: Invoice total is $42.00"),
    ]));
    let agent = neurobrowser::ReActAgent::new(AgentConfig::default(), provider);

    let run = agent
        .execute_with_policy("Find the invoice total", &browser, &ActionPolicy::default())
        .await
        .unwrap();

    assert_eq!(run.status, AgentRunStatus::Completed);
    assert_eq!(
        run.final_response.as_deref(),
        Some("Invoice total is $42.00")
    );
    assert!(
        run.events.iter().any(
            |event| matches!(event, AgentRunEvent::ToolCallResult { tool, success: true, .. } if tool == "get_text")
        ),
        "parsed ToolCall JSON must dispatch"
    );
}

#[tokio::test]
async fn approval_required_run_stops_before_click() {
    let browser = TestBrowser::new("https://form.example", "Submit");
    let provider = Arc::new(FakeProvider::new(vec![response(
        "ToolCall: {\"name\":\"click\",\"arguments\":{\"selector\":\"#submit\"}}",
        vec![call("click", &[("selector", "#submit")])],
    )]));
    let agent = neurobrowser::ReActAgent::new(AgentConfig::default(), provider);

    let run = agent
        .execute_with_policy("Click submit", &browser, &ActionPolicy::default())
        .await
        .unwrap();

    assert_eq!(run.status, AgentRunStatus::AwaitingApproval);
    assert!(run.pending_tool_call.is_some());
    assert!(run.events.iter().any(
        |event| matches!(event, AgentRunEvent::ApprovalRequested { tool, .. } if tool == "click")
    ));
}

#[test]
fn parses_structured_tool_calls_without_provider_specific_logic() {
    let calls = neurobrowser::providers::parse_tool_calls(
        "Thought: browse\nToolCall: {\"name\":\"navigate\",\"arguments\":{\"url\":\"https://example.com\",\"count\":2}}",
    );

    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].name, "navigate");
    assert_eq!(
        calls[0].arguments.get("url").map(String::as_str),
        Some("https://example.com")
    );
    assert_eq!(
        calls[0].arguments.get("count").map(String::as_str),
        Some("2")
    );
}

#[test]
fn parses_legacy_action_syntax_for_compatibility() {
    let calls = neurobrowser::providers::parse_tool_calls("Action: click(selector=\"#go\")");

    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].name, "click");
    assert_eq!(
        calls[0].arguments.get("selector").map(String::as_str),
        Some("#go")
    );
}

#[tokio::test]
async fn zero_arg_action_wait_reaches_the_tool_path() {
    let tool_turn = "Action: wait()";
    let parsed = neurobrowser::providers::parse_tool_calls(tool_turn);
    assert_eq!(parsed.len(), 1, "Action: wait() must not be dropped");
    assert_eq!(parsed[0].name, "wait");
    assert!(parsed[0].arguments.is_empty());

    let browser = TestBrowser::new("https://invoice.example", "ready");
    let provider = Arc::new(FakeProvider::new(vec![
        real_shaped_response(tool_turn),
        real_shaped_response("Final Answer: page is ready"),
    ]));
    let agent = neurobrowser::ReActAgent::new(AgentConfig::default(), provider);

    let run = agent
        .execute_with_policy("Wait for the page", &browser, &ActionPolicy::default())
        .await
        .unwrap();

    assert_eq!(run.status, AgentRunStatus::Completed);
    assert_eq!(run.final_response.as_deref(), Some("page is ready"));
    assert!(
        run.events.iter().any(
            |event| matches!(event, AgentRunEvent::ToolCallResult { tool, success: true, result, .. } if tool == "wait" && result == "Page is ready")
        ),
        "parsed Action: wait() must dispatch"
    );
}

#[test]
fn parses_argument_less_legacy_calls_to_known_tools() {
    // Kept so the agent can run `wait()` and report `navigate()`.
    let calls = neurobrowser::providers::parse_tool_calls("Action: navigate()");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].name, "navigate");
    assert!(calls[0].arguments.is_empty());

    let calls = neurobrowser::providers::parse_tool_calls("Action: wait()");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].name, "wait");
    assert!(calls[0].arguments.is_empty());

    // Memory tools are known names too; the agent decides if they are attached.
    let calls = neurobrowser::providers::parse_tool_calls("Action: inspect_active_page()");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].name, "inspect_active_page");

    // An unknown name still needs arguments to count as a call.
    assert!(neurobrowser::providers::parse_tool_calls("Action: frobnicate()").is_empty());
}

#[test]
fn drops_truncated_legacy_calls() {
    // A legacy call must end with `)`. Without it the model output was cut
    // off, so nothing runs, even for a tool that needs no arguments.
    for truncated in [
        "Action: back(",
        "Action: reload(",
        "Action: navigate(https://ex",
        "Action: type(#q, hel",
    ] {
        assert!(
            neurobrowser::providers::parse_tool_calls(truncated).is_empty(),
            "{truncated}"
        );
    }

    let calls = neurobrowser::providers::parse_tool_calls("Action: back()");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].name, "back");
    assert!(calls[0].arguments.is_empty());
}

#[test]
fn parses_legacy_action_syntax_with_multiple_positional_args() {
    // `type(selector, text)` is a two-arg legacy positional call. Both
    // args must survive as distinct values (mapped to the `type` tool's
    // real `selector`/`text` parameter names) rather than the second
    // positional arg overwriting the first under a shared "value" key.
    let calls = neurobrowser::providers::parse_tool_calls("Action: type(#input, hello)");

    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].name, "type");
    assert_eq!(calls[0].arguments.len(), 2);
    assert_eq!(
        calls[0].arguments.get("selector").map(String::as_str),
        Some("#input")
    );
    assert_eq!(
        calls[0].arguments.get("text").map(String::as_str),
        Some("hello")
    );
}

#[tokio::test]
async fn transient_post_tool_snapshot_failure_retries_the_read_and_updates_model_context() {
    // Navigate reads the landed URL once, so one failure is left for the agent's retry.
    let browser = MutBrowser::new("https://example.com/start").with_snapshot_failures(2);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let provider = Arc::new(RecordingProvider::new(
        vec![
            response(
                "navigate",
                vec![call("navigate", &[("url", "https://example.com/final")])],
            ),
            response("done", vec![]),
        ],
        seen.clone(),
    ));
    let agent = neurobrowser::ReActAgent::new(AgentConfig::default(), provider);
    let run = agent
        .execute_with_policy("navigate", &browser, &ActionPolicy::default())
        .await
        .expect("run");
    assert_eq!(run.status, AgentRunStatus::Completed);
    assert_eq!(
        *seen.lock().unwrap(),
        ["https://example.com/start", "https://example.com/final"]
    );
    assert_eq!(
        run.events
            .iter()
            .filter(|event| matches!(event,
        AgentRunEvent::ToolCallResult { tool, success: true, .. } if tool == "navigate"))
            .count(),
        1
    );
}

#[tokio::test]
async fn persistent_post_tool_snapshot_failure_keeps_success_and_previous_context() {
    // Navigate reads once, then the agent tries the refresh twice.
    let browser = MutBrowser::new("https://example.com/start").with_snapshot_failures(3);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let provider = Arc::new(RecordingProvider::new(
        vec![
            response(
                "navigate",
                vec![call("navigate", &[("url", "https://example.com/final")])],
            ),
            response("done", vec![]),
        ],
        seen.clone(),
    ));
    let agent = neurobrowser::ReActAgent::new(AgentConfig::default(), provider);
    let run = agent
        .execute_with_policy("navigate", &browser, &ActionPolicy::default())
        .await
        .expect("run");
    assert_eq!(run.status, AgentRunStatus::Completed);
    assert_eq!(
        *seen.lock().unwrap(),
        ["https://example.com/start", "https://example.com/start"]
    );
    assert_eq!(
        run.events
            .iter()
            .filter(|event| matches!(event,
        AgentRunEvent::ToolCallResult { tool, success: true, .. } if tool == "navigate"))
            .count(),
        1
    );
}

/// Counts readiness waits. The first wait can fail while a later one would
/// succeed, which is the contradiction a second dispatch wait used to create.
struct ReadinessBrowser {
    waits: AtomicUsize,
    fail_first_wait: bool,
    url: Mutex<String>,
}

impl ReadinessBrowser {
    fn new(fail_first_wait: bool) -> Self {
        Self {
            waits: AtomicUsize::new(0),
            fail_first_wait,
            url: Mutex::new("https://example.com/start".to_string()),
        }
    }

    fn waits(&self) -> usize {
        self.waits.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl BrowserInterface for ReadinessBrowser {
    async fn navigate(&self, url: &str) -> Result<(), String> {
        // Desktop navigate waits before it returns. That is the one wait.
        self.wait_for_navigation().await?;
        *self.url.lock().unwrap() = url.to_string();
        Ok(())
    }
    async fn query_selector(&self, selector: &str) -> Result<Vec<ElementInfo>, String> {
        Ok(vec![ElementInfo {
            tag: "main".into(),
            id: None,
            classes: Vec::new(),
            text: String::new(),
            attributes: HashMap::new(),
            selector: selector.to_string(),
        }])
    }
    async fn get_text(&self, _selector: &str) -> Result<String, String> {
        Ok("ready".into())
    }
    async fn click(&self, _selector: &str) -> Result<(), String> {
        Ok(())
    }
    async fn type_text(&self, _selector: &str, _text: &str) -> Result<(), String> {
        Ok(())
    }
    async fn submit_form(&self, _selector: &str) -> Result<(), String> {
        Ok(())
    }
    async fn scroll_to(&self, _selector: &str) -> Result<(), String> {
        Ok(())
    }
    async fn scroll_by(&self, _x: f32, _y: f32) -> Result<(), String> {
        Ok(())
    }
    async fn keypress(&self, _key: &str) -> Result<(), String> {
        Ok(())
    }
    async fn browser_reload(&self) -> Result<(), String> {
        Ok(())
    }
    async fn browser_back(&self) -> Result<(), String> {
        Ok(())
    }
    async fn snapshot(&self) -> Result<PageSnapshot, String> {
        Ok(PageSnapshot {
            url: self.url.lock().unwrap().clone(),
            title: "Test Page".into(),
            text: Some("ready".into()),
            html: Some("<html><body>ready</body></html>".into()),
            viewport_width: 1280,
            viewport_height: 720,
            interactive_ready: true,
            ..PageSnapshot::default()
        })
    }
    async fn wait_for_navigation(&self) -> Result<(), String> {
        let index = self.waits.fetch_add(1, Ordering::SeqCst);
        if self.fail_first_wait && index == 0 {
            Err("Timed out waiting for page to finish loading".into())
        } else {
            Ok(())
        }
    }
}

fn high_autonomy() -> ActionPolicy {
    ActionPolicy {
        autonomy_level: AutonomyLevel::HighAutonomy,
        ..ActionPolicy::default()
    }
}

fn tool_event(run: &neurobrowser::AgentRunResult) -> (bool, String) {
    run.events
        .iter()
        .find_map(|event| match event {
            AgentRunEvent::ToolCallResult {
                result, success, ..
            } => Some((*success, result.clone())),
            _ => None,
        })
        .expect("tool result event")
}

/// Run one mutation, then a final answer if dispatch lets the loop continue.
async fn run_mutation(
    browser: &ReadinessBrowser,
    tool_call: ToolCall,
) -> (neurobrowser::AgentRunResult, Vec<AiContext>) {
    let contexts = Arc::new(Mutex::new(Vec::new()));
    let provider = Arc::new(ContextRecordingProvider {
        responses: Mutex::new(VecDeque::from([
            response("act", vec![tool_call]),
            response("Final Answer: done", vec![]),
        ])),
        contexts: contexts.clone(),
    });
    let agent = neurobrowser::ReActAgent::new(AgentConfig::default(), provider);
    let run = agent
        .execute_with_policy("act", browser, &high_autonomy())
        .await
        .expect("run");
    let seen = contexts.lock().unwrap().clone();
    (run, seen)
}

#[tokio::test]
async fn mutation_that_already_waited_is_not_waited_on_twice() {
    for (name, args) in [
        ("click", vec![("selector", "#go")]),
        ("keypress", vec![("key", "Enter")]),
        ("reload", vec![]),
        ("navigate", vec![("url", "https://example.com/next")]),
    ] {
        let browser = ReadinessBrowser::new(false);
        let (run, contexts) = run_mutation(&browser, call(name, &args)).await;
        assert_eq!(browser.waits(), 1, "{name} waited more than once");
        assert_eq!(run.status, AgentRunStatus::Completed, "{name}");
        assert_eq!(contexts.len(), 2, "{name} should reach the next model turn");
        let seen = &contexts[1].tool_results[0].result;
        assert!(
            !seen.contains("unconfirmed"),
            "{name} told the model the page was not ready: {seen}"
        );
        assert!(
            !run.final_response
                .as_deref()
                .unwrap_or("")
                .contains("unconfirmed"),
            "{name}"
        );
    }

    // The tool wait times out. A second wait would succeed and the run would
    // continue, telling the model the page was not ready after it was.
    for (name, args) in [
        ("click", vec![("selector", "#go")]),
        ("keypress", vec![("key", "Enter")]),
        ("reload", vec![]),
    ] {
        let browser = ReadinessBrowser::new(true);
        let (run, contexts) = run_mutation(&browser, call(name, &args)).await;
        assert_eq!(
            browser.waits(),
            1,
            "{name} waited again after its own timeout"
        );
        assert_eq!(run.status, AgentRunStatus::Failed, "{name}");
        assert_eq!(
            contexts.len(),
            1,
            "{name} continued after an unconfirmed page"
        );
        let (success, reported) = tool_event(&run);
        assert!(success, "{name} dispatch truth");
        assert!(
            reported.contains("page readiness is unconfirmed"),
            "{reported}"
        );
        assert!(
            reported.contains("Timed out waiting for page to finish loading"),
            "{reported}"
        );
        assert_eq!(run.final_response.as_deref(), Some(reported.as_str()));
    }

    // navigate waits inside the runtime call. A timeout is an action error,
    // and dispatch must not wait a second time.
    let browser = ReadinessBrowser::new(true);
    let (run, contexts) = run_mutation(
        &browser,
        call("navigate", &[("url", "https://example.com/next")]),
    )
    .await;
    assert_eq!(browser.waits(), 1);
    assert_eq!(run.status, AgentRunStatus::Failed);
    assert_eq!(contexts.len(), 1);
    let (success, _) = tool_event(&run);
    assert!(!success);
    assert_eq!(
        run.final_response.as_deref(),
        Some("Legacy action outcome unresolved; inspect the page before any further action")
    );

    // submit_form waits in the tool, then stops for approval before dispatch.
    let browser = ReadinessBrowser::new(true);
    let tool_call = call("submit_form", &[("selector", "form")]);
    let agent =
        neurobrowser::ReActAgent::new(AgentConfig::default(), Arc::new(FakeProvider::new(vec![])));
    let pending = agent
        .propose_tool_with_policy(tool_call.clone(), &browser, &high_autonomy())
        .await
        .expect("proposal");
    assert_eq!(pending.status, AgentRunStatus::AwaitingApproval);
    assert_eq!(browser.waits(), 0);
    let run = agent
        .execute_approved_tool_with_policy(
            pending.run_id,
            pending.approval_id.expect("approval id"),
            tool_call,
            &browser,
            true,
            None,
            &high_autonomy(),
        )
        .await
        .expect("approved submit");
    assert_eq!(browser.waits(), 1);
    assert_eq!(run.status, AgentRunStatus::Failed);
    let (success, reported) = tool_event(&run);
    assert!(success);
    assert!(reported.contains("page readiness is unconfirmed"));
    assert!(reported.contains("Timed out waiting for page to finish loading"));
    assert_eq!(run.final_response.as_deref(), Some(reported.as_str()));
}

#[tokio::test]
async fn mutation_that_did_not_wait_still_gets_the_dispatch_wait() {
    for (name, args) in [("scroll_to", vec![("selector", "#main")]), ("back", vec![])] {
        let browser = ReadinessBrowser::new(false);
        let (run, contexts) = run_mutation(&browser, call(name, &args)).await;
        assert_eq!(browser.waits(), 1, "{name} skipped the dispatch wait");
        assert_eq!(run.status, AgentRunStatus::Completed, "{name}");
        assert_eq!(contexts.len(), 2, "{name}");
        assert!(
            !contexts[1].tool_results[0].result.contains("unconfirmed"),
            "{name}"
        );

        let browser = ReadinessBrowser::new(true);
        let (run, contexts) = run_mutation(&browser, call(name, &args)).await;
        assert_eq!(browser.waits(), 1, "{name}");
        assert_eq!(run.status, AgentRunStatus::Failed, "{name}");
        assert_eq!(
            contexts.len(),
            1,
            "{name} continued without a readiness wait"
        );
        let (success, reported) = tool_event(&run);
        assert!(success, "{name}");
        assert!(
            !reported.contains("unconfirmed"),
            "{name} tool waited itself: {reported}"
        );
        assert_eq!(
            run.final_response.as_deref(),
            Some(
                "Legacy action acknowledged; page readiness unconfirmed. Inspect before any further action"
            )
        );
    }
}
