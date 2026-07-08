# NeuroBrowser Frontend Architecture Spike

## Shared Contract

Both lanes use the same browser-control contract:

| Capability | Contract command |
| --- | --- |
| Create session | `create_session` |
| Create page/tab | `create_page(sessionId)` |
| Activate page | `set_active_page(sessionId, pageId)` |
| Sync native page bounds | `sync_browser_viewport(pageId, rect)` |
| Navigate | `navigate(sessionId, pageId, url)` |
| Snapshot | `get_page_snapshot(sessionId, pageId)` |
| Agent request | `ask(sessionId, pageId, prompt)` |
| Agent run | `start_agent_run(sessionId, pageId, prompt)` |
| Approval resolution | `submit_approval(runId, approved, message)`, `cancel_agent_run(runId)` |
| Action policy | `get_action_policy`, `set_action_policy(policy)` |
| Browser actions | `browser_back`, `browser_forward`, `browser_reload`, `close_page` |
| Provider selection | `set_provider(provider)` |

The Tauri lane calls the Rust commands directly through Tauri IPC. The AppKit lane
uses the same command names over a `WKScriptMessageHandler` named `neurobrowser`,
with Swift routing commands into the native `WKWebView` host.

## Lane A: React + Tauri

- React now owns browser chrome, tabs, provider selection, page stats, and agent chat.
- Rust remains the authority for sessions, provider config, child webviews, snapshots,
  and agent execution.
- Real pages still render in Rust-owned Tauri child webviews. React only reserves and
  syncs the viewport rectangle.
- The autonomous-agent path now exposes run status, approval cards, policy mode, and
  action history through the React surface.
- This is the current primary path because it already reaches the Rust command layer.

## Lane B: React + AppKit

- Swift/AppKit owns the window, split view, menus, tabs, and page `WKWebView`.
- A sidebar `WKWebView` loads the same React control surface in compact mode.
- The React control surface posts contract commands to Swift, and Swift can dispatch
  status/snapshot events back into React.
- This lane proves the native-browser ceiling, but the Rust agent/backend bridge is
  still the largest remaining gap.
- Deferred mismatch: the AppKit lane currently uses native tab indexes/page ids at
  the Swift boundary. That must be normalized to the Rust `page_id` contract before
  AppKit can become primary.

## Generated Artifacts

`src-tauri/dist/` and `NeuroBrowser/ControlSurface/` are generated build outputs and
are ignored for future changes. Existing tracked generated files should be removed
from version control in a dedicated cleanup commit using `git rm --cached` so the
source of truth remains `src-tauri/src/`.

## Objective-C Policy

No Objective-C or Objective-C++ was added in this spike. Swift/AppKit and WebKit cover
the current bridge cleanly. Objective-C++ remains allowed only for a future concrete
interop seam where Swift/Rust/AppKit/WebKit bridging becomes materially worse without it.

## Scorecard

| Criterion | React + Tauri | React + AppKit |
| --- | --- | --- |
| Real web rendering quality | Strong: existing Rust-owned child webviews, viewport sync, runtime snapshots | Strong native potential: direct `WKWebView`, AppKit lifecycle; needs more tab/page id hardening |
| React control ergonomics | Strong: full shell is React and calls real Rust commands | Good: compact sidebar React drives native host commands |
| Native browser ceiling | Medium: Tauri windowing plus child webviews | Strong: AppKit menus, responder chain, delegates, accessibility path |
| Rust integration simplicity | Strong: contract already backed by Tauri commands | Weak today: command bridge exists, Rust backend/agent bridge still pending |
| Verification cost | Moderate: Rust + Vite + Tauri cargo check | Higher: Vite AppKit bundle + Xcode build + future Rust bridge verification |

## Recommendation

Keep **React + Tauri** as the primary frontend path now. It preserves the Rust backend
as the authority and already supports the real command contract. Continue the
**React + AppKit** lane as a focused native spike only if AppKit/WebKit behavior
becomes a product constraint that Tauri cannot meet.

## Next Work

1. Stabilize page identity in the AppKit lane so page ids survive tab close/reorder.
2. Add a real Rust bridge for AppKit provider selection, snapshots, and agent calls.
3. Add UI smoke tests for the React shell and a native AppKit launch/snapshot smoke.
4. Decide whether Tauri child-webview behavior is good enough after live navigation,
   snapshot, and keyboard shortcut testing on macOS.
