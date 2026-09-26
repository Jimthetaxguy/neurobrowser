#!/usr/bin/env bash
# NeuroBrowser verify chain. Tauri cargo check needs macOS or GTK/WebKit on Linux.
set -euo pipefail
echo "=== NeuroBrowser Verify Chain ==="
echo "→ cargo fmt --check..."
cargo fmt -- --check
echo "→ cargo clippy..."
cargo clippy --all-targets -- -D warnings
echo "→ cargo test..."
cargo test --all-targets
echo "→ neuro-memory cargo test..."
cargo test --manifest-path crates/neuro-memory/Cargo.toml
echo "→ Tauri frontend tests and build..."
(cd src-tauri && npm ci && npm test && npm run build)
echo "→ Tauri cargo check..."
cargo check --manifest-path src-tauri/Cargo.toml --locked
echo "→ Tauri binary tests..."
cargo test --manifest-path src-tauri/Cargo.toml --locked --bin neurobrowser-tauri
echo "→ Headless check and tests..."
cargo check --manifest-path src-tauri/Cargo.toml --features headless --locked
cargo test --manifest-path src-tauri/Cargo.toml --features headless --locked --bin neurobrowser-headless
echo "→ Webview capability regression tests..."
cargo test --manifest-path src-tauri/Cargo.toml --locked --test runtime_capabilities
echo "→ cargo build --release..."
cargo build --release
echo "=== All checks passed ==="
