# NeuroBrowser — optimization & focus effort (index)

Started 2026-07-20. An autoresearch-driven pass over NeuroBrowser: burn down a verified
optimization backlog, spec net-new features, and set strategic direction — each code change
gated green (`verify.sh`) and committed per phase.

**Branch:** `agent/claude-autoresearch-optimize-2026-07-20` (local, not pushed)

## Artifacts

| Doc | What it is |
|---|---|
| [focus-areas-2026-07-20.md](./focus-areas-2026-07-20.md) | **Start here** — the 5 key focus areas for the browser tool |
| [goal-autoresearch-optimize-2026-07-20.md](./goal-autoresearch-optimize-2026-07-20.md) | Goal control doc — success criteria, KPIs, loop mechanics, run log |
| [optimization-backlog-2026-07-20.md](./optimization-backlog-2026-07-20.md) | 69 confirmed + 4 plausible existing-code optimizations (audit) |
| [feature-backlog-2026-07-20.md](./feature-backlog-2026-07-20.md) | 18 ranked net-new features + prior-art resolutions |
| [../specs/programmatic-surface-design-2026-07-20.md](../specs/programmatic-surface-design-2026-07-20.md) | Wired daemon + MCP + CLI + library API design |
| [../../.autoresearch/](../../.autoresearch/) | Loop harness — `eval.sh` (gate + panel), `config.json`, `state.json` (run log) |

## Status

**Shipped** (5 gated commits, all `verify.sh` green — fmt + clippy `-D warnings` + tests + tauri check):

| Commit | Phase | Summary |
|---|---|---|
| `ce64c46` | SG0 | Charter + harness + backlogs + fmt-gate fix |
| `dfecd0e` | SG1 | Anthropic provider P0 (broken vs live API) |
| `78fb62b` | SG2 | 6 security P0s (injection/scheme/SSRF/policy/socket) |
| `7f6641c` | SG3a | Correctness P0s (stale nav URL, worker page-pin) |
| `0247d49` | — | Programmatic-surface design doc |

Tests: 55 → **60** (11 new regression tests). Security P0s open: 6 → **0**.

**Next:** SG3b (interactive-tool no-ops + dead `dom_snapshot`), the per-tool-risk cluster
(externally validated), and Phase 0 of the programmatic surface — sequenced per the focus areas.

## Follow-up tasks spun off

- Headless socket peer-cred authz (SO_PEERCRED + per-connection session isolation) — beyond the chmod stopgap.

## Resuming the loop

Read `.autoresearch/state.json` (`next_mutation`) + this index. Run one gate at a time via
`.autoresearch/eval.sh` — **never** kill a running `cargo` build (corrupts the incremental
target dir and yields false test results).
