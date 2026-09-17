# Prior Art — NeuroBrowser

Repos and projects that informed NeuroBrowser's architecture. Each entry lists
the role the prior art played and where NeuroBrowser follows vs. diverges.

## agent-browser (vercel-labs/agent-browser)

**Role observed:** agent-facing CLI that wraps Playwright. An external LLM
agent invokes the CLI; the CLI runs Playwright in a Node daemon and returns
structured page snapshots / diffs / screenshots. Ships a SKILL.md that any
AI agent can load to drive the browser.

**What NeuroBrowser takes:**
- The **SKILL.md / agent-facing interface** model — a single canonical doc
  that any agent loads to invoke the browser.
- The **ref-based interaction model** — agents pass `[@e1, @e2, ...]` refs
  instead of CSS selectors.

**What NeuroBrowser does NOT take:**
- A separate Playwright daemon as the runtime. NeuroBrowser is in-process
  with Tauri; the headless daemon only evaluates policy and returns a
  hardcoded snapshot. It does not execute browser tools.
- iOS Simulator support.
- `npm install -g` packaging. NeuroBrowser ships via Tauri bundling.

**Repo:** https://github.com/vercel-labs/agent-browser

## fastrender (wilsonzlin/fastrender)

**Role observed:** a Rust HTML/CSS renderer once considered as the
rendering engine. The library uses `scraper`; the desktop uses a Tauri child webview.
The headless policy protocol does not construct either browser runtime.

**Status:** not integrated (dependency conflicts; desktop JS execution
comes from the OS webview).

**Repo:** https://github.com/wilsonzlin/fastrender

## Real systems

Integrations use real backing systems:
- **OpenAI** (API key via env)
- **Anthropic** (API key via env)
- **Ollama** (local daemon)
- **Tauri child webview** (macOS WKWebView)

No mock browser page on product paths.
