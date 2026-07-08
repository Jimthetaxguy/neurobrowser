# NeuroBrowser — Development Runbook

How to build, run, test, and develop NeuroBrowser locally. Updated 2026-07-08
during Phase B of the v0.1 roadmap.

## Prerequisites

- **Rust** stable toolchain (1.96.0 confirmed working; pin in `rust-toolchain.toml`
  if needed).
- **Node.js** + **npm** (Vite 6.4 frontend).
- **macOS** (the merged tree's icon set + CSP are macOS-flavored; Windows/Linux
  builds are deferred to v0.2 — see `docs/ROADMAP-v0.2.md`).
- For Apple Silicon: Xcode command-line tools for `swift-rs` (the build pulls in
  swift-rs via the AppKit spike under `NeuroBrowser/`; the spike is not required
  to compile the main `src-tauri` binary).

## One-shot green build

From repo root:

```bash
chmod +x verify.sh   # only needed the first time
./verify.sh
```

This runs the full verification chain (per `verification-before-completion`):

1. `cargo fmt -- --check` — formatting.
2. `cargo clippy --all-targets -- -D warnings` — lints as errors.
3. `cargo test --all-targets` — runs 17 lib tests + 11 integration tests
   (`tests/{action_policy,autonomous_agent,error_types,streaming}.rs`).
4. `cd src-tauri && npm ci && npm run build` — builds the Vite frontend.
5. `CARGO_TARGET_DIR=/tmp/neurobrowser-tauri-target cargo check --manifest-path
   src-tauri/Cargo.toml` — type-check the Tauri shell.
6. `cargo build --release` — full release build.

Expected output ends with `=== All checks passed ===`.

## Iteration loop

For day-to-day development, the slow `cargo build --release` is overkill. Use:

```bash
cd src-tauri
cargo tauri dev --no-watch
```

This:

1. Runs `npm run dev` (Vite dev server on `http://localhost:5173/`).
2. Compiles the Tauri binary in dev profile.
3. Launches the desktop window.
4. Hot-reloads on Rust file changes (via `--no-watch` you control it; remove the
   flag for full auto-rebuild on every save).

The first `cargo tauri dev` run takes 3-5 minutes for a clean build
(471 crates). Subsequent runs are incremental (5-30s).

## Running tests

```bash
cargo test --all-targets
```

Integration tests:

- `tests/action_policy.rs` — `ActionPolicy` deny-wins-over-allow, assisted-mode
  click approval, sensitive-arg redaction, prompt-injection blocking.
- `tests/autonomous_agent.rs` — end-to-end ReAct loop with a mocked provider.
- `tests/error_types.rs` — error conversion paths.
- `tests/streaming.rs` — `StreamingAgent` / `StreamEvent` wiring.

Library tests are colocated (`src/**/*_test.rs` if any, plus `#[cfg(test)] mod
tests` blocks inside source files).

## Tauri IPC surface

The desktop app exposes 22 Tauri commands (see `src-tauri/src/main.rs:574`).
From the React frontend, call them via:

```javascript
import { invoke } from "@tauri-apps/api/core";

const sessionId = await invoke("create_session");
const pageId = await invoke("create_page", { sessionId });
await invoke("navigate", { sessionId, pageId, url: "https://example.com" });
const snapshot = await invoke("get_page_snapshot", { sessionId, pageId });
const result = await invoke("start_agent_run", { sessionId, pageId, prompt: "Summarize this page" });
```

For typed wrappers, see `src-tauri/src/hostAdapters.js` — it defines
`createTauriHostAdapter()` which wraps every command as an async function.

## Agent-facing IPC surface (for external agents)

External agents (ROSA, Claude Code, custom workers) drive NeuroBrowser via:

- **In-process** (preferred): call the Rust `neurobrowser::*` API directly.
  See `src/agent/mod.rs::ReActAgent::execute` and
  `src-tauri/src/runtime.rs::TauriBrowserRuntime`.
- **Headless daemon** (Phase D4): connects over Unix domain socket. Not yet
  shipped; will live at `src-tauri/src/bin/headless.rs`.

The full surface spec lives in `docs/AGENT-SURFACE.md` (Phase D3).

## Tauri capabilities (security)

`src-tauri/capabilities/main.json` enumerates 22 permissions, one per command,
plus `core:default` and `shell:allow-open`. **Do not** add a wildcard
permission — every command must be individually allow-listed so the user can
audit exactly what the frontend can invoke.

If you add a new Tauri command:

1. Define the command in `src-tauri/src/main.rs`.
2. Add it to `tauri::generate_handler!` (line 574).
3. Add `allow-<command-name>` to `src-tauri/capabilities/main.json`.
4. Add a wrapper in `src-tauri/src/hostAdapters.js`.

## verify.sh failures and what to do

| Step | Symptom | Fix |
|---|---|---|
| `cargo fmt --check` | diff output | `cargo fmt` to auto-fix, re-run verify.sh |
| `cargo clippy` | warnings-as-errors | read the warning, fix the underlying code, re-run |
| `cargo test` | failed assertions | read the trace, fix the failing test or the code under test |
| `npm ci && npm run build` | Vite error | check `src-tauri/src/*.{jsx,js}` syntax; usually a missing import |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Tauri compile error | usually a missing icon or capability — re-read A5 of the v0.1 roadmap |
| `cargo build --release` | linker / symbol error | clear `target/` and re-run; check `rustc --version` matches the lock |

## Pre-commit hooks

None are installed by default. Recommended before pushing:

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test --all-targets
```

This is a subset of `verify.sh` and will catch ~90% of issues pre-push.

## Environment variables

The Tauri shell reads these at boot:

| Variable | Used by | Default |
|---|---|---|
| `OPENAI_API_KEY` | `set_provider("openai")` | (none — required) |
| `OPENAI_MODEL` | `set_provider("openai")` | `gpt-4o` |
| `ANTHROPIC_API_KEY` | `set_provider("anthropic")` | (none — required) |
| `ANTHROPIC_MODEL` | `set_provider("anthropic")` | `claude-3-5-sonnet-latest` |
| `OLLAMA_BASE_URL` | `set_provider("ollama")` | (none — Ollama defaults to `http://localhost:11434`) |
| `OLLAMA_MODEL` | `set_provider("ollama")` | `llama3.2` |
| `CUSTOM_PROVIDER_API_KEY` | `set_provider("custom")` | falls back to `OPENAI_API_KEY` |
| `CUSTOM_PROVIDER_BASE_URL` | `set_provider("custom")` | (none — required for custom) |
| `CUSTOM_PROVIDER_MODEL` | `set_provider("custom")` | `gpt-4o` |

For production, source these from Infisical (per `real-systems-only` rule). Local
dev typically uses a `.env` file (which is gitignored).

## Architecture at a glance

```
┌─────────────────────────────────────────────────────────────────────┐
│                      NeuroBrowser Desktop                            │
│                                                                      │
│  ┌──────────────────────────┐   ┌──────────────────────────────┐    │
│  │   React frontend         │   │   Tauri child webviews        │    │
│  │   (src-tauri/src/App.jsx)│   │   (per page, real WKWebView)  │    │
│  │                          │   │                                │    │
│  │   - Sidebar / chat       │   │   - RUNTIME_INIT_SCRIPT        │    │
│  │   - Tab strip            │◄──┤   - window.__NEUROBROWSER_     │    │
│  │   - URL bar              │IPC│     RUNTIME__ (snapshot, click,│    │
│  │   - Action history       │   │     type, submit, scroll)     │    │
│  └────────────┬─────────────┘   └────────────────┬───────────────┘    │
│               │                                  │ JSON-RPC           │
│               │ invoke('start_agent_run', …)     │                    │
│               ▼                                  ▼                    │
│  ┌────────────────────────────────────────────────────────────────┐  │
│  │                  src-tauri/src/main.rs                         │  │
│  │  - 22 Tauri commands                                           │  │
│  │  - ActionPolicy gating                                        │  │
│  │  - SessionManager + BrowserRuntimeRegistry                     │  │
│  └────────────┬───────────────────────────────────────────────────┘  │
│               │                                                      │
│               ▼                                                      │
│  ┌────────────────────────────────────────────────────────────────┐  │
│  │                  neurobrowser (lib crate)                       │  │
│  │  - ReActAgent (agent/mod.rs)                                   │  │
│  │  - ActionPolicy (agent/policy.rs)                              │  │
│  │  - BrowserEngine (browser/mod.rs)                              │  │
│  │  - AiProvider trait + OpenAI/Anthropic/Ollama impls            │  │
│  │  - ToolRegistry + 14 tools                                     │  │
│  └────────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────┘
```

## Where things live

| Layer | Path |
|---|---|
| Library crate | `src/` |
| Tauri shell | `src-tauri/` |
| React frontend | `src-tauri/src/App.jsx` + `hostAdapters.js` |
| Tauri IPC bridge | `src-tauri/src/main.rs` |
| Webview JS bridge | `src-tauri/src/runtime.rs` (`RUNTIME_INIT_SCRIPT`) |
| AppKit Swift spike | `NeuroBrowser/` (deferred — see `docs/SPIKES.md`) |
| Specs / Stories / ADRs | `docs/specs/`, `docs/stories/`, `docs/adr/` |
| Verification | `verify.sh` |
| Tests | `tests/` |
| Archive | `_archive-2026-07-01-L1/` (untracked) |

## See also

- `PROJECT.md` — vision, roadmap, prior art references.
- `docs/references/prior-art.md` — what NeuroBrowser takes / leaves from
  agent-browser, hyperbrowser-app-examples, etc.
- `docs/specs/` — detailed spec docs (SPEC-AUTONOMOUS-BROWSER-AGENT, etc.).
- `docs/stories/` — user stories.
- `docs/adr/` — architecture decision records.
- `verify.sh` — green-build chain.