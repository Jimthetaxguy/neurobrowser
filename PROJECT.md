# NeuroBrowser Project Documentation

# Project Vision, Status, Roadmap and Goals

version: "0.1.1"
created: "2026-02-23"
updated: "2026-07-08"

# =============================================================================

# v0.1.1 — DELTA (what shipped 2026-07-08)

# =============================================================================

> The body below documents the **v0.1.0** original snapshot. The actual
> shipped state of the tree at `main@cea9af9` is **v0.1.1** — the result
> of merging `codex-live-tauri-browser-runtime` into `main` and then
> closing phases A–E of the v0.1 roadmap. Highlights:

- **Tauri desktop shell:** React frontend with tab strip, URL bar,
  chat panel, command palette, settings drawer — runs against a Rust
  backend that owns a Tauri child webview per page (`src-tauri/src/runtime.rs`).
- **Agent surface:** 12 structured tools (`snapshot`, `click`, `type_text`,
  `submit_form`, `query_selector`, `evaluate`, `navigate`, `get_text`,
  `get_attribute`, `wait_for`, `extract_text`, `screenshot`) plus
  ref-based interaction (`@eN` references resolved through a `ref_map`).
  See `docs/AGENT-SURFACE.md`.
- **Action policy:** `ReadOnly` / `Assisted` / `HighAutonomy` modes with
  per-domain allow/deny lists, sensitive-arg redaction, prompt-injection
  detection, and three outcomes (`Allow`, `RequireApproval`, `Deny`).
- **Headless daemon:** `cargo run --bin neurobrowser-headless --features headless`
  exposes the same surface over a Unix Domain Socket or TCP for external
  agents (ROSA, Claude Code, custom workers).
- **Skill file:** `SKILL.md` at the repo root — the agent-loadable
  invocation spec.
- **Worker model (tabs-as-workers):** `src/agent/worker.rs` types plus
  SessionManager-level worker registry, cross-worker observation ring,
  in-session inbox routing. Tauri commands `list_workers`/`get_worker`
  expose them. React sidebar (E5) and headless fan-out (E6) deferred to v0.2.
- **Observability:** `AgentMetrics` global singleton, `AgentEvent`
  episodic log, `MetricsSnapshot`, correlation spans (`llm_call_span`,
  `tool_call_span`, `agent_iteration_span`).
- **Streaming:** `StreamingAgent` trait, `ReActAgent` impl, integration
  test in `tests/streaming_agent.rs`.
- **Native tool calling:** deferred from Phase C — JSON-tool-call plumbing
  is in place (`tools::ToolCall::parse_native`) but the agent loop still
  consumes text-only tool invocations. v0.2.
- **Test coverage:** 50 tests (8 integration, 42 unit) — `cargo test --all-targets`
  green. `./verify.sh` runs fmt + clippy `-D warnings` + lib + integration +
  Tauri frontend + headless release build.
- **Documentation:** `docs/RUNBOOK-DEV.md`, `docs/TESTING-NOTES.md`,
  `docs/AGENT-SURFACE.md`, `docs/references/prior-art.md`.

# =============================================================================

# PROJECT VISION

# =============================================================================

vision:
  name: "NeuroBrowser"
  tagline: "AI-Native Browser with Custom Rendering Engine"
  
  description: |
    An AI-first browser that combines a custom Rust rendering engine with a
    pluggable AI agent layer, enabling unprecedented control over how AI
    interacts with web content. Unlike existing "AI browsers" that layer AI
    on top of Chromium/WebView, NeuroBrowser controls the rendering pipeline
    for AI-native features like structural DOM querying, semantic metadata
    extraction, and parallel page analysis at scale.
  
  core_differentiation: |
    Unlike Arc Search, Opera Aria, and other AI browsers that use WebView/Chromium,
    NeuroBrowser provides full DOM control, is lightweight (~50MB vs 200MB+),
    and offers complete privacy with no external browser telemetry.

  target_users:
    - "Individual users who want an AI companion while browsing"
    - "Teams needing scalable web research and competitive intelligence"
    - "Enterprises requiring private, controllable AI browsing"

# =============================================================================

# PRODUCT SPECIFICATION

# =============================================================================

product:
  variants:
    - name: "NeuroBrowser Desktop"
      type: "desktop"
      description: "Tauri + React native desktop application"
      priority: 1

    - name: "NeuroBrowser CLI"
      type: "cli" 
      description: "Headless operation for automation and scripting"
      priority: 2

  features:
    core:
      - name: "DOM Query Tool"
        description: "AI can query DOM structure directly (not just visual)"
        priority: P0
        status: implemented

      - name: "Parallel Tabs"
        description: "10 concurrent page instances"
        priority: P0
        status: architecture_ready
        
      - name: "Scroll Automation"
        description: "Programmatic scroll, infinite scroll detection"
        priority: P0
        status: stub
        
      - name: "Form Filling"
        description: "Text input, checkboxes, selects, submission"
        priority: P0
        status: stub
        
      - name: "Provider Pluggability"
        description: "Swap AI providers without code changes"
        priority: P0
        status: implemented
        
    secondary:
      - name: "CLI Mode"
        description: "Headless operation for automation"
        priority: P1
        
      - name: "Session Memory"
        description: "Context preservation across pages"
        priority: P2
        
      - name: "Data Export"
        description: "JSON/CSV extraction"
        priority: P2

  ai_tools:
    - query_dom(selector)
    - get_text(selector)
    - get_attributes(selector)
    - click(selector)
    - type(selector, text)
    - select_option(selector)
    - check(selector)
    - scroll_to(selector)
    - scroll_by(x, y)
    - wait_for(selector)
    - submit_form(selector)
    - get_links()
    - get_images()
    - get_prices()
    - get_tables()
    - take_screenshot()

# =============================================================================

# CURRENT STATUS

# =============================================================================

status:
  overall: "v0.1.1_shipped"
  build_status: "verify_sh_green"
  last_updated: "2026-07-08"

  components:
    - name: "Core Library"
      path: "src/"
      status: "compiles_and_tested"
      notes: "50 tests green; fmt + clippy -D warnings clean"

    - name: "AI Providers"
      path: "src/providers/"
      status: "implemented"
      providers:
        - OpenAI (gpt-4o)
        - Anthropic (Claude)
        - Ollama (local models)
        - Custom (HTTP, OpenAI-compatible)

    - name: "ReAct Agent"
      path: "src/agent/"
      status: "implemented"
      features:
        - Tool execution loop
        - Context building
        - Final answer extraction
        - AgentMemory (episodic/semantic/state)
        - StreamingAgent implementation
        - Process-global AgentMetrics
        - Conversation-window bound (20 messages)
        - Worker model (tabs-as-workers) — types + registry

    - name: "Browser Engine"
      path: "src/browser/"
      status: "implemented"
      engine: "scraper (HTML parsing) for headless daemon; Tauri child webview for desktop"
      features:
        - HTML parsing
        - DOM queries via CSS selectors
        - Link/Image/Form extraction
        - Price extraction via regex
        - Ref-based interaction (@eN) via runtime.js
        - 12 agent-facing tools (snapshot/click/type/submit/etc.)

    - name: "Session Management"
      path: "src/session/"
      status: "implemented"
      features:
        - Multi-session support
        - Page tracking
        - Session listing
        - Worker registry (workers/inbox/observations)

    - name: "Action Policy"
      path: "src/agent/policy.rs"
      status: "implemented"
      features:
        - Three autonomy levels (ReadOnly/Assisted/HighAutonomy)
        - Per-domain allow/deny
        - Sensitive-arg redaction
        - Prompt-injection detection
        - Three outcomes (Allow/RequireApproval/Deny)
        - 4 unit tests in tests/action_policy.rs

    - name: "DOM Tools"
      path: "src/tools/"
      status: "implemented"
      tools_count: 12

    - name: "Tauri Desktop Shell"
      path: "src-tauri/"
      status: "green_build"
      commands: 24
      notes: "All commands wired through invoke_handler; capability allowlist in main.json"

    - name: "Frontend"
      path: "src-tauri/dist/ (Vite build of src-tauri/src/App.jsx)"
      status: "implemented"
      features:
        - URL bar
        - Tab strip
        - Chat interface
        - Settings drawer
        - Command palette
        - Streaming agent run events
        - ActionPolicy panel

    - name: "Headless Daemon"
      path: "src-tauri/src/bin/headless.rs"
      status: "implemented"
      notes: "UDS/TCP, dispatches ping/policy.get/policy.set/policy.evaluate/snapshot/policy.snapshot"

# =============================================================================

# TECHNICAL ARCHITECTURE

# =============================================================================

architecture:
  stack:
    frontend: "React + Tauri WebView"
    backend: "Rust (Tauri)"
    agent: "Rust (ReAct pattern)"
    rendering: "scraper crate (HTML/CSS)"
    ai_providers: "pluggable (OpenAI/Anthropic/Ollama)"

  modules:
    - name: "lib.rs"
      purpose: "Core exports and types"

    - name: "agent/mod.rs"
      purpose: "ReAct agent implementation"
      dependencies:
        - providers
        - tools
        
    - name: "browser/mod.rs"
      purpose: "Browser engine with HTML parsing"
      dependencies:
        - scraper
        - reqwest (blocking)
        
    - name: "providers/"
      purpose: "AI provider abstraction"
      files:
        - "mod.rs (trait)"
        - "openai.rs"
        - "anthropic.rs"
        - "ollama.rs"
        
    - name: "session/mod.rs"
      purpose: "Session and page management"
      
    - name: "tools/mod.rs"
      purpose: "Browser tool definitions"

# =============================================================================

# BLOCKERS AND ISSUES

# =============================================================================

blockers:

# None active as of v0.1.1.

resolved_in_v0_1_1:
  - id: "tauri_config"
    note: "Icons added, bundle config validated; tauri cargo check + release build green via verify.sh"

  - id: "fastrender_integration"
    note: "FastRender still not integrated (dependency conflicts). Desktop uses Tauri child webview; headless daemon uses scraper. JS execution works in desktop via the Tauri webview."

  - id: "iframe_x_frame_options"
    note: "Click interceptor scripts in src-tauri/src/runtime.rs (RUNTIME_INIT_SCRIPT) route navigations through Tauri commands; obsolete (we no longer render via blob-URL iframe)."

  - id: "backend_command_mismatch"
    note: "All 24 commands are wired in src-tauri/src/main.rs with matching implementations and capability permissions."

remaining_for_v0_2:
  - id: "native_tool_calling"
    severity: "medium"
    description: "Agent loop consumes text-only tool invocations. JSON tool-call parsing exists (tools::ToolCall::parse_native) but the ReActAgent prompt does not yet ask providers to emit structured JSON."
    solution: "Update the build_context tool spec to require JSON emission when the provider supports tool calls (OpenAI/Anthropic tool_calls field)."

  - id: "worker_ui"
    severity: "low"
    description: "Worker types and SessionManager methods are in place; no React sidebar yet."
    solution: "Add a Workers panel to App.jsx that calls listWorkers/getWorker."

  - id: "headless_fan_out"
    severity: "low"
    description: "Headless daemon exposes policy.get/set/evaluate but does not yet spawn workers across multiple sessions."
    solution: "Add a worker.spawn command to headless.rs that creates a SessionManager worker and a worker.list command that calls SessionManager::list_workers."

# =============================================================================

# ROADMAP

# =============================================================================

roadmap:
  v0_1_phases:
    phase_a:
      name: "Merge codex-live-tauri-browser-runtime into main"
      status: "complete (commit 53e0e31, 2026-07-08)"
    phase_b:
      name: "Green Tauri desktop build + RUNBOOK-DEV.md"
      status: "complete (commit 53e0e31, 2026-07-08)"
    phase_c:
      name: "Wire AgentMemory, StreamingAgent, AgentMetrics, conversation bound"
      status: "complete (commit 53e0e31, 2026-07-08). Native function calling deferred to v0.2."
    phase_d:
      name: "Agent-native primitives (SKILL.md, headless daemon, ref-based tool surface)"
      status: "complete (commit 53e0e31, 2026-07-08)"
    phase_e:
      name: "Worker model (tabs-as-workers)"
      status: "complete (commit cea9af9, 2026-07-08). E5/E6 deferred to v0.2."
    phase_f:
      name: "Documentation sweep + triptych write-back"
      status: "complete (this commit)"

  phase_1:
    name: "Foundation"
    status: "complete"
    deliverables:
      - "Project structure created"
      - "AI provider plugins working"
      - "ReAct agent implemented"
      - "Basic DOM tools working"

  phase_2:
    name: "Tools & Forms"
    status: "complete (v0.1)"
    deliverables:
      - "Full DOM query implementation"
      - "Form interaction (input, select, submit)"
      - "Scroll automation"
      - "Price/Table extraction"

  phase_3:
    name: "Parallelism"
    status: "architecture_ready_v0_2"

  phase_4:
    name: "Desktop UI"
    status: "complete (v0.1)"

  phase_5:
    name: "CLI"
    status: "complete_v0_1 (headless daemon), full CLI deferred to v0.2"

  phase_6:
    name: "Provider Ecosystem"
    status: "ongoing"
    deliverables:
      - "Anthropic support (done)"
      - "Local models (Ollama)"
      - "Plugin SDK documentation"

  phase_v0_2:
    name: "v0.2 Roadmap"
    status: "planned"
    deliverables:
      - "Native function calling in ReActAgent (structured JSON tool calls)"
      - "React Workers sidebar (E5)"
      - "Headless worker fan-out (E6) — worker.spawn + worker.list"
      - "Full CLI wrapper over the headless daemon"
      - "Visual regression tests against a fixture site"
      - "Real-LLM end-to-end integration test (budget-capped via Infisical)"

# =============================================================================

# GOALS

# =============================================================================

goals:
  immediate:
    - id: "fix_tauri_build"
      description: "Fix Tauri config and get build working"
      priority: "critical"
      owner: "james"

    - id: "frontend_connected"
      description: "Connect frontend to backend properly"
      priority: "critical"
      
    - id: "demo_working"
      description: "Get demo mode working with navigation"
      priority: "high"

  short_term:
    - id: "form_support"
      description: "Implement full form filling capabilities"
      priority: "high"

    - id: "scroll_automation"
      description: "Implement scroll automation tools"
      priority: "high"
      
    - id: "price_extraction"
      description: "Refine price extraction logic"
      priority: "medium"

  medium_term:
    - id: "parallel_tabs"
      description: "Implement 10-tab parallel browsing"
      priority: "high"

    - id: "local_ai"
      description: "Full Ollama/local model support"
      priority: "medium"
      
    - id: "cli_variant"
      description: "Build CLI tool variant"
      priority: "medium"

  long_term:
    - id: "js_support"
      description: "Add JavaScript execution support"
      priority: "high"

    - id: "fastrender_full"
      description: "Full FastRender integration"
      priority: "high"
      
    - id: "production_ready"
      description: "Production release with稳定性"
      priority: "critical"

# =============================================================================

# SUCCESS METRICS

# =============================================================================

metrics:
  technical:
    - name: "Page render success rate"
      target: ">95%"

    - name: "Tool execution success"
      target: ">90%"
      
    - name: "Parallel tab stability"
      target: "<1% crashes"
      
    - name: "Memory per tab"
      target: "<200MB"
      
    - name: "Time to first interaction"
      target: "<5s"

  product:
    - name: "Feature completion"
      target: "All P0 features"

    - name: "Build success"
      target: "Desktop app compiles and runs"

# =============================================================================

# COMPETITIVE ANALYSIS

# =============================================================================

competitors:

- name: "Arc Search"
    approach: "AI on Chromium"
    weakness: "No DOM control, heavy"

- name: "Opera Aria"
    approach: "AI on Chromium"
    weakness: "No DOM control"

- name: "BrowserUse"
    approach: "Automation on existing browsers"
    weakness: "Heavy, limited by sandbox"

- name: "Jina Reader"
    approach: "Server-side rendering"
    weakness: "No interactivity"

# =============================================================================

# REFERENCES

# =============================================================================

references:
  source_repos:
    - name: "agent-browser (vercel-labs)"
      url: "https://github.com/vercel-labs/agent-browser"
      use: "Agent-facing CLI patterns, ref-based snapshot model, SKILL.md distribution, encrypted profile persistence. See docs/references/prior-art.md."
    - name: "agent-browser (AIAnytime)"
      url: "https://github.com/AIAnytime/agent-browser"
      use: "Earlier ReAct-pattern reference (legacy citation retained for provenance; vercel-labs/agent-browser is the current authoritative reference)"

    - name: "fastrender"
      url: "https://github.com/wilsonzlin/fastrender"
      use: "Rendering engine (not yet integrated due to version conflicts)"
      
  inspiration:
    - "ROSA (Relationship Operating System Agent)"
    - "Cursor's collaborative AI coding research"

# =============================================================================

# NOTES

# =============================================================================

notes: |

- Project started 2026-02-23
- Derived from analyzing agent-browser and fastrender repos
- Core library compiles successfully
- Tauri build blocked on config issues
- Using scraper crate instead of FastRender due to dependency conflicts
- Desktop-first approach per user request
- Both personal (A) and enterprise (B) use cases viable
  
  Key insight: The differentiation is controlling the rendering pipeline itself,
  not just layering AI on top of existing browsers. This enables:

- Full DOM access for AI querying
- Privacy (no Chrome/Safari dependency)
- Custom rendering optimized for AI extraction
- Lightweight (~50MB vs 200MB+)
- Control (no feature restrictions from upstream browsers)
