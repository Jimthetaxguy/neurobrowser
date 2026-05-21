---
id: ADR-001
status: accepted
updated: 2026-05-21
---

# React + Tauri Is The Primary Frontend Path

## Decision

NeuroBrowser will keep React + Tauri + Rust-owned child webviews as the primary
frontend path for the autonomous browser agent core.

Swift/AppKit remains a parity and native-ceiling lane. Objective-C or Objective-C++
may be added only as narrow interop glue if Swift/Rust/AppKit/WebKit bridging is
materially worse without it.

## Context

The Tauri lane already reaches real Rust sessions, provider selection, snapshots,
browser tools, policy state, and run events. It can preserve the Rust backend as
the authority while React owns the control surface.

The AppKit lane has a higher native browser ceiling through `WKWebView`, AppKit
menus, responder-chain behavior, and native accessibility, but it still lacks
parity with the Rust agent/backend bridge and has a documented page-id/index gap.

## Consequences

- New autonomous agent features should land in the React + Tauri path first.
- Shared command contracts must stay portable enough for the AppKit lane to adapt.
- AppKit work should focus on native parity blockers, not duplicating the primary
  implementation prematurely.
- Objective-C is not a primary frontend language for this project.
