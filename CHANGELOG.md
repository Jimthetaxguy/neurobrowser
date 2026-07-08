# Changelog

All notable changes to NeuroBrowser are recorded here. Dates are UTC.

The format is loosely Keep-a-Changelog: Added / Changed / Fixed / Removed
sections per release.

## [0.1.1] — 2026-07-08

The first release after the `codex-live-tauri-browser-runtime` branch was
merged into `main`. Captures Phases A–F of the v0.1 roadmap. The pre-merge
history lives on `codex-live-tauri-browser-runtime` and the prior `main`
contained only the v0.1.0 foundation snapshot.

### Added
- **Tauri desktop shell** (`src-tauri/`) with React frontend: tab strip,
  URL bar, chat panel, command palette, settings drawer, streaming agent
  event timeline, ActionPolicy panel.
- **24 Tauri commands**: `ask`, `browser_back`, `browser_forward`,
  `browser_reload`, `browser_runtime_report`, `cancel_agent_run`,
  `close_page`, `create_page`, `create_session`, `get_action_policy`,
  `get_page_info`, `get_page_snapshot`, `get_worker`, `list_sessions`,
  `list_workers`, `navigate`, `set_active_page`, `set_action_policy`,
  `set_provider`, `start_agent_run`, `submit_approval`,
  `sync_browser_viewport`, `validate_url`, `wait_for_page_ready`.
- **Agent-facing tool surface** (`docs/AGENT-SURFACE.md`): 12 tools
  (`snapshot`, `click`, `type_text`, `submit_form`, `query_selector`,
  `evaluate`, `navigate`, `get_text`, `get_attribute`, `wait_for`,
  `extract_text`, `screenshot`) with ref-based interaction (`@eN`
  references resolved through `ref_map`).
- **`SKILL.md`** at repo root — agent-loadable invocation spec.
- **ActionPolicy** (`src/agent/policy.rs`): three autonomy levels
  (`ReadOnly` / `Assisted` / `HighAutonomy`), per-domain allow/deny lists,
  sensitive-arg redaction, prompt-injection detection, three outcomes
  (`Allow` / `RequireApproval` / `Deny`).
- **Streaming agent** (`src/agent/streaming.rs`): `StreamingAgent` trait
  + `ReActAgent` impl that emits `StreamEvent` over a channel.
- **AgentMemory** (`src/agent/memory.rs`): episodic, semantic, and state
  memories. `ReActAgent` owns a `Mutex<AgentMemory>` and pushes
  `LlmCall` / `ToolCall` / `Navigation` events.
- **AgentMetrics** (`src/agent/observability.rs`): process-global
  singleton, `MetricsSnapshot`, per-tool counts, correlation spans.
- **Conversation window** (`src/agent/mod.rs`): bounded `VecDeque<AgentMessage>`
  of capacity 20 — replaces the unbounded `Vec` that hid context drift.
- **Worker model** (`src/agent/worker.rs` + `src/session/mod.rs`):
  `WorkerSpec`, `WorkerHandle`, `WorkerSummary`, `WorkerSnapshot`,
  `WorkerStatus`, `WorkerMessage`, `WorkerMessageKind`,
  `CrossWorkerObservations`. `SessionManager` exposes `spawn_worker`,
  `list_workers`, `get_worker`, `set_worker_status`, `send_message`,
  `drain_inbox`, `cross_worker_observations`, `record_observation`.
- **Headless daemon** (`src-tauri/src/bin/headless.rs`): UDS/TCP service
  dispatching `ping`, `policy.get`, `policy.set`, `policy.evaluate`,
  `snapshot`, `policy.snapshot`. Gated by the `headless` feature flag.
- **Reference docs** (`docs/`): `RUNBOOK-DEV.md` (build/dev), `TESTING-NOTES.md`
  (automated + manual verification), `AGENT-SURFACE.md` (tool spec),
  `references/prior-art.md` (vercel-labs/agent-browser, AIAnytime/agent-browser,
  hyperbrowser-app-examples, fastrender).
- **verify.sh**: one-shot green build covering fmt + clippy `-D warnings` +
  lib + integration + Tauri frontend + headless release build.
- **Test coverage**: 50 tests across 8 files (`action_policy`,
  `agent_memory_metrics`, `browser_engine`, `concurrency`, `session`,
  `streaming_agent`, `tools`, `workers`).

### Changed
- `ReActAgent::execute_with_policy` now seeds the conversation with the
  user prompt, records metrics at each step, pushes episodic events, and
  derives `current_prompt` from `build_context` instead of overwriting it
  after every tool call.
- `SessionManager::create_page` reorders mutex acquisition
  (`page_counter` before `sessions`) to break a deadlock observed in
  `tests/concurrency.rs`.
- `src-tauri/src/runtime.rs` gained `buildSelector`, `elementToXPath`,
  `serializeElement` (now returns `xpath`), `collectRefMap` (assigns
  `@eN` refs to interactive elements), and `clickRef` / `typeTextRef` /
  `submitFormRef` methods.
- `src-tauri/build.rs` `app_manifest.commands` is now in sync with the
  `invoke_handler!` macro — was the source of the "Permission not found"
  build error.
- `PROJECT.md` updated to v0.1.1 (delta block + corrected status +
  resolved blockers + v0.2 roadmap).

### Fixed
- Duplicate `BrowserTool` trait impls on `main` were never imported; the
  merged tree resolves to a single definition in `src/tools/mod.rs`.
- `Cargo.toml` gains `tempfile = "3"` dev-dep for tests that need scratch
  directories.
- `tokio` features in `src-tauri/Cargo.toml` set to `["full", "net",
  "io-util", "macros", "rt-multi-thread", "sync"]` — required by the
  headless daemon's UDS/TCP listeners.

### Deferred to v0.2
- Native function-calling in the ReActAgent loop (parse_native exists;
  the prompt does not yet ask providers to emit structured JSON).
- React Workers sidebar (E5).
- Headless daemon worker fan-out commands (E6).
- Real-LLM integration tests (currently unit tests use a stub provider;
  a real-provider test is gated behind an Infisical budget).

## [0.1.0] — 2026-02-23

Foundation snapshot on `main`:
- Core library (`src/`): AI providers (OpenAI/Anthropic/Ollama), ReAct
  agent, BrowserEngine (scraper), SessionManager, DOM Tools.
- Tauri shell stubs but no compile (missing icons, config path issues).