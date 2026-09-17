# neurobrowser

**v0.1.1** — AI-native desktop browser: Rust library + Tauri/React shell.
The desktop drives a real OS webview through `ActionPolicy`-gated tools.
The Rust library also supplies an HTTP scraper. The headless daemon is a
policy JSON-RPC protocol stub: its `about:blank` snapshot is hardcoded and
it cannot navigate or execute browser tools. There is no custom rendering
engine; FastRender was never integrated.

See [SKILL.md](SKILL.md) and [docs/AGENT-SURFACE.md](docs/AGENT-SURFACE.md)
for the agent surface. Build and run: [docs/RUNBOOK-DEV.md](docs/RUNBOOK-DEV.md).

## Tech Stack

- **Two crates, no workspace:** `neurobrowser` (`src/`) and `neurobrowser-tauri` (`src-tauri/`)
- **Desktop (macOS only):** React + Vite shell, Tauri v2 commands, OS child webview per page
- **Library:** Tokio, reqwest, `scraper` HTML parsing, serde, thiserror v2
- **Headless protocol stub:** Tokio policy evaluation and a hardcoded snapshot
- **Agent IPC:** `SKILL.md` / `docs/AGENT-SURFACE.md`; `neurobrowser-headless` JSON-RPC over UDS or TCP

## Quick Start

```bash
cargo build                # Library crate (neurobrowser)
cargo test                 # Unit + integration tests
cargo clippy --all-targets # Lint
cargo build --release      # Library release (LTO + strip + abort)
./verify.sh                # Full verification chain
```

Desktop (`npx tauri dev` in `src-tauri/`) is macOS. Full `./verify.sh` also
type-checks the Tauri crate (macOS, or GTK/WebKit on Linux); see
[docs/RUNBOOK-DEV.md](docs/RUNBOOK-DEV.md).

Start the headless policy stub on Unix:

```bash
NEUROBROWSER_SOCKET="$HOME/.neurobrowser/daemon.sock" \
  cargo run --manifest-path src-tauri/Cargo.toml --features headless --bin neurobrowser-headless
```

It prints its Unix socket address (or a loopback TCP fallback if Unix bind
fails). Send newline-delimited JSON such as
`{"id":"1","method":"ping","params":{}}`. This daemon does not browse.
See [docs/RUNBOOK-DEV.md](docs/RUNBOOK-DEV.md) for more build details.

## Architecture

Shipped run/policy surface:

- `start_agent_run` evaluates model tool calls against `ActionPolicy`
  (`ReadOnly` / `Assisted` / `HighAutonomy`)
- `submit_approval` and `cancel_agent_run` resolve approval-gated actions
- every proposed, blocked, approved, rejected, and executed action is returned
  as a structured run event
- default autonomy is Assisted: reads, snapshots, scrolling, and same-domain
  navigation can run; typing, form submission, high-impact actions, denylisted
  domains, and suspicious page content stop for approval or blocking
- headless methods: `ping`, `policy.get` / `policy.set` / `policy.evaluate` /
  `snapshot`

React + Tauri is the primary frontend path
([ADR-001](docs/adr/ADR-001-react-tauri-primary.md)).
Agent contract: [docs/AGENT-SURFACE.md](docs/AGENT-SURFACE.md).
Unimplemented work: [CHANGELOG.md](CHANGELOG.md#still-unimplemented).

## Documentation Workflow

Shared specs, stories, and architecture decisions are committed under `docs/specs/`,
`docs/stories/`, and `docs/adr/`. Local process notes stay under
`docs/notes/local/` and are ignored. See [docs/notes/README.md](docs/notes/README.md)
for the promotion rules and local worklog path.

## Real Systems

- OpenAI: `OPENAI_API_KEY`, optional `OPENAI_MODEL`
- Anthropic: `ANTHROPIC_API_KEY`, optional `ANTHROPIC_MODEL`
- Ollama: local `OLLAMA_BASE_URL`, optional `OLLAMA_MODEL`

The Tauri shell fails closed if the desktop IPC bridge is unavailable. Browser and
agent flows should run through the real Tauri runtime, not a mocked browser page.

### Real-systems gap

The headless `snapshot` response is a protocol fixture; it does not read a
browser. Its migration requires attaching a real session/runtime, applying
policy before tool execution, and testing that connection against real
pages. Until then use the Tauri runtime for interactive browser work.
