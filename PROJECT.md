# NeuroBrowser

AI-native desktop browser: Rust library + Tauri/React shell. Agents drive a
real OS webview through the desktop bridge. The Rust library provides a
17-tool CSS-selector registry gated by `ActionPolicy`. The headless daemon is
a policy JSON-RPC protocol stub: its snapshot is hardcoded `about:blank`,
and it cannot navigate or execute browser tools.

See `docs/AGENT-SURFACE.md` and `SKILL.md` for the agent surface.
See `docs/RUNBOOK-DEV.md` to build and run. Changelog: `CHANGELOG.md`.

## Status (v0.1.1)

Shipped on `main`. `./verify.sh` is the green-build chain.

- **Desktop:** React + Tauri child webview per page (`src-tauri/`).
- **Library:** ReAct agent, providers (OpenAI / Anthropic / Ollama), sessions,
  episodic memory, and metrics (`src/`).
- **Policy:** `ReadOnly` / `Assisted` / `HighAutonomy` with domain allow/deny,
  sensitive-arg redaction, and prompt-injection detection.
- **Headless protocol stub:** `neurobrowser-headless` over a Unix socket
  with loopback TCP fallback after a failed Unix bind. Policy evaluation
  does not execute a tool; `snapshot` returns hardcoded data. Methods:
  `ping`, `policy.get` / `policy.set` / `policy.evaluate`,
  `snapshot`.
- **Workers:** summary types and empty compatibility readers remain in the
  library. Worker execution, Tauri worker IPC, sidebar, and headless fan-out
  are not shipped.

## v0.2

- Native function calling in `ReActAgent` (`ToolCall::parse_native` exists;
  the loop still consumes text-only tool invocations).
- React Workers sidebar.
- Headless `worker.spawn` / `worker.list`.
- CLI wrapper over the daemon (no `neurobrowser-cli` today).
- Visual regression tests; budget-capped real-LLM integration tests.
- Wire the daemon to the same session/page/ask/tool surface as desktop
  (`docs/specs/programmatic-surface-design-2026-07-20.md`).
