# NeuroBrowser — Development Runbook

How to build, run, and test NeuroBrowser locally.

## Prerequisites

- **Rust** stable toolchain.
- **Node.js** + **npm** (Vite frontend under `src-tauri/`). CI pins Node 22 on the `tauri` job.
- **macOS** for the desktop app (icon set + CSP are macOS-flavored).
  Windows/Linux desktop builds are not shipped. The library crate builds
  without a display or the Tauri toolchain. `neurobrowser-headless` is a
  bin in the `neurobrowser-tauri` package, so it shares that toolchain
  (`tauri` / `tauri-build`: macOS SDK, or GTK/WebKit on Linux; a display
  is not required). Full `./verify.sh` is not a portable one-shot green on
  bare Ubuntu: the Tauri `cargo check` steps need macOS or GTK/WebKit on Linux.

## One-shot green build

From repo root:

```bash
./verify.sh
```

Commands and order are in `verify.sh`. CI jobs are in `.github/workflows/ci.yml`. `verify.sh` ends with `=== All checks passed ===`.

Darwin only, after the webview capability tests: `./tests/run_appkit_navigation.sh`, then `bash tests/run_capability_webkit.sh`.

CI also runs a `guards` job that `verify.sh` does not. That job rejects mock crates, conflict markers, and iCloud duplicate names.

CI pins Node 22 on the `tauri` job. Local `verify.sh` uses the `node` and `npm` on `PATH`.

## Desktop app

```bash
cd src-tauri
npx tauri dev --no-watch
```

Runs the Vite dev server, compiles the Tauri binary, and launches the
window. Drop `--no-watch` to rebuild on save.

## Headless policy protocol stub (shipped in v0.1.1)

```bash
NEUROBROWSER_SOCKET="$HOME/.neurobrowser/daemon.sock" \
  cargo run --bin neurobrowser-headless --manifest-path src-tauri/Cargo.toml --features headless
```

Prints `NEUROBROWSER_LISTENING=unix://…` or falls back to
`NEUROBROWSER_LISTENING=tcp://127.0.0.1:…`. Methods: `ping`,
`policy.get` / `policy.set` / `policy.evaluate`,
`snapshot`. Speak newline-delimited JSON-RPC on the socket. There is no
CLI wrapper. `snapshot` is a hardcoded `about:blank` payload. The daemon
does not construct `BrowserEngine`, navigate, or execute registry tools.

## Tests

```bash
cargo test --all-targets
cargo test --manifest-path crates/neuro-memory/Cargo.toml
```

Integration tests live in `tests/`:

- `action_policy.rs` — deny-wins-over-allow, assisted-mode click approval,
  sensitive-arg redaction, prompt-injection blocking.
- `autonomous_agent.rs` — ReAct loop with a mocked provider.
- `memory_tools.rs` — personal-memory tools over `MemoryService`.
- `real_provider_smoke.rs` — `#[ignore]` real HTTP; outside `verify.sh` and CI.
- Headless argument and policy tests live in `src-tauri/src/headless_bin/main.rs`
  and run explicitly with the `headless` feature.

## Tauri IPC

The desktop app exposes 22 commands (`tauri::generate_handler!` in
`src-tauri/src/main.rs`), including `get_page_observation` and
`execute_browser_tool`. From the React frontend:

```javascript
import { invoke } from "@tauri-apps/api/core";

const sessionId = await invoke("create_session");
const pageId = await invoke("create_page", { sessionId });
await invoke("navigate", { sessionId, pageId, url: "https://example.com" });
const snapshot = await invoke("get_page_snapshot", { sessionId, pageId });
const result = await invoke("start_agent_run", { sessionId, pageId, prompt: "Summarize this page" });
```

`get_page_snapshot` returns `url`, `title`, and count fields (`link_count`, `image_count`, `form_count`, `price_count`, `table_count`). That payload is not the fuller crate `PageSnapshot` (`html`, `text`, viewport, and the element lists).

`start_agent_run` events keep `tool`, `success`, `reasons`, and `redacted_arguments`. Tool result text, policy `outcome`, `risk_flags`, and `approval_id` stay off the wire.

Memory IPC commands are `search_local_memory`, `explain_memory_result`, and `forget_memory`. The IPC search default limit is 8 (`search_local_memory`; `explain_memory_result` uses the same default when `limit` is omitted). Crate tools `search_personal_memory` and `inspect_active_page` remain as documented in `docs/AGENT-SURFACE.md`, with their own defaults.

Typed wrappers: `src-tauri/src/hostAdapters.js` (`createTauriHostAdapter()`).

Adding a command:

1. Define it in `src-tauri/src/main.rs`.
2. Add it to `tauri::generate_handler!`.
3. Add the command name to the app manifest in `src-tauri/build.rs`.
4. Add `allow-<command-name>` to `src-tauri/capabilities/main.json` (control webview only). Do not grant host commands to `page-runtime.json`.
5. Regenerate and commit `src-tauri/gen/schemas/` so the ACL files match the handler.
6. Add a wrapper in `src-tauri/src/hostAdapters.js`.

Do not add a wildcard capability permission.

Page webviews receive only `browser_runtime_report`, including the initial
`about:blank` document. URLPattern requires its colon to be escaped: the JSON
entry is `"about\\:blank"`. The capability tests exercise Tauri's compiled ACL
without launching the app: blank/HTTP/HTTPS reports succeed, other document
schemes and webview labels are denied, and page webviews cannot invoke control
commands (including provider changes and approval submission).

## verify.sh failures

The failing command is the one `verify.sh` printed. Fix that step and re-run `./verify.sh`. Job names are in `.github/workflows/ci.yml`.

## Environment variables

| Variable | Used by | Default |
|---|---|---|
| `OPENAI_API_KEY` | `set_provider("openai")` | (none — required) |
| `OPENAI_MODEL` | `set_provider("openai")` | `gpt-4o` |
| `ANTHROPIC_API_KEY` | `set_provider("anthropic")` | (none — required) |
| `ANTHROPIC_MODEL` | `set_provider("anthropic")` | `claude-sonnet-5` |
| `OLLAMA_BASE_URL` | `set_provider("ollama")` | `http://localhost:11434` |
| `OLLAMA_MODEL` | `set_provider("ollama")` | `llama3.2` |
| `CUSTOM_PROVIDER_API_KEY` | `set_provider("custom")` | falls back to `OPENAI_API_KEY` |
| `CUSTOM_PROVIDER_BASE_URL` | `set_provider("custom")` | `https://api.openai.com` (via `resolve_endpoint`) |
| `CUSTOM_PROVIDER_MODEL` | `set_provider("custom")` | `gpt-4o` |
| `NEUROBROWSER_SOCKET` | headless daemon | temp dir `neurobrowser-<pid>.sock` |
| `RUST_LOG` | desktop (`src-tauri/src/main.rs`) and headless (`src-tauri/src/headless_bin/main.rs`). `neurobrowser` is the library crate; shell events are `neurobrowser_tauri`. The headless bin's crate root is `neurobrowser_headless` (no `headless` module). | `neurobrowser=info` (desktop); `neurobrowser=info,headless=info` (headless) |

## Where things live

| Layer | Path |
|---|---|
| Library crate | `src/` |
| Memory crate | `crates/neuro-memory` |
| Tauri shell | `src-tauri/` |
| React frontend | `src-tauri/src/App.jsx` + `hostAdapters.js` |
| Tauri IPC bridge | `src-tauri/src/main.rs` |
| Webview JS bridge | `src-tauri/src/runtime.rs` |
| AppKit Swift spike | `NeuroBrowser/` |
| Status / agent / ADR | `README.md`, `CHANGELOG.md`, `docs/AGENT-SURFACE.md`, `docs/adr/` |
| Verification | `verify.sh` |
| Tests | `tests/` |

## See also

- `README.md` — current status and shipped surface.
- `CHANGELOG.md` — history and still-unimplemented work.
- `docs/AGENT-SURFACE.md` — agent tool / policy spec.
- `docs/adr/ADR-001-react-tauri-primary.md` — primary frontend path.
- `docs/references/prior-art.md` — prior art.

## Native AppKit shell

The optional AppKit shell bundles its generated React control surface as an
Xcode folder resource. Build that resource before building the native app:

```bash
(cd src-tauri && npm ci && npm run build:appkit)
xcodebuild -project NeuroBrowser.xcodeproj -scheme NeuroBrowser -configuration Debug CODE_SIGNING_ALLOWED=NO build
```

`NeuroBrowser/ControlSurface` is generated and remains gitignored. Its
`appkit.html` and relative assets must appear in the built app's
`Contents/Resources/ControlSurface`. The XcodeGen source of truth is
`project.yml`; after editing that file, regenerate the checked-in project
with `xcodegen generate --spec project.yml`.
