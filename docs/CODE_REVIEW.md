# NeuroBrowser Cleanup Review

Date: 2026-05-22

## Review Result

No current P0 correctness issue was found in the cleanup scope. The stale review
content in this file was itself misleading because it described pre-refactor code
and claimed zero test coverage after the repo had already gained focused Rust
tests and CI.

## Findings

| Priority | Finding | Owner area | Status |
| --- | --- | --- | --- |
| P1 | AppKit agent responses are host-lane placeholders, not Rust-backed agent execution. | `src-tauri/src/hostAdapters.js`, `NeuroBrowser/` | Documented spike gap; do not treat AppKit as primary. |
| P1 | Live Tauri child-webview behavior still needs native runtime smoke coverage after UI changes. | `src-tauri/` | Verify with native Tauri run when working on page rendering; Chrome/Playwright checks only cover the React shell. |
| P2 | Frontend has build coverage but no committed interaction smoke test. | `src-tauri/src/` | Add a focused shell smoke before expanding UI behavior. |
| P2 | Root docs can drift quickly because architecture spans Rust, Tauri, React, and AppKit. | `README.md`, `PROJECT.md`, `docs/adr/` | Keep docs in the same changeset as architecture changes. |

## Current Acceptance Checklist

- Rust formatting passes.
- Rust clippy passes with `-D warnings`.
- Rust tests pass with `cargo test --all-targets`.
- Tauri frontend builds with `npm run build`.
- AppKit control-surface bundle builds with `npm run build:appkit` when AppKit
  surfaces are touched.
- Tauri cargo check passes.
- Release build passes.
- `npm audit --audit-level=moderate` reports no moderate-or-higher findings.
- React-shell smoke confirms nonblank shell, no framework overlay, and
  fail-closed host-bridge messaging outside native hosts.
- Native Tauri smoke confirms real browser page rendering, focus, snapshots,
  and command routing.

## Review Notes

The main architecture rule remains simple: keep one runtime authority. React can
own browser chrome and interaction state, but Rust/Tauri must remain the primary
source for sessions, pages, snapshots, providers, tool calls, action policies,
and approvals. AppKit work should prove native behavior without duplicating
agent runtime ownership.
