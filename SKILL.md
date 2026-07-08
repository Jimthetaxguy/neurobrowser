---
name: neurobrowser
description: Drive the NeuroBrowser Rust/Tauri browser from any agent. Provides structured snapshot/click/type/extract tools over a Tauri child webview, with optional headless daemon mode. Use when an agent needs a real browser session with policy-gated autonomy and both programmatic and visual access to web pages.
---

# NeuroBrowser — Agent Skill

NeuroBrowser is an AI-native desktop browser built on Rust + Tauri. Agents can
drive it in two ways:

1. **In-process** — call the `neurobrowser::*` Rust crate directly (best
   when the agent is also a Rust binary).
2. **Headless daemon** (Phase D4) — connect over Unix domain socket.

This skill documents the **agent-facing tool surface**: 12 tools (`snapshot`,
`click`, `type_text`, `submit_form`, `query_selector`, `evaluate`,
`navigate`, `get_text`, `get_attribute`, `wait_for`, `extract_text`,
`screenshot`), the autonomy levels (`ReadOnly` / `Assisted` /
`HighAutonomy`), and the policy gates that bind them.

Full spec: `docs/AGENT-SURFACE.md`.

## When to use

- An external agent needs a real browser session (real WKWebView / WebView2 /
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

This builds the desktop app. To run as a headless daemon (cross-process IPC):

```bash
cargo build --release --manifest-path src-tauri/Cargo.toml --features headless
./target/release/neurobrowser-headless --socket ~/.neurobrowser/daemon.sock
```

(Headless daemon ships in v0.1.1 — see `docs/ROADMAP-v0.2.md`.)

## Invocation

### In-process (Rust agent)

```rust
use neurobrowser::{ReActAgent, SessionManager, PageConfig, ActionPolicy, AutonomyLevel};
use neurobrowser::agent::policy::{AutonomyLevel as AV, PolicyDomain};

let browser_config = PageConfig::default();
let agent_config = AgentConfig::default();
let session_manager = SessionManager::new(browser_config, agent_config);
let session_id = session_manager.create_session();
let page = session_manager.create_page(&session_id)?;

let browser = /* concrete BrowserInterface implementation */;
let policy = ActionPolicy::read_only()
    .with_allowed_domains(vec!["example.com".parse()?]);
let agent = ReActAgent::new(/* ... */);

// One-shot blocking ask
let response = agent.execute("Summarize the page", &browser).await?;
```

### Cross-process (any agent via headless daemon — v0.1.1)

```bash
# Assume the daemon is running and socket is at $NB_SOCKET.
neurobrowser-cli ask --session auto --prompt "Summarize the page"
neurobrowser-cli click --session auto --ref @e3
neurobrowser-cli snapshot --session auto
```

(`neurobrowser-cli` ships in v0.1.1 as a thin Rust binary that talks to the
daemon.)

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
use neurobrowser::ActionPolicy;
use neurobrowser::agent::policy::AutonomyLevel;

let policy = ActionPolicy::default().with_autonomy(AutonomyLevel::Assisted);
let policy = policy.with_allowed_domains(vec!["example.com".parse()?]);
let policy = policy.with_denied_domains(vec!["blocked.example".parse()?]);
```

| Level | Read | Click / Type / Submit | Navigate | Approve-or-block?
|---|---|---|---|---|
| `ReadOnly` | ✓ | ✗ (RequireApproval) | ✗ (Block) | Never
| `Assisted` | ✓ | ✓ (RequireApproval → UI) | ✓ | Per-call UI dialog
| `HighAutonomy` | ✓ | ✓ | ✓ | Sensitive-arg auto-redact; UI optional

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
// or CLI / IPC).
await tools.navigate({ url: "https://example.com/login" });
const snap = await tools.snapshot({ url_or_ref: "@self" });

const email_ref = snap.ref_map["@e1"];
const pw_ref = snap.ref_map["@e2"];
const submit_ref = snap.ref_map["@e3"];

await tools.type_text({ ref: email_ref.id, text: process.env.EMAIL });
// NB: type_text() redacts "password" in the audit trail but uses the real
// value at call time.
await tools.type_text({ ref: pw_ref.id, text: process.env.PASSWORD });

const r = await tools.submit_form({ ref: submit_ref.id });
if (!r.ok) {
  if (r.error?.code === "BLOCKED") {
    // The page is on the denied list. Abort.
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
- `docs/TESTING-NOTES.md` — what's tested.
- `docs/references/prior-art.md` — what NeuroBrowser takes / leaves from
  agent-browser, hyperbrowser-app-examples, etc.