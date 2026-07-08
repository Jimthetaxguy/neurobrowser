# Prior Art — NeuroBrowser

Repos and projects that informed NeuroBrowser's architecture. Each entry lists
the role the prior art played, the version observed, and where NeuroBrowser
follows vs. diverges.

## agent-browser (vercel-labs/agent-browser)

**Role observed:** agent-facing CLI that wraps Playwright. An external LLM agent
invokes the CLI; the CLI runs Playwright in a Node daemon and returns structured
page snapshots / diffs / screenshots. Distributed via npm, Homebrew, and from
source. Ships an iOS Simulator mode, encrypted profile persistence, and a
SKILL.md that any AI agent can load to drive the browser.

**What NeuroBrowser takes:**
- The **SKILL.md / agent-facing interface** model — a single canonical doc that
  any agent loads to invoke the browser. NeuroBrowser's `SKILL.md` is the
  equivalent for the Tauri child-webview surface.
- The **ref-based interaction model** — agents pass `[@e1, @e2, ...]` refs
  instead of CSS selectors. Each ref maps to a stable element identity.
- The **snapshot / visual diff** ergonomics — `neurobrowser diff snapshot_a
  snapshot_b` and `neurobrowser diff --pixel a.png b.png`.
- The **encrypted profile / state persistence** story — persist
  `SessionManager` state across invocations behind a keychain-derived key.

**What NeuroBrowser does NOT take:**
- agent-browser's separate-daemon architecture. NeuroBrowser is in-process with
  Tauri; the headless daemon (Phase D4) is a thin cross-process shim, not a
  full separate runtime.
- agent-browser's iOS Simulator support. Out of scope for v0.1.
- agent-browser's `npm install -g` packaging. NeuroBrowser is a desktop app and
  ships via Tauri bundling.

**Repo:** `https://github.com/vercel-labs/agent-browser`

> **TODO:** PROJECT.md still cites `https://github.com/AIAnytime/agent-browser`
> at line 447. The user's local Desktop analysis observed `vercel-labs/agent-browser`
> as the actual reference. Need to verify which repo is the real inspiration
> (or whether both exist) and update PROJECT.md accordingly.

## agent-browser (AIAnytime/agent-browser)

**Role observed:** PROJECT.md:447 cites this URL as a source for "AI agent
patterns, ReAct implementation."

**Status:** needs verification — see TODO above. Likely either an older name
for the same project, a fork, or a separate project that informed an earlier
incarnation of NeuroBrowser's design.

**Repo:** `https://github.com/AIAnytime/agent-browser` (per PROJECT.md:447)

## hyperbrowser-app-examples

**Role observed:** showcase of 45 thin Next.js apps that all funnel into a
single hosted product — the `@hyperbrowser/sdk` cloud-browser API. Most apps
delegate "intelligence" to Hyperbrowser's server-side `hyperAgent`; the
apps themselves are UX shells with hard-coded prompts and cheerio extraction.

**What NeuroBrowser takes:**
- The **decompose → fan-out → synthesize** LLM pipeline shape (hyperswarm).
- The **parallel aspect extraction** pattern with `Promise.allSettled`
  (yc-research-bot).
- The **benchmark harness over an agent** framing (agent-web-index) — relevant
  for NeuroBrowser's testing strategy in `tests/autonomous_agent.rs`.

**What NeuroBrowser does NOT take:**
- The hosted-only model. NeuroBrowser is a local desktop app with optional
  headless-daemon mode for cross-process driving.
- The Next.js app-of-apps showcase. NeuroBrowser has one desktop app.

**Repo:** `https://github.com/hyperbrowserai/hyperbrowser-app-examples`

## fastrender (wilsonzlin/fastrender)

**Role observed:** a Rust HTML/CSS renderer that NeuroBrowser's PROJECT.md
considered as a rendering engine. PROJECT.md:470 says "Using scraper crate
instead of FastRender due to dependency conflicts."

**Status:** deferred — see `docs/SPIKES.md` (created in Phase F) for the
re-evaluation. The merged tree's actual approach is "Tauri child webview with
JS RPC," which obsoletes the FastRender spike.

**Repo:** `https://github.com/wilsonzlin/fastrender`

## Arc, Opera Aria

**Role observed:** commercial "AI browser" products that layer AI on top of
Chromium/WebView. Cited in PROJECT.md's `competitive_analysis`.

**What NeuroBrowser differentiates on:**
- Full DOM control (vs. AI layered on top of a normal browser).
- Lightweight (~50MB vs. 200MB+).
- Local-first privacy (no external browser telemetry).
- Policy-gated autonomy (ReadOnly / Assisted / HighAutonomy) — Arc / Opera
  Aria do not offer this.

**Repos / sites:**
- `https://arc.net`
- `https://www.opera.com/features/opera-aria`

## Code-quality reference: real-systems-only

Per `~/.ai-memory/core/agent-rules/rules.md` (`real-systems-only` rule),
NeuroBrowser's integrations must use real backing systems:
- **OpenAI** (real API, key via env / Infisical)
- **Anthropic** (real API, key via env / Infisical)
- **Ollama** (real local daemon, no mock fallback in production code)
- **Tauri child webview** (real WKWebView/WebView2/WebKitGTK, no iframe stub)

Every PR landing in NeuroBrowser must keep this invariant. See `PROJECT.md` §
"Real systems" (added in Phase F1).