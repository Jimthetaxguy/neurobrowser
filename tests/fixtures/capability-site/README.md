---
author: codex/real_workloads
created: 2026-10-04
agent: codex/real_workloads
date: 2026-10-04
type: test-corpus
status: active
scope: Neurobrowser page evidence and scoped actions in real WebKit
---

# Local WebKit capability corpus

Run `bash tests/run_capability_webkit.sh` on macOS. The runner starts a real loopback
HTTP server on an ephemeral port, extracts the exact `RUNTIME_INIT_SCRIPT` from
`src-tauri/src/runtime.rs`, and injects it into an isolated, nonpersistent
`WKWebView`. It creates no visible browser window and uses no personal sessions.
The Swift harness has an overall deadline and per-request deadlines.

The fixture is ordinary HTML and JavaScript with no Neurobrowser API calls. It
provides static facts, a fetched JSON fact, a table, button replacement, SPA
history changes, and a form with actual input/change/submit event handlers.
Canary values test the exclusion of password, hidden and ordinary input values
from observation evidence. Direct property writes check freshness without a DOM
mutation event. Full reload checks document identity replacement.

The harness substitutes only Tauri's `browser_runtime_report` transport with a
`WKScriptMessageHandler`. Its JSON receipt records every shipped-runtime request,
response, assertion count and failure. The printed receipt path and HTTP log stay
in the temporary output directory. This proves real engine execution and HTTP
transport; Rust caller authorization and Tauri host routing have separate tests.
