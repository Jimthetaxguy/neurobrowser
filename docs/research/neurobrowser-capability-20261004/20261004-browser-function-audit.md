---
author: codex/Codex
created: '2026-10-04T14:56:16-04:00'
agent: codex/Codex
date: '2026-10-04T14:56:16-04:00'
type: code-audit
task: Assess browser function beyond the shared capability foundation
status: complete-with-findings
summary: The OS webview supplies engine behavior; browser-host ownership remains incomplete, with reproduced React input and legacy export gaps.
next_steps: []
remaining: []
open_questions: [Intended browser profile sharing, Scope of human navigation policy, Which native workflows take priority]
---

# Browser function audit

Reviewed feature `f50f5aec9599a8b8926167dc17fcf3d012e6be31` in PR #111,
plus the existing paths retained by that branch. Read-only source review and isolated
DOM probes; no production edits, provider calls or real private data access. Three
specialists reviewed runtime, authority/lifecycle and human-shell boundaries.

The OS webview already supplies HTML/CSS/JS execution and rendering. Rust should own
browser-host contracts: identity, authority, lifecycle, observation and durable effects.
This audit does not suggest rebuilding a standards engine.

## Confirmed defects and inadequate guarantees

Measured at `f50f5ae`, not current. Merged `runtime.rs` skips non-rendered targets and uses the prototype value setter.

| Finding | Evidence and trigger | Consequence |
| --- | --- | --- |
| Controlled React input bypass | Shipped runtime sets input.value then synthetic input/change; DOM probe produced acknowledged, visible updated value, zero React onChange calls, submitted React state initial. runtime.rs:415 | A form may submit the old application value despite apparently successful typing. Native WK regression is still required. |
| CSS-hidden target selection | Exact shipped script observes display:none button as enabled, and its click handler runs. runtime.rs:350 | Current targeting lacks rendered visibility/actionability semantics. |
| Tauri page changes do not feed shell state | Adapter has no host event subscription; runtime callbacks update loading flags only. hostAdapters.js:16, runtime.rs:1160, App.jsx:604 | In-page links, SPA routes and changing titles can leave address/tab UI stale. |
| Cross-tab shell snapshot race | updateSnapshot unconditionally writes global URL/snapshot/status; createNewTab leaves prior values. App.jsx:337,400 | A delayed prior-page snapshot can replace the active page's shell evidence; new evidence-panel generation guards do not cover this older path. Source trace, no assembled-app reproduction yet. |
| Legacy private-value export | Dummy API-key-shaped ordinary text input survives legacy HTML and form-value extraction; inline script canary survives HTML. New observation excludes both. runtime.rs:41,190 | Legacy capture can retain these values. main.rs:682 copies snapshot HTML/text to stored pages; CapturePolicy defaults enabled/all non-denied public hosts. No real secret read or actual private capture was performed. |
| Loading is not application readiness | wait_for_ready watches a navigation flag; postcondition is checked once immediately afterwards. runtime.rs:848, capability/tools.rs:255 | Delayed SPA fetches can yield premature unsatisfied predicates. Need task predicates with bounded waiting, not a universal network-idle rule. |
| Main-world execution is not isolated | Exact DOM probe overrides element.click with no-op; dispatch still acknowledges and zero events occur. runtime.rs:413 | Native caller provenance does not authenticate page-world DOM methods or claims. Acknowledgment remains weaker than effects; isolated instrumentation is unresolved. |
| No active-run cancellation | cancel_agent_run revokes pending approval; provider loop has no cancellation signal. main.rs:533, agent/mod.rs:140 | Closing or requesting stop does not reliably stop active provider work. Pending runtime requests do drain on page closure. |
| HTTP source body is unbounded | BrowserEngine uses response.text without byte cap before parsing/observation truncation. browser/mod.rs:137 | Bounded observations do not impose bounded fetch/parse memory. Resource budgets need their own enforcement. |

## Missing browser-host capabilities

- **Profile/login ownership:** sessions group page handles, with no explicit browser
  datastore, separate-account, ephemeral-profile or clear-site-data contract. Actual OS
  cookie sharing was not tested; it must not be inferred from Rust session IDs.
- **Contexts:** scoped observations query one document. Same-origin iframe and open-shadow
  buttons are absent in an exact-script DOM probe. Multi-context targeting and ownership
  are not implemented. First-80 targeting also has no scoped search/pagination path.
- **Site integrations:** no explicit popup-to-tab routing, download lifecycle, governed
  upload selection, or permission-management surface. Native defaults may provide some
  behavior; reliable product behavior for SSO, downloads, camera/mic is unverified.
- **Persistence/recovery:** session/tab state is memory-only and startup creates a fresh
  session. No restore/reopen contract, renderer-termination recovery or process budgets.
- **Visual and external/background agents:** screenshots unsupported, live headless/MCP
  session transport absent. AppKit remains navigation/snapshot parity, without new
  evidence/agent/approval controls.
- **Network policy:** native top-level preflight is not connection-wide confinement for
  fetch, WebSocket, scripts or images. This is separate from the engine's own SOP/TLS/CSP.
- **Policy scope:** human omnibox and native navigation use netguard, while proposals use
  ActionPolicy. This may be intentional human authority, but the UI/contract must define
  it. Trusted public Rust interfaces can also dispatch without agent policy; future
  external transports must expose the governed route only.
- **Convenience and accessibility:** ordinary browser features such as find-in-page,
  bookmarks, explicit zoom/printing controls and browser-chrome accessibility need a
  separate product inventory. This audit did not exercise their OS-default behavior.

## Verification limits

React/JSDOM probes establish real React DOM behavior using the exact script at
`f50f5ae`; they are not native WebKit proofs. Source traces establish control/ownership
gaps, not observed cookie leaks or a demonstrated hostile-site exploit. The tests in
that checkout exercise mounted shell components with adapters and genuine WKWebView
with a test-only report bridge. A whole live Tauri IPC session was not part of this audit.

Primary platform references: [Tauri webview ownership](https://v2.tauri.app/reference/webview-versions/),
[Tauri webview integration hooks](https://docs.rs/tauri/latest/tauri/webview/struct.WebviewBuilder.html),
[DOM event trust](https://dom.spec.whatwg.org/#dom-event-istrusted).
