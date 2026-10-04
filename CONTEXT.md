---
author: codex/Codex
created: '2026-10-04T02:39:35-04:00'
agent: codex/Codex
date: '2026-10-04T02:39:35-04:00'
type: project-context
task: Shared vocabulary and boundaries for Neurobrowser
status: active
summary: Rust owns browsing capability contracts and governed execution; React/Tauri hosts the human surface and OS webviews execute modern pages.
next_steps: [Maintain contracts with their tests]
remaining: [See README capability limitations]
open_questions: [Measure background workloads before choosing another runtime]
---

# Neurobrowser context

Purpose: help humans and their agents make reliable, authorized progress on the web
with evidence that explains what happened. Neurobrowser owns browser control in Rust;
it delegates modern page execution and rendering to an OS webview.

## Vocabulary

| Term | Meaning and owner |
| --- | --- |
| Capability | A versioned runtime claim in `RuntimeCapabilities`; unsupported claims remain false. |
| HTTP engine | `BrowserEngine`: guarded HTTP fetch and HTML parsing; no JavaScript or interaction. |
| Desktop runtime | `TauriBrowserRuntime`: real OS child webview; Rust controls its lifetime and host requests. |
| Page snapshot | Compatibility format containing page content and extraction collections; not an accessibility tree. |
| Observation | `PageObservation`: source URL, bounded text/links/tables, scoped targets, omissions and runtime capabilities. No raw HTML or field values. |
| Document stamp | Runtime ID, per-document random ID and revision. Navigation creates a new document; relevant DOM, URL or field changes invalidate reviewed state. |
| Scoped target | A runtime-issued reference to one observed DOM node. Node identity and relevant state are checked in the same JavaScript turn as dispatch. |
| Proposal | A registered tool call routed through `ReActAgent::propose_tool_with_policy`; human and agent clients share this route. |
| Action policy | Rust `ActionPolicy`: domains, tools, autonomy and approvals. Page text never creates authority. |
| Approval grant | Private agent state bound to the exact run/call/policy and reviewed page/target. IDs are lookup keys, consumed once and expire after five minutes. |
| Dispatch acknowledgment | Runtime reported action execution. It does not prove the website accepted a business operation. |
| Action receipt | `ActionReceipt`: dispatch state, page readiness and observed predicate verification, without submitted input values. |
| Postcondition | Exact URL equality or text presence in a bounded subsequent observation. Evidence of that page condition only. |
| Uncertain dispatch | Action may have executed, but acknowledgment is missing. The run stops and requires inspection. |
| Policy lease | Desktop governed operations hold a read lease; policy installation takes a write lease and acknowledges only after installation. |
| Page operation lock | Rust serialization of host/agent operations on one session-owned page. A human can still interact directly with the real page. |
| Runtime report | Report-only page IPC. Native caller label and request ownership must match; page payload stays untrusted evidence. |
| Persistent memory | `neuro_memory::MemoryService`: captured page index, governed separately by capture policy. |
| Headless daemon | Existing JSON-RPC policy stub. It does not browse and does not implement the new live capability surface. |

## Module boundaries

- `src/capability/`: versioned evidence and receipt contracts, bounds and scoped tools.
- `src/agent/`: provider loop, shared proposal path, policy and private approval grants.
- `src/browser/`: HTTP engine, sanitization/extraction and tool registry.
- `src/session/`: session/page ownership and per-page operation serialization.
- `src/netguard/`: HTTP connection resolver/redirect checks and desktop navigation preflight.
- `src-tauri/src/runtime.rs`: OS-webview adapter, bounded DOM observation, target dispatch and request correlation.
- `src-tauri/src/main.rs`: trusted control IPC, policy leases, pending human approvals and memory capture.
- `src-tauri/src/EvidencePanel.jsx`: human evidence and explicit tool proposals; no browser authority in React.
- `crates/neuro-memory/`: persistent page memory, distinct from transient run events and grants.

## Stable decisions and verification

React/Tauri is primary; AppKit remains a parity lane ([ADR-001](docs/adr/ADR-001-react-tauri-primary.md)).
Shared contracts stay independent of a vendor engine ([ADR-002](docs/adr/ADR-002-shared-browser-capability.md)).
No Lightpanda runtime or implementation is adopted.

The browser registry contains 22 tools, or 24 with persistent-memory tools attached.
External Rust clients use the shared proposal/approval API; Tauri provides the human
control IPC. The policy daemon remains a stub, and there is no new MCP transport.

Desktop screenshots, background JavaScript and a firewall enforcing every subresource
connection remain unavailable. The main-world runtime script is not a hostile-page
sandbox: provenance/correlation/freshness improve evidence and prevent tested stale or
cross-page actions, but cannot make arbitrary website JavaScript trusted.

`./verify.sh` runs Rust, frontend, Tauri ACL, native navigation and genuine WebKit
workload checks. `tests/capability_observation.rs` includes a separately invoked public
HTTPS integration test. Credentialed model-provider smoke tests require an explicit run.
