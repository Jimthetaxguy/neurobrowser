# neurobrowser

AI-native desktop browser: Rust library + Tauri/React shell.
The desktop drives a real OS webview through `ActionPolicy`-gated tools.
The Rust library also supplies an HTTP scraper. The headless daemon is a
policy JSON-RPC protocol stub: its `about:blank` snapshot is hardcoded and
it cannot navigate or execute browser tools. There is no custom rendering
engine; FastRender was never integrated.

See [SKILL.md](SKILL.md) and [docs/AGENT-SURFACE.md](docs/AGENT-SURFACE.md)
for the agent surface. Build and run: [docs/RUNBOOK-DEV.md](docs/RUNBOOK-DEV.md).

## Tech Stack

- **Three path crates, no workspace:** `neurobrowser` (`src/`), `neurobrowser-tauri` (`src-tauri/`), and `neuro-memory` (`crates/neuro-memory/`)
- **Desktop (macOS only):** React + Vite shell, Tauri v2 commands, OS child webview per page
- **Library:** Tokio, reqwest, `scraper` HTML parsing, serde, thiserror v2
- **Headless protocol stub:** Tokio policy evaluation and a hardcoded snapshot
- **Desktop IPC:** Tauri commands
- **Policy socket:** JSON-RPC (`ping`, `policy.*`, stub `snapshot`)

## Quick Start

Run `./verify.sh`. Darwin-only steps and the CI-only notes are in [docs/RUNBOOK-DEV.md](docs/RUNBOOK-DEV.md).

Desktop (`npx tauri dev` in `src-tauri/`) is macOS.

Start the headless policy stub on Unix:

```bash
NEUROBROWSER_SOCKET="$HOME/.neurobrowser/daemon.sock" \
  cargo run --manifest-path src-tauri/Cargo.toml --features headless --bin neurobrowser-headless
```

`neurobrowser-headless` is a bin in the `neurobrowser-tauri` package, so that
command compiles `tauri` and runs `tauri-build` (macOS SDK, or GTK/WebKit on
Linux). A display is not required. The root library crate does not need that
toolchain.

The daemon prints its Unix socket address (or a loopback TCP fallback if Unix
bind fails). Send newline-delimited JSON such as
`{"id":"1","method":"ping","params":{}}`. This daemon does not browse.
See [docs/RUNBOOK-DEV.md](docs/RUNBOOK-DEV.md) for more build details.

## Shared capability for humans and agents

The desktop evidence panel reads the same `PageObservation` that agents receive from
`observe_page`: bounded page text/tables, source URL, runtime capabilities, target IDs
and explicit omissions. Its provider-free **Propose action** controls use the same
Rust policy and approval path as the model-driven agent.

The 22 browser tools include `observe_page`, `click_target`, `type_target`,
`submit_target` and `scroll_target` (24 tools with persistent memory attached).
Scoped actions require the exact document stamp and target ID from a current observation.
Optional URL/text postconditions produce typed action receipts. Dispatch acknowledgment,
page readiness and observed page conditions are distinct from website transaction success.

`ReActAgent::propose_tool_with_policy` executes a single proposal without a model call.
`execute_approved_tool_with_policy` rechecks the latest policy and reviewed state; an
approval ID alone grants nothing; grants expire after five minutes. The compatibility approval method can only reuse a
stored proposal's policy. See [CONTEXT](CONTEXT.md) and [ADR-002](docs/adr/ADR-002-shared-browser-capability.md).

The real WebKit corpus runs in `./verify.sh` on macOS. A separate real public HTTP
check is available with `cargo test --test capability_observation real_public_http_engine_observes_example_domain -- --ignored --exact`.

## Architecture

Shipped run/policy surface:

- `start_agent_run` evaluates model tool calls against `ActionPolicy`
  (`ReadOnly` / `Assisted` / `HighAutonomy`)
- `submit_approval` and `cancel_agent_run` resolve approval-gated actions
- every proposed, blocked, approved, rejected, and executed action is returned
  as a structured run event
- default autonomy is Assisted: reads, scrolling, and navigate that is not
  cross-domain can run; a hostless current page (`about:blank`, empty URL) is
  not cross-domain. Typing, form submission, high-impact actions, denylisted
  domains, and suspicious page content stop for approval or blocking
- headless methods: `ping`, `policy.get` / `policy.set` (`params` is the
  `ActionPolicy` object) / `policy.evaluate` (`params.tool` and
  `params.arguments`) / `snapshot`

React + Tauri is the primary frontend path
([ADR-001](docs/adr/ADR-001-react-tauri-primary.md)).
Agent contract: [docs/AGENT-SURFACE.md](docs/AGENT-SURFACE.md).
Unimplemented work: [CHANGELOG.md](CHANGELOG.md#still-unimplemented).

## Real Systems

- OpenAI: `OPENAI_API_KEY`, optional `OPENAI_MODEL`
- Anthropic: `ANTHROPIC_API_KEY`, optional `ANTHROPIC_MODEL`
- Ollama: local `OLLAMA_BASE_URL`, optional `OLLAMA_MODEL`

The Tauri shell fails closed if the desktop IPC bridge is unavailable. Browser and
agent flows should run through the real Tauri runtime, not a mocked browser page.

### Real-systems gap

The headless `snapshot` response is a hardcoded stub
`{ "url": "about:blank", "title": "", "viewport": { "width": 0, "height": 0, "scroll_x": 0, "scroll_y": 0 }, "tree": "" }`,
not a `PageSnapshot`. It does not read a browser. Its migration requires
attaching a real session/runtime, applying policy before tool execution, and
testing that connection against real pages. Until then use the Tauri runtime
for interactive browser work.
