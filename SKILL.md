---
name: neurobrowser
description: Drive NeuroBrowser from a Rust agent via the crate's 17 CSS-selector tools, or talk to the headless daemon's JSON-RPC (ping / policy.* / snapshot stub). Desktop is macOS WKWebView; the separate BrowserEngine library implementation is reqwest+scraper. The headless daemon has no browser backend.
---

# NeuroBrowser — Agent Skill

NeuroBrowser is a Rust library plus a macOS Tauri desktop (WKWebView). Agents
drive it in two ways:

1. **In-process** — call the `neurobrowser` crate (`ToolRegistry` in
   `src/browser/mod.rs`).
2. **Headless daemon** — newline-delimited JSON-RPC over a Unix socket (TCP
   fallback). Methods are `ping`, `policy.get`, `policy.set`,
   `policy.evaluate`, and `snapshot`. `snapshot` is a hardcoded
   `about:blank` stub. The daemon does not execute browser tools.

The shipped agent surface is **17 CSS-selector tools**. There is no `ref_map`
(`PageSnapshot` has no such field). Not shipped as named tools: `evaluate`,
`get_attribute`, `wait_for`, `extract_text`.

Full spec: `docs/AGENT-SURFACE.md`.

## When to use

- A Rust agent calling `BrowserInterface` / `default_tool_registry()`.
- A client speaking the daemon's JSON-RPC (`ping` / `policy.*` / stub
  `snapshot`).

Do not treat this as a Playwright ref-map driver. Interactive tools on
`BrowserEngine` (reqwest+scraper) return honest errors; they do not click a
live DOM.

## Install

```bash
git clone https://github.com/Jimthetaxguy/neurobrowser.git
cd neurobrowser
./verify.sh
```

Full `./verify.sh` type-checks the Tauri crate (macOS, or GTK/WebKit on
Linux). Library-only: `cargo test`. See `docs/RUNBOOK-DEV.md`.

Headless daemon:

```bash
NEUROBROWSER_SOCKET="$HOME/.neurobrowser/daemon.sock" \
  cargo run --bin neurobrowser-headless --manifest-path src-tauri/Cargo.toml --features headless
```

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
{"id":"3","method":"snapshot","params":{}}
```

`snapshot` returns a hardcoded stub, not a `PageSnapshot` from the crate.

## Tools

Registered by `default_tool_registry()`. Arguments are CSS selectors (or
pixels / a key), not element refs.

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

`screenshot` is registered. `BrowserInterface::screenshot` defaults to
`"screenshot is not supported by this browser"`. Neither `BrowserEngine` nor
the Tauri runtime overrides it.

## Autonomy

| Level | Auto-allow | Gate |
|---|---|---|
| `ReadOnly` | Read, wait, scroll | Other actions, including navigate, `Block` |
| `Assisted` (default) | Read, wait, scroll, same-domain navigate | Click / type / submit / cross-domain → `RequireApproval` |
| `HighAutonomy` | Remaining non-high-impact actions | Submit / purchase / auth / upload / message / destructive → `RequireApproval` |

The mode table applies after the common gates below. Sensitive inputs and
explicit approval-list matches return `RequireApproval` before mode evaluation,
including in `ReadOnly` and `HighAutonomy`. Tool/domain denials and injection
checks run first and return `Block`.

## Policy gates

1. `denied_domains` — `Block`.
2. `allowed_domains` — if non-empty, URLs not on the list are `Block`.
3. Sensitive keys (`password`, `token`, `secret`, `api_key`, `authorization`,
   and related credential tokens) become `[REDACTED]` in the decision payload.
   Sensitive keys or sensitive tool metadata require approval. `type` is marked
   sensitive; metadata alone does not redact every argument value.
4. Prompt-injection substrings (`ignore previous instructions`,
   `reveal your instructions`) → `Block`.

## See also

- `docs/AGENT-SURFACE.md` — spec-of-record.
- `docs/RUNBOOK-DEV.md` — build + run.
- `src/browser/mod.rs` — `default_tool_registry()`.
- `src/agent/policy.rs` — policy gates.
