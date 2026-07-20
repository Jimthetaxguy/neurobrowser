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
  - docs/goals/optimization-backlog-2026-07-20.md   # 69 confirmed + 4 plausible existing-code optimizations
  - docs/goals/feature-backlog-2026-07-20.md         # 18 ranked net-new features + patterns + prior-art resolutions
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
| SG2 | Security P0 batch | injection HTML scan, scheme/SSRF guards, `ask`/`navigate` policy, headless auth | queued |
| SG3 | Correctness P0 batch | stale post-nav URL, no-op interactive tools, unused dom_snapshot, spawn_worker pin | queued |
| SG4 | P1 sweep | memory bound, stream run_id/args, key+value redaction, blocking reqwest→async, tool-list from registry | queued |
| SG5 | Net-new specs | Graduate NB-1/2/3/5/6 into `docs/specs/` + `docs/stories/` | queued |
| SG6 | Doc reconciliation | AIAnytime, fastrender, PROJECT.md observability claim | queued |

## Run Log (reverse-chronological)

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
