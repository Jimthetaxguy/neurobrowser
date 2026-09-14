# NeuroBrowser — Programmatic Surface

One core, thin clients. Four layers, not four silos.

## Layers

1. **Library (`neurobrowser` crate)** — owns `SessionManager`, `ReActAgent`
   (`execute` / `execute_with_policy` / `execute_approved_tool` /
   `execute_stream`), `ActionPolicy`, `BrowserInterface` plus its two
   impls (`BrowserEngine` scraper path, `TauriBrowserRuntime`), and the
   12-tool `ToolRegistry`. Planned: a `NeuroBrowser<B: BrowserInterface>`
   facade that owns one browser + agent + policy.

2. **Headless daemon** (`src-tauri/src/bin/headless.rs`) — the one running
   process. Newline-delimited JSON-RPC over UDS (TCP fallback). Shipped
   today: `ping`, `policy.get` / `set` / `evaluate` / `snapshot`,
   `snapshot`. Planned: the same session/page/ask/tool/worker surface as
   the desktop app, over the same `SessionManager` objects.

3. **Thin clients** — future MCP server and CLI. Socket clients only;
   they reuse core serde types and do not embed a second engine.

4. **Direct embed** — a Rust program links the crate (or the planned
   facade) with no daemon. Method names stay in lockstep with the RPC
   vocabulary (`ask` / `ask_with_policy` / `resume_approved`).

## Contract

Every surface honors one contract:

1. **Types** — `PageSnapshot`, `ActionPolicy` / `AutonomyLevel` /
   `PolicyDecision` / `PolicyOutcome`, `AgentRunResult` / `AgentRunEvent`,
   `ToolDefinition` / `ToolRisk`, `ProviderConfig`, `WorkerSpec` /
   `WorkerSummary`. Serde-derived in the core crate; no hand-rolled DTOs.
2. **Wire** — `{id, method, params}` → `{id, ok, result|error:{code,message}}`,
   optional `stream` for `run`. Methods take `session_id` / `page_id`.
   Error codes and `@eN` refs: `docs/AGENT-SURFACE.md`.
3. **Policy** — `ActionPolicy::evaluate` is the only gate. The two-call
   approval flow (`ask` → `AwaitingApproval` → `approval.resolve`) is
   identical on desktop, MCP, and CLI.

## Phases

0. **Core contract** — `ref_map` on `PageSnapshot`; per-session policy +
   decision log; ref-based `BrowserInterface` methods; facade.
1. **Daemon wiring** — real navigate/snapshot/tool/ask/run/approval/worker
   methods over `SessionManager`.
1b. **Socket authz** — `SO_PEERCRED` + session ownership check.
2. **Thin clients** — CLI crate and sibling MCP server over the Phase 1
   protocol.
3. **Library polish** — examples, crate docs, pre-1.0 semver.
4. **Real interactivity** — promote `src-tauri` runtime to a `[lib]` so
   headless can use `TauriBrowserRuntime`; close tools that have no Rust
   path yet.
