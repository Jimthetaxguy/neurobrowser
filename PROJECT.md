# NeuroBrowser Project Documentation

# Project Vision, Status, Roadmap and Goals

version: "0.1.0"
created: "2026-02-23"
updated: "2026-02-23"

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
  overall: "foundation_complete_blocked_on_tauri"
  build_status: "library_compiles_tauri_blocked"
  last_updated: "2026-02-23"
  
  components:
    - name: "Core Library"
      path: "neurobrowser/"
      status: "compiles"
      notes: "All core modules implemented"

    - name: "AI Providers"
      path: "src/providers/"
      status: "implemented"
      providers:
        - OpenAI (gpt-4o)
        - Anthropic (Claude)
        - Ollama (local models)
        
    - name: "ReAct Agent"
      path: "src/agent/"
      status: "implemented"
      features:
        - Tool execution loop
        - Context building
        - Final answer extraction
        
    - name: "Browser Engine"
      path: "src/browser/"
      status: "implemented"
      engine: "scraper (HTML parsing)"
      features:
        - HTML parsing
        - DOM queries via CSS selectors
        - Link/Image/Form extraction
        - Price extraction via regex
        
    - name: "Session Management"
      path: "src/session/"
      status: "implemented"
      features:
        - Multi-session support
        - Page tracking
        - Session listing
        
    - name: "DOM Tools"
      path: "src/tools/"
      status: "implemented"
      tools_count: 10
      
    - name: "Tauri Desktop Shell"
      path: "src-tauri/"
      status: "blocked"
      blockers:
        - "Missing icons for bundle"
        - "Config path issues"
        
    - name: "Frontend"
      path: "src-tauri/index.html"
      status: "implemented"
      features:
        - URL bar
        - Tab UI
        - Chat interface
        - Tauri integration

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

- id: "tauri_config"
    severity: "high"
    description: "Tauri build blocked - missing icons and config path issues"
    location: "src-tauri/tauri.conf.json"
    solution: "Add placeholder icons or fix config"

- id: "fastrender_integration"
    severity: "medium"
    description: "FastRender version conflicts - using scraper instead"
    impact: "Static HTML only, no JavaScript execution"
    solution: "Add JS support via alternative (e.g., headless chromium fallback)"

- id: "iframe_x_frame_options"
    severity: "medium"
    description: "Blob URL iframe rendering is blocked by sites using X-Frame-Options: SAMEORIGIN"
    impact: "Users clicking links inside the rendered iframe trigger a browser crash/block instead of routing through the Tauri backend."
    solution: "Inject click interceptor scripts into HTML payload to proxy navigations via postMessage (implemented as a hotfix in index.html)."

- id: "backend_command_mismatch"
    severity: "high"
    description: "Tauri commands exposed to the frontend don't always match the internal Rust structure implementations."
    impact: "Commands like `set_provider` or `close_page` were declared in main.rs but not implemented in `ReActAgent` or `SessionManager`, causing fatal compile errors."
    solution: "Ensure trait boundaries and implementations are synced before registering Tauri commands."

# =============================================================================

# ROADMAP

# =============================================================================

roadmap:
  phase_1:
    name: "Foundation"
    status: "complete"
    duration: "Week 1"
    deliverables:
      - "Project structure created"
      - "AI provider plugins working"
      - "ReAct agent implemented"
      - "Basic DOM tools working"

  phase_2:
    name: "Tools & Forms"
    status: "in_progress"
    duration: "Weeks 2-3"
    deliverables:
      - "Full DOM query implementation"
      - "Form interaction (input, select, submit)"
      - "Scroll automation"
      - "Price/Table extraction"

  phase_3:
    name: "Parallelism"
    status: "pending"
    duration: "Week 4"
    deliverables:
      - "Tab pool (10 instances)"
      - "Shared resource management"
      - "Concurrent task orchestration"

  phase_4:
    name: "Desktop UI"
    status: "pending"
    duration: "Weeks 5-6"
    deliverables:
      - "Tauri app shell"
      - "Tab management UI"
      - "Settings/preferences"
      - "Keyboard shortcuts"

  phase_5:
    name: "CLI"
    status: "pending"
    duration: "Week 7"
    deliverables:
      - "Headless mode"
      - "JSON output"
      - "Script integration"

  phase_6:
    name: "Provider Ecosystem"
    status: "ongoing"
    deliverables:
      - "Anthropic support (done)"
      - "Local models (Ollama)"
      - "Plugin SDK documentation"

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
    - name: "agent-browser"
      url: "<https://github.com/AIAnytime/agent-browser>"
      use: "AI agent patterns, ReAct implementation"

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
