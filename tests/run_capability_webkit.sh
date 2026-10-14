#!/usr/bin/env bash
set -euo pipefail

task_repo_root="$(cd "$(dirname "$0")/.." && pwd)"
task_output="$(mktemp -d "${TMPDIR:-/tmp}/neurobrowser-capability-webkit.XXXXXX")"
task_server_pid=""
trap 'if [[ -n "$task_server_pid" ]]; then kill "$task_server_pid" 2>/dev/null || true; wait "$task_server_pid" 2>/dev/null || true; fi' EXIT

# kill -0 is true for a zombie, so a crashed server would look alive for the
# whole wait and its stderr would stay hidden until the timeout.
task_http_server_alive() {
  local stat
  kill -0 "$task_server_pid" 2>/dev/null || return 1
  stat="$(ps -p "$task_server_pid" -o stat= 2>/dev/null || true)"
  stat="${stat//[[:space:]]/}"
  [[ -z "$stat" || "$stat" != Z* ]]
}

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
# HTTPServer.server_bind calls socket.getfqdn on 127.0.0.1 before the constructor
# returns. On the macOS 26 GitHub runner that reverse lookup stalls (often longer
# than this wait) with no stderr, so the port file never appears. Bind through
# TCPServer and keep the numeric host instead.
python3 - "$task_repo_root/tests/fixtures/capability-site" "$task_output/port" >"$task_output/http.log" 2>&1 <<'PY' &
import functools, http.server, pathlib, socketserver, sys

class LoopbackHTTPServer(http.server.ThreadingHTTPServer):
    def server_bind(self):
        socketserver.TCPServer.server_bind(self)
        host, port = self.server_address[:2]
        self.server_name = host
        self.server_port = port

handler = functools.partial(http.server.SimpleHTTPRequestHandler, directory=sys.argv[1])
with LoopbackHTTPServer(('127.0.0.1', 0), handler) as server:
    pathlib.Path(sys.argv[2]).write_text(str(server.server_port))
    server.serve_forever()
PY
task_server_pid=$!
for _ in {1..200}; do
  [[ -s "$task_output/port" ]] && break
  task_http_server_alive || { task_dump_http_failure; exit 1; }
  sleep 0.1
done
[[ -s "$task_output/port" ]] || { task_dump_http_failure; exit 1; }
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
