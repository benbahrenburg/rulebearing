#!/usr/bin/env bash
# The greenfield init proof for one row (design § Test beds, item 2): clone the repository at its
# pinned SHA, build it with the manifest's `build` command, regenerate its init fixture and compare
# it with the committed testbeds/init/<owner>__<name>/rulebearing.yaml, then write the full
# proposal with `rulebearing init` and cruise it back. The row passes when the fixture is unchanged
# and the cruise exits 0: every rule live, every finding at error severity baselined.
#
# Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 15.
# Source: docs/artifacts/design.md#test-beds-open-source-repositories-to-validate-against.
# Usage: testbeds/greenfield.sh <owner/repo> [out-dir]   (default out-dir: testbeds/out)
# The binary is $RULEBEARING_BIN, else target/release/rulebearing; checkouts go to
# $RB_TESTBED_CHECKOUTS as for testbeds/run.sh.
#
# Writes <out-dir>/<owner>__<repo>/result.json (the shape testbeds/run.sh writes) with status:
#   ok      the fixture is unchanged and the cruise with the proposal exits 0
#   failed  the fixture changed (fixture.diff), or init or the cruise did not exit 0 (cruise.out)
#   error   the clone or the build failed, or the row has no committed fixture
# and rulebearing-timing.json, the median of three cruises, which check-regression.sh compares.
# Like run.sh, it never fails itself: the nightly reports the row and carries on.
set -uo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
# shellcheck source=testbeds/lib.sh
. "$here/lib.sh"
repo="${1:?usage: testbeds/greenfield.sh <owner/repo> [out-dir]}"
slug="${repo//\//__}"
out="${2:-$here/out}/$slug"
checkout="${RB_TESTBED_CHECKOUTS:-${RUNNER_TEMP:-${TMPDIR:-/tmp}}/rulebearing-testbeds}/$slug"
bin="${RULEBEARING_BIN:-$here/../target/release/rulebearing}"
mkdir -p "$out"
out="$(cd "$out" && pwd -P)"
sha="$(manifest_field sha)" || exit 2
role="$(manifest_field role)"
fixture="$here/init/$slug/rulebearing.yaml"

if [ ! -f "$fixture" ]; then
  row_result error "no committed init fixture at testbeds/init/$slug/rulebearing.yaml"
  exit 0
fi
if ! clone_row > "$out/clone.log" 2>&1; then
  row_result error "clone failed at $sha (see clone.log)"
  exit 0
fi
if ! build_row "$out/build.log"; then
  row_result error "the build command failed (see build.log)"
  exit 0
fi

# The fixture first, while the checkout has no configuration of init's writing.
RB_INIT_FIXTURES="$out/fixture" RULEBEARING_BIN="$bin" "$here/init/run.sh" "$checkout" "$repo" > "$out/fixture.log" 2>&1
regenerated="$out/fixture/$slug/rulebearing.yaml"
if [ ! -s "$regenerated" ]; then
  row_result failed "init --dry-run did not produce a proposal (see fixture.log)"
  exit 0
fi

# The full proposal, baseline entries and all, written where a user would have it.
if ! (cd "$checkout" && SOURCE_DATE_EPOCH=1790000000 "$bin" init --owner testbed --force) > "$out/init.log" 2>&1; then
  row_result failed "init did not write a passing proposal (see init.log)"
  exit 0
fi
cp "$checkout/rulebearing.yaml" "$out/rulebearing.yaml"
paths=()
while IFS= read -r path; do paths+=("$path"); done < <(init_paths)
median_cruise "${paths[@]}"
status=$?

if ! diff -u "$fixture" "$regenerated" > "$out/fixture.diff"; then
  row_result failed "the init fixture changed; fixture.diff is the review (regenerate with testbeds/init/run.sh)"
elif [ "$status" -ne 0 ]; then
  row_result failed "the cruise with init's proposal exits $status (see cruise.out)"
else
  rm -f "$out/fixture.diff"
  row_result ok "init's proposal cruises back with exit 0; the fixture is unchanged"
fi
exit 0
