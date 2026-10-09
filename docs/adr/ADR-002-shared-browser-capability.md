---
author: codex/Codex
created: '2026-10-04T02:39:35-04:00'
agent: codex/Codex
date: '2026-10-04T02:39:35-04:00'
type: architecture-decision
id: ADR-002
status: accepted
task: Shared browser capability for humans and agents
summary: Add Rust-owned evidence, scoped dispatch and approval contracts on the existing HTTP and desktop paths.
next_steps: [Evaluate runtime gaps against the workload corpus]
remaining: [Screenshots, Background JavaScript, Enforcing desktop subresource boundary]
open_questions: [Which measured workloads justify another execution runtime?]
---

# Shared browser capability

## Decision

Keep the existing HTTP and OS-webview runtimes. Rust owns a versioned capability
manifest, bounded observations, document-scoped action references, policy, private
approval grants and receipts. The human evidence panel and Rust agents use the same
registered tools and governed proposal path. React expresses intent and displays
results; it cannot create a grant by manufacturing a public approval ID.

The first capability contract is additive. `PageSnapshot`, `ToolResult` and
`AgentRunResult` keep their field layouts. Existing adapters default to conservative
capabilities and no scoped interaction. New action tools require explicit support.

Observations cap text and collections, then enforce a 64 KiB serialized envelope.
Authority URLs and document/target identities are never shortened into aliases.
Omitted evidence is explicit. Raw HTML and input values are excluded from the new
observation route. HTTP sanitization also protects its legacy snapshot/query export.

Targets belong to a runtime/document/revision, preserve DOM node identity and compare
relevant state, including form values internally. Property-only form changes are
detected before re-observation refreshes private fingerprints. Validation and dispatch
share one synchronous page evaluation. An old target never silently selects a
replacement node. Reviewed form destinations must match the actual submitter used.

Approvals bind exact calls, original policy and reviewed state, are consumed once and expire after five minutes.
Desktop approval execution uses the latest host policy. Policy updates and governed
operations are ordered with asynchronous write/read leases; a policy update takes
effect when its host command acknowledges installation. Page operations are serialized.
Closing a page drops its host approvals and cancels its outstanding runtime requests.

Receipts separate `not_dispatched`, `acknowledged` and `unknown`, page readiness and
requested predicate verification. URL equality/text presence proves an observed page
condition; it does not prove exactly-once remote effects or server transaction success.
Unknown or unresolved execution stops the agent loop. The human history retains a
dispatched receipt even when the active tab changes while the response is pending.

Runtime replies are bound to the sending native webview label and pending page request.
A mismatched response does not consume the legitimate request. Source URLs come from
the native webview, checked before and after collection. Page reports remain untrusted
evidence. Frozen bridge properties reduce accidental replacement, but main-world
execution is not a hostile-page isolation mechanism.

## Consequences and remaining gaps

The existing desktop supplies modern JavaScript execution and visible human browsing.
HTTP supplies static reading. Clients can discover their capabilities without mistaking
the policy daemon for a live browser. This decision introduces no external engine,
provider dependency or new production fake.

The desktop manifest advertises no screenshots, background JavaScript or enforcing
subresource network firewall. Its navigation guard is not connection-wide confinement.
The current daemon stays a documented stub; exposing a live external MCP/socket route
requires a separately tested session/transport boundary. Runtime selection remains
conditional on measured workloads, rather than a vendor announcement.

Acceptance evidence belongs in Rust negative tests, mounted frontend tests, real Tauri
ACL resolution and the independently authored real HTTP/WebKit corpus. Provider
comparisons require a declared workload and token/tool/cancellation budget first.
