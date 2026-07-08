# NeuroBrowser — Testing Notes

What was verified during the v0.1 Phase B green-build, and what remains for
manual verification on real websites.

## Automated test coverage (verified 2026-07-08)

`cargo test --all-targets` after the Phase A merge:

- **17 lib tests pass** (in-crate `#[cfg(test)] mod tests` blocks).
- **11 integration tests pass** across 4 test files:
  - `tests/action_policy.rs` (4 tests) — `ActionPolicy` semantics:
    - `denylist_beats_allowlist` — denylist overrides allowlist, returns
      `Block` with `RiskFlag::DomainDenied`.
    - `assisted_mode_requires_approval_for_clicks` — in `Assisted` mode,
      `click` returns `RequireApproval`.
    - Sensitive-arg redaction — keys matching
      `password|token|secret|api_key|apikey|ssn|social|credit|card|cvv|otp|auth`
      become `[REDACTED]` in audit trail.
    - Prompt-injection blocking — substrings like `ignore previous instructions`
      / `reveal your instructions` cause `Block`.
  - `tests/autonomous_agent.rs` — end-to-end ReAct loop with a stub provider.
  - `tests/error_types.rs` — error conversion paths across the type lattice.
  - `tests/streaming.rs` — `StreamingAgent` / `StreamEvent` wiring.

## Code-level verification of click/type/submit

Per `CURRENT_STATE.md §2.3` and the prior analysis pass: the merged tree's
`TauriBrowserRuntime` IS a real browser control surface, not a scraper.
Verified at the source level (`src-tauri/src/runtime.rs`):

| Method | Implementation | Real? |
|---|---|---|
| `navigate(url)` | `self.webview()?.navigate(parsed)` — real `WKWebView::navigate` | YES — actual webview navigation |
| `click(selector)` | `runtime.click(selector)` in the webview's JS context → `element.click()` DOM event → `wait_for_navigation()` | YES — real DOM click via JSON-RPC into the live webview |
| `type_text(selector, text)` | `runtime.typeText(selector, text)` → JS `element.value = text` (or `dispatchEvent(InputEvent)`) | YES — real keystroke injection |
| `submit_form(selector)` | `runtime.submitForm(selector)` → JS `form.submit()` then `wait_for_navigation()` | YES — real form submission |
| `scroll_to(selector)` / `scroll_by(x, y)` | `runtime.scrollTo(...)` / `runtime.scrollBy(...)` → JS `element.scrollIntoView()` / `window.scrollBy(...)` | YES — real scroll |
| `snapshot()` | `runtime.snapshot()` returns a structured `{url, title, html, text, links, images, forms, prices, tables}` enriched by `browser::enrich_snapshot` | YES — real snapshot of the live webview |
| `browser_back()` / `forward()` / `reload()` | `history.back()` / `history.forward()` / `webview.reload()` | YES — real history navigation |

The JS bridge lives in `RUNTIME_INIT_SCRIPT` at `src-tauri/src/runtime.rs:15`
and is injected into every webview at creation via
`WebviewBuilder::initialization_script` (line 586). It defines
`window.__NEUROBROWSER_RUNTIME__` with `dispatch(pageId, requestId, producer)`
which calls back to Rust via `invoke('browser_runtime_report', ...)`.

## Manual verification (Phase B2 — to run after `cargo tauri dev`)

The merged tree compiles and the test suite passes, but the **actual
WKWebView-driven page rendering** can only be exercised by launching the app
and pointing it at real websites. Recommended smoke tests:

### 1. Basic navigation (single page)

```
create_session
create_page
navigate(sessionId, pageId, "https://example.com")
get_page_snapshot(sessionId, pageId)
```

Expected: snapshot returns url=`https://example.com`, title contains
"Example Domain", link_count≥1, text non-empty.

### 2. Click navigates the webview

```
navigate(sessionId, pageId, "https://example.com")
query_dom("a")  →  returns the "More information..." link
click("a[href*='iana']")
get_page_snapshot(sessionId, pageId)
```

Expected: snapshot now reports url=`https://www.iana.org/...`,
link_count ≥ 1, page title contains "IANA".

### 3. Type into a form and submit

```
navigate(sessionId, pageId, "https://www.google.com")
query_dom("input[name=q]")
type_text("input[name=q]", "neurobrowser rust tauri")
submit_form("form")
```

Expected: snapshot now reports the Google search results page; title contains
"neurobrowser rust tauri - Google Search".

### 4. Scroll

```
navigate(sessionId, pageId, "https://en.wikipedia.org/wiki/Rust_(programming_language)")
scroll_by(0, 2000)
get_page_snapshot(sessionId, pageId)
```

Expected: page is scrolled down; text content is preserved.

### 5. Multi-tab

```
create_session
create_page → pageId=0
create_page → pageId=1
navigate(0, "https://example.com")
navigate(1, "https://www.rust-lang.org")
set_active_page(1)
get_page_snapshot(1)
```

Expected: snapshot shows the Rust language site; switching back to page 0
preserves its example.com state.

### 6. ActionPolicy (assisted mode)

In the React UI, select policy mode "Assisted", then run `start_agent_run`
with any prompt that triggers a `click` tool call. Expected:
`AgentRunEvent::ApprovalRequested` fires, the timeline shows the request,
clicking Approve continues the run, clicking Deny returns a Blocked status.

### 7. ActionPolicy (denied domain)

```
set_action_policy({
  autonomy_level: "assisted",
  allowed_domains: [],
  denied_domains: ["blocked.example"],
  ...
})
navigate → "https://blocked.example"
```

Expected: the navigation is `Block`-ed with `RiskFlag::DomainDenied` in the
event trail.

## What is NOT yet wired (Phase C will close these)

- **`AgentMemory` is not yet written from `ReActAgent::execute`** — the types
  exist in `src/agent/memory.rs`, but the loop doesn't call `episodic.push`.
  The trajectory history is recoverable only from `AgentRunEvent::ToolCallStarted`
  in the most recent run.
- **`AgentMetrics` counters are defined but never incremented** —
  `src/agent/observability.rs` declares them; `ReActAgent::execute` doesn't
  call `record_*`.
- **`StreamingAgent::execute_stream` is declared but `ReActAgent` doesn't impl it
  with live `mpsc::Sender<StreamEvent>` emission**. Events come back as part of
  `AgentRunResult.events` (good enough for the timeline UI), but not as a live
  stream.
- **Cross-iteration conversation history**: `AiContext.conversation_history`
  is built by `build_context` but the providers (`openai.rs`, `anthropic.rs`,
  `ollama.rs`) only consume the latest user message + system prompt.
- **Native tool calls**: providers parse `Action: name(args)` text out of the
  LLM response; they don't send the `tools` JSON Schema block. Migration to
  OpenAI / Anthropic / Ollama native tool-calling is Phase C5.

## Coverage gaps to address in future work

- **No visual regression tests** for the React UI. Phase D5 will add
  `neurobrowser diff --pixel` for screenshot diffing, but the frontend
  itself has no jest/vitest tests.
- **No end-to-end test against a real LLM provider** — all 11 integration
  tests use stub providers. Real-provider tests would catch integration drift
  but require API keys; gate behind `INTEGRATION=1` env var.
- **No fuzz tests** for `validate_url` / `parse_arguments` / the policy
  evaluator. A `cargo fuzz` harness would be cheap to add.