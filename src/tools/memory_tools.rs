//! Agent tools over persistent page memory.
//!
//! `SearchPersonalMemoryTool` and `InspectActivePageTool` close over
//! `neuro_memory::MemoryService`. That store is durable page memory. The
//! shipped crate has no in-run `agent::memory` store.
//!
//! `BrowserTool::execute` still receives a browser. Search does not use it.
//! Inspect reads the current URL from `snapshot()` and does not navigate.

use crate::tools::{
    BrowserInterface, BrowserTool, ToolAction, ToolArgumentDefinition, ToolDefinition,
    ToolRegistry, ToolResult, ToolRisk,
};
use async_trait::async_trait;
use neuro_memory::{CaptureDecision, CapturePolicy, MemoryService, SearchRequest, SearchResult};
use std::collections::HashMap;
use std::sync::Arc;

const SEARCH_PERSONAL_MEMORY: &str = "search_personal_memory";
const SEARCH_PERSONAL_MEMORY_DESCRIPTION: &str =
    "Search persistent personal page memory (MemoryService). This is not an in-run agent log.";
const INSPECT_ACTIVE_PAGE: &str = "inspect_active_page";
const INSPECT_ACTIVE_PAGE_DESCRIPTION: &str = "Return captured personal-memory content for the current page URL, or a policy-denied error. Uses MemoryService, not an in-run agent log.";

/// Hits returned when `limit` is omitted.
const DEFAULT_SEARCH_LIMIT: usize = 5;
/// Upper bound accepted for the `limit` argument.
const MAX_SEARCH_LIMIT: usize = 20;

/// Register both personal-memory tools on `registry`.
///
/// `policy` gates `inspect_active_page` only. Search returns whatever is
/// already indexed.
pub fn register_memory_tools(
    registry: &mut ToolRegistry,
    memory: Arc<MemoryService>,
    policy: CapturePolicy,
) {
    registry.register(Arc::new(SearchPersonalMemoryTool {
        memory: Arc::clone(&memory),
    }));
    registry.register(Arc::new(InspectActivePageTool { memory, policy }));
}

/// Definitions of both personal-memory tools.
///
/// They need no `MemoryService`, so the provider prompt lists them from here
/// when `AiContext::personal_memory` is set.
pub(crate) fn definitions() -> Vec<ToolDefinition> {
    vec![search_definition(), inspect_definition()]
}

/// Argument names for positional `Action: tool(...)` calls.
///
/// `default_tool_registry()` does not contain these tools, so the provider
/// parser reads the names from [`definitions`].
pub(crate) fn positional_argument_names(tool_name: &str) -> Option<Vec<String>> {
    definitions()
        .into_iter()
        .find(|definition| definition.name == tool_name)
        .map(|definition| {
            definition
                .arguments
                .into_iter()
                .map(|argument| argument.name)
                .collect()
        })
}

fn search_definition() -> ToolDefinition {
    ToolDefinition::new(
        SEARCH_PERSONAL_MEMORY,
        SEARCH_PERSONAL_MEMORY_DESCRIPTION,
        ToolRisk::new(ToolAction::Read),
    )
    .with_arguments(vec![
        ToolArgumentDefinition::required("query", "Text to search in persistent personal memory"),
        ToolArgumentDefinition {
            name: "limit".to_string(),
            required: false,
            description: format!(
                "Maximum hits to return (default {DEFAULT_SEARCH_LIMIT}, max {MAX_SEARCH_LIMIT})"
            ),
        },
    ])
}

fn inspect_definition() -> ToolDefinition {
    ToolDefinition::new(
        INSPECT_ACTIVE_PAGE,
        INSPECT_ACTIVE_PAGE_DESCRIPTION,
        ToolRisk::new(ToolAction::Read),
    )
}

/// Search [`MemoryService`] and ignore the live browser.
struct SearchPersonalMemoryTool {
    memory: Arc<MemoryService>,
}

#[async_trait]
impl BrowserTool for SearchPersonalMemoryTool {
    fn definition(&self) -> ToolDefinition {
        search_definition()
    }

    async fn execute(
        &self,
        args: HashMap<String, String>,
        _browser: &dyn BrowserInterface,
    ) -> ToolResult {
        let Some(query) = args
            .get("query")
            .map(|query| query.trim())
            .filter(|query| !query.is_empty())
        else {
            return ToolResult::error(
                SEARCH_PERSONAL_MEMORY,
                "search_personal_memory requires a non-empty query".to_string(),
            );
        };
        let limit = match parse_limit(args.get("limit")) {
            Ok(limit) => limit,
            Err(error) => return ToolResult::error(SEARCH_PERSONAL_MEMORY, error),
        };
        let request = SearchRequest {
            query: query.to_string(),
            limit,
        };
        match self.memory.search(request).await {
            Ok(hits) if hits.is_empty() => ToolResult::success(
                SEARCH_PERSONAL_MEMORY,
                "No personal memory matches.".to_string(),
            ),
            Ok(hits) => ToolResult::success(SEARCH_PERSONAL_MEMORY, format_search_hits(&hits)),
            Err(error) => ToolResult::error(SEARCH_PERSONAL_MEMORY, error.to_string()),
        }
    }
}

/// Return captured blocks for the browser's current URL, or a policy denial.
struct InspectActivePageTool {
    memory: Arc<MemoryService>,
    policy: CapturePolicy,
}

#[async_trait]
impl BrowserTool for InspectActivePageTool {
    fn definition(&self) -> ToolDefinition {
        inspect_definition()
    }

    async fn execute(
        &self,
        _args: HashMap<String, String>,
        browser: &dyn BrowserInterface,
    ) -> ToolResult {
        let snapshot = match browser.snapshot().await {
            Ok(snapshot) => snapshot,
            Err(error) => return ToolResult::error(INSPECT_ACTIVE_PAGE, error),
        };
        let page_url = snapshot.url;
        if let CaptureDecision::Deny { reason } = self.policy.evaluate(&page_url) {
            return ToolResult::error(INSPECT_ACTIVE_PAGE, format!("capture denied: {reason}"));
        }
        let Ok(parsed) = url::Url::parse(&page_url) else {
            return ToolResult::error(
                INSPECT_ACTIVE_PAGE,
                "capture denied: URL could not be parsed".to_string(),
            );
        };
        match self.memory.blocks_for_url(&parsed).await {
            Ok(blocks) if blocks.is_empty() => ToolResult::error(
                INSPECT_ACTIVE_PAGE,
                format!("No captured content for {page_url}"),
            ),
            Ok(blocks) => {
                let body = blocks
                    .iter()
                    .map(|block| format_section(&block.heading_path, &block.text))
                    .collect::<Vec<_>>()
                    .join("\n\n");
                ToolResult::success(INSPECT_ACTIVE_PAGE, format!("{page_url}\n\n{body}"))
            }
            Err(error) => ToolResult::error(INSPECT_ACTIVE_PAGE, error.to_string()),
        }
    }
}

fn parse_limit(raw: Option<&String>) -> Result<usize, String> {
    let Some(raw) = raw else {
        return Ok(DEFAULT_SEARCH_LIMIT);
    };
    let parsed = raw
        .trim()
        .parse::<usize>()
        .map_err(|_| format!("limit must be a non-negative integer, got {raw:?}"))?;
    Ok(parsed.min(MAX_SEARCH_LIMIT))
}

fn format_search_hits(hits: &[SearchResult]) -> String {
    hits.iter()
        .map(|hit| {
            let mut section = format!(
                "{}\n{}",
                hit.page_url,
                format_section(&hit.heading_path, &hit.text)
            );
            section.push_str(&format!("\nscore: {}", hit.score));
            section
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn format_section(heading_path: &[String], text: &str) -> String {
    if heading_path.is_empty() {
        text.to_string()
    } else {
        format!("{}\n{text}", heading_path.join(" > "))
    }
}
