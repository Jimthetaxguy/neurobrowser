# neurobrowser

AI-native browser with a custom Rust rendering engine and Tauri desktop runtime.

## Tech Stack

- **Rust** (single crate + Tauri v2 wrapper)
- **Tokio** full async runtime
- **thiserror v2** for error handling (AgentError + ProviderError)
- **reqwest** for HTTP, **scraper** for HTML parsing, **serde** for serialization
- **async-trait** for async trait methods

## Quick Start

```bash
cargo build                # Dev build
cargo test                 # Unit + integration tests
cargo clippy --all-targets # Lint
cargo check --manifest-path src-tauri/Cargo.toml # Tauri wrapper check
cargo build --release      # Release (LTO + strip + abort)
./verify.sh                # Full verification chain
```

## Architecture

This repository keeps the architecture in the Rust modules and project docs, including:
- Error architecture (ToolError struct with builder pattern, AgentError enum)
- Async streaming via mpsc channels + StreamEvent tagged JSON
- Tauri IPC bridge between the desktop shell and Rust backend

See [docs/frontend-architecture-spike.md](docs/frontend-architecture-spike.md)
for the React + Tauri and React + AppKit frontend lane comparison.

## Autonomous Agent Core

The primary agent path is provider-agnostic and run-oriented:

- `start_agent_run` evaluates model tool calls against `ActionPolicy`
- `submit_approval` and `cancel_agent_run` resolve approval-gated actions
- every proposed, blocked, approved, rejected, and executed action is returned as a structured run event
- default autonomy is assisted: reads, snapshots, scrolling, and same-domain navigation can run; typing, form submission, high-impact actions, denylisted domains, and suspicious page content stop for approval or blocking

## Real Systems

- OpenAI: `OPENAI_API_KEY`, optional `OPENAI_MODEL`
- Anthropic: `ANTHROPIC_API_KEY`, optional `ANTHROPIC_MODEL`
- Ollama: local `OLLAMA_BASE_URL`, optional `OLLAMA_MODEL`

The Tauri shell fails closed if the desktop IPC bridge is unavailable. Browser and
agent flows should run through the real Tauri runtime, not a mocked browser page.
