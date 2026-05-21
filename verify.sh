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
CARGO_TARGET_DIR=/tmp/neurobrowser-tauri-target cargo check --manifest-path src-tauri/Cargo.toml
echo "→ cargo build --release..."
cargo build --release
echo "=== All checks passed ==="
