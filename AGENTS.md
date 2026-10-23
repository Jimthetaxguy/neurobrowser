# NeuroBrowser

Contributor contract for Codex and other agents changing this repo.
[SKILL.md](SKILL.md) is the procedure for agents driving the browser.
[CONTEXT.md](CONTEXT.md) is the shared vocabulary.
Build and run detail is in [docs/RUNBOOK-DEV.md](docs/RUNBOOK-DEV.md).

## Layout

Three Cargo packages, three manifests, three lockfiles. Nothing declares
a `[workspace]`. A root `cargo` command builds only `neurobrowser`.

| Path | What it is |
| --- | --- |
| `Cargo.toml`, `src/` | Library crate `neurobrowser`. HTTP scraper and policy-gated agent. No display and no Tauri dependency. |
| `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock` | Crate `neurobrowser-tauri`. Tauri v2 shell. `default-run` is `neurobrowser-tauri`. Bin `neurobrowser-headless` requires feature `headless`. |
| `src-tauri/package.json`, `src-tauri/package-lock.json` | npm package for the React 19 + Vite shell. |
| `crates/neuro-memory/` | Crate `neuro-memory`. Persistent page memory. Its manifest says it is an independent crate. |
| `project.yml`, `NeuroBrowser.xcodeproj`, `NeuroBrowser/` | AppKit shell. `project.yml` is the XcodeGen source. The generated project is checked in. |
| `tests/` | Integration tests for the root crate, plus the macOS scripts below. |
| `.github/workflows/ci.yml` | Jobs `check` (Ubuntu), `tauri` (macOS), and `guards` (Ubuntu). |

`CONTEXT.md` has an older table row, "Agent surface", that lists 12 tools
(`snapshot`, `click`, `type_text`, and the rest of that row). The shipped
registry is `default_tool_registry()` in `src/browser/mod.rs`: 22 tools.
`default_tool_registry_with_memory()` adds `search_personal_memory` and
`inspect_active_page`. Tool use belongs in [SKILL.md](SKILL.md) and
[docs/AGENT-SURFACE.md](docs/AGENT-SURFACE.md).

## Toolchain

- Rust edition 2021. No `rust-toolchain` file, `rustfmt.toml`, or `clippy.toml`.
  The `check` job installs stable with rustfmt and clippy
  (`dtolnay/rust-toolchain@stable`). All three lockfiles are Cargo lock
  version 4. The root and `neuro-memory` lockfiles pin `time-core` 0.1.9,
  whose manifest requires edition 2024. Cargo from Rust 1.83 cannot parse
  that manifest. Edition 2024 has been stable since Rust 1.85; use a current
  stable toolchain. `src-tauri/Cargo.lock` pins `time-core` 0.1.8.
- Linux builds of the root crate need OpenSSL headers on `pkg-config`
  (`openssl`). Reqwest is enabled with the `json` feature, so the default
  native-tls stack stays on.
- Node 22 on the `tauri` job (`actions/setup-node`, cache
  `src-tauri/package-lock.json`). npm is the package manager.
  `package-lock.json` is lockfileVersion 3. `package.json` has no
  `packageManager` field. From `src-tauri/`, install with `npm ci`.
- `project.yml`: macOS deployment target 13.0, `xcodeVersion` 15.0, Swift 5.9,
  `CODE_SIGNING_ALLOWED` and `CODE_SIGNING_REQUIRED` set to `NO`, bundle id
  `com.neuro.neurobrowser`.

## Verify

`./verify.sh` is the local chain (`set -euo pipefail`). It prints
`=== All checks passed ===` only after every step. CI runs a different split,
documented below.

Order, from the repo root:

```bash
cargo fmt -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
cargo test --manifest-path crates/neuro-memory/Cargo.toml
(cd src-tauri && npm ci && npm test && npm run build)
cargo check --manifest-path src-tauri/Cargo.toml --locked
cargo test --manifest-path src-tauri/Cargo.toml --locked --bin neurobrowser-tauri
cargo check --manifest-path src-tauri/Cargo.toml --features headless --locked
cargo test --manifest-path src-tauri/Cargo.toml --features headless --locked --bin neurobrowser-headless
cargo test --manifest-path src-tauri/Cargo.toml --locked --test runtime_capabilities
# macOS only — see below
cargo build --release
```

`npm test` is `node --test src/*.test.js`. `npm run build` is `vite build`.
Root fmt, clippy, and `cargo test` cover the `neurobrowser` package only.
`src-tauri` cargo steps pass `--locked`. The root and `neuro-memory` steps
do not. `cargo build --release` is the root package. Its release profile sets
`strip`, `lto`, `codegen-units = 1`, and `panic = "abort"`.

The `src-tauri` cargo steps compile the Tauri package. On Linux that links
`gtk` 0.18, `javascriptcore-rs` =1.1, and `webkit2gtk` =2.0.2 with feature
`v2_38` (`src-tauri/Cargo.toml`). On macOS it needs the macOS SDK. A display
is not required. Feature `headless` only gates the headless bin; the package
still compiles Tauri and `tauri-build`. `./verify.sh` runs these steps on
every OS and stops when the native libraries are missing.

### macOS only

After `runtime_capabilities`, `verify.sh` runs the following when `uname -s`
is Darwin:

```bash
bash tests/run_native_app_smoke.sh
./tests/run_appkit_navigation.sh
bash tests/run_capability_webkit.sh
```

`tests/run_native_app_smoke.sh` also exits unless the OS is Darwin. It builds
a debug app with
`npm run tauri -- build --debug --features custom-protocol --bundles app`.
The other two compile Swift with `xcrun swiftc`: AppKit navigation, then the
WebKit capability workload against `tests/fixtures/capability-site`. Those
scripts are separate from the Xcode build below.

### What CI runs where

| Job | Runner | Steps |
| --- | --- | --- |
| `check` | `ubuntu-latest` | Root fmt, clippy `-D warnings`, `cargo test --all-targets`, `neuro-memory` tests, `cargo build --release`. |
| `tauri` | `macos-latest` | Node 22, `npm ci`, `npm test`, `npm run build`, the locked `src-tauri` cargo commands, then the three Darwin scripts. |
| `guards` | `ubuntu-latest` | Absent from `verify.sh`. Rejects `mockall`, `mockito`, and `wiremock` in the three crates' manifests and lockfiles. Rejects conflict markers. Rejects iCloud duplicate names (`* 2.*`, `* 3.*`, `* 4.*`). |

The Ubuntu `check` job does not run npm or `src-tauri` cargo. The macOS
`tauri` job does not run fmt, clippy, root tests, or the release build.

### Opt-in tests

Live public HTTPS, ignored unless requested:

```bash
cargo test --test capability_observation real_public_http_engine_observes_example_domain -- --ignored --exact
```

`tests/real_provider_smoke.rs` is `#[ignore]` and needs a network plus live
provider credentials. Keep credentials in the process environment. Leave
them out of source, Vite variables, docs, and shell history.

## Desktop and the Xcode build

From `src-tauri/`:

```bash
npm run desktop
npm run desktop:build
```

`desktop` is `tauri dev --no-watch`. `desktop:build` is
`tauri build --features custom-protocol --bundles app`.

Headless policy stub (JSON-RPC `ping`, `policy.*`, and a hardcoded
`snapshot`; it does not browse):

```bash
NEUROBROWSER_SOCKET="$HOME/.neurobrowser/daemon.sock" \
  cargo run --manifest-path src-tauri/Cargo.toml --features headless --bin neurobrowser-headless
```

The AppKit shell is a macOS parity lane
([ADR-001](docs/adr/ADR-001-react-tauri-primary.md)). `verify.sh` does not
build it. `npm run build:appkit` writes gitignored
`NeuroBrowser/ControlSurface`, then `xcodebuild` uses the checked-in project:

```bash
(cd src-tauri && npm ci && npm run build:appkit)
xcodebuild -project NeuroBrowser.xcodeproj -scheme NeuroBrowser -configuration Debug CODE_SIGNING_ALLOWED=NO build
```

After editing `project.yml`, regenerate that project:

```bash
xcodegen generate --spec project.yml
```

## Guardrails

- The `guards` job rejects mock crates (`mockall`, `mockito`, `wiremock`).
- Host Tauri commands go in `src-tauri/capabilities/main.json`. Page webviews
  stay on `browser_runtime_report`. The "Adding a command" list is in
  [docs/RUNBOOK-DEV.md](docs/RUNBOOK-DEV.md).
- This repo has no Dockerfile, Railway, Fly, or Vercel config. The shipped
  desktop is macOS.
- Leave [SKILL.md](SKILL.md) as the browser-use procedure.

## License

Apache-2.0 OR MIT ([LICENSE-APACHE](LICENSE-APACHE), [LICENSE-MIT](LICENSE-MIT)).
