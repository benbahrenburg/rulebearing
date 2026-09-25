#!/usr/bin/env bash
# The .NET oracle harness: NetArchTest or ArchUnitNET tests under `dotnet test` against the same
# tests imported into Rulebearing, test by test, on one pinned repository.
#
# Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 11 (the oracle
# harness) and section 1.4.5. Requirement: docs/prd.md#nfr-conf-03. Design:
# docs/artifacts/design.md, "Test beds", item 1.
# Usage: testbeds/oracles/dotnet.sh <owner/repo> [out-dir]   (default out-dir: testbeds/out)
#
# For a manifest row whose tool is netarchtest or archunitnet:
#   1. clone at the SHA (the checkout testbeds/run.sh and spike-b-attribution.sh use);
#   2. `dotnet build <test> -c Release -p:DebugType=portable`, with Windows targeting on and NuGet
#      signature checks off for the throwaway clone, as testbeds/run.sh does;
#   3. `dotnet test <test> --no-build --logger trx`, with the row's `filter` when it has one;
#   4. `rulebearing import archunit <tests>` in the checkout (`tests`: the folder of the
#      architecture tests, default the test project's folder), imported again with `--graph` over
#      a cruise of the assemblies the first import names, so that types from packages resolve;
#      then `rulebearing cruise -T junit` with the imported configuration;
#   5. compare.py joins each TRX result with the JUnit cases of the rules named from its method.
# A test the importer writes commented out is recorded as `stays` (custom predicate) or
# `not-imported` with the importer's reason; it is never counted as a disagreement.
#
# Writes testbeds/results/<owner>__<repo>.json (or under $RB_ORACLE_RESULTS), which
# testbeds/oracles/table.py renders, and keeps every intermediate file in <out-dir>/<owner>__<repo>.
# Exit 0 when every compared test agrees, 1 when one disagrees, 2 when the row could not be compared.
set -uo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
# shellcheck source-path=SCRIPTDIR source=lib.sh
. "$here/lib.sh"
repo="${1:?usage: testbeds/oracles/dotnet.sh <owner/repo> [out-dir]}"
slug="${repo//\//__}"
out="${2:-$here/../out}/$slug"
results="${RB_ORACLE_RESULTS:-$here/../results}"
checkouts="${RB_TESTBED_CHECKOUTS:-${RUNNER_TEMP:-${TMPDIR:-/tmp}}/rulebearing-testbeds}"
checkout="$checkouts/$slug"
bin="${RULEBEARING_BIN:-$here/../../target/release/rulebearing}"
manifest="$here/../manifest.yaml"
mkdir -p "$out" "$results" "$checkouts"
out="$(cd "$out" && pwd -P)"
result="$results/$slug.json"
[ -x "$bin" ] || { echo "dotnet-oracle: no binary at $bin; cargo build --release -p rb-cli" >&2; exit 2; }
bin="$(cd "$(dirname "$bin")" && pwd -P)/$(basename "$bin")"
sha="$(oracle_field "$manifest" "$repo" sha)" || { echo "dotnet-oracle: $repo is not in the manifest" >&2; exit 2; }
tool="$(oracle_field "$manifest" "$repo" tool)"
case "$tool" in
  netarchtest | archunitnet) ;;
  *) echo "dotnet-oracle: $repo's tool is $tool, not netarchtest or archunitnet" >&2; exit 2 ;;
esac
test_project="$(oracle_field "$manifest" "$repo" test)"
tests="$(oracle_field "$manifest" "$repo" tests)"
tests="${tests:-$(dirname "$test_project")}"
filter="$(oracle_field "$manifest" "$repo" filter)"

oracle_clone "$repo" "$sha" "$checkout" "$out/clone.log" ||
  { oracle_error "$result" "$repo" "$sha" "$tool" "clone failed at $sha"; exit 2; }

if ! (cd "$checkout" && DOTNET_NUGET_SIGNATURE_VERIFICATION=false \
      dotnet build "$test_project" -c Release -p:DebugType=portable -p:EnableWindowsTargeting=true) > "$out/build.log" 2>&1; then
  oracle_error "$result" "$repo" "$sha" "$tool" "dotnet build $test_project failed (see build.log)"
  exit 2
fi

filter_args=()
[ -n "$filter" ] && filter_args=(--filter "$filter")
rm -f "$out/incumbent.trx"
(cd "$checkout" && dotnet test "$test_project" -c Release --no-build ${filter_args[@]+"${filter_args[@]}"} \
  --logger "trx;LogFileName=incumbent.trx" --results-directory "$out") > "$out/incumbent.log" 2>&1
test_status=$?
if [ ! -f "$out/incumbent.trx" ]; then
  oracle_error "$result" "$repo" "$sha" "$tool" "dotnet test wrote no TRX file (exit $test_status, see incumbent.log)"
  exit 2
fi

if ! (cd "$checkout" && "$bin" import archunit "$tests" --out "$out/imported.yaml") 2> "$out/import.err"; then
  oracle_error "$result" "$repo" "$sha" "$tool" "rulebearing import archunit failed: $(head -c 300 "$out/import.err")"
  exit 2
fi
# A test that names a type from a package (MediatR's IRequestHandler<>) cannot be resolved from
# source; `--graph` gives the importer the code layer of the built assemblies, referenced types
# included, as a user migrating would. The first import names the assemblies; `-T json` does not
# gate, so a cruise that completes writes the graph whatever it finds.
if (cd "$checkout" && "$bin" cruise --config "$out/imported.yaml" -T json --no-progress .) > "$out/graph.json" 2> "$out/graph.err" &&
   (cd "$checkout" && "$bin" import archunit "$tests" --graph "$out/graph.json" --out "$out/imported-graph.yaml") 2>> "$out/import.err"; then
  mv "$out/imported-graph.yaml" "$out/imported.yaml"
fi
rm -f "$out/graph.json"
rm -f "$out/rulebearing.xml"
# Without an active rule there is nothing to cruise and no report: compare.py then gives every test
# `stays` or `not-imported`, and the row the status `nothing-compared` (exit 3), never agreement.
if grep -q '^ *- name:' "$out/imported.yaml"; then
  (cd "$checkout" && "$bin" cruise --config "$out/imported.yaml" -T junit --no-progress .) > "$out/rulebearing.xml" 2> "$out/rulebearing.err"
  cruise_status=$?
  if ! grep -q '<testsuites' "$out/rulebearing.xml"; then
    oracle_error "$result" "$repo" "$sha" "$tool" "rulebearing cruise exited $cruise_status with no JUnit report: $(grep -v '^warning' "$out/rulebearing.err" | head -c 300)"
    exit 2
  fi
fi

python3 "$here/compare.py" dotnet --trx "$out/incumbent.trx" --imported "$out/imported.yaml" \
  --junit "$out/rulebearing.xml" --tests-dir "$checkout/$tests" --tests-shown "$tests" \
  --cwd "$checkout" --repo "$repo" --sha "$sha" --tool "$tool" --out "$result"
