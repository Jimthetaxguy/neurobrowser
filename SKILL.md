---
name: neurobrowser
description: Drive the NeuroBrowser Rust/Tauri browser from any agent. Provides structured snapshot/click/type/extract tools over a Tauri child webview, with optional headless daemon mode. Use when an agent needs a real browser session with policy-gated autonomy and both programmatic and visual access to web pages.
---

# NeuroBrowser — Agent Skill

NeuroBrowser is an AI-native desktop browser built on Rust + Tauri. Agents can
drive it in two ways:

1. **In-process** — call the `neurobrowser::*` Rust crate directly (best
   when the agent is also a Rust binary).
2. **Headless daemon** — connect over a Unix domain socket (TCP fallback).
   Ships in v0.1.1 as `neurobrowser-headless`.

The agent-facing surface is 12 tools (`snapshot`, `click`, `type_text`,
`submit_form`, `query_selector`, `evaluate`, `navigate`, `get_text`,
`get_attribute`, `wait_for`, `extract_text`, `screenshot`), three autonomy
levels (`ReadOnly` / `Assisted` / `HighAutonomy`), and `ActionPolicy` gates.

Full spec: `docs/AGENT-SURFACE.md`.

## When to use

- An external agent needs a real browser session (WKWebView / WebView2 /
  WebKitGTK, not a scraper) with policy-gated autonomy.
- The agent needs **both** programmatic and **visual** access to web pages.
- The agent must work with pages that use CORS, web sockets, or rich
  JavaScript — `reqwest` + `scraper` will fail where a real browser
  succeeds.

Do NOT use for:

- Pure HTTP APIs (use your language's HTTP client).
- Sites with strict bot blocking (use a stealth browser like
  `playwright-stealth`).
- Tasks where you don't need a browser — `WebFetch` / `curl` is faster.

## Install

```bash
git clone https://github.com/Jimthetaxguy/neurobrowser.git
cd neurobrowser
chmod +x verify.sh
./verify.sh
```

Headless daemon (cross-process IPC):

```bash
NEUROBROWSER_SOCKET="$HOME/.neurobrowser/daemon.sock" \
  cargo run --bin neurobrowser-headless --manifest-path src-tauri/Cargo.toml --features headless
```

The process prints `NEUROBROWSER_LISTENING=unix://…` (or `tcp://…` if the
Unix bind fails). There is no CLI wrapper; speak JSON-RPC on that socket.

## Invocation

### In-process (Rust agent)

```rust
use neurobrowser::{
    ActionPolicy, AgentConfig, AutonomyLevel, PageConfig, ReActAgent, SessionManager,
};

let policy = ActionPolicy {
    autonomy_level: AutonomyLevel::ReadOnly,
    allowed_domains: vec!["example.com".into()],
    ..ActionPolicy::default()
};

let sessions = SessionManager::new(PageConfig::default(), AgentConfig::default());
let session_id = sessions.create_session();
let _page = sessions.create_page(&session_id)?;

let agent = ReActAgent::new(AgentConfig::default(), provider);
let response = agent
    .execute_with_policy("Summarize the page", &browser, &policy)
    .await?;
```

`ActionPolicy` is a public struct (`autonomy_level`, `allowed_domains`,
`denied_domains`, `denied_tools`, `approval_required_tools`,
`block_prompt_injection`) with `Default` and `evaluate(...)`. There are no
builder helpers.

### Cross-process (headless daemon)

Newline-delimited JSON on the socket:

```json
{"id":"1","method":"ping","params":{}}
{"id":"2","method":"policy.get","params":{}}
{"id":"3","method":"snapshot","params":{}}
```

Shipped methods: `ping`, `policy.get`, `policy.set`, `policy.evaluate`,
`snapshot`, `policy.snapshot`.

## Tools

See `docs/AGENT-SURFACE.md` for the full JSON schemas. Quick reference:

| Tool | Purpose |
|---|---|
| `snapshot` | Get URL + title + ref-map + ARIA tree |
| `click` | Click an element by ref |
| `type_text` | Type into an input by ref |
| `submit_form` | Submit a form / click a button by ref |
| `query_selector` | Resolve CSS selector → list of refs |
| `evaluate` | Run JS in the page sandbox |
| `navigate` | Navigate the active page |
| `get_text` | Read element text by ref |
| `get_attribute` | Read one attribute by ref |
| `wait_for` | Block until a selector matches |
| `extract_text` | Read + parse text (total/date/price heuristics) |
| `screenshot` | PNG screenshot (base64) |

## Autonomy

```rust
use neurobrowser::{ActionPolicy, AutonomyLevel};

let policy = ActionPolicy {
    autonomy_level: AutonomyLevel::Assisted,
    allowed_domains: vec!["example.com".into()],
    denied_domains: vec!["blocked.example".into()],
    ..ActionPolicy::default()
};
```

| Level | Read | Click / Type / Submit | Navigate | Approve-or-block? |
|---|---|---|---|---|
| `ReadOnly` | ✓ | ✗ (RequireApproval) | ✗ (Block) | Never |
| `Assisted` | ✓ | ✓ (RequireApproval → UI) | ✓ | Per-call UI dialog |
| `HighAutonomy` | ✓ | ✓ | ✓ | Sensitive-arg auto-redact; UI optional |

## Policy gates

1. `denied_domains` — calls to a URL on this list are `Block`-ed.
2. `allowed_domains` — if non-empty, calls to URLs NOT on this list are
   `Block`-ed.
3. Argument redaction — keys matching
   `password|token|secret|api_key|apikey|ssn|social|credit|card|cvv|otp|auth`
   become `[REDACTED]` in audit trails.
4. Prompt-injection detection — values containing `ignore previous
   instructions` / `reveal your instructions` cause `Block`.

## Worked example: log in + extract

```javascript
// Pseudocode; real call shape depends on your integration (in-process Rust
// or JSON-RPC over the daemon socket).
await tools.navigate({ url: "https://example.com/login" });
const snap = await tools.snapshot({ url_or_ref: "@self" });

const email_ref = snap.ref_map["@e1"];
const pw_ref = snap.ref_map["@e2"];
const submit_ref = snap.ref_map["@e3"];

await tools.type_text({ ref: email_ref.id, text: process.env.EMAIL });
await tools.type_text({ ref: pw_ref.id, text: process.env.PASSWORD });

const r = await tools.submit_form({ ref: submit_ref.id });
if (!r.ok) {
  if (r.error?.code === "BLOCKED") {
    throw new Error("Login is on the denied-domains list.");
  }
}

await tools.wait_for({ selector: ".dashboard", timeout_ms: 5000 });
const after = await tools.snapshot({ url_or_ref: "@self" });
const total_text = await tools.extract_text({ ref: "@e20", structured: true });
```

## Troubleshooting

| Symptom | Cause | Fix |
|---|---|---|
| `BLOCKED` on every call | URL on `denied_domains` | Update policy; or ask the user to override |
| `pending_approval_id` returned in Assisted mode | Click / type requires user approval | Surface the UI approval prompt; do not auto-approve |
| `TIMEOUT` on `wait_for` | Selector never matched (page is slow, or selector is wrong) | Increase `timeout_ms`; re-snapshot and check the ref-map |
| `NOT_FOUND` on `click` | Ref is stale (page re-rendered) | Re-`snapshot` and re-resolve the ref |
| `evaluate` returns empty string | Cross-origin blocked | Use `get_text` / `get_attribute` instead; or check the page's iframe sandboxing |
| Screenshot is blank | Element is offscreen / occluded | Scroll first via `scroll_to`, then capture |
| "Tauri invoke bridge is not available" | You're calling tools outside the Tauri webview runtime | Run via the headless daemon or invoke directly from Rust |

## See also

- `docs/AGENT-SURFACE.md` — the spec-of-record.
- `docs/RUNBOOK-DEV.md` — how to build + run.
- `docs/references/prior-art.md` — what NeuroBrowser takes / leaves from
  agent-browser, hyperbrowser-app-examples, etc.
