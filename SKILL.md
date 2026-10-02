---
name: neurobrowser
description: Drive NeuroBrowser from a Rust agent via the crate's 19 tools (17 browser tools plus search_personal_memory and inspect_active_page), or talk to the headless daemon's JSON-RPC (ping / policy.* / snapshot stub). Desktop is macOS WKWebView; the separate BrowserEngine library implementation is reqwest+scraper. The headless daemon has no browser backend.
---

# NeuroBrowser — Agent Skill

NeuroBrowser is a Rust library plus a macOS Tauri desktop (WKWebView). Agents
drive it in two ways:

1. **In-process** — call the `neurobrowser` crate (`ToolRegistry` in
   `src/browser/mod.rs`).
2. **Headless daemon** — newline-delimited JSON-RPC over a Unix socket (TCP
   fallback). Methods are `ping`, `policy.get`, `policy.set`,
   `policy.evaluate`, and `snapshot`. `policy.set` replaces the policy from
   `params` as an `ActionPolicy` object. `policy.evaluate` reads `params.tool`
   and `params.arguments`. `snapshot` ignores `params` and returns a hardcoded
   stub (`url`, `title`, `viewport`, `tree: ""`), not a crate `PageSnapshot`.
   The daemon does not execute browser tools.

The shipped agent surface is **19 tools**. Seventeen are browser
tools from `default_tool_registry()`. `search_personal_memory` and
`inspect_active_page` are added by `default_tool_registry_with_memory()` and
`ReActAgent::with_memory`. `ReActAgent::new` stays at the 17 browser tools.
There is no `ref_map` (`PageSnapshot` has no such field). Not shipped as named
tools: `evaluate`, `get_attribute`, `wait_for`, `extract_text`.

`neuro_memory::MemoryService` is persistent page memory on disk. The two
memory tools close over that service. They do not read an in-run agent log.
The shipped crate has no `agent::memory` module.

Full spec: `docs/AGENT-SURFACE.md`.

## When to use

- A Rust agent calling `BrowserInterface` / `default_tool_registry()`.
- A client speaking the daemon's JSON-RPC (`ping` / `policy.*` / stub
  `snapshot`).

Do not treat this as a Playwright ref-map driver. Interactive tools on
`BrowserEngine` (reqwest+scraper) return honest errors; they do not click a
live DOM.

## Checkout and verify

There is no installed CLI. Check out the repo and run the verification chain:

```bash
git clone https://github.com/Jimthetaxguy/neurobrowser.git
cd neurobrowser
./verify.sh
```

The numbered `./verify.sh` steps are in `docs/RUNBOOK-DEV.md` (One-shot green
build). Library-only: `cargo test`.

Headless daemon:

```bash
NEUROBROWSER_SOCKET="$HOME/.neurobrowser/daemon.sock" \
  cargo run --bin neurobrowser-headless --manifest-path src-tauri/Cargo.toml --features headless
```

`neurobrowser-headless` is a bin in the `neurobrowser-tauri` package, so that
command compiles `tauri` and runs `tauri-build` (macOS SDK, or GTK/WebKit on
Linux). A display is not required. The root library crate does not need that
toolchain.

There is no `neurobrowser-cli`. Speak JSON-RPC on the socket.

## Invocation

### In-process (Rust)

```rust
use std::sync::Arc;
use neurobrowser::{
    ActionPolicy, AgentConfig, AgentRunResult, AiProvider, AutonomyLevel,
    BrowserInterface, ReActAgent,
};

// The caller supplies a configured real provider and a browser with a loaded page.
async fn summarize_page(
    browser: &dyn BrowserInterface,
    provider: Arc<dyn AiProvider + Send + Sync>,
) -> Result<AgentRunResult, String> {
    let policy = ActionPolicy {
        autonomy_level: AutonomyLevel::ReadOnly,
        allowed_domains: vec!["example.com".into()],
        ..ActionPolicy::default()
    };
    let agent = ReActAgent::new(AgentConfig::default(), provider);
    agent.execute_with_policy("Summarize the page", browser, &policy).await
}
```

`ActionPolicy` is a public struct. There are no builder helpers.

### Headless daemon

```json
{"id":"1","method":"ping","params":{}}
{"id":"2","method":"policy.get","params":{}}
{"id":"3","method":"policy.evaluate","params":{"tool":"get_text","arguments":{"selector":"h1"}}}
{"id":"4","method":"snapshot","params":{}}
{"id":"5","method":"policy.set","params":{"autonomy_level":"read_only","allowed_domains":["example.com"],"denied_domains":[],"denied_tools":[],"approval_required_tools":[],"block_prompt_injection":true}}
```

`policy.set` sends `params` as the `ActionPolicy` object itself.
`autonomy_level` is snake_case: `read_only` | `assisted` | `high_autonomy`.
`ActionPolicy` fields have no serde defaults (`allowed_domains`,
`denied_domains`, `denied_tools`, `approval_required_tools`,
`block_prompt_injection`).
`policy.evaluate` reads `params.tool` and `params.arguments`.
Daemon `policy.evaluate` outcomes are `Allow`, `RequireApproval`, and `Block`
(Rust serde uses snake_case; this dispatcher formats enum names).
`snapshot` ignores `params` and returns
`{ "url": "about:blank", "title": "", "viewport": { "width": 0, "height": 0, "scroll_x": 0, "scroll_y": 0 }, "tree": "" }`,
not a `PageSnapshot` from the crate.

## Tools

Browser tools are registered by `default_tool_registry()`. The two memory
tools are registered by `default_tool_registry_with_memory()` /
`ReActAgent::with_memory`. Browser arguments are CSS selectors (or pixels /
a key), not element refs.

Call format is `ToolCall: {"name":"tool_name","arguments":{"key":"value"}}`.
The model's tool list is built from each tool's `ToolDefinition`. The two
memory tools appear only when memory is attached.

A call to a registered tool that omits a required argument (or sets it to
`""`) does not run. The agent records
`Error: missing required argument(s): …` and shows it to the model on the
next turn. That turn does not complete the run.

| Tool | Args | Purpose |
|---|---|---|
| `navigate` | `url` | Fetch / open a URL |
| `wait` | — | Wait for navigation to settle |
| `query_dom` | `selector` | Query elements by CSS selector |
| `get_text` | `selector` | Read text of matching elements |
| `get_links` | — | List links on the current page |
| `get_prices` | — | Extract price-like strings |
| `get_tables` | — | Extract table summaries |
| `click` | `selector` | Click an element |
| `type` | `selector`, `text` | Type into an input |
| `scroll_to` | `selector` | Scroll an element into view |
| `scroll_by` | `x`, `y` | Scroll by pixel offset |
| `submit_form` | `selector` | Submit a form |
| `keypress` | `key` | Send a key |
| `screenshot` | — | Registered; interface default is an error |
| `back` | — | History back |
| `forward` | — | History forward |
| `reload` | — | Reload |
| `search_personal_memory` | `query`, optional `limit` | Search persistent `MemoryService` (not an in-run agent log). Ignores the browser. Registered when a `MemoryService` is attached. |
| `inspect_active_page` | — | Captured content for the current URL, or a `capture denied` error. Registered when a `MemoryService` is attached. |

`screenshot` is registered. `BrowserInterface::screenshot` defaults to
`"screenshot is not supported by this browser"`. Neither `BrowserEngine` nor
the Tauri runtime overrides it. `BrowserEngine` does not override `keypress`
either; that call uses the `BrowserInterface` default error, as do `back`,
`forward`, and `reload`.

## Autonomy

| Level | Auto-allow | Gate |
|---|---|---|
| `ReadOnly` | Read, wait, scroll | Other actions, including navigate, `Block` |
| `Assisted` (default) | Read, wait, scroll, navigate that is not cross-domain; a hostless current page (`about:blank`, empty URL) is not cross-domain | otherwise `RequireApproval` |
| `HighAutonomy` | Remaining non-high-impact actions | Submit / purchase / auth / upload / message / destructive → `RequireApproval` |

The mode table applies after the common gates below. Sensitive inputs and
explicit approval-list matches return `RequireApproval` before mode evaluation,
including in `ReadOnly` and `HighAutonomy`. Tool/domain denials and injection
checks run first and return `Block`.

## Policy gates

Same order as `ActionPolicy::evaluate`; first match wins:

1. `denied_tools` → `Block`.
2. Prompt-injection on the page → `Block`.
3. Unsafe navigation schemes (`javascript:` / `data:` / `file:` / …) → `Block`.
4. If the URL has a parsed host, block it when it appears in `denied_domains` or misses a non-empty allowlist. A rule matches that host or its subdomains (`example.com` matches `a.example.com`). If the URL has no parsed host, skip this gate.
5. Sensitive keys or sensitive tool metadata → `RequireApproval`.
6. `approval_required_tools` → `RequireApproval`.
7. Mode table.

Headless `policy.evaluate` reads `params.tool` and `params.arguments`, then
builds an empty `PageSnapshot` (no URL, HTML, or text) before calling
evaluate, so prompt-injection cannot fire,
`is_cross_domain` is false with no host, and non-navigate tools never get a
domain; allow/deny lists still apply to `navigate` via the argument URL.

Credential tokens (`password`, `token`, `secret`, `api_key`, `authorization`,
and related) are `[REDACTED]` in the decision. `type` is marked sensitive;
metadata alone does not redact every argument value.

## See also

- `docs/AGENT-SURFACE.md` — spec-of-record.
- `docs/RUNBOOK-DEV.md` — build + run.
- `src/browser/mod.rs` — `default_tool_registry()`.
- `src/agent/policy.rs` — policy gates.
