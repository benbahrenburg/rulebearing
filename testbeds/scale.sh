#!/usr/bin/env bash
# The scale timing for one row (design § Test beds, item 3): clone the repository at its pinned
# SHA, build it with the manifest's `build` command, write a configuration with `rulebearing init`,
# and time three cruises of it with the JSON reporter; the median is the row's Rulebearing time,
# which testbeds/check-regression.sh fails the night on when it grows by more than 20%.
#
# Plan: docs/plans/implemented/0002-wave-2-dotnet-python-element-rules.md, Step 15.
# Source: docs/artifacts/design.md#test-beds-open-source-repositories-to-validate-against.
# Usage: testbeds/scale.sh <owner/repo> [out-dir]   (default out-dir: testbeds/out)
# The binary is $RULEBEARING_BIN, else target/release/rulebearing; checkouts go to
# $RB_TESTBED_CHECKOUTS as for testbeds/run.sh.
#
# A .NET row is also cruised three times in source mode (`--mode source`, plan 0003 Step 14), its
# median kept as rulebearing-source-timing.json beside the compiled one: the scale table
# (testbeds/bench/scale_table.py, plan 0003 Step 24) has a row for each mode.
#
# With $RULEBEARING_BASELINE_BIN, the baseline build is cruised after each run on this runner
# (median_cruise in lib.sh), its medians kept as baseline-timing.json and, for a .NET row,
# baseline-source-timing.json. The scale table's regression check compares each row with that
# pair, because hosted runners differ by more than 20% on the same binary from one night to the next.
#
# Writes <out-dir>/<owner>__<repo>/result.json (the shape testbeds/run.sh writes) with status:
#   ok      the cruises completed; the detail counts the modules per language
#   failed  init could not write a configuration, or a cruise did not complete (see the logs)
#   error   the clone or the build failed
# Like run.sh, it never fails itself: the nightly reports the row and carries on.
set -uo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
# shellcheck source=testbeds/lib.sh
. "$here/lib.sh"
repo="${1:?usage: testbeds/scale.sh <owner/repo> [out-dir]}"
slug="${repo//\//__}"
out="${2:-$here/out}/$slug"
checkout="${RB_TESTBED_CHECKOUTS:-${RUNNER_TEMP:-${TMPDIR:-/tmp}}/rulebearing-testbeds}/$slug"
bin="${RULEBEARING_BIN:-$here/../target/release/rulebearing}"
mkdir -p "$out"
out="$(cd "$out" && pwd -P)"
sha="$(manifest_field sha)" || exit 2
role="$(manifest_field role)"

if ! clone_row > "$out/clone.log" 2>&1; then
  row_result error "clone failed at $sha (see clone.log)"
  exit 0
fi
if ! build_row "$out/build.log"; then
  row_result error "the build command failed (see build.log)"
  exit 0
fi
if ! (cd "$checkout" && SOURCE_DATE_EPOCH=1790000000 "$bin" init --owner testbed --force) > "$out/init.log" 2>&1; then
  row_result failed "init could not write a configuration: $(tail -1 "$out/init.log")"
  exit 0
fi
cp "$checkout/rulebearing.yaml" "$out/rulebearing.yaml"
# json does not gate (docs/adr/0030-the-reporter-decides-the-error-count-exit.md): 0 is a
# completed run whatever it found.
paths=()
while IFS= read -r path; do paths+=("$path"); done < <(init_paths)
if ! median_cruise --output-type json "${paths[@]}"; then
  row_result failed "a cruise did not complete (see cruise.out.time)"
  exit 0
fi
if [[ " $(manifest_field languages) " == *dotnet* ]]; then
  # median_cruise clears and rewrites the baseline's files, so the compiled pass's are set aside.
  mv "$out/rulebearing-timing.json" "$out/rulebearing-compiled-timing.json"
  [ -f "$out/baseline-timing.json" ] && mv "$out/baseline-timing.json" "$out/baseline-compiled-timing.json"
  [ -f "$out/baseline.err" ] && mv "$out/baseline.err" "$out/baseline-compiled.err"
  if ! median_cruise --output-type json --mode source "${paths[@]}"; then
    mv "$out/rulebearing-compiled-timing.json" "$out/rulebearing-timing.json"
    row_result failed "a source-mode cruise did not complete (see cruise.out.time)"
    exit 0
  fi
  mv "$out/rulebearing-timing.json" "$out/rulebearing-source-timing.json"
  mv "$out/rulebearing-compiled-timing.json" "$out/rulebearing-timing.json"
  [ -f "$out/baseline-timing.json" ] && mv "$out/baseline-timing.json" "$out/baseline-source-timing.json"
  [ -f "$out/baseline.err" ] && mv "$out/baseline.err" "$out/baseline-source.err"
  [ -f "$out/baseline-compiled-timing.json" ] && mv "$out/baseline-compiled-timing.json" "$out/baseline-timing.json"
  [ -f "$out/baseline-compiled.err" ] && mv "$out/baseline-compiled.err" "$out/baseline.err"
fi
modules="$(python3 -c '
import collections, json, sys
counts = collections.Counter(m.get("language", "unknown") for m in json.load(open(sys.argv[1]))["modules"])
print(", ".join(f"{language} {n}" for language, n in sorted(counts.items())))
' "$out/cruise.out")"
rm -f "$out/cruise.out"
row_result ok "modules: $modules"
exit 0
