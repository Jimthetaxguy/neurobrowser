#!/usr/bin/env bash
set -euo pipefail

# Shared with tests/run_appkit_navigation.sh, which sets
# task_import_http_helpers=1 and sources this file. kill -0 is true for a
# zombie, so a crashed server would look alive for the whole wait and its
# stderr would stay hidden until the timeout. An empty ps status still counts
# as alive: a blank stat line is not proof the process died.
task_http_server_alive() {
  local stat
  kill -0 "$task_server_pid" 2>/dev/null || return 1
  stat="$(ps -p "$task_server_pid" -o stat= 2>/dev/null || true)"
  stat="${stat//[[:space:]]/}"
  [[ -z "$stat" || "$stat" != Z* ]]
}

# Poll until the port file has contents. Fail as soon as the server exits or
# becomes a zombie. attempts is how many 0.1s pauses to allow.
task_wait_for_http_port() {
  local port_file="$1"
  local attempts="$2"
  local attempt
  for ((attempt = 0; attempt < attempts; attempt++)); do
    [[ -s "$port_file" ]] && return 0
    task_http_server_alive || return 1
    sleep 0.1
  done
  [[ -s "$port_file" ]]
}

if [[ "${task_import_http_helpers:-}" == 1 ]]; then
  unset task_import_http_helpers
  return 0
fi

task_repo_root="$(cd "$(dirname "$0")/.." && pwd)"
task_output="$(mktemp -d "${TMPDIR:-/tmp}/neurobrowser-capability-webkit.XXXXXX")"
task_server_pid=""
trap 'if [[ -n "$task_server_pid" ]]; then kill "$task_server_pid" 2>/dev/null || true; wait "$task_server_pid" 2>/dev/null || true; fi' EXIT

task_dump_http_failure() {
  echo 'Local workload HTTP server did not start' >&2
  if [[ -s "$task_output/http.log" ]]; then
    echo '--- workload HTTP server stderr ---' >&2
    cat "$task_output/http.log" >&2
  else
    echo 'Workload HTTP server stderr was empty.' >&2
  fi
  if [[ -n "${task_server_pid}" ]] && task_http_server_alive; then
    echo "Workload HTTP server is still running (pid ${task_server_pid})." >&2
  else
    echo 'Workload HTTP server exited before the port file was written.' >&2
  fi
}

# Bind an ephemeral loopback port and write it only after the listener is ready.
# LoopbackHTTPServer in navigation_server.py skips the reverse lookup described there.
python3 - "$task_repo_root/tests/fixtures/capability-site" "$task_output/port" "$task_repo_root/tests/navigation_server.py" >"$task_output/http.log" 2>&1 <<'PY' &
import functools, http.server, importlib.util, pathlib, sys

spec = importlib.util.spec_from_file_location("navigation_server", sys.argv[3])
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
handler = functools.partial(http.server.SimpleHTTPRequestHandler, directory=sys.argv[1])
with module.LoopbackHTTPServer(("127.0.0.1", 0), handler) as server:
    pathlib.Path(sys.argv[2]).write_text(str(server.server_port))
    server.serve_forever()
PY
task_server_pid=$!
task_wait_for_http_port "$task_output/port" 200 || { task_dump_http_failure; exit 1; }
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
