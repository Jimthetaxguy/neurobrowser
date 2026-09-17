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

**What NeuroBrowser does NOT take:**
- A separate Playwright daemon as the runtime. NeuroBrowser is in-process
  with Tauri; the headless daemon only evaluates policy and returns a
  hardcoded snapshot. It does not execute browser tools.
- iOS Simulator support.
- `npm install -g` packaging.

**Repo:** https://github.com/vercel-labs/agent-browser

This is the current agent-CLI reference. It is unrelated to
`AIAnytime/agent-browser` (name collision only).

## agent-browser (AIAnytime/agent-browser)

**Role observed:** earlier ReAct-pattern teaching demo (Tauri + React +
placeholder tools). No wired DOM/WebView automation, no policy layer, no
shared history with vercel-labs/agent-browser.

**Status:** retained as a provenance citation only. Not an architectural
ancestor.

**Repo:** https://github.com/AIAnytime/agent-browser

## hyperbrowser-app-examples

**Role observed:** showcase of thin Next.js apps that funnel into a hosted
`@hyperbrowser/sdk` cloud-browser API.

**What NeuroBrowser does NOT take:**
- The hosted-only model. NeuroBrowser is a local desktop app with an
  optional headless daemon.
- The Next.js app-of-apps showcase.

**Repo:** https://github.com/hyperbrowserai/hyperbrowser-app-examples

## fastrender (wilsonzlin/fastrender)

**Role observed:** a Rust HTML/CSS renderer once considered as the
rendering engine. The library uses `scraper`; the desktop uses a Tauri child webview.
The headless policy protocol does not construct either browser runtime.

**Status:** not integrated (dependency conflicts; desktop JS execution
comes from the OS webview).

**Repo:** https://github.com/wilsonzlin/fastrender

## Arc, Opera Aria

**Role observed:** commercial "AI browser" products that layer AI on
Chromium/WebView.

**What NeuroBrowser differentiates on:**
- Local-first privacy (no external browser telemetry).
- Policy-gated autonomy (`ReadOnly` / `Assisted` / `HighAutonomy`).

**Sites:** https://arc.net · https://www.opera.com/features/opera-aria

## Real systems

Integrations use real backing systems:
- **OpenAI** (API key via env)
- **Anthropic** (API key via env)
- **Ollama** (local daemon)
- **Tauri child webview** (macOS WKWebView)

No mock browser page on product paths.
