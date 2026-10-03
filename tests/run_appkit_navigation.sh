#!/usr/bin/env bash
set -euo pipefail

task_repo_root="$(cd "$(dirname "$0")/.." && pwd)"
task_binary="${CARGO_TARGET_DIR:-$task_repo_root/target}/appkit-navigation-test"
mkdir -p "$(dirname "$task_binary")"
xcrun swiftc "$task_repo_root/NeuroBrowser/ContentViewController.swift" \
  "$task_repo_root/tests/appkit_navigation.swift" -o "$task_binary"
"$task_binary"
