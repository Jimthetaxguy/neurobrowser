# NeuroBrowser — optimization & focus effort (index)

Started 2026-07-20. An autoresearch-driven pass over NeuroBrowser: burn down a verified
optimization backlog, spec net-new features, and set strategic direction — each code change
gated green (`verify.sh`) and committed per phase.

**Branch:** `agent/claude-autoresearch-optimize-2026-07-20` (local, not pushed)

## Artifacts

| Doc | What it is |
|---|---|
| [focus-areas-2026-07-20.md](./focus-areas-2026-07-20.md) | **Start here** — 10 focus areas (FA-1…5 original + FA-6…10 end-to-end expansion) + re-sequenced roadmap |
| [quality-audit-2026-07-20.md](./quality-audit-2026-07-20.md) | Raw grounded audit (8 dimensions, 52 file:line findings) behind FA-6…10 — the source of truth |
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
| `c951ef0` | — | Focus areas (FA-1…5) + docs index |
| `ce29e7a` | SG3b | Interactive-tool honesty — static engine errors instead of faking success |

Tests: 55 → **63** (14 new regression tests). Security P0s open: 6 → **0**.

**Goals expanded 2026-07-20:** an 8-dimension end-to-end quality workflow (9 agents, 52 grounded
findings) added **FA-6…FA-10** (Runtime Robustness · Durable State · Operability · Distribution ·
Code Health) + enrichments to FA-1…5 + a substrate-first re-sequenced roadmap. See
[focus-areas](./focus-areas-2026-07-20.md) + [quality-audit](./quality-audit-2026-07-20.md).

**Next (per the re-sequenced roadmap):** `base_url` + `RUST_LOG` honesty/operability fixes (FA-8,
immediate next iter), then the FA-10 enabling code-health slice, then FA-6 runtime robustness.

## Follow-up tasks spun off

- Headless socket peer-cred authz (SO_PEERCRED + per-connection session isolation) — beyond the chmod stopgap.

## Resuming the loop

Read `.autoresearch/state.json` (`next_mutation`) + this index. Run one gate at a time via
`.autoresearch/eval.sh` — **never** kill a running `cargo` build (corrupts the incremental
target dir and yields false test results).
