# NeuroBrowser — project context glossary

**Role:** first-party-active AI-native browser (Rust lib + Tauri v2 desktop).  
**Path:** `/Users/jamespustorino/code/neurobrowser`  
**Remote:** `https://github.com/Jimthetaxguy/neurobrowser.git`  
**Branch:** `main` (as of 2026-07-10)

## Purpose

Drive a **real browser session** (WKWebView / WebView2 / WebKitGTK via Tauri) with policy-gated agent autonomy — not a pure HTTP scraper. Agents get programmatic tools **and** visual page access.

## Domain vocabulary

| Term | Meaning |
|------|---------|
| **Agent run** | `start_agent_run` → model proposes tools → `ActionPolicy` evaluates → events stream |
| **ActionPolicy** | Gate for proposed browser actions (read/scroll/nav vs type/submit/high-impact) |
| **Autonomy levels** | `ReadOnly` / `Assisted` (default) / `HighAutonomy` |
| **Assisted default** | Reads, snapshots, scrolling, same-domain nav auto-run; typing, form submit, denylist, suspicious content require approval or block |
| **Submit approval** | `submit_approval` / `cancel_agent_run` resolve gated actions |
| **StreamEvent** | Tagged JSON events for proposed / blocked / approved / rejected / executed actions |
| **Tabs-as-workers** | Phase E model: tabs as parallel work units for agent tasks |
| **Headless daemon** | Phase D4: Unix domain socket control plane for external agents |
| **Agent surface** | 12 tools: snapshot, click, type_text, submit_form, query_selector, evaluate, navigate, get_text, get_attribute, wait_for, extract_text, screenshot |
| **Provider** | Pluggable LLM backends: OpenAI, Anthropic, Ollama (real keys via env) |
| **ReAct loop** | Library agent path (`src/agent/`) with memory/observability structures |

## Module map

| Path | Role |
|------|------|
| `src/` | Library crate: agent, browser tools, providers, session, tools |
| `src-tauri/` | Tauri v2 desktop wrapper + IPC bridge |
| `docs/specs/`, `docs/stories/`, `docs/adr/` | Shared GitSpec-style product docs |
| `docs/notes/local/` | Local process notes (gitignored; promote per docs/notes/README) |
| `SKILL.md` | Agent skill entry for driving NeuroBrowser |
| `docs/AGENT-SURFACE.md` | Full agent tool/autonomy surface |
| `CURRENT_STATE.md` | Point-in-time audit (may lag tip — re-verify with cargo) |
| `verify.sh` | Full verification chain |

## Real systems

- LLM: `OPENAI_API_KEY` / `ANTHROPIC_API_KEY` / local Ollama (`OLLAMA_BASE_URL`)
- Browser: real Tauri webview runtime (fail closed if IPC bridge unavailable)
- **No mock browser page** for product paths

## Verify

```bash
cargo check --lib
cargo test
cargo clippy --all-targets
cargo check --manifest-path src-tauri/Cargo.toml
./verify.sh   # full chain when shipping
```

## Non-goals / safety

- Not ROSA product shell; do not import ROSA identity/voice KB wire mutations here
- Do not push to vendor upstreams; this is first-party
- Archive-don't-delete for large remove batches (`_archive-*/`)
- Stage-by-name only; auto-generated `src-tauri/gen/schemas/*` often dirty after local Tauri runs — do not bulk-add without review

## Cleanup note (2026-07-10)

CONTEXT added for project-context-glossary compliance. Residual dirty: `Cargo.lock` + Tauri gen schemas (build noise); `.cursor/` and `_archive-*` should stay local.

## Cargo.lock / Tauri gen schemas policy (2026-07-10)

- **`Cargo.lock` is tracked** — commit intentional dependency resolution (e.g. `tempfile` for tests).
- **`src-tauri/gen/schemas/*` is tracked** — regenerate via Tauri build when capabilities change; commit with the capability/IPC change that caused the regen (worker list permissions, etc.). Do not leave machine-local schema drift uncommitted if it reflects source-of-truth capability config.
- **`_working-files/`** stays gitignored (session notes).
