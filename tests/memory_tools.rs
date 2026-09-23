//! Personal-memory tools over a real `MemoryService` index.
//!
//! `MemoryService` is durable page memory. These tools do not read an in-run
//! agent log. The shipped crate has no `agent::memory` module.

use async_trait::async_trait;
use neuro_memory::{CapturePolicy, CapturedPage, MemoryService};
use neurobrowser::providers::{build_system_prompt, parse_tool_calls, ProviderResult};
use neurobrowser::{
    default_tool_registry, default_tool_registry_with_memory, ActionPolicy, AgentConfig,
    AgentRunEvent, AgentRunStatus, AiContext, AiProvider, AiResponse, BrowserInterface,
    ElementInfo, PageSnapshot, ReActAgent, ToolCall,
};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use url::Url;

const PAGE_URL: &str = "https://example.com/docs";
const MEMORY_TOKEN: &str = "zephyrquartz";
const BROWSER_TOKEN: &str = "unrelated-browser-token";

fn page() -> CapturedPage {
    CapturedPage {
        url: Url::parse(PAGE_URL).expect("url"),
        title: "Notes".to_string(),
        html: format!("<h1>Notes</h1><p>{MEMORY_TOKEN} lives in the body.</p>"),
        text: format!("{MEMORY_TOKEN} lives in the body."),
        content_hash: "pending".to_string(),
        captured_at: 1_700_000_000_000,
    }
}

async fn open_with_page() -> (tempfile::TempDir, Arc<MemoryService>) {
    let dir = tempfile::tempdir().expect("temp dir");
    let service = Arc::new(MemoryService::open(dir.path()).expect("open"));
    service
        .capture(page(), &CapturePolicy::default())
        .await
        .expect("capture");
    (dir, service)
}

struct PageBrowser {
    snapshot: PageSnapshot,
    calls: Mutex<u32>,
}

impl PageBrowser {
    fn new(url: &str, text: &str) -> Self {
        Self {
            snapshot: PageSnapshot {
                url: url.to_string(),
                title: "Browser page".to_string(),
                text: Some(text.to_string()),
                html: Some(format!("<html><body>{text}</body></html>")),
                viewport_width: 800,
                viewport_height: 600,
                interactive_ready: true,
                ..PageSnapshot::default()
            },
            calls: Mutex::new(0),
        }
    }

    fn calls(&self) -> u32 {
        *self.calls.lock().expect("call count")
    }
}

#[async_trait]
impl BrowserInterface for PageBrowser {
    async fn navigate(&self, _url: &str) -> Result<(), String> {
        Ok(())
    }

    async fn query_selector(&self, _selector: &str) -> Result<Vec<ElementInfo>, String> {
        Ok(Vec::new())
    }

    async fn get_text(&self, _selector: &str) -> Result<String, String> {
        Ok(String::new())
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
        *self.calls.lock().expect("call count") += 1;
        Ok(self.snapshot.clone())
    }
}

struct ScriptedProvider {
    responses: Mutex<VecDeque<AiResponse>>,
    personal_memory: Mutex<Option<bool>>,
}

impl ScriptedProvider {
    fn new(responses: Vec<AiResponse>) -> Self {
        Self {
            responses: Mutex::new(responses.into()),
            personal_memory: Mutex::new(None),
        }
    }
}

#[async_trait]
impl AiProvider for ScriptedProvider {
    async fn complete(&self, _prompt: &str, context: &AiContext) -> ProviderResult<AiResponse> {
        *self.personal_memory.lock().expect("flag") = Some(context.personal_memory);
        Ok(self
            .responses
            .lock()
            .expect("responses")
            .pop_front()
            .expect("scripted response"))
    }

    fn provider_name(&self) -> &str {
        "scripted"
    }
}

fn tool_response(name: &str, args: &[(&str, &str)]) -> AiResponse {
    AiResponse {
        content: format!("ToolCall: {name}"),
        tool_calls: vec![ToolCall {
            name: name.to_string(),
            arguments: args
                .iter()
                .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
                .collect(),
        }],
    }
}

fn final_response(text: &str) -> AiResponse {
    AiResponse {
        content: format!("Final Answer: {text}"),
        tool_calls: Vec::new(),
    }
}

fn tool_result<'a>(events: &'a [AgentRunEvent], name: &str) -> &'a AgentRunEvent {
    events
        .iter()
        .find(|event| matches!(event, AgentRunEvent::ToolCallResult { tool, .. } if tool == name))
        .unwrap_or_else(|| panic!("missing tool result for {name}"))
}

#[tokio::test]
async fn registry_grows_from_17_browser_tools_to_19_with_memory() {
    let (_dir, service) = open_with_page().await;
    assert_eq!(default_tool_registry().len(), 17);

    let registry =
        default_tool_registry_with_memory(Arc::clone(&service), CapturePolicy::default());
    assert_eq!(registry.len(), 19);
    assert!(registry.get("navigate").is_some());
    assert!(registry.get("search_personal_memory").is_some());
    assert!(registry.get("inspect_active_page").is_some());
}

#[tokio::test]
async fn search_reads_memory_and_does_not_call_the_browser() {
    let (_dir, service) = open_with_page().await;
    let registry =
        default_tool_registry_with_memory(Arc::clone(&service), CapturePolicy::default());
    let tool = registry.get("search_personal_memory").expect("search tool");
    let browser = PageBrowser::new("https://other.test/blank", BROWSER_TOKEN);
    let mut args = HashMap::new();
    args.insert("query".to_string(), MEMORY_TOKEN.to_string());

    let result = tool.execute(args, &browser).await;

    assert!(result.success, "{}", result.result);
    assert!(result.result.contains(MEMORY_TOKEN), "{}", result.result);
    assert!(result.result.contains(PAGE_URL), "{}", result.result);
    assert!(
        !result.result.contains(BROWSER_TOKEN),
        "search included live browser text: {}",
        result.result
    );
    assert_eq!(browser.calls(), 0, "search called the browser");
}

#[tokio::test]
async fn search_requires_a_query_and_honors_limit() {
    let (_dir, service) = open_with_page().await;
    let registry =
        default_tool_registry_with_memory(Arc::clone(&service), CapturePolicy::default());
    let tool = registry.get("search_personal_memory").expect("search tool");
    let browser = PageBrowser::new(PAGE_URL, "unused");

    let missing = tool.execute(HashMap::new(), &browser).await;
    assert!(!missing.success);
    assert!(
        missing.result.contains("non-empty query"),
        "{}",
        missing.result
    );

    let mut bad_limit = HashMap::new();
    bad_limit.insert("query".to_string(), MEMORY_TOKEN.to_string());
    bad_limit.insert("limit".to_string(), "many".to_string());
    let bad = tool.execute(bad_limit, &browser).await;
    assert!(!bad.success);
    assert!(bad.result.contains("limit"), "{}", bad.result);

    let mut zero = HashMap::new();
    zero.insert("query".to_string(), MEMORY_TOKEN.to_string());
    zero.insert("limit".to_string(), "0".to_string());
    let none = tool.execute(zero, &browser).await;
    assert!(none.success, "{}", none.result);
    assert!(none.result.contains("No personal memory matches"));
    assert_eq!(browser.calls(), 0);
}

#[tokio::test]
async fn inspect_returns_captured_content_for_the_current_url() {
    let (_dir, service) = open_with_page().await;
    let registry =
        default_tool_registry_with_memory(Arc::clone(&service), CapturePolicy::default());
    let tool = registry.get("inspect_active_page").expect("inspect tool");
    let browser = PageBrowser::new(PAGE_URL, BROWSER_TOKEN);
    let mut args = HashMap::new();
    args.insert("url".to_string(), "https://evil.test/ignore-me".to_string());

    let result = tool.execute(args, &browser).await;

    assert!(result.success, "{}", result.result);
    assert!(result.result.contains(PAGE_URL), "{}", result.result);
    assert!(result.result.contains(MEMORY_TOKEN), "{}", result.result);
    assert!(result.result.contains("Notes"), "{}", result.result);
    assert!(
        !result.result.contains("evil.test"),
        "inspect followed an argument URL: {}",
        result.result
    );
    assert_eq!(browser.calls(), 1);
}

#[tokio::test]
async fn inspect_reports_policy_denial_and_a_missing_capture() {
    let (_dir, service) = open_with_page().await;
    let denied = CapturePolicy {
        enabled: true,
        allowed_domains: Vec::new(),
        denied_domains: vec!["example.com".to_string()],
    };
    let registry = default_tool_registry_with_memory(Arc::clone(&service), denied);
    let tool = registry.get("inspect_active_page").expect("inspect tool");
    let blocked = tool
        .execute(HashMap::new(), &PageBrowser::new(PAGE_URL, BROWSER_TOKEN))
        .await;
    assert!(!blocked.success);
    assert!(
        blocked.result.contains("capture denied"),
        "{}",
        blocked.result
    );
    assert!(blocked.result.contains("example.com"), "{}", blocked.result);
    assert!(
        !blocked.result.contains(MEMORY_TOKEN),
        "denied inspect returned captured text: {}",
        blocked.result
    );

    let open = default_tool_registry_with_memory(service, CapturePolicy::default());
    let tool = open.get("inspect_active_page").expect("inspect tool");
    let missing = tool
        .execute(
            HashMap::new(),
            &PageBrowser::new("https://missing.test/page", "empty"),
        )
        .await;
    assert!(!missing.success);
    assert!(
        missing
            .result
            .contains("No captured content for https://missing.test/page"),
        "{}",
        missing.result
    );

    let blank = tool
        .execute(HashMap::new(), &PageBrowser::new("about:blank", "blank"))
        .await;
    assert!(!blank.success);
    assert!(blank.result.contains("capture denied"), "{}", blank.result);
}

#[tokio::test]
async fn with_memory_dispatches_search_and_new_does_not() {
    let (_dir, service) = open_with_page().await;
    let browser = PageBrowser::new("https://other.test/blank", BROWSER_TOKEN);
    let policy = ActionPolicy::default();

    let attached = Arc::new(ScriptedProvider::new(vec![
        tool_response("search_personal_memory", &[("query", MEMORY_TOKEN)]),
        final_response("found"),
    ]));
    let agent = ReActAgent::with_memory(
        AgentConfig::default(),
        attached.clone(),
        Some(Arc::clone(&service)),
    );
    let run = agent
        .execute_with_policy("find the note", &browser, &policy)
        .await
        .expect("run");
    assert_eq!(run.status, AgentRunStatus::Completed);
    match tool_result(&run.events, "search_personal_memory") {
        AgentRunEvent::ToolCallResult {
            success, result, ..
        } => {
            assert!(success, "{result}");
            assert!(result.contains(MEMORY_TOKEN), "{result}");
        }
        other => panic!("unexpected event {other:?}"),
    }
    assert_eq!(*attached.personal_memory.lock().expect("flag"), Some(true));

    let bare = Arc::new(ScriptedProvider::new(vec![tool_response(
        "search_personal_memory",
        &[("query", MEMORY_TOKEN)],
    )]));
    let agent = ReActAgent::new(AgentConfig::default(), bare.clone());
    let run = agent
        .execute_with_policy("find the note", &browser, &policy)
        .await
        .expect("run");
    assert_eq!(run.status, AgentRunStatus::Blocked);
    let message = run.final_response.as_deref().unwrap_or_default();
    assert!(
        message.contains("Unknown tool 'search_personal_memory'"),
        "{message}"
    );
    assert_eq!(*bare.personal_memory.lock().expect("flag"), Some(false));
}

#[test]
fn prompt_lists_memory_tools_only_when_personal_memory_is_attached() {
    let context = AiContext {
        current_url: PAGE_URL.to_string(),
        page_title: "Notes".to_string(),
        tool_results: Vec::new(),
        personal_memory: false,
    };
    let without = build_system_prompt(&context);
    assert!(!without.contains("search_personal_memory"));
    assert!(!without.contains("inspect_active_page"));

    let with_memory = AiContext {
        personal_memory: true,
        ..context
    };
    let prompt = build_system_prompt(&with_memory);
    assert!(prompt.contains("search_personal_memory"));
    assert!(prompt.contains("inspect_active_page"));
    assert!(prompt.contains("MemoryService"));
    assert!(prompt.contains("in-run agent log"));

    let calls = parse_tool_calls("Action: search_personal_memory(zephyrquartz)");
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].arguments.get("query").map(String::as_str),
        Some(MEMORY_TOKEN)
    );
}

#[tokio::test]
async fn zero_arg_action_inspect_active_page_reaches_the_tool_path() {
    let (_dir, service) = open_with_page().await;
    let content = "Action: inspect_active_page()";
    let calls = parse_tool_calls(content);
    assert_eq!(
        calls.len(),
        1,
        "Action: inspect_active_page() must not be dropped"
    );
    assert_eq!(calls[0].name, "inspect_active_page");
    assert!(calls[0].arguments.is_empty());

    let provider = Arc::new(ScriptedProvider::new(vec![
        AiResponse {
            content: content.to_string(),
            tool_calls: calls,
        },
        final_response("inspected"),
    ]));
    let agent = ReActAgent::with_memory(AgentConfig::default(), provider, Some(service));
    let run = agent
        .execute_with_policy(
            "inspect this page",
            &PageBrowser::new(PAGE_URL, BROWSER_TOKEN),
            &ActionPolicy::default(),
        )
        .await
        .expect("run");

    assert_eq!(run.status, AgentRunStatus::Completed);
    match tool_result(&run.events, "inspect_active_page") {
        AgentRunEvent::ToolCallResult {
            success, result, ..
        } => {
            assert!(success, "{result}");
            assert!(result.contains(MEMORY_TOKEN), "{result}");
            assert!(result.contains(PAGE_URL), "{result}");
        }
        other => panic!("unexpected event {other:?}"),
    }
}
