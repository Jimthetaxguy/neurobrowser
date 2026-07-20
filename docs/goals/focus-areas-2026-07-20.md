# NeuroBrowser — Key Focus Areas

_2026-07-20. Synthesized from the code audit (69 optimizations), the net-new feature backlog
(18 items), the programmatic-surface design, and external real-world validation
(`macos-mcp-rs` review). **Expanded 2026-07-20** with an 8-dimension end-to-end quality audit
(9-agent workflow, all findings grounded in file:line evidence) — adding **FA-6…FA-10** and
enrichments to FA-1…5. Ten focus areas, dependency-ordered, each grounded in the codebase.
This is the strategic lens; the backlogs are the task-level detail._

**The one-line thesis (original):** NeuroBrowser is a genuinely differentiated seam — a native,
local-first, **policy-governed** agentic browser — but today it is more *designed* than *proven*.

**The revised thesis (expanded, end-to-end):** the loop *runs*, but it cannot yet be **cancelled,
bounded, persisted, observed, configured, or built off the author's Mac**. The goal is to make
NeuroBrowser not only callable, functional, safe, smart, and measured (FA-1…5) but also **robust,
durable, operable, distributable, and structurally sound** (FA-6…10) — so the differentiator can
actually run unattended and be trusted end to end. FA-1…5 make it *good*; FA-6…10 make it *real*.

---

## FA-1 — Make it callable (Programmatic Surface) 🎯 near-term thrust

**Why.** The whole point for ROSA / external agents / automation is driving the browser
programmatically. Today only the in-process Rust library works; the headless daemon is a
**policy-only stub** (`snapshot` returns a hardcoded `about:blank`, no `navigate`/`ask`/tool
methods, not wired to a real engine). This is the capability James explicitly asked for.

**What's in it.** The [programmatic-surface design](../specs/programmatic-surface-design-2026-07-20.md):
one core + a `NeuroBrowser` facade → **wired daemon** (the single running process) → **MCP + CLI**
as thin socket clients → **stabilized library API**. Subsumes the daemon-wiring gap, NB-12
tool-profile scoping, CLI v0.2, the socket-authz follow-up, and NB-3 credential conventions.

**State.** Designed, not built. **Phase 0** (contract hardening — the `ref_map` serde fix + the
facade) is the concrete next step and also unblocks FA-2.

**Outcome.** `neurobrowser navigate|ask` from a shell; an MCP tool ROSA/Claude can call directly;
a stable Rust facade — all over one shared contract and one policy gate.

---

## FA-2 — Make the browser actually *do things* (Real Interactivity & Engine Honesty)

**Why.** An agent that reports success without acting is worse than one that errors. Today
`BrowserEngine`'s `click`/`type_text`/`submit_form`/`scroll_*` are **silent no-ops that return
success**; 5 of 12 advertised tools have no working Rust path; the JS-built `@eN` ref-map is
**dropped by serde** (no field on `PageSnapshot`) even in the Tauri path; a `clickRef`/`typeTextRef`
bridge exists in JS with zero Rust callers.

**What's in it.** Fix the no-op interactive tools or make them honestly error
(`effective_under_engine`); the `ref_map` serde fix; wire the dead ref-based JS bridge; the
`src-tauri` `[lib]` refactor for a real-webview headless mode (design Phase 4); close the 5
non-working tools. (Optimization backlog: `noop-interactive-tools`, `unique-selectors`,
`table-content`; design Phases 0/4.)

**State.** The Tauri desktop path is real (`runtime.rs`); the scraper engine is read-only + no-op
interactive. Honesty first (don't lie about success), real interactivity second.

**Outcome.** Every advertised tool either works or honestly reports it cannot, on both engines.

---

## FA-3 — Policy-scored autonomy as the differentiator (Safety & Trust)

**Why.** This is NeuroBrowser's **defensible edge** — the only surveyed system shipping a 3-level
`ActionPolicy` + per-domain allow/deny + injection detection + redaction together. No competitor
benchmark (WebArena/Mind2Web/WebVoyager) even measures policy compliance. Recently reinforced by
external review (`macos-mcp-rs`) and by SG2's SSRF/injection hardening.

**What's in it.** Already shipped: SG2 closed all 6 security P0s. Remaining, high-leverage:
**wire per-tool `RiskLevel` into ActionPolicy** (P1 `dead-risklevel`, externally validated) +
**fail-safe default tool risk** (P2, currently defaults unclassified tools to low-risk Read —
fail-open); **value-based redaction** (P1); credential vault (NB-3); durable policy audit log;
per-run rate limiting; and the **Completion-under-Policy** scoring (NB-6) that makes the edge
publishable.

**State.** Strong foundation (policy engine shipped; 6 security P0s fixed). Gaps in per-tool
wiring and, crucially, measurement.

**Outcome.** "Policy-compliant task completion" becomes a published, benchmarked metric no one
else has.

---

## FA-4 — A ReAct loop worth trusting (Agent-Loop Quality)

**Why.** The loop is the core product. It works but is bare-bones: **text-parsed** tool calls
(fragile) instead of provider-native JSON tools; no plan/reflect staging; the model is fed
**element counts, not real selectors** (`dom_snapshot`/accessibility_tree computed then ignored);
tracing spans and memory are half-wired; streaming reaches no surface.

**What's in it.** Native provider tool-calling (P0 gap — the biggest single agent-quality lever);
plan/act/reflect staging; wire real selectors + `dom_snapshot` into the prompt (SG3b); wire the
dead tracing spans + bound episodic memory; drive `StreamEvent`s to a surface; per-role
planner/actor LLM split (NB-11).

**State.** Functional flat loop; several good structures defined but unwired.

**Outcome.** A loop that plans, reflects, uses structured tool calls, remembers, and streams —
observably.

---

## FA-5 — Prove it works (Evaluation & Verification)

**Why.** Everything above is unverifiable without an eval harness. Today: 8 ad-hoc integration
tests (now 60 total unit/integration), **no task catalog, no real-LLM e2e** — while every
competitor benchmark ships hundreds of tasks. This is the foundation that lets you *claim* the
differentiator (FA-3) and catch regressions across all the other areas.

**What's in it.** TaskSpec catalog + `neurobrowser-eval` harness (NB-5); Completion-under-Policy +
Risk Ratio scoring (NB-6); real-LLM budget-capped CI + live-site drift subset (NB-14); offline
fixture capture for deterministic runs.

**State.** 60 tests, green gate. No task catalog or real-LLM e2e.

**Outcome.** "Does the agent still work — and stay policy-compliant?" is one command with
diffable, machine-readable output.

---

## FA-6 — Runtime Robustness & Resource Lifecycle (make failure bounded) 🆕

**Why.** FA-1 wants this to run as an unattended daemon and FA-5 wants to measure it — but today
a run has **no wall-clock deadline and no working cancel**; `cancel_agent_run` only removes a
pending-approval map entry ([main.rs:453](../../src-tauri/src/main.rs)) while the sole implemented
deadline wrapper `execute_with_timeout` is **dead code with zero callers**
([streaming.rs:78](../../src/agent/streaming.rs)). One panic while holding any `SessionManager` /
`BrowserRuntimeRegistry` lock **permanently poisons it** (`.lock().unwrap()` repo-wide, no poison
recovery anywhere — [session/mod.rs:46](../../src/session/mod.rs)). `EpisodicMemory` grows without
bound while its sibling `CrossWorkerObservations` is windowed at 200; there is no `close_session`.
Concurrent `execute_with_policy` on the same agent interleave shared-state writes with no
exclusivity guard. `BrowserEngine::navigate` calls a **blocking** reqwest client inside `async`
([browser/mod.rs:162](../../src/browser/mod.rs)).

**What's in it.** Wire `execute_with_timeout` into `start_agent_run`/`ask` + a real `AbortHandle`
cancel; poison-safe locks (`unwrap_or_else(|e| e.into_inner())` or `parking_lot`); one bounded
retry-with-backoff that actually honors `ToolError.retryable` / `RateLimited`; move `navigate` off
blocking reqwest; collapse the 2M+1 per-turn snapshots to M+1; cap `EpisodicMemory` + inbox; add
`close_session`/worker eviction; a run-exclusivity `try_lock` guard; stop `wait_for_navigation`
swallowing page-load timeouts as `Ok(())`.

**State.** Good patterns exist in isolated spots (Result propagation, non-await-spanning locks,
per-provider 30s timeouts, a windowed observation buffer) — simply not applied to the runtime as a
whole. **Outcome.** A daemon that runs unattended for hours: cancellable, deadline-bounded,
panic-isolated, flat-memory, exactly-one-run-per-agent.

---

## FA-7 — Durable State & Crash Recovery (a source of truth beyond RAM) 🆕

**Why.** FA-1's long-running daemon and FA-3's "durable policy audit log" + completion-under-policy
metric both presume state that outlives a crash — yet a repo-wide grep for
`fs::write|serde_json::to_writer|sled|rusqlite|persist` returns **nothing**. `SessionManager`,
`AgentMemory`, and `pending_approvals` are pure process memory
([session/mod.rs:9](../../src/session/mod.rs)); `kill -9` silently discards every session,
conversation, in-flight approval, and audit event, and resets page ids to 0.

**What's in it.** A minimal durable store (sqlite/sled) behind `SessionManager`'s existing API and
behind `ReActAgent` for episodic memory + `pending_approvals`, keeping the interior-mutability
contract so **no call site changes**; restart-stable ids (persisted high-water mark); materialize
FA-3's policy-decision audit log as the first table.

**State.** Zero persistence anywhere in either crate. **Outcome.** `kill -9` then restart, and
sessions, pending approvals, agent memory, and the policy trail are all still there with stable ids
— the precondition for both an unattended daemon and a publishable policy-compliance metric.

---

## FA-8 — Operability: Observability & Configuration (make a failed run diagnosable) 🆕

**Why.** You cannot operate or tune a policy-governed agent whose 698-line ReAct loop emits **zero
tracing spans** (the implemented `llm_call_span`/`tool_call_span`/`agent_iteration_span`/
`CorrelationContext` have no callers — [observability.rs:46](../../src/agent/observability.rs)),
whose `PolicyDecision` reasons/risk_flags are discarded before any log sees them, whose
`AgentMetrics` has no reader, whose `RUST_LOG` **does nothing** (the env filter is a hardcoded
string literal, not `EnvFilter::from_default_env()`), and whose `ProviderConfig.base_url` is read
from env but then **ignored** — OpenAI/Anthropic hardcode the request URL
([openai.rs:55](../../src/providers/openai.rs), [anthropic.rs:103](../../src/providers/anthropic.rs)).

**What's in it.** Wire the dead spans + correlation ids into `execute_with_policy`; log every
`PolicyDecision` at production time; expose `AgentMetrics::snapshot()` via a command/daemon method;
add `AiResponse.usage` token plumbing (today every provider discards the API usage object); route
real failures through `ToolError` so `code`/`retryable` survive to the caller; fix `RUST_LOG`
(`from_default_env`); **wire `base_url`** into the request URL (honesty defect, same class as FA-2);
centralize `timeout_secs`/`max_tokens`/`temperature`/`max_iterations` as env-overridable config
instead of constants duplicated across 3-4 sites.

**State.** A full observability module, a structured `ToolError`, and `AgentMetrics` all **exist but
are disconnected** from the code they were built to instrument (only 16 tracing sites across 4,663
LoC, 10 in the read-only scraper). **Outcome.** `RUST_LOG=debug` surfaces every LLM call, tool
dispatch, and policy verdict with correlation ids; one command reads request/error/token counts;
one config surface sets keys, endpoints, and per-run budgets without a rebuild.

---

## FA-9 — Distribution, Packaging & Integrator Trust (everything it claims is true) 🆕

**Why.** FA-1's "stabilized library API" has nothing honest to stabilize while `SKILL.md` documents
a builder API that **does not exist** (`ActionPolicy::read_only()`/`.with_allowed_domains()`/
`PolicyDomain` — none exist; `ActionPolicy` is a plain struct + `Default`). The headless daemon (the
FA-1 deliverable) is **never compiled or unit-tested by CI** in any config and is **broken on
Windows** (unconditional `UnixListener` + unix `SignalKind` — [headless.rs:43](../../src-tauri/src/bin/headless.rs));
CI's only `src-tauri` step is a macOS `cargo check` with no `--features headless`. No release
automation exists despite `tauri.conf.json` declaring bundle targets `"all"` + a Windows signing
stanza. Two unlinked crates with independent `Cargo.lock` files; near-zero rustdoc; no `examples/`.

**What's in it.** Rewrite `SKILL.md`/`README`/examples so every snippet compiles against the real
API (or add the promised builder methods); crate-level + item rustdoc on the ~30 re-exported public
items; a compiling `examples/basic_usage.rs` wired into `verify.sh`; `#[cfg(unix)]`-gate + a
`#[cfg(windows)]` `ctrl_c` fallback in the daemon; `windows-latest` + a webkit2gtk-provisioned
`ubuntu-latest` CI job that builds/tests `--features headless`; a tag-triggered `tauri-action`
release workflow; a root `[workspace]` collapsing the two lock/target trees; portable `verify.sh`
(drop the hardcoded `/tmp/neurobrowser-tauri-target`).

**State.** One macOS-only CI job that never compiles the daemon; docs describing a fictional API.
**Outcome.** Green multi-OS CI that actually builds + unit-tests the daemon and desktop crate on
Windows/Linux/macOS; tag-triggered signed releases; docs/examples whose every snippet compiles.

---

## FA-10 — Code Health & Internal Structure (a substrate that absorbs the other FAs) 🆕

**Why.** The FA-2/FA-4/FA-6/FA-8 changes must repeatedly touch a **1,305-line** `browser/mod.rs`,
a **221-line** `execute_with_policy` fusing six responsibilities (state-reset, LLM call, memory,
tool resolution, 3-way policy branch, metrics — with its unknown-tool block duplicated near-verbatim
and an `AgentRunResult{…}` literal recurring 6×), **four** independently hand-rolled
`BrowserInterface` fakes, and **two field-for-field-identical `ToolResult` structs** kept apart by
an `as AiToolResult` alias ([tools/mod.rs:15](../../src/tools/mod.rs)) — so every functional change
is amplified and untested at the unit level (53 of 61 tests are black-box). A vestigial `thiserror`
hierarchy + unused `futures`/`schemars` deps sit dead beside 55 stringly-typed `Result<_,String>`.

**What's in it.** Collapse the two `ToolResult` into one; a shared `test_support::FakeBrowser`
(delete Noop/Test/Mut/Stub); decompose `execute_with_policy` into named steps + an `AgentRunResult`
constructor; split `browser/mod.rs` into engine/ssrf/extract/tools (a macro over the 13 near-identical
`BrowserTool` impls); inline unit tests next to `policy.rs`/`session`/`worker`; adopt-or-delete the
dead error hierarchy; remove unused deps; reconcile the two unrelated types both named `SessionState`.

**State.** 8,231 LoC, giant files, mostly black-box tests, duplicate types, dead error hierarchy.
**Outcome.** Structure that matches the logic — keeping FA-2/FA-4/FA-6/FA-8 diffs small, local, and
unit-testable instead of forcing four-file edits per change.

---

## Enrichments to the original FA-1…5

The expansion also **deepened** the first five areas rather than only adding new ones:

- **FA-1 (Programmatic Surface).** The daemon's **credential** path is unwired, not just its tool
  surface — provider env vars are read only in the Tauri binary; `headless.rs` names `create_provider`
  only to silence dead-code and builds a `model:"stub"` config. The TCP fallback has **zero auth**
  (no `chmod`, no secret) so any local process can send `policy.set` and escalate autonomy. And the
  "stable API" has **no honest docs to stabilize** (see FA-9).
- **FA-2 (Engine Honesty).** The "reports success but no-ops" pattern extends to **`base_url`** (read
  into config, ignored by OpenAI/Anthropic — same defect one layer down) and quantifies a hidden
  cost: because the no-op tools never mutate the page, the loop's **2M+1 full snapshots per turn are
  demonstrably redundant** (pairs with FA-6's snapshot collapse).
- **FA-3 (Policy autonomy).** Fail-open is the **boot state**, not just per-tool: every process starts
  at `ActionPolicy::default()` (Assisted, empty allow/deny) and `evaluate()` skips the allowlist when
  it's empty — a fresh daemon is domain-unrestricted until someone calls `policy.set`. The durable
  audit log is blocked twice: no persistence (FA-7) **and** the `PolicyDecision` is computed then
  thrown away (never logged/stored).
- **FA-4 (ReAct loop).** "Spans/memory half-wired" resolves to concrete dead code (spans, correlation,
  `execute_with_timeout`, uncapped `EpisodicMemory::push`). Token/cost visibility is **structurally
  impossible** today (`AiResponse` has no `usage` field; every parser discards the API usage object)
  — which blocks the planner/actor split and any budget cap.
- **FA-5 (Evaluation).** An entire **modeled-but-uncalled** subsystem needs eval scope: the
  worker/observation/inbox state machine has **no production caller** (only `tests/workers.rs`), so
  its real concurrency behavior has never been observed. And the 60-test gate is **53/61 black-box**
  — every core logic module has zero inline unit tests.

---

## How they relate (dependency + priority — expanded)

The original ordering (eval underpins; FA-2/4 make it work; FA-1 makes it usable; FA-3 leads) still
holds for FA-1…5. The expansion adds a **substrate-first insight**: FA-6/FA-7/FA-8/FA-10 are the
foundation the functional/differentiator work (FA-1…5) must land *on*. Doing functional depth before
the substrate means building on a runtime that can't be cancelled, persisted, observed, or built off
one Mac.

```
FA-10 (structure) ─ shrinks the blast radius of every other FA
FA-8  (observe)   ─ makes FA-6/7 debuggable instead of black boxes
FA-6  (robust)    ─ bounds + isolates the runtime  ── precondition for FA-1 daemon + FA-5 eval
FA-7  (durable)   ─ state survives restart          ── precondition for FA-1 daemon + FA-3 audit log
FA-9  (ship)      ─ builds + releases where it claims to run
FA-3  (secure)    ─ boots restricted + logs verdicts (needs FA-7 store, FA-8 config)
FA-1  (callable)  ─ wire the surface once the runtime is robust/durable/observable/secured
FA-2/4/5 (depth)  ─ close no-ops, native tool-calls, eval catalog ── on solid ground
```

**Re-sequenced roadmap** (dependency-ordered; supersedes the original 5-step near-term list):

1. **Enabling code-health slice** (FA-10 subset) — merge the two `ToolResult`, extract a shared
   `FakeBrowser`, decompose `execute_with_policy`. First, because it shrinks every downstream diff.
2. **Observability + config wiring** (FA-8) — wire the dead spans, log `PolicyDecision`, expose
   metrics, fix `RUST_LOG`, wire `base_url`, centralize budgets. Cheap (scaffolding exists) and it
   makes steps 3-7 debuggable. _(base_url + RUST_LOG land as the immediate next autoresearch iter.)_
3. **Runtime robustness** (FA-6) — cancellation + deadline, poison-safe locks, run-exclusivity,
   snapshot collapse, growth caps, eviction, blocking-client fix. Prereq for the daemon + eval.
4. **Cross-platform daemon + CI** (FA-9 part 1) — cfg-gate the daemon + Windows fallback, add
   windows/ubuntu CI compiling `--features headless`, add the root `[workspace]`.
5. **Durable state** (FA-7) — the sqlite/sled store, now that state is bounded (3) + consistent (3).
   Materializes FA-3's durable audit log using step 2's `PolicyDecision` logging.
6. **Close fail-open + secure the socket** (FA-3) — startup policy source (boot restricted) + auth on
   both Unix and TCP control paths. Needs step 5 (durable policy) + step 2 (config).
7. **Wire the callable surface + ship** (FA-1 + FA-9 part 2) — daemon credential/dispatch path, real
   API docs + rustdoc + examples, tag-triggered release. FA-1 lands on solid ground, not a stub.
8. **Functional depth + proof** (FA-2, FA-4, FA-5) — close the no-op/`ref_map`/`base_url` honesty
   gaps, native tool-calling + plan/reflect + token plumbing, finish the FA-10 split, build the
   TaskSpec catalog (incl. a live multi-worker task + an inline unit layer). The differentiator,
   finally measured end to end.

> **Note on the original near-term list.** SG3b's interactive-tool honesty is **done**
> (`ce29e7a`), and its second half — wiring real page structure/selectors into the prompt — is a
> step-8 (FA-2/FA-4) item. `base_url` + `RUST_LOG` (step 2) are the immediate next iteration.
> Native provider tool-calling (FA-4) remains the biggest single quality lever and its own
> sub-project. This roadmap is ordering, not time estimates.
