---
created: 2026-07-10
updated: 2026-09-17
status: active
type: project-glossary
scope: NeuroBrowser library and desktop shells
---

# NeuroBrowser context

NeuroBrowser combines a Rust browser-agent library, a Tauri/React desktop
shell, and a separate AppKit shell. This glossary records shared vocabulary;
[PROJECT.md](PROJECT.md) owns current scope and planned work.

| Term | Meaning |
|---|---|
| BrowserInterface | Rust interface implemented by the HTTP scraper and Tauri webview runtime. Unsupported capabilities return errors. |
| BrowserEngine | reqwest + scraper implementation; it does not run JavaScript or provide a live interactive DOM. |
| TauriBrowserRuntime | Desktop runtime that routes browser operations to real child webviews. |
| ActionPolicy | Evaluates proposed tool calls and returns Allow, RequireApproval, or Block with redacted arguments. |
| ReadOnly / Assisted / HighAutonomy | Policy modes; domain/tool denials, sensitive input, and high-impact actions retain explicit gates. |
| Agent run | ReAct loop using a real configured provider and BrowserInterface; pending actions require a caller-managed approval. |
| Tool registry | CSS-selector browser tools in default_tool_registry; see the canonical [agent surface](docs/AGENT-SURFACE.md). |
| Headless daemon | Unix-socket policy protocol stub with loopback TCP fallback; snapshot is hardcoded about:blank and tools are not executed. |
| AppKit shell | Separate native Swift window/browser implementation; agent execution is not connected to the Rust runtime. |
| AppKit page ID | Stable tab identity: React uses nonnegative IDs; native menu tabs use decreasing negative IDs. Native tab events synchronize both controls. |

| Boundary | Source |
|---|---|
| Library, policy, providers | src/ |
| Tauri IPC, React, webview bridge, headless protocol | src-tauri/ |
| Native AppKit | NeuroBrowser/ and NeuroBrowser.xcodeproj/ |
| Agent contract and skill | docs/AGENT-SURFACE.md and SKILL.md |
| Build commands | docs/RUNBOOK-DEV.md and verify.sh |

Keep generated schemas aligned with intentional Tauri capability changes.
Keep local credentials, build products, and working notes out of commits.
The headless protocol stub is a known real-systems gap, not browser execution.
