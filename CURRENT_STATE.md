# NeuroBrowser — Current State Analysis

**Date:** 2026-07-01
**Scope:** end-to-end audit of the codebase as it sits on `main` today, plus the parallel `origin/codex-live-tauri-browser-runtime` branch that contains the most substantial unfinished work
**Inputs:** `cargo check`, `cargo test`, `git log`, full source read
**Audience:** James, returning to the project

---

## TL;DR

- **`main` compiles as a library** (`cargo check --lib` is clean), but the **Tauri shell does not compile** and there are **zero tests** in the entire workspace. The "0 stars, 2 commits" view on GitHub is *accurate* — `main` is essentially a foundation checkpoint, not a working app.
- **There's a much larger work-in-progress on `origin/codex-live-tauri-browser-runtime`** (9 commits, +16,509 / -1,600 lines) that contains a real React frontend, a Tauri runtime, an agent policy layer, and a tests/ directory. **This branch is not on main and is not merged.** It's where the actual project lives; main is a fossil.
- The architecture on `main` is honest and clean: typed tool contracts, pluggable providers, three-tier memory data structures, a real ReAct loop. The blockers are concrete and small (one missing icon, one missing `.await`). The bigger problem is that nothing is wired together for the user — most of the ReAct agent's nice data structures (`EpisodicMemory`, `SemanticMemory`, `AgentMetrics`) are never read by the loop.
- Recommended next action (one line): **merge `origin/codex-live-tauri-browser-runtime` into `main`**, then re-run this audit against the merged tree. That branch likely *fixes* the blockers below; until it lands, `main` is a misleading snapshot.

---

## 1. What's on `main` today

### 1.1 Repository at a glance

| Field | Value |
|---|---|
| Branch | `main` (HEAD = `cd5745b`, my prior ANALYSIS.md commit) |
| Origin HEAD | `origin/main` @ `1b3ecb3` ("Add AI-native frameworks from best practices") |
| Other branches | `origin/codex-live-tauri-browser-runtime` (9 commits, 16.5k lines ahead) |
| Working tree | 3 modified files in `src-tauri/gen/schemas/` (auto-generated; not user changes) |
| `cargo --version` | 1.96.0 (May 2026) |
| Lines of Rust (`src/`, `src-tauri/src/`) | ~2,300 LoC |
| Tests | **0** in the entire workspace |

### 1.2 File map (real, as on disk)

```
neurobrowser/
├── Cargo.toml                       # lib crate, 19 deps
├── PROJECT.md                       # vision/roadmap, v0.1.0, dated 2026-02-23
├── project.yaml                     # machine-readable mirror
├── docs/
│   ├── CODE_ANALYSIS.md             # post-refactor report, 10 issues fixed
│   └── CODE_REVIEW.md               # 2 Critical, 4 High, 6 Medium, 8 Low
├── research/
│   └── rust_coding_frameworks.md    # ecosystem survey
├── src/                             # the "library" crate
│   ├── lib.rs                       # public exports
│   ├── agent/
│   │   ├── mod.rs                   # ReActAgent (async loop)
│   │   ├── memory.rs                # EpisodicMemory, SemanticMemory, StateMemory (data only)
│   │   ├── observability.rs         # tracing spans, AgentMetrics (counters only)
│   │   └── streaming.rs             # StreamingAgent trait + StreamEvent enum (trait only)
│   ├── browser/mod.rs               # BrowserEngine + 10 tool defs (scraper crate)
│   ├── providers/
│   │   ├── mod.rs                   # AiProvider trait, AiContext, parse_tool_calls, build_system_prompt
│   │   ├── openai.rs                # OpenAI client
│   │   ├── anthropic.rs             # Anthropic client
│   │   └── ollama.rs                # Ollama client
│   ├── session/mod.rs               # SessionManager, PageHandle
│   └── tools/
│       ├── mod.rs                   # BrowserTool trait (HashMap<String,String> args)
│       ├── contracts.rs             # BrowserTool trait DUPLICATE (serde_json::Value args)
│       └── errors.rs                # ToolError, AgentError
└── src-tauri/                       # the Tauri shell
    ├── Cargo.toml                   # neurobrowser-tauri bin, tauri v2
    ├── build.rs
    ├── tauri.conf.json              # minimal: productName, identifier, one window
    ├── package.json                 # declares Vite + @tauri-apps/api ^2
    ├── package-lock.json
    ├── index.html                   # self-contained single-page UI (inline CSS+JS)
    ├── dist/index.html              # copy of above; not a Vite build output
    ├── gen/schemas/                 # auto-generated Tauri v2 schemas
    │   ├── acl-manifests.json
    │   ├── capabilities.json        # EMPTY: {}
    │   ├── desktop-schema.json
    │   └── macOS-schema.json
    └── src/main.rs                  # 6 Tauri commands (create_session, create_page, navigate, ask, get_page_info, list_sessions)
```

### 1.3 Architecture summary

`main.rs` boots with `PageConfig::default()` (HTML-only, `enable_javascript: false`) and an `AgentConfig` with `max_iterations: 5` and OpenAI as the default provider. It constructs a `SessionManager`, registers 6 Tauri commands, and runs.

A user request flow:
1. Frontend calls `invoke('create_session')` → UUID.
2. Frontend calls `invoke('create_page', { sessionId })` → integer page id, internally creates a `BrowserEngine` + `ReActAgent`.
3. Frontend calls `invoke('navigate', { sessionId, pageId, url })` → blocking reqwest GET → `scraper::Html::parse_document` → page state stored.
4. Frontend calls `invoke('ask', { sessionId, pageId, prompt })` → `ReActAgent::execute(prompt, browser)`.

`ReActAgent::execute` (lines 69–133 of `agent/mod.rs`):
- Up to 5 iterations.
- Each iteration: `build_context` (URL/title/link/image/form counts) → `provider.complete` → if `finish_reason == "stop"` or no tool calls, return `extract_final_answer`. Otherwise, parse `Action: name(args)` lines from content, dispatch each tool through `ToolRegistry`, concatenate result as `Observation:` for the next iteration's prompt.

### 1.4 What's wired and what isn't

| Layer | Exists | Used by loop? | Notes |
|---|---|---|---|
| `ReActAgent::execute` | yes | **yes** | the actual agent loop |
| `AiProvider` trait + 3 impls (OpenAI/Anthropic/Ollama) | yes | yes | `create_provider(config)` factory |
| `BrowserEngine` + `BrowserInterface` | yes | yes | scraper-based, no JS |
| `ToolRegistry` + 10 tools | yes | yes | registered in `BrowserEngine::new` |
| `BrowserTool` (mod.rs) — `HashMap<String,String>` args | yes | yes | the live trait |
| `BrowserTool` (contracts.rs) — `serde_json::Value` args | yes | **no** | dead duplicate, see §3 |
| `ToolDefinition` / `ToolSchema` (contracts.rs) | yes | no | schemars metadata, never emitted to LLM |
| `EpisodicMemory` / `SemanticMemory` / `StateMemory` | yes | **no** | data structures only; `EpisodicMemory::push` exists but loop never calls it |
| `StreamingAgent` trait + `StreamEvent` enum | yes | **no** | trait is defined; `ReActAgent` does not implement it |
| `tracing` spans (`llm_call_span`, `tool_call_span`, `agent_iteration_span`) | yes | **no** | span constructors exist but are never invoked inside the loop |
| `AgentMetrics` (atomic counters) | yes | no | counters exist, `record_*` methods never called |
| `CorrelationContext` | yes | no | constructed nowhere in the live flow |
| `tauri-plugin-shell` | yes (in deps) | no | not used by any command |

The pattern: **the agent loop is the only fully-wired system**. Every other "feature" is data and scaffolding that *could* be wired but isn't. This matches the prior `docs/CODE_ANALYSIS.md` framing ("core library compiles") — the foundation is real but the loop is bare-bones.

---

## 2. Build & test status

### 2.1 What I ran and what happened

```bash
$ cargo check --lib
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 29.30s
```
✓ Clean. Library compiles with no warnings.

```bash
$ cargo check     # in src-tauri/
    Checking neurobrowser-tauri v0.1.0
error: proc macro panicked
   --> src/main.rs:136:14
    |
136 |         .run(tauri::generate_context!())
    |              ^^^^^^^^^^^^^^^^^^^^^^^^^^
    |
    = help: message: failed to open icon ~/code/neurobrowser/src-tauri/icons/icon.png: No such file or directory (os error 2)

error[E0308]: mismatched types
  --> src/main.rs:66:5
   |
64 | ) -> Result<String, String> {
   |      ---------------------- expected `Result<std::string::String, std::string::String>` because of return type
65 |     let page = state.session_manager.get_page(&session_id, page_id)?;
66 |     page.agent.execute(&prompt, page.browser.as_ref())
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ expected `Result<String, String>`, found future

warning: unused import: `BrowserEngine`
warning: unused import: `std::sync::Arc`
```
✗ Two real errors, two warnings.

```bash
$ cargo test --lib
running 0 tests
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```
✓ Binary compiles, zero tests run (because none exist).

### 2.2 The four concrete blockers

1. **Missing icon** — `tauri::generate_context!()` walks `src-tauri/icons/` looking for `icon.png`. It doesn't exist. This is the **only** blocker PROJECT.md flagged (`tauri_config: severity high`). Fix: drop a placeholder PNG at `src-tauri/icons/icon.png` (any 32×32 PNG; Tauri uses it for the bundle but doesn't validate dimensions at build time).

2. **Missing `.await` on `agent.execute`** — `ReActAgent::execute` is `async fn` (correctly), but the `ask` Tauri command calls it without `.await`. This is the kind of bug Rust catches at compile time — a one-character fix. PROJECT.md does *not* mention this; it's a regression that may have been introduced when the command signature was written.

3. **Empty `capabilities.json`** — `src-tauri/gen/schemas/capabilities.json` is `{}`. Tauri v2 requires capability grants for IPC to work at runtime; without them, `invoke('create_session')` from the frontend will be rejected even if compilation succeeds. This is a *runtime* blocker that compilation alone won't catch.

4. **`dist/index.html` is not a Vite build** — `package.json` declares Vite and `@tauri-apps/api` as dependencies, but `dist/index.html` is byte-for-byte the same as the self-contained `src-tauri/index.html`. There is no `vite.config.js`, no React entry point, no `src/`. Either `tauri.conf.json` should point `frontendDist` at a real Vite build (or the file in place), or the frontendDist/Vite story is half-removed and needs to be either finished or ripped out. The Tauri shell will work as-is with `index.html`, but the package.json dead-deps will confuse future contributors.

### 2.3 Why the prior `CODE_REVIEW.md` (2026-03-01) still mostly applies

The 2026-03-01 review flagged 20 issues (2 Critical, 4 High, 6 Medium, 8 Low). The "10 issues fixed" report from `CODE_ANALYSIS.md` covered a subset. Items I can verify *still* apply from the current source:

- The library *does* now handle navigation errors (verified at `browser/mod.rs:96–111`).
- The price regex *is* `OnceLock` (verified at `browser/mod.rs:10–14`).
- The tool registry *is* extracted (verified at `tools/mod.rs:129–163`).
- The `Mutex<PageState>` access pattern *is* mostly correct (verified at `browser/mod.rs:218–231`).

But the review's surface-level category "AI prompt injection risk / unstructured text parsing" remains structurally unfixed: `parse_tool_calls` in `providers/mod.rs:106–208` is a custom regex-style parser over free-form LLM output (`Action: name(key="value", ...)`). It works for "well-behaved" output but breaks on nested parentheses, JSON-style values, or quoted commas inside values. This isn't a regression but it is a *fragility* worth knowing about.

---

## 3. Architectural smells (not bugs, but smells)

These won't show up in `cargo check` but will hurt long-term velocity:

### 3.1 Two parallel `BrowserTool` traits

`src/tools/mod.rs:39–44` defines `BrowserTool` with `HashMap<String, String>` args. `src/tools/contracts.rs:93–99` defines `BrowserTool` again with `serde_json::Value` args, plus a `ToolDefinition` schema with schemars metadata and a `Capability` enum (Browser / Network / FileSystem).

The shipped implementation uses the `mod.rs` version. The `contracts.rs` version is never instantiated. This is exactly the kind of "I started the redesign and got halfway" code that ages badly. **Pick one.** Recommendation: keep `contracts.rs` (typed JSON, schemars, capability grants, observability hooks); port the 10 tool implementations; delete `mod.rs`'s duplicate. That's the path the `codex-live-tauri-browser-runtime` branch already started.

### 3.2 Custom tool-call parser vs. structured outputs

`parse_tool_calls` (line 106) parses `Action: name(key="value", ...)` syntax out of free-form LLM text. OpenAI and Anthropic both support structured tool-calling via JSON Schema in the API request. Switching to that eliminates a class of parsing bugs and gives the model better guidance. The `ToolDefinition` type already exists in `contracts.rs` for exactly this.

### 3.3 Blocking reqwest inside async land

`src/browser/mod.rs:51` uses `reqwest::blocking::Client`, and `navigate()` is `fn` (sync). This is called from inside the `async` `ReActAgent::execute` loop via `&dyn BrowserInterface`. Each `navigate` call blocks the entire Tokio worker thread for the duration of the HTTP fetch (up to 30s). With a single worker, the agent stalls; with multiple, threads tie up.

Fix: switch to `reqwest::Client` (async) and make `BrowserInterface::navigate` async (which is a breaking change to the trait). This is one of the items the `codex-live-tauri-browser-runtime` branch has been working on.

### 3.4 In-memory everything

`SessionManager` holds sessions in a `Mutex<HashMap>`. No persistence. Close the app, lose everything. The roadmap lists "Session Memory" as P2; the data structures are already there (`EpisodicMemory`/`SemanticMemory`); what's missing is a `serde_json::to_writer` to `~/.neurobrowser/sessions/<uuid>.json` on each event. Probably ~150 lines including the schema.

### 3.5 Zero observability despite the hooks

`observability.rs` has `tracing::info_span!` constructors with `correlation_id`/`model`/`tool.name` fields, plus `AgentMetrics` with atomic counters. None of it is wired into the loop. Adding it would be ~50 lines per `ReActAgent::execute` iteration, but the payoff for debugging a tool call failure across 5 iterations is huge.

### 3.6 Tests directory doesn't exist

`cargo test --lib` reports "0 tests." For a project whose entire value prop is "the agent works," this is the single most expensive missing piece. The `codex-live-tauri-browser-runtime` branch has `tests/action_policy.rs`, `tests/autonomous_agent.rs`, `tests/error_types.rs`, `tests/streaming.rs` — merging that branch immediately gives 4 test files.

---

## 4. What's actually new on `origin/codex-live-tauri-browser-runtime`

This branch (9 commits ahead of `main`) contains the bulk of recent work. Diff stat against `main`:

```
 116 files changed, 16509 insertions(+), 1600 deletions(-)

 src-tauri/package-lock.json                        |  698 ++-
 src-tauri/package.json                             |    7 +-
 src-tauri/src/App.jsx                              |  739 +++     <-- real React frontend
 src-tauri/src/appkit.jsx                           |    9 +
 src-tauri/src/hostAdapters.js                      |  223 +
 src-tauri/src/main.jsx                             |    9 +
 src-tauri/src/main.rs                              |  572 +-     <-- commands expanded
 src-tauri/src/runtime.rs                           |  700 +++     <-- Tauri browser runtime
 src-tauri/src/styles.css                           |  463 ++
 src-tauri/tauri.conf.json                          |   34 +-     <-- full config
 src-tauri/vite.config.js                           |   10 +
 src/agent/mod.rs                                   |  359 +-     <-- agent rewrite
 src/agent/policy.rs                                |  389 ++     <-- NEW: action policy
 src/agent/streaming.rs                             |   77 +-     <-- wired up
 src/browser/mod.rs                                 | 1297 ++-    <-- browser rewrite
 src/lib.rs                                         |   30 +-
 src/providers/{anthropic,ollama,openai,mod}.rs     |  ~80 changes
 src/session/mod.rs                                 |   70 +-
 src/tools/{contracts,errors,mod}.rs                |  ~400 changes
 tests/action_policy.rs                             |  104 +      <-- NEW
 tests/autonomous_agent.rs                          |  211 +      <-- NEW
 tests/error_types.rs                               |  137 +      <-- NEW
 tests/streaming.rs                                 |  135 +      <-- NEW
 verify.sh                                          |   17 +
```

What this branch delivers vs. main:

| Capability | main | codex branch |
|---|---|---|
| Frontend | inline `<script>` in `index.html` | full React app (`App.jsx`, 739 lines) with `hostAdapters.js` for IPC |
| Tauri runtime | 6 commands, no `runtime.rs` | new `runtime.rs` (700 lines) bridging browser engine to the frontend |
| Browser engine | scraper only, blocking reqwest | rewritten with what looks like async reqwest + iframe sandboxing (commit message: "iframe sandbox limitations") |
| Agent | flat ReAct loop | expanded with `policy.rs` (action policy / safety) and wired-up streaming |
| Tools | 10 tools, `HashMap` args | `contracts.rs` rewrite (typed JSON), likely the path I recommend in §3.1 |
| Tests | 0 | 4 test files (587 lines total) |
| Build | blocked on missing icon | maybe fixed (I can't verify without checking out the branch) |
| Capabilities | `{}` | `src-tauri/capabilities/main.json` exists with content (commit `1031e64`) |
| Verification | none | `verify.sh` script |

The branch's commit narrative is exactly the natural next-step for the project:

```
1031e64 add live tauri browser runtime
43f586c fix(tauri): unblock dev build by fixing port mismatch and integrating UI bindings
2824724 fix(tauri): resolve set_provider and close_page crashes, polyfill missing window.__TAURI__
ea4e9e8 docs(project): document iframe sandbox limitations and backend crashes, apply click hotfix
47aa53b docs: add CORS limitation to technical debt in ralph-plan
5f68b51 add remaining local project files
b593449 fix tauri runtime bridge reports
b331595 Implement autonomous browser agent core
7c28a89 Harden push hygiene for agent artifacts
7bf79f5 Document GitSpec workflow for agent core
```

This is the "fix the dev build, document the limitations, implement the agent core" arc that `main` is missing.

**Recommendation: do not start a new feature on `main`. Switch to this branch, run `cargo check` and `cargo test` to confirm it actually compiles end-to-end, and if so, fast-forward `main` to it.**

---

## 5. Prioritized next actions

### P0 — Verify `origin/codex-live-tauri-browser-runtime` is the working tree

The single most valuable thing James can do in the next 30 minutes:

```bash
cd ~/code/neurobrowser
git checkout origin/codex-live-tauri-browser-runtime
cargo check --workspace
cargo test --workspace
./verify.sh
```

If that works (and the commit narrative suggests it should), this is the real codebase. From there, the next decisions are about *which* codex commit to merge vs. `main`. If it doesn't work, the gap between the two branches is the actual punch list.

### P0 — Fix `main`'s blockers if you must stay on `main`

If for any reason you stay on `main`:

1. Create `src-tauri/icons/icon.png` (any 32×32 PNG; a solid color is fine).
2. Add `.await` to `page.agent.execute(&prompt, page.browser.as_ref())` in `src-tauri/src/main.rs:66`.
3. Fill `src-tauri/gen/schemas/capabilities.json` (or hand-author `src-tauri/capabilities/main.json` with `{ "identifier": "default", "windows": ["main"], "permissions": ["core:default"] }`) so Tauri IPC isn't blocked at runtime.
4. Decide the `dist/` story: either remove the `dist/` + `package.json` deps, or wire up a real Vite build.

After those four, `cargo check` should be clean and `cargo tauri dev` should boot a window.

### P1 — Decide and unify on one `BrowserTool` trait

If the codex branch is adopted, this likely lands for free. If you stay on `main`, port the 10 tool impls from `tools/mod.rs` (`HashMap<String,String>` args) to `tools/contracts.rs` (`serde_json::Value` args + `ToolDefinition`). Then delete `tools/mod.rs`'s trait. Single source of truth; `ToolDefinition` becomes the JSON Schema we hand to OpenAI/Anthropic for native tool calling.

### P1 — Wire memory + observability into the loop

`ReActAgent::execute` is the only fully-wired system. Add:

- `EpisodicMemory::push(AgentEvent::ToolCall { ... })` before each tool invocation.
- `tracing::Instrument::instrument` on the per-iteration span (`agent_iteration_span`).
- `AgentMetrics::record_request()` / `record_tokens()` / `record_tool_call()` / `record_error()` at the matching sites.

Roughly 30 lines of code, huge debuggability win.

### P2 — Add session persistence

`SessionManager` already has `created_at` and `pages`; persist each `SessionState` to `~/.neurobrowser/sessions/<uuid>.json` on `create_session` / `create_page` / every agent iteration. Restore on app boot. ~150 lines.

### P2 — Async reqwest + async `BrowserInterface::navigate`

This is a breaking trait change. `BrowserEngine` becomes async. `ReActAgent::execute` calls `browser.navigate(url).await`. The async foundation (`tokio = "full"`) is already in the dependency tree.

### P2 — Replace custom `parse_tool_calls` with provider-native structured outputs

OpenAI's `tools` field in the chat completions API; Anthropic's `tools` field in the messages API. Both already support JSON Schema. The `ToolDefinition` already exists. This deletes `parse_tool_calls` and the brittle `Action: ...` parsing, replacing it with typed responses.

### P3 — Write a "demo mode" so the app runs without an API key

The frontend already has demo-mode fallbacks (`statusText.textContent = 'Tauri not connected - running in demo mode'`), but the backend has no such concept. Adding a `DemoProvider: AiProvider` that returns canned responses for known prompts would let the desktop app be exercised without `OPENAI_API_KEY` set, which is critical for contributor onboarding.

---

## 6. Quick reference — file index

**Library crate (`src/`):**

- `lib.rs` — public API exports (re-exports from each module)
- `agent/mod.rs` — `ReActAgent::execute`, the live loop (lines 69–133)
- `agent/memory.rs` — `EpisodicMemory`, `SemanticMemory`, `StateMemory` (data only)
- `agent/observability.rs` — `CorrelationContext`, span constructors, `AgentMetrics`
- `agent/streaming.rs` — `StreamingAgent` trait, `StreamEvent` enum, `AgentStatus`
- `browser/mod.rs` — `BrowserEngine`, `BrowserInterface` impl, 10 tool definitions
- `providers/mod.rs` — `AiProvider` trait, `AiContext`, `parse_tool_calls` (custom parser), `build_system_prompt`
- `providers/openai.rs` — `OpenAiProvider` (POSTs to chat completions)
- `providers/anthropic.rs` — `AnthropicProvider` (POSTs to messages API)
- `providers/ollama.rs` — `OllamaProvider` (POSTs to local generate API)
- `session/mod.rs` — `SessionManager`, `PageHandle`
- `tools/mod.rs` — `BrowserTool` (`HashMap` args), `ToolRegistry`, `PageInfo`
- `tools/contracts.rs` — `BrowserTool` (JSON args, `ToolDefinition` schema), `Capability` enum
- `tools/errors.rs` — `ToolError`, `AgentError`, `AgentResult<T>`

**Tauri shell (`src-tauri/`):**

- `src/main.rs` — 6 commands: `create_session`, `create_page`, `navigate`, `ask`, `get_page_info`, `list_sessions`
- `Cargo.toml` — `tauri v2`, `tauri-plugin-shell`, `tokio = "full"`
- `tauri.conf.json` — minimal config; no capabilities
- `gen/schemas/capabilities.json` — empty `{}` (runtime blocker)
- `index.html` / `dist/index.html` — identical self-contained single-page UI

**Docs:**

- `PROJECT.md` — vision, roadmap, status (foundation_complete, Tauri blocked)
- `docs/CODE_ANALYSIS.md` — 2026-03-01 refactor report, 10 fixes
- `docs/CODE_REVIEW.md` — 2026-03-01 review, 20 issues
- `research/rust_coding_frameworks.md` — ecosystem survey (Rig, MCP, Axum, etc.)

**Branch not on main:**

- `origin/codex-live-tauri-browser-runtime` — 9 commits, +16,509 / -1,600 lines, including real frontend, Tauri runtime, agent policy, and tests