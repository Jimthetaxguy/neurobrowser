use async_trait::async_trait;
use neurobrowser::providers::ProviderResult;
use neurobrowser::{
    ActionPolicy, AgentConfig, AgentRunEvent, AgentRunStatus, AiContext, AiProvider, AiResponse,
    AutonomyLevel, BrowserInterface, ElementInfo, PageSnapshot, ToolCall,
};
use std::collections::{HashMap, VecDeque};
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

    async fn get_attributes(&self, _selector: &str) -> Result<HashMap<String, String>, String> {
        Ok(HashMap::new())
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

    fn provider_name(&self) -> &str {
        "fake"
    }

    fn is_configured(&self) -> bool {
        true
    }
}

fn response(content: &str, tool_calls: Vec<ToolCall>, finish_reason: &str) -> AiResponse {
    AiResponse {
        content: content.to_string(),
        reasoning: None,
        tool_calls,
        finish_reason: finish_reason.to_string(),
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
        }
    }
}

#[async_trait]
impl BrowserInterface for MutBrowser {
    async fn navigate(&self, url: &str) -> Result<(), String> {
        self.snapshot.lock().unwrap().url = url.to_string();
        Ok(())
    }
    async fn query_selector(&self, _selector: &str) -> Result<Vec<ElementInfo>, String> {
        Ok(Vec::new())
    }
    async fn get_text(&self, _selector: &str) -> Result<String, String> {
        Ok("ready".to_string())
    }
    async fn get_attributes(&self, _selector: &str) -> Result<HashMap<String, String>, String> {
        Ok(HashMap::new())
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
    fn provider_name(&self) -> &str {
        "recording"
    }
    fn is_configured(&self) -> bool {
        true
    }
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
                "tool_calls",
            ),
            response("Final Answer: done", vec![], "stop"),
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
    // The second iteration's context must carry the POST-navigation URL; before
    // the fix it carried the stale pre-navigation URL.
    assert_eq!(seen[1], "https://after.example");
}

#[tokio::test]
async fn deterministic_provider_runs_read_tool_loop() {
    let browser = TestBrowser::new("https://invoice.example", "Invoice total is $42.00");
    let provider = Arc::new(FakeProvider::new(vec![
        response(
            "Need text.\nToolCall: {\"name\":\"get_text\",\"arguments\":{\"selector\":\"main\"}}",
            vec![call("get_text", &[("selector", "main")])],
            "tool_calls",
        ),
        response("Final Answer: Invoice total is $42.00", vec![], "stop"),
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
async fn approval_required_run_stops_before_click() {
    let browser = TestBrowser::new("https://form.example", "Submit");
    let provider = Arc::new(FakeProvider::new(vec![response(
        "ToolCall: {\"name\":\"click\",\"arguments\":{\"selector\":\"#submit\"}}",
        vec![call("click", &[("selector", "#submit")])],
        "tool_calls",
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
