# NeuroBrowser — Development Runbook

How to build, run, and test NeuroBrowser locally.

## Prerequisites

- **Rust** stable toolchain.
- **Node.js** + **npm** (Vite frontend under `src-tauri/`).
- **macOS** for the desktop app (icon set + CSP are macOS-flavored).
  Windows/Linux desktop builds are not shipped. The library crate and
  headless daemon build on Unix without a display.

## One-shot green build

From repo root:

```bash
chmod +x verify.sh   # only needed the first time
./verify.sh
```

This is the verification chain in `verify.sh`:

1. `cargo fmt -- --check`
2. `cargo clippy --all-targets -- -D warnings`
3. `cargo test --all-targets`
4. `cd src-tauri && npm ci && npm run build`
5. `cargo check --manifest-path src-tauri/Cargo.toml --locked`
6. Locked headless check and binary tests with `--features headless`
7. `cargo test --manifest-path src-tauri/Cargo.toml --locked --test runtime_capabilities`
8. `cargo build --release` (library crate)

Expected output ends with `=== All checks passed ===`.

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
```

Integration tests live in `tests/`:

- `action_policy.rs` — deny-wins-over-allow, assisted-mode click approval,
  sensitive-arg redaction, prompt-injection blocking.
- `autonomous_agent.rs` — ReAct loop with a mocked provider.
- `agent_memory_metrics.rs` — memory + metrics.
- Headless argument and policy tests live in `src-tauri/src/bin/headless.rs`
  and run explicitly with the `headless` feature.

## Tauri IPC

The desktop app exposes 18 commands (see `src-tauri/src/main.rs`). From
the React frontend:

```javascript
import { invoke } from "@tauri-apps/api/core";

const sessionId = await invoke("create_session");
const pageId = await invoke("create_page", { sessionId });
await invoke("navigate", { sessionId, pageId, url: "https://example.com" });
const snapshot = await invoke("get_page_snapshot", { sessionId, pageId });
const result = await invoke("start_agent_run", { sessionId, pageId, prompt: "Summarize this page" });
```

Typed wrappers: `src-tauri/src/hostAdapters.js` (`createTauriHostAdapter()`).

Adding a command:

1. Define it in `src-tauri/src/main.rs`.
2. Add it to `tauri::generate_handler!`.
3. Add the command name to the app manifest in `src-tauri/build.rs`.
4. Add `allow-<command-name>` to `src-tauri/capabilities/main.json` (control webview only). Do not grant host commands to `page-runtime.json`.
5. Add a wrapper in `src-tauri/src/hostAdapters.js`.

Do not add a wildcard capability permission.

Page webviews receive only `browser_runtime_report`, including the initial
`about:blank` document. URLPattern requires its colon to be escaped: the JSON
entry is `"about\\:blank"`. The capability tests exercise Tauri's compiled ACL
without launching the app: blank/HTTP/HTTPS reports succeed, other document
schemes and webview labels are denied, and page webviews cannot invoke control
commands (including provider changes and approval submission).

## verify.sh failures

| Step | Symptom | Fix |
|---|---|---|
| `cargo fmt --check` | diff output | `cargo fmt`, re-run |
| `cargo clippy` | warnings-as-errors | fix the warning, re-run |
| `cargo test` | failed assertions | fix the test or the code |
| `npm ci && npm run build` | Vite error | check `src-tauri/src/*.{jsx,js}` |
| `cargo check --manifest-path src-tauri/Cargo.toml --locked` | Tauri compile error | missing icon or capability |
| locked headless `cargo check` / `cargo test --bin neurobrowser-headless` | headless compile or binary test failure | fix the headless feature path |
| `cargo build --release` | linker / symbol error | inspect linker diagnostics and `rustc --version` |

## Environment variables

| Variable | Used by | Default |
|---|---|---|
| `OPENAI_API_KEY` | `set_provider("openai")` | (none — required) |
| `OPENAI_MODEL` | `set_provider("openai")` | `gpt-4o` |
| `ANTHROPIC_API_KEY` | `set_provider("anthropic")` | (none — required) |
| `ANTHROPIC_MODEL` | `set_provider("anthropic")` | `claude-3-5-sonnet-latest` |
| `OLLAMA_BASE_URL` | `set_provider("ollama")` | `http://localhost:11434` |
| `OLLAMA_MODEL` | `set_provider("ollama")` | `llama3.2` |
| `CUSTOM_PROVIDER_API_KEY` | `set_provider("custom")` | falls back to `OPENAI_API_KEY` |
| `CUSTOM_PROVIDER_BASE_URL` | `set_provider("custom")` | `https://api.openai.com` (via `resolve_endpoint`) |
| `CUSTOM_PROVIDER_MODEL` | `set_provider("custom")` | `gpt-4o` |
| `NEUROBROWSER_SOCKET` | headless daemon | temp dir `neurobrowser-<pid>.sock` |
| `RUST_LOG` | tracing | `neurobrowser=info,headless=info` |

Local keys go in a gitignored `.env`.

## Where things live

| Layer | Path |
|---|---|
| Library crate | `src/` |
| Tauri shell | `src-tauri/` |
| React frontend | `src-tauri/src/App.jsx` + `hostAdapters.js` |
| Tauri IPC bridge | `src-tauri/src/main.rs` |
| Webview JS bridge | `src-tauri/src/runtime.rs` |
| AppKit Swift spike | `NeuroBrowser/` |
| Specs / stories / ADRs | `docs/specs/`, `docs/stories/`, `docs/adr/` |
| Verification | `verify.sh` |
| Tests | `tests/` |

## See also

- `PROJECT.md` — current status and v0.2 list.
- `docs/AGENT-SURFACE.md` — agent tool / policy spec.
- `docs/references/prior-art.md` — prior art.
- `docs/specs/` — product specs.
- `docs/stories/` — user stories.
- `docs/adr/` — architecture decision records.

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
