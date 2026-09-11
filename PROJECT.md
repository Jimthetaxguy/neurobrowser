# NeuroBrowser

AI-native desktop browser: Rust library + Tauri/React shell. Agents drive a
real OS webview (or the scraper-backed headless daemon) through a 12-tool
surface gated by `ActionPolicy`.

See `docs/AGENT-SURFACE.md` and `SKILL.md` for the agent surface.
See `docs/RUNBOOK-DEV.md` to build and run. Changelog: `CHANGELOG.md`.

## Status (v0.1.1)

Shipped on `main`. `./verify.sh` is the green-build chain.

- **Desktop:** React + Tauri child webview per page (`src-tauri/`).
- **Library:** ReAct agent, providers (OpenAI / Anthropic / Ollama), session +
  worker registry, streaming + metrics (`src/`).
- **Policy:** `ReadOnly` / `Assisted` / `HighAutonomy` with domain allow/deny,
  sensitive-arg redaction, and prompt-injection detection.
- **Headless daemon:** `neurobrowser-headless` over UDS or TCP. Methods:
  `ping`, `policy.get` / `policy.set` / `policy.evaluate` / `policy.snapshot`,
  `snapshot`.
- **Workers:** types + `SessionManager` registry + Tauri `list_workers` /
  `get_worker`. Sidebar (E5) and headless fan-out (E6) are not shipped.

## v0.2

- Native function calling in `ReActAgent` (`ToolCall::parse_native` exists;
  the loop still consumes text-only tool invocations).
- React Workers sidebar.
- Headless `worker.spawn` / `worker.list`.
- CLI wrapper over the daemon (no `neurobrowser-cli` today).
- Visual regression tests; budget-capped real-LLM integration tests.
- Wire the daemon to the same session/page/ask/tool surface as desktop
  (`docs/specs/programmatic-surface-design-2026-07-20.md`).
