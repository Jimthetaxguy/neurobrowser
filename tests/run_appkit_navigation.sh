#!/usr/bin/env bash
set -euo pipefail

task_repo_root="$(cd "$(dirname "$0")/.." && pwd)"
task_binary="${CARGO_TARGET_DIR:-$task_repo_root/target}/appkit-navigation-test"
mkdir -p "$(dirname "$task_binary")"
xcrun swiftc "$task_repo_root/NeuroBrowser/ContentViewController.swift" \
  "$task_repo_root/tests/appkit_navigation.swift" -o "$task_binary"
task_output="$(mktemp -d "${TMPDIR:-/tmp}/neurobrowser-navigation.XXXXXX")"
python3 "$task_repo_root/tests/navigation_server.py" "$task_output/port" >"$task_output/http.log" 2>&1 &
task_server_pid=$!
trap 'kill "$task_server_pid" 2>/dev/null || true; wait "$task_server_pid" 2>/dev/null || true' EXIT
for _ in {1..100}; do
  [[ -s "$task_output/port" ]] && break
  kill -0 "$task_server_pid" 2>/dev/null || { cat "$task_output/http.log"; exit 1; }
  sleep 0.1
done
[[ -s "$task_output/port" ]] || { cat "$task_output/http.log"; exit 1; }
"$task_binary" "http://127.0.0.1:$(cat "$task_output/port")"
