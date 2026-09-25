#!/usr/bin/env bash
# The scale timing for one row (design § Test beds, item 3): clone the repository at its pinned
# SHA, build it with the manifest's `build` command, write a configuration with `rulebearing init`,
# and time three cruises of it with the JSON reporter; the median is the row's Rulebearing time,
# which testbeds/check-regression.sh fails the night on when it grows by more than 20%.
#
# Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 15.
# Source: docs/artifacts/design.md#test-beds-open-source-repositories-to-validate-against.
# Usage: testbeds/scale.sh <owner/repo> [out-dir]   (default out-dir: testbeds/out)
# The binary is $RULEBEARING_BIN, else target/release/rulebearing; checkouts go to
# $RB_TESTBED_CHECKOUTS as for testbeds/run.sh.
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
modules="$(python3 -c '
import collections, json, sys
counts = collections.Counter(m.get("language", "unknown") for m in json.load(open(sys.argv[1]))["modules"])
print(", ".join(f"{language} {n}" for language, n in sorted(counts.items())))
' "$out/cruise.out")"
rm -f "$out/cruise.out"
row_result ok "modules: $modules"
exit 0
