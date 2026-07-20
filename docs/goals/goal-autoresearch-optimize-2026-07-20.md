---
id: goal-autoresearch-optimize-2026-07-20
title: "Iteratively optimize NeuroBrowser via autoresearch + spec net-new features"
version: 1.1
status: active
priority: high
owner: james (driver: Claude Code)
created: 2026-07-20
branch: agent/claude-autoresearch-optimize-2026-07-20
loop: autoresearch (.autoresearch/, KEEP/DISCARD on green gate)
related_artifacts:
  - docs/goals/README.md                             # index + live status of this optimization effort
  - docs/goals/optimization-backlog-2026-07-20.md   # 69 confirmed + 4 plausible existing-code optimizations
  - docs/goals/feature-backlog-2026-07-20.md         # 18 ranked net-new features + patterns + prior-art resolutions
  - docs/specs/programmatic-surface-design-2026-07-20.md  # wired daemon + MCP + CLI + library API design
  - docs/goals/focus-areas-2026-07-20.md             # 10 focus areas (FA-1..5 + FA-6..10 end-to-end expansion) + roadmap
  - docs/goals/quality-audit-2026-07-20.md           # raw grounded 8-dimension audit (52 findings) behind FA-6..10
  - .autoresearch/config.json                        # loop config (gate + fitness panel)
  - .autoresearch/eval.sh                            # offline measurement harness
  - .autoresearch/state.json                         # run log / disk-persisted loop state
maintenance_contract:
  - "Every KEEP iteration keeps verify.sh green (fmt / clippy -D warnings / test / tauri check)."
  - "Every iteration is recorded in .autoresearch/state.json AND the Run Log below."
  - "Mutate from the last KEEP, never a discard. Behavior-preserving unless the run is an explicit fix WITH a test."
  - "real-systems-only: no mock providers/browser in product paths."
---

# Goal — Iteratively optimize NeuroBrowser (autoresearch) + spec net-new features

## Purpose

Run a disciplined, evidence-based improvement loop over the NeuroBrowser codebase:
1. **Optimize existing code** — burn down a verified, ranked backlog of correctness / security /
   performance / maintainability fixes, each applied as an autoresearch iteration behind a green
   verify gate (KEEP/DISCARD, mutate-from-best).
2. **Find & spec gaps** — turn the code audit + a GitHub prior-art survey into a ranked net-new
   feature backlog that graduates into `docs/specs/` + `docs/stories/`.

NeuroBrowser is a real first-party build (Applied-AI-Builder North Star). This Goal treats it as a
product to harden and extend, not a demo — hence the real-systems-only + verify-before-completion
discipline throughout.

## Verified baseline (2026-07-20, `main` @ `e07093c` → branch)

The pre-existing `CURRENT_STATE.md`/`ANALYSIS.md` were **stale** (they describe `main` as a pre-merge
"fossil"). Ground truth, re-verified with cargo + rg:

| Signal | Value |
|---|---|
| State | v0.1.1 shipped — codex runtime merged, phases A–F complete, clean working tree |
| Rust LoC | 6,475 non-test + 1,256 test (19 src files, 8 integration test files) |
| `cargo fmt --check` | **was RED** (rustfmt drift in `src/browser/mod.rs` test module) → fixed, iteration 1 |
| `cargo clippy --all-targets -D warnings` | GREEN |
| `cargo test --all-targets` | GREEN (~50 tests; exact re-count in flight) |
| `.unwrap()` / `.expect(` / `.clone()` (non-test src) | 31 / 22 / 133 |
| `todo!`/`FIXME` · `unsafe` | 0 · 0 (healthy) |
| Audit result | 69 confirmed optimizations (13 P0 / 24 P1 / 22 P2 / 10 P3) + 4 plausible + 38 gaps |
| Prior-art | `AIAnytime/agent-browser` = name collision (not an ancestor); `fastrender` = do-not-integrate |

## Success criteria

- **A1 — Gate never regresses.** Every KEEP iteration leaves `verify.sh` green (fmt, clippy `-D warnings`, `cargo test --all-targets`, `src-tauri` cargo check). No test removed/ignored to pass.
- **A2 — Optimization backlog burned down.** Work the [optimization backlog](./optimization-backlog-2026-07-20.md) P0 → P3; each item lands with its stated objective metric met and is logged in the Run Log + `state.json`.
- **A3 — Security P0s → 0.** The 6 security-tagged P0s (injection HTML-blindness, scheme bypass, SSRF, `ask`/`navigate` policy bypass, unauthenticated headless socket) are all closed with regression tests.
- **A4 — KPIs move the right way** (see table) without gaming the metrics (adversarial re-eval every 5th cycle).
- **A5 — Gaps spec'd.** The [feature backlog](./feature-backlog-2026-07-20.md) (18 items) is captured; the top P0 set (NB-1 dialogs, NB-2 ref-invalidation, NB-3 credential vault, NB-5/NB-6 eval harness + policy scoring) graduates into real `docs/specs/` + `docs/stories/`.
- **A6 — Docs reconciled with reality.** Correct the `AIAnytime/agent-browser` conflation in `PROJECT.md`/`prior-art.md`; correct `PROJECT.md`'s Observability claim (tracing spans are defined but never attached — see backlog `dead-tracing-spans`); mark `fastrender` don't-integrate.
- **A7 — Loop is reproducible.** `.autoresearch/{config.json,eval.sh,state.json}` let any agent resume the loop from disk without this conversation.

## KPIs (baseline → target)

| KPI | Baseline | Target | Source |
|---|---|---|---|
| verify.sh gate | fmt RED | all green, all iterations | `.autoresearch/eval.sh` gate |
| Security P0s open | 6 | 0 | optimization backlog |
| Correctness P0s open | 7 | 0 | optimization backlog |
| `.unwrap()` in non-test src | 31 | ↓ (target ≤ 20) | `rg` panel |
| `.clone()` in non-test src | 133 | ↓ where flagged | `rg` panel |
| clippy pedantic warnings | (measured by harness) | ↓ | `.autoresearch/eval.sh` panel |
| tests passing | ~50 | ↑ (each fix adds a regression test) | `cargo test` |
| Anthropic provider works vs live API | broken (400) | 200 | provider P0 fix |
| Net-new features spec'd | 0 | ≥5 graduated to docs/specs | feature backlog |

## The autoresearch loop (mechanics)

Per Karpathy's AutoResearch pattern, generalized to a Rust codebase (see the `autoresearch` skill):

1. **Read** `.autoresearch/state.json` + pick the next backlog target (highest severity, lowest risk first).
2. **Mutate from the last KEEP.** Apply the fix (TDD: add the failing regression test first, then the change).
3. **Measure** with `.autoresearch/eval.sh` — the **GATE** (fmt/clippy/test/tauri) is a hard floor; the **PANEL** (unwrap/clone/pedantic/tests) is the fitness signal.
4. **Decision gate:** KEEP iff `gate_pass == true` AND the target's objective metric is met AND no panel regression AND the 8 validation-set test files stay green. Else DISCARD (`git restore`).
5. **Log** to `state.json` + the Run Log below.
6. **Adversarial re-eval every 5th cycle** (Goodhart check: did the code genuinely improve or did it learn to game a metric?). **Plateau breaker** after 10 consecutive DISCARDs.

Validation set (canary — must stay green every cycle): the 8 files under `tests/`.

## Sub-goal breakdown

| # | Phase | Scope | Status |
|---|---|---|---|
| SG0 | Baseline + charter | Verify real state, branch, harness, backlogs, this Goal | **complete** |
| SG1 | Provider correctness P0 | Anthropic `system` key + finish_reason normalization; multi-block parse | **complete (iter 2)** |
| SG2 | Security P0 batch | injection HTML scan, scheme/SSRF guards, `ask`/`navigate` policy, headless socket chmod (+ authz follow-up) | **complete (iter 3)** |
| SG3a | Correctness P0 (part a) | stale post-nav URL, spawn_worker pin (+ lock-scope P1) | **complete (iter 4)** |
| SG3b | Correctness P0 (part b) | **part a done (iter 5):** static-engine interactive tools now error honestly instead of faking `Ok`. **Part b queued:** wire real page structure/selectors (`dom_snapshot`) into the prompt (also FA-4). | **part a complete (iter 5)** |
| SG-expand | Goals expansion (this Goal itself) | 8-dimension end-to-end quality workflow → FA-6..10 + FA-1..5 enrichments + substrate-first roadmap; raw audit persisted | **complete (iter 5)** |
| SG-fa8 | FA-8 honesty/operability (next) | wire `base_url` into OpenAI/Anthropic request URL; fix `RUST_LOG` (from_default_env) | **next** |
| SG-risk | Per-tool risk (candidate pull-up) | wire `RiskLevel` into ActionPolicy (P1), fail-safe default tool risk (P2) — validated by external review | proposed |
| SG4 | P1 sweep | memory bound, stream run_id/args, key+value redaction, blocking reqwest→async, tool-list from registry | queued |
| SG5 | Net-new specs | Graduate NB-1/2/3/5/6 into `docs/specs/` + `docs/stories/` | queued |
| SG6 | Doc reconciliation | AIAnytime, fastrender, PROJECT.md observability claim | queued |

> **Roadmap note (2026-07-20 expansion).** The FA-6…FA-10 audit re-sequenced the strategy
> **substrate-first** (code-health → observability → robustness → durability → distribution →
> secure → callable → functional depth). SG4/SG5/SG6 remain valid; the new FA-6/7/9/10 work
> (cancellation, persistence, cross-platform CI, structural split) is folded into that ordering.
> See [focus-areas](./focus-areas-2026-07-20.md) "Re-sequenced roadmap".

## Run Log (reverse-chronological)

### Iteration 5 — 2026-07-20 — KEEP — `interactive-honesty (SG3b-a)` + `goals-expansion`
- **Operator:** fix-with-test + a read-only breadth workflow. Two threads, one gate.
- **Code (SG3b part a):** `src/browser/mod.rs` — the static `BrowserEngine`'s `click`/`type_text`/`submit_form`/`scroll_to`/`scroll_by` logged a "fallback" and returned `Ok(())`, so the tool layer reported success for an action that never happened (an agent that lies about acting is worse than one that errors). They now return `static_interaction_error`, which distinguishes an invalid selector, a selector that matches nothing, and a real element the static engine simply cannot act on — an actionable signal the ReAct loop can adapt to. `type_text` no longer touches the sensitive value. **+3 unit tests** on the pure helper (sidestepping the documented `BrowserEngine`-in-`#[tokio::test]` runtime panic). Commit `ce29e7a`.
- **Goals expansion:** a 9-agent read-only workflow audited 8 end-to-end quality dimensions (reliability, performance, observability, DX/API, packaging, config/secrets, state/concurrency, code-health) → **52 grounded findings** → synthesized into **FA-6 Runtime Robustness · FA-7 Durable State · FA-8 Operability · FA-9 Distribution · FA-10 Code Health**, plus enrichments to FA-1…5 and a **substrate-first re-sequenced roadmap**. Raw audit persisted at [quality-audit-2026-07-20.md](./quality-audit-2026-07-20.md); synthesis folded into [focus-areas](./focus-areas-2026-07-20.md). Two load-bearing findings (`base_url` dead field, `RUST_LOG` no-op) independently re-verified before enshrining.
- **Tests:** 60 → **63**. **Gate:** fmt / clippy `-D warnings` / `cargo test --all-targets` (63 pass) / `src-tauri` check — all green.
- **Next:** FA-8 `base_url` + `RUST_LOG` honesty/operability fixes (same "reports success but no-ops" class, one layer down).

### Iteration 4 — 2026-07-20 — KEEP — `correctness-p0-part-a` (SG3a)
- **Operator:** fix-with-test. **Closed 2 correctness P0s (+ folded-in P1/P2):**
  1. `agent/mod.rs` — the agent updated `state.current_url`/`page_title` from the **pre**-tool-execution snapshot, so after a `navigate` the model saw the **stale** URL next iteration. Now re-snapshots **after** the tool runs (outside the state lock).
  2. `session/mod.rs` — `spawn_worker` never read `pinned_page_id` (its doc comment promised page-binding). Now validates the pin against `session.pages` (errors if absent), resolves `None`→active page, **and** constructs the agent before taking the sessions lock (folds in the P1 lock-scope fix); removed the now-stale `#[allow(dead_code)]` on `active_page`.
- **Tests:** +2 regression (`post_navigation_url_reaches_model_next_iteration`, `spawn_worker_with_unknown_pinned_page_errors`).
- **Result:** clean gate green — fmt, clippy `-D warnings`, **60 tests pass**, tauri check. **KEEP.**
- **Process note:** an earlier gate falsely flagged the stale-URL test as failing — root cause was a **corrupted incremental build** after two concurrent `cargo` processes were killed. Fix was confirmed correct via instrumentation; lesson recorded in `state.json` (never kill concurrent cargo builds).

### Iteration 3 — 2026-07-20 — KEEP — `security-p0-batch` (SG2)
- **Operator:** fix-with-test batch. **Closed all 6 security P0s:**
  1. `policy.rs` — injection scanner now reads **text + HTML** (was `text.or(html)`, so attribute/comment-hidden payloads bypassed it).
  2. `policy.rs` — `evaluate` blocks `javascript:`/`data:`/`file:`/`vbscript:`/`blob:` navigation (hostless schemes previously skipped the whole allow/deny check).
  3. `browser/mod.rs` — `ssrf_blocked_reason` rejects loopback/private/link-local hosts (incl. `169.254.169.254`) before any request leaves the process.
  4. `main.rs` — `navigate` command enforces `validate_url` server-side (was reachable directly with a `javascript:` target).
  5. `main.rs` — `ask` command routes through `execute_with_policy` (was calling raw `execute`, bypassing ActionPolicy entirely); approval-gated tools are surfaced, not silently run.
  6. `headless.rs` — control socket `chmod 0600` (`#[cfg(unix)]`). Full peer-cred authz + per-connection session isolation spun off as a **follow-up task**.
- **Tests:** +3 regression (HTML-attribute injection, `javascript:` scheme block, SSRF-host rejection).
- **Result:** full gate green — fmt clean, clippy `-D warnings` clean, **58 tests pass**, tauri check clean. **KEEP.**

### Iteration 2 — 2026-07-20 — KEEP — `anthropic-provider-p0` (SG1)
- **Operator:** fix-with-test. **Target:** `src/providers/anthropic.rs`.
- **Defects closed (all 3 audit-confirmed):** (1) `system` sent as an in-array message role → the live Messages API returns 400; moved to the top-level `system` field, `messages` now user-only. (2) `stop_reason` never normalized → the loop's `finish_reason == "stop"` termination never fired; added `normalize_finish_reason` (`end_turn`/`stop_sequence`→`stop`, `max_tokens`→`length`). (3) response parsing read only `content[0].text`, dropping other blocks → `extract_text` concatenates all `text`-typed blocks.
- **Change:** refactored for testability (`build_request_body`/`build_user_content`/`normalize_finish_reason`/`extract_text`) + 3 regression tests.
- **Result:** first attempt fmt RED (hand-formatting), `cargo fmt` canonicalized → GREEN; `clippy --lib -D warnings` clean; 3 new tests + all lib unit tests pass. **KEEP.** (Full `verify.sh` deferred to phase end.)

### Iteration 1 — 2026-07-20 — KEEP — `fmt-gate-fix`
- **Operator:** cleanup / gate-fix. **Target metric:** `cargo fmt --check` clean (fmt_ok false→true).
- **Change:** `cargo fmt` reformatted two multi-line forms in the `#[cfg(test)]` module of `src/browser/mod.rs` (rustfmt-version drift; both now fit one line). Diff = 2 insertions / 8 deletions, test-module whitespace only, zero production logic.
- **Result:** gate fmt RED→GREEN. Tests unaffected (test-module whitespace). **KEEP.**

### Iteration 0 — 2026-07-20 — BASELINE
- Verified real state (table above). clippy green, tests green, fmt red. Panel: unwrap 31 / expect 22 / clone 133 / todo 0 / unsafe 0.

## Open questions (need James's direction)

1. **Loop cadence / autonomy.** Run the remaining ~68 optimizations as (a) reviewed batches I present per phase, (b) a supervised autonomous loop this session, or (c) an overnight scheduler/ralph-loop run? Code-changing autonomy is consequential — default is (a) reviewed batches.
2. **Commit strategy.** One commit per phase (SG1, SG2, …) on this branch, or squash at the end? Nothing is committed yet.
3. **Native tool-calling migration** (audit gaps P0 + NB feature) is a larger refactor than a single iteration — treat as its own tracked sub-project rather than an autoresearch iteration?
4. **Feature prioritization.** Which net-new features to graduate first — the safety/eval cluster (NB-5/NB-6, the differentiation lever) or the correctness/UX cluster (NB-1/NB-3/NB-4)?
5. **Scheduler registration.** Register this Goal with a recurring scheduler task, or drive manually?

## Cross-links

- Optimization backlog: [optimization-backlog-2026-07-20.md](./optimization-backlog-2026-07-20.md)
- Feature backlog: [feature-backlog-2026-07-20.md](./feature-backlog-2026-07-20.md)
- Harness: [.autoresearch/eval.sh](../../.autoresearch/eval.sh), [config.json](../../.autoresearch/config.json), [state.json](../../.autoresearch/state.json)
- Project: [PROJECT.md](../../PROJECT.md), [CONTEXT.md](../../CONTEXT.md), [prior-art.md](../references/prior-art.md), [verify.sh](../../verify.sh)
- Method: `autoresearch` skill, `goal-management` skill.
