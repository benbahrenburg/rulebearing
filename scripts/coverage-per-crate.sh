#!/usr/bin/env bash
# Enforces the per-crate line-coverage floor from docs/adr/0018-test-coverage-threshold.md.
# Usage: scripts/coverage-per-crate.sh <percent>
set -euo pipefail
floor="${1:-70}"
fail=0
for crate in crates/*/ xtask/; do
  name="$(basename "$crate")"
  # rb-cli and xtask are binary crates; llvm-cov measures them through their unit tests.
  pct="$(cargo llvm-cov --package "$name" --all-features --summary-only --json 2>/dev/null \
        | python3 -c 'import json,sys; d=json.load(sys.stdin); print(d["data"][0]["totals"]["lines"]["percent"])' 2>/dev/null || echo 0)"
  printf '%-20s %6.2f%%\n' "$name" "$pct"
  if python3 -c "import sys; sys.exit(0 if float('$pct') >= float('$floor') else 1)"; then :; else
    echo "::error::$name is below the ${floor}% line-coverage floor (ADR-0018)"
    fail=1
  fi
done
exit "$fail"
