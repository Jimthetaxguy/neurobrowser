#!/usr/bin/env bash
# Build and run the real embedded desktop app; never reuse a user's profile.
set -euo pipefail
[[ "$(uname -s)" == Darwin ]] || { echo 'Native app smoke requires macOS' >&2; exit 1; }
task_repo_root="$(cd "$(dirname "$0")/.." && pwd)"
task_target_dir="${CARGO_TARGET_DIR:-$task_repo_root/src-tauri/target}"
[[ "$task_target_dir" = /* ]] || task_target_dir="$PWD/$task_target_dir"
task_output="$(mktemp -d "${TMPDIR:-/tmp}/neurobrowser-native-smoke.XXXXXX")"
CARGO_TARGET_DIR="$task_target_dir" npm --prefix "$task_repo_root/src-tauri" run tauri -- build --debug --features custom-protocol --bundles app >"$task_output/build.log" 2>&1 || { cat "$task_output/build.log"; exit 1; }
task_binary="$task_target_dir/debug/bundle/macos/NeuroBrowser.app/Contents/MacOS/neurobrowser-tauri"
NEUROBROWSER_DATA_DIR="$task_output/profile" NEUROBROWSER_SMOKE_TEST=1 "$task_binary" >"$task_output/app.log" 2>&1 &
task_app_pid=$!
trap 'kill "$task_app_pid" 2>/dev/null || true; wait "$task_app_pid" 2>/dev/null || true' EXIT
for ((task_attempt=0; task_attempt<150; task_attempt++)); do
  if grep -Fq 'native smoke: page created' "$task_output/app.log"; then
    kill -0 "$task_app_pid"
    echo 'Standalone native app: embedded frontend, native IPC and page creation passed'
    exit 0
  fi
  kill -0 "$task_app_pid" 2>/dev/null || { cat "$task_output/app.log"; exit 1; }
  sleep 0.1
done
cat "$task_output/app.log"
echo 'Native page creation did not complete' >&2
exit 1
