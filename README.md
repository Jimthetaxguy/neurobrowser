# NeuroBrowser

AI-native desktop browser with a Rust browser/agent core and Tauri-native page
runtime.

## Current Status

NeuroBrowser is in an active React + Tauri implementation lane. This is intended
to run as its own desktop browser app, not as a Chrome-hosted web app. The Rust
crate owns browser/session/agent/provider behavior, while the Tauri shell exposes
a React control surface for tabs, navigation, provider selection, policy mode,
page snapshots, and approval-gated agent runs.

Rendered pages belong to the native desktop host. The React shell can be opened
in Chrome/Playwright only as a layout smoke test; that does not validate the real
Tauri child-webview browser runtime.

The accepted frontend direction is documented in
[docs/adr/ADR-001-react-tauri-primary.md](docs/adr/ADR-001-react-tauri-primary.md).
The AppKit lane remains a native-browser spike, not the primary implementation.

## Quick Start

```bash
cargo build
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
./verify.sh
```

Frontend-only commands live under `src-tauri/`:

```bash
cd src-tauri
npm ci
npm run dev
npm run build
npm run build:appkit
```

## Architecture

Core implementation areas:

- `src/browser/` - HTML fetching, parsing, DOM querying, and page extraction.
- `src/session/` - multi-session and page tracking.
- `src/tools/` - browser tool contracts and typed tool errors.
- `src/agent/` - autonomous run loop, streaming events, memory, observability,
  and action policy enforcement.
- `src/providers/` - OpenAI, Anthropic, and Ollama provider adapters.
- `src-tauri/src/` - React control surface and Tauri runtime bridge.
- `NeuroBrowser/` - Swift/AppKit spike for native host comparison.

## Autonomous Agent Core

The primary agent path is provider-agnostic and run-oriented:

- `start_agent_run` evaluates model tool calls against `ActionPolicy`
- `submit_approval` and `cancel_agent_run` resolve approval-gated actions
- every proposed, blocked, approved, rejected, and executed action is returned as a structured run event
- default autonomy is assisted: reads, snapshots, scrolling, and same-domain navigation can run; typing, form submission, high-impact actions, denylisted domains, and suspicious page content stop for approval or blocking

See [docs/specs/SPEC-AUTONOMOUS-BROWSER-AGENT.md](docs/specs/SPEC-AUTONOMOUS-BROWSER-AGENT.md)
and [docs/stories/STORY-001-autonomous-browser-agent-core.md](docs/stories/STORY-001-autonomous-browser-agent-core.md)
for the committed spec and acceptance trail.

## Verification

`./verify.sh` is the canonical local gate. It runs:

- `cargo fmt -- --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo test --all-targets`
- `npm ci && npm run build` in `src-tauri/`
- `cargo check --manifest-path src-tauri/Cargo.toml`
- `cargo build --release`

CI mirrors the Rust and Tauri checks and includes guards for mock dependencies,
merge conflicts, and iCloud duplicate files under `src/`.

Frontend smoke tests can load `http://127.0.0.1:5173/` in Playwright to catch
React/CSS regressions and fail-closed host-bridge states. Browser-runtime
validation must use the native Tauri app because Chrome does not exercise Tauri
IPC, child webviews, snapshots, keyboard routing, or native page focus.

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
