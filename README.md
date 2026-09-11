# neurobrowser

**v0.1.1** — AI-native desktop browser: Rust library + Tauri/React shell.
Agents drive a real OS webview (or the scraper-backed headless daemon)
through a 12-tool surface gated by `ActionPolicy`. There is no custom
rendering engine; FastRender was never integrated.

See [SKILL.md](SKILL.md) and [docs/AGENT-SURFACE.md](docs/AGENT-SURFACE.md)
for the agent surface. Build and run: [docs/RUNBOOK-DEV.md](docs/RUNBOOK-DEV.md).

## Tech Stack

- **Two crates, no workspace:** `neurobrowser` (`src/`) and `neurobrowser-tauri` (`src-tauri/`)
- **Desktop (macOS only):** React + Vite shell, Tauri v2 commands, OS child webview per page
- **Library + headless:** Tokio, reqwest, `scraper` HTML parsing, serde, thiserror v2
- **Agent IPC:** `SKILL.md` / `docs/AGENT-SURFACE.md`; `neurobrowser-headless` JSON-RPC over UDS or TCP

## Quick Start

```bash
cargo build                # Library crate (neurobrowser)
cargo test                 # Unit + integration tests
cargo clippy --all-targets # Lint
cargo check --manifest-path src-tauri/Cargo.toml # Desktop crate type-check
cargo build --release      # Library release (LTO + strip + abort)
./verify.sh                # Full verification chain
```

Desktop (`npx tauri dev` in `src-tauri/`) is macOS. Headless daemon commands
live in [docs/RUNBOOK-DEV.md](docs/RUNBOOK-DEV.md).

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
  `policy.snapshot`, `snapshot`

React + Tauri is the primary frontend path
([ADR-001](docs/adr/ADR-001-react-tauri-primary.md)).
Lane comparison: [docs/frontend-architecture-spike.md](docs/frontend-architecture-spike.md).

Spec and acceptance trail:
[SPEC-AUTONOMOUS-BROWSER-AGENT](docs/specs/SPEC-AUTONOMOUS-BROWSER-AGENT.md),
[STORY-001](docs/stories/STORY-001-autonomous-browser-agent-core.md).

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
