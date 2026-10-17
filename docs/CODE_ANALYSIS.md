# NeuroBrowser Code Analysis

Updated: 2026-05-22

## Summary

NeuroBrowser is no longer in the February/March "Tauri blocked" state. The
current repo has a working Rust core, focused autonomous-agent tests, a React +
Tauri primary frontend, and an AppKit comparison spike.

The app should be evaluated as a native desktop browser, not as a Chrome-hosted
web app. Playwright/Chrome is useful for fast React-shell smoke tests only; it
does not exercise Tauri IPC, native child webviews, page focus, or runtime
snapshots.

The primary cleanup need is keeping the system boundary sharp:

- Rust owns sessions, page state, provider configuration, tool execution, and
  action-policy decisions.
- React owns the control surface and renders state returned by the native host.
- Tauri is the primary command bridge.
- AppKit is a spike until it reaches Rust-backed parity.

## Architecture Health

| Dimension | Current read |
| --- | --- |
| Layering | Good: browser/session/tools/providers/agent modules are separated. |
| Runtime boundary | Good in Tauri: React fails closed when IPC is unavailable; browser runtime still requires native Tauri validation. |
| AppKit boundary | Partial: host messaging exists, but Rust provider/agent parity is deferred. |
| Error handling | Improved: typed tool/provider/agent errors are covered by tests. |
| Real systems | Good: providers are real env-backed adapters; no production mock provider lane should be added. |
| Documentation | Improved by this cleanup; stale pre-spike docs were the main drift risk. |

## Active Strengths

- `./verify.sh` is a single local gate for format, clippy, tests, frontend build,
  Tauri cargo check, and release build.
- CI covers Rust and Tauri lanes and includes guard checks for mock dependency
  drift, merge conflict markers, and iCloud duplicate files.
- The React control surface now exposes tabs, navigation, provider selection,
  policy mode, page stats, agent runs, approvals, and action history through one
  host adapter contract.
- The ADR makes React + Tauri the accepted primary path, preventing the AppKit
  spike from becoming a second source of truth.

## Remaining Risks

| Priority | Risk | Path | Notes |
| --- | --- | --- | --- |
| P1 | AppKit parity gap | `NeuroBrowser/`, `src-tauri/src/hostAdapters.js` | AppKit still returns host-lane placeholder responses for agent parity. |
| P1 | Live desktop smoke gap | `src-tauri/` | Browser-shell layout can be smoke-tested in Vite/Playwright, but real browser behavior needs native Tauri runtime checks. |
| P2 | Frontend test gap | `src-tauri/src/` | Add targeted browser smoke coverage for shell load, provider/policy controls, tab actions, and fail-closed host status. |
| P2 | Provider retry/timeout behavior | `src/providers/` | Keep provider failures explicit and add bounded retries only where the provider contract supports them. |
| P2 | AppKit page identity | `NeuroBrowser/` | Page ids must survive tab close/reorder before AppKit can move past spike status. |

## Dependency Notes

Core dependencies remain small and conventional:

- Rust: `tokio`, `reqwest`, `serde`, `thiserror`, `tracing`, `scraper`, `url`,
  `uuid`, `schemars`, `futures`, `async-trait`
- Frontend: React 19, Vite 6, Tauri API/CLI v2

Run security/dependency checks as part of cleanup work:

```bash
cd src-tauri
npm audit --audit-level=moderate
```

## Recommended Next Steps

1. Add a small frontend smoke test or script that loads `/` and `/appkit.html`
   outside desktop hosts and asserts nonblank fail-closed states.
2. Add native Tauri smoke notes for live navigation and snapshot behavior.
3. Reduce AppKit placeholders by bridging provider selection and snapshots to
   Rust before adding more AppKit UI.
4. Keep `README.md`, `PROJECT.md`, and the ADR in sync whenever the primary lane
   changes.
