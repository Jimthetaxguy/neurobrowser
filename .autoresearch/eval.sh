#!/usr/bin/env bash
# NeuroBrowser AutoResearch eval harness.
#
# Emits a single JSON object describing the objective quality of the current tree.
# Two parts:
#   1. GATE  — the hard floor. Every KEEP candidate MUST have gate_pass=true.
#              (fmt clean, clippy -D warnings clean, all tests pass, tauri crate checks)
#   2. PANEL — objective counters to MAXIMIZE/MINIMIZE (the fitness signal).
#
# Design constraints:
#   - Offline, no extra crates (only cargo + rg, both present on this machine).
#   - Deterministic given a fixed tree + toolchain (fixed evaluation budget).
#   - Never edits anything. Read + measure only.
#
# Usage:
#   .autoresearch/eval.sh            # full gate (lib + all-targets + tauri check)
#   .autoresearch/eval.sh --quick    # lib-only gate (fast inner-loop iteration)
#
# The KEEP/DISCARD decision (gate_pass AND target-metric improved AND no regression)
# is made by the loop driver, not this script — this script only measures.

set -uo pipefail
cd "$(dirname "$0")/.." || exit 3

MODE="${1:-full}"
TAURI_TARGET="/tmp/neurobrowser-tauri-target"

run() { # run <logfile> <cmd...> -> echoes exit code
  local log="$1"; shift
  "$@" >"$log" 2>&1
  echo $?
}

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# ---- GATE ----
fmt_rc=$(run "$TMP/fmt.log" cargo fmt -- --check)

if [ "$MODE" = "--quick" ] || [ "$MODE" = "quick" ]; then
  clippy_rc=$(run "$TMP/clippy.log" cargo clippy --lib -- -D warnings)
  test_rc=$(run "$TMP/test.log" cargo test --lib)
  tauri_rc=0  # skipped in quick mode
else
  clippy_rc=$(run "$TMP/clippy.log" cargo clippy --all-targets -- -D warnings)
  test_rc=$(run "$TMP/test.log" cargo test --all-targets)
  CARGO_TARGET_DIR="$TAURI_TARGET" >/dev/null 2>&1
  tauri_rc=$(CARGO_TARGET_DIR="$TAURI_TARGET" bash -c 'cargo check --manifest-path src-tauri/Cargo.toml' >"$TMP/tauri.log" 2>&1; echo $?)
fi

# tests passed count (sum "N passed" across the test log)
test_passed=$(grep -oE '[0-9]+ passed' "$TMP/test.log" 2>/dev/null | grep -oE '[0-9]+' | paste -sd+ - | bc 2>/dev/null)
[ -z "$test_passed" ] && test_passed=0

# clippy warning count under pedantic (metric only; does NOT gate)
cargo clippy --all-targets -- -W clippy::pedantic -W clippy::nursery >"$TMP/pedantic.log" 2>&1
pedantic_warnings=$(grep -cE '^warning' "$TMP/pedantic.log" 2>/dev/null)
[ -z "$pedantic_warnings" ] && pedantic_warnings=0

gate_pass=false
if [ "$fmt_rc" -eq 0 ] && [ "$clippy_rc" -eq 0 ] && [ "$test_rc" -eq 0 ] && [ "$tauri_rc" -eq 0 ]; then
  gate_pass=true
fi

# ---- PANEL (non-test source only; minimize these) ----
SRC_GLOBS=(src src-tauri/src)
count_pat() { rg -t rust --glob '!**/tests/**' -c "$1" "${SRC_GLOBS[@]}" 2>/dev/null | awk -F: '{s+=$2} END{print s+0}'; }

unwrap_count=$(count_pat '\.unwrap\(\)')
expect_count=$(count_pat '\.expect\(')
clone_count=$(count_pat '\.clone\(\)')
todo_count=$(count_pat '\b(todo!|unimplemented!|TODO|FIXME)\b')
allow_count=$(count_pat '#\[allow\(')
src_loc=$(rg -t rust --glob '!**/tests/**' --files "${SRC_GLOBS[@]}" 2>/dev/null | xargs wc -l 2>/dev/null | tail -1 | awk '{print $1+0}')

cat <<JSON
{
  "mode": "${MODE}",
  "gate": {
    "gate_pass": ${gate_pass},
    "fmt_ok": $([ "$fmt_rc" -eq 0 ] && echo true || echo false),
    "clippy_ok": $([ "$clippy_rc" -eq 0 ] && echo true || echo false),
    "test_ok": $([ "$test_rc" -eq 0 ] && echo true || echo false),
    "tauri_ok": $([ "$tauri_rc" -eq 0 ] && echo true || echo false)
  },
  "panel": {
    "tests_passed": ${test_passed},
    "pedantic_warnings": ${pedantic_warnings},
    "unwrap_count": ${unwrap_count},
    "expect_count": ${expect_count},
    "clone_count": ${clone_count},
    "todo_fixme_count": ${todo_count},
    "allow_attr_count": ${allow_count},
    "src_loc": ${src_loc}
  }
}
JSON
