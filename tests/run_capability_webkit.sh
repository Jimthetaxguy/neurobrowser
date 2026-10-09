#!/usr/bin/env bash
set -euo pipefail

task_repo_root="$(cd "$(dirname "$0")/.." && pwd)"
task_output="$(mktemp -d "${TMPDIR:-/tmp}/neurobrowser-capability-webkit.XXXXXX")"
task_server_pid=""
trap 'if [[ -n "$task_server_pid" ]]; then kill "$task_server_pid" 2>/dev/null || true; wait "$task_server_pid" 2>/dev/null || true; fi' EXIT

# Bind an ephemeral loopback port and write it only after the listener is ready.
python3 - "$task_repo_root/tests/fixtures/capability-site" "$task_output/port" >"$task_output/http.log" 2>&1 <<'PY' &
import functools, http.server, pathlib, sys
handler = functools.partial(http.server.SimpleHTTPRequestHandler, directory=sys.argv[1])
with http.server.ThreadingHTTPServer(('127.0.0.1', 0), handler) as server:
    pathlib.Path(sys.argv[2]).write_text(str(server.server_port))
    server.serve_forever()
PY
task_server_pid=$!
for _ in {1..200}; do
  [[ -s "$task_output/port" ]] && break
  kill -0 "$task_server_pid" 2>/dev/null || { cat "$task_output/http.log"; exit 1; }
  sleep 0.1
done
[[ -s "$task_output/port" ]] || { echo 'Local workload HTTP server did not start'; exit 1; }
task_port="$(cat "$task_output/port")"
xcrun swiftc -parse-as-library "$task_repo_root/tests/capability_webkit.swift" -o "$task_output/capability-webkit"
if "$task_output/capability-webkit" "$task_repo_root/src-tauri/src/runtime.rs" \
  "http://127.0.0.1:$task_port/" "$task_output/receipt.json"; then
  printf 'WebKit workload receipt: %s\n' "$task_output/receipt.json"
else
  task_status=$?
  printf 'Failed WebKit workload receipt: %s\n' "$task_output/receipt.json" >&2
  exit "$task_status"
fi
