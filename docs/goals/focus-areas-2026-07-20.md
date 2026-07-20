# NeuroBrowser — Key Focus Areas

_2026-07-20. Synthesized from the code audit (69 optimizations), the net-new feature backlog
(18 items), the programmatic-surface design, and external real-world validation
(`macos-mcp-rs` review). Five focus areas, dependency-ordered, each grounded in committed
artifacts. This is the strategic lens; the backlogs are the task-level detail._

**The one-line thesis:** NeuroBrowser is a genuinely differentiated seam — a native, local-first,
**policy-governed** agentic browser — but today it is more *designed* than *proven*. The focus is
to make it (1) actually callable, (2) actually functional, (3) provably safe, (4) genuinely smart,
and (5) measurable — in that dependency order.

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

## How they relate (dependency + priority)

```
FA-5 (eval)  ── underpins ──►  everything (measure before you claim)
FA-2 + FA-4  ── make it genuinely work (functional + smart)
FA-1         ── make it usable by ROSA / agents (the near-term ask)
FA-3         ── the differentiator to lead with (safety, measurable)
```

**Recommended near-term sequence** (ordering, not time estimates):
1. **Finish the correctness P0s** (SG3b) — cheap, closes FA-2/FA-4 honesty gaps.
2. **Programmatic surface Phase 0** (FA-1) — the facade + the `ref_map` serde fix; also unblocks FA-2.
3. **Per-tool-risk wiring + fail-safe default** (FA-3) — small, externally validated, high-trust win.
4. **Minimal eval harness** (FA-5) — stand up the TaskSpec catalog early so steps 5+ are gated by it.
5. **Daemon wiring Phase 1** (FA-1) — the real programmatic payoff, once Phase 0 + a smoke eval exist.

Native provider tool-calling (FA-4) is the biggest single quality lever but is a larger refactor —
track it as its own sub-project rather than a single autoresearch iteration.
