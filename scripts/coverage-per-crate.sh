#!/usr/bin/env bash
# Enforces the per-crate line-coverage floor from docs/adr/0018-test-coverage-threshold.md.
# Usage: scripts/coverage-per-crate.sh <percent>
#
# A crate whose measurement fails (a failing test, a build error) is reported as a failure with
# the tail of cargo's output, never as 0%: a number that was not measured is not printed.
set -euo pipefail
floor="${1:-70}"
fail=0
log="$(mktemp)"
trap 'rm -f "$log"' EXIT
for crate in crates/*/ xtask/; do
  name="$(basename "$crate")"
  # rb-cli and xtask are binary crates; llvm-cov measures them through their unit tests.
  if ! json="$(cargo llvm-cov --package "$name" --all-features --summary-only --json 2>"$log")"; then
    printf '%-20s %s\n' "$name" "not measured"
    echo "::error::$name: cargo llvm-cov failed, so its coverage was not measured (ADR-0018)"
    tail -n 20 "$log"
    fail=1
    continue
  fi
  pct="$(printf '%s' "$json" | python3 -c 'import json,sys; d=json.load(sys.stdin); print(d["data"][0]["totals"]["lines"]["percent"])')"
  printf '%-20s %6.2f%%\n' "$name" "$pct"
  if python3 -c "import sys; sys.exit(0 if float('$pct') >= float('$floor') else 1)"; then :; else
    echo "::error::$name is below the ${floor}% line-coverage floor (ADR-0018)"
    fail=1
  fi
done
exit "$fail"
