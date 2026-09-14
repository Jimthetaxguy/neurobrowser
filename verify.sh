#!/usr/bin/env bash
# NeuroBrowser verify chain (XP-7)
set -euo pipefail
echo "=== NeuroBrowser Verify Chain ==="
echo "→ cargo fmt --check..."
cargo fmt -- --check
echo "→ cargo clippy..."
cargo clippy --all-targets -- -D warnings
echo "→ cargo test..."
cargo test --all-targets
echo "→ Tauri frontend build..."
(cd src-tauri && npm ci && npm run build)
echo "→ Tauri cargo check..."
cargo check --manifest-path src-tauri/Cargo.toml --locked
echo "→ Headless check and tests..."
cargo check --manifest-path src-tauri/Cargo.toml --features headless --locked
cargo test --manifest-path src-tauri/Cargo.toml --features headless --locked --bin neurobrowser-headless
echo "→ Webview capability regression tests..."
cargo test --manifest-path src-tauri/Cargo.toml --locked --test runtime_capabilities
echo "→ cargo build --release..."
cargo build --release
echo "=== All checks passed ==="
