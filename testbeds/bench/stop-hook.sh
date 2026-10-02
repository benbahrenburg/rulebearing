#!/usr/bin/env bash
# The Stop-hook p95 for one repository (NFR-PERF-02): clone it at its pinned SHA without building
# it, give it the benchmark's configuration, and time the hook over seeded one-line edits with a
# warm cache (testbeds/bench/stop_hook.py). The nightly runs it on dotnet/aspnetcore in source mode
# and fails when the p95 reaches 2 s.
#
# Plan: docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md, Step 17 and section 1.7.
# Requirement: docs/prd.md#nfr-perf-02. Figures: docs/perf.md.
# Usage: testbeds/bench/stop-hook.sh <owner/repo> <configuration> [results-file]
#   (default results file: testbeds/results/stop-hook.json)
# The binary is $RULEBEARING_BIN, else target/release/rulebearing; checkouts go to
# $RB_TESTBED_CHECKOUTS as for testbeds/run.sh; $RB_STOP_HOOK_EDITS (default 200) and
# $RB_STOP_HOOK_SEED (default 42) set the edits. Exits as stop_hook.py does: 0 under the
# threshold, 1 at or above it, 2 when the clone or a hook run fails.
set -uo pipefail

bench="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
here="$(cd "$bench/.." && pwd -P)"
# shellcheck source=testbeds/lib.sh
. "$here/lib.sh"
repo="${1:?usage: testbeds/bench/stop-hook.sh <owner/repo> <configuration> [results-file]}"
configuration="${2:?usage: testbeds/bench/stop-hook.sh <owner/repo> <configuration> [results-file]}"
results="${3:-$here/results/stop-hook.json}"
slug="${repo//\//__}"
checkout="${RB_TESTBED_CHECKOUTS:-${RUNNER_TEMP:-${TMPDIR:-/tmp}}/rulebearing-testbeds}/$slug"
bin="${RULEBEARING_BIN:-$here/../target/release/rulebearing}"
bin="$(cd "$(dirname "$bin")" && pwd -P)/$(basename "$bin")"
sha="$(manifest_field sha)" || exit 2

if ! clone_row; then
  echo "stop-hook benchmark: the clone of $repo at $sha failed" >&2
  exit 2
fi
cp "$configuration" "$checkout/rulebearing.yaml"
python3 "$bench/stop_hook.py" --repo "$checkout" --name "$repo" --bin "$bin" \
  --edits "${RB_STOP_HOOK_EDITS:-200}" --seed "${RB_STOP_HOOK_SEED:-42}" --out "$results"
