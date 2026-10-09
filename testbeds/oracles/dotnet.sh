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
#      signature checks off for the throwaway clone, as testbeds/run.sh does, and the row's
#      `msbuild` arguments when it has them;
#   3. `dotnet test <test> --no-build --logger trx`, with the row's `filter` and `msbuild`
#      arguments when it has them;
#   4. `rulebearing import archunit <tests>` in the checkout (`tests`: the folder of the
#      architecture tests, default the test project's folder), imported again with `--graph` over
#      a cruise of the assemblies the first import names, so that types from packages resolve;
#      then `rulebearing cruise -T junit` with the imported configuration;
#   5. compare.py joins each TRX result with the JUnit cases of the rules named from its method;
#   6. the plantuml round trip over the graph of step 4 (docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md,
#      Step 9): the diagram written from namespaces and from slices, then enforced.
# A test the importer writes commented out is recorded as `stays` (a custom predicate, or a test
# that runs no rule and checks the architecture in C#) or `not-imported` with the importer's
# reason; it is never counted as a disagreement.
#
# Writes testbeds/results/<owner>__<repo>.json (or under $RB_ORACLE_RESULTS), which
# testbeds/oracles/table.py renders, and keeps every intermediate file in <out-dir>/<owner>__<repo>.
# Exit 0 when every compared test agrees, 1 when one disagrees or the round trip reports a
# violation, 2 when the row could not be compared.
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
# `msbuild`: extra MSBuild arguments, separated by spaces, for a repository that builds on Linux
# only with them (testbeds/README.md, "Adding or bumping a row").
msbuild_args=()
read -r -a msbuild_args <<< "$(oracle_field "$manifest" "$repo" msbuild)"

oracle_clone "$repo" "$sha" "$checkout" "$out/clone.log" ||
  { oracle_error "$result" "$repo" "$sha" "$tool" "clone failed at $sha"; exit 2; }

if ! (cd "$checkout" && DOTNET_NUGET_SIGNATURE_VERIFICATION=false \
      dotnet build "$test_project" -c Release -p:DebugType=portable -p:EnableWindowsTargeting=true \
        ${msbuild_args[@]+"${msbuild_args[@]}"}) > "$out/build.log" 2>&1; then
  oracle_error "$result" "$repo" "$sha" "$tool" "dotnet build $test_project failed (see build.log)"
  exit 2
fi

filter_args=()
[ -n "$filter" ] && filter_args=(--filter "$filter")
rm -f "$out/incumbent.trx"
(cd "$checkout" && dotnet test "$test_project" -c Release --no-build ${filter_args[@]+"${filter_args[@]}"} \
  ${msbuild_args[@]+"${msbuild_args[@]}"} --logger "trx;LogFileName=incumbent.trx" --results-directory "$out") > "$out/incumbent.log" 2>&1
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
# The plantuml round trip (plan 0003, Step 9; docs/reporters.md#plantuml): over the oracle's own
# graph, write the diagram from namespaces and from slices (`<first namespace segment>.(*)`), then
# enforce each with an `adhereTo` rule over the types it describes; any violation is a defect.
# Written to <results>/<owner>__<repo>.plantuml.json; a violation makes the harness exit 1.
plantuml_status=0
if [ -s "$out/graph.json" ]; then
  top="$(python3 - "$out/graph.json" <<'PY'
import collections, json, sys
types = json.load(open(sys.argv[1])).get("code", {}).get("types", [])
counts = collections.Counter(
    (t.get("namespace") or "").split(".")[0] for t in types if not t.get("referenced"))
print(counts.most_common(1)[0][0] if counts else "")
PY
)"
  mkdir -p "$out/plantuml"
  summary="{\"repo\": \"$repo\", \"sha\": \"$sha\""
  for form in namespaces slices; do
    config="$out/plantuml/generate-$form.yaml"
    : > "$config"
    [ "$form" = slices ] &&
      printf 'options:\n  reporterOptions:\n    plantuml:\n      Matching: "%s.(*)"\n' "$top" > "$config"
    diagram="$out/plantuml/$form.puml"
    if ! (cd "$out/plantuml" && "$bin" cruise --config "$config" --graph "$out/graph.json" -T plantuml \
          --from "$form" -f "$diagram") 2> "$out/plantuml/$form.err"; then
      summary="$summary, \"$form\": \"not generated\""
      plantuml_status=1
      continue
    fi
    python3 - "$diagram" "$out/plantuml/enforce-$form.yaml" "$form.puml" <<'PY'
import json, re, sys
stereotypes = re.findall(r"^\[[^\]]+\] <<(.*)>>$", open(sys.argv[1]).read(), re.M)
select = "|".join(f"(?:{s})" for s in stereotypes) or "^$"
open(sys.argv[2], "w").write(
    "rules:\n  diagrams:\n    - name: adheres-to-the-generated-diagram\n"
    "      comment: \"The diagram the plantuml reporter wrote. adr:0009\"\n"
    f"      select: {{ kind: type, where: {{ resideInNamespaceMatching: {json.dumps(select)} }} }}\n"
    f"      adhereTo: {sys.argv[3]}\n")
PY
    violations="$(cd "$out/plantuml" && "$bin" cruise --config "enforce-$form.yaml" --graph "$out/graph.json" -T json 2> "$out/plantuml/enforce-$form.err" |
      python3 -c 'import json, sys; print(len(json.load(sys.stdin)["summary"]["violations"]))' 2>/dev/null)"
    summary="$summary, \"$form\": ${violations:-\"not enforced\"}"
    [ "${violations:-x}" = 0 ] || plantuml_status=1
  done
  printf '%s}\n' "$summary" > "$results/$slug.plantuml.json"
fi
# Source mode's precision against this compiled graph (plan 0003, Step 14): the checkout cruised
# again with --mode source and no rules, and the file-to-file edges both graphs know compared into
# <results>/<owner>__<repo>.source-mode.json; the nightly joins them into source-mode-precision.json.
# The solution the row names is the one source mode reads too, so both graphs are one build's.
solution="$(oracle_field "$manifest" "$repo" solution)"
if [ -s "$out/graph.json" ]; then
  printf 'languages:\n  dotnet:\n    mode: source\n' > "$out/source-mode.yaml"
  [ -n "$solution" ] && printf '    solution: %s\n' "$solution" >> "$out/source-mode.yaml"
  if (cd "$checkout" && "$bin" cruise --config "$out/source-mode.yaml" -T json --no-progress --liveness off .) > "$out/source.json" 2> "$out/source.err"; then
    python3 "$here/precision.py" --compiled "$out/graph.json" --source "$out/source.json" \
      --repo "$repo" --sha "$sha" --out "$results/$slug.source-mode.json" ||
      echo "dotnet-oracle: the source-mode comparison failed; see $out/source.err" >&2
  else
    echo "dotnet-oracle: the source-mode cruise failed; see $out/source.err" >&2
  fi
  rm -f "$out/source.json"
fi
# Rulebearing.Analyzer against the gate (plan 0003, Step 21): the imported rules as written and
# with every `should` negated, the analyzer's RB0002 findings on a rebuild of the test project
# against cruise's over this graph (testbeds/oracles/analyzer_parity.py). Run when
# RULEBEARING_ANALYZER names the folder holding Rulebearing.Analyzer.dll and YamlDotNet.dll; a
# difference fails the row. Written to <results>/<owner>__<repo>.analyzer.json.
analyzer_status=0
if [ -n "${RULEBEARING_ANALYZER:-}" ] && [ -s "$out/graph.json" ]; then
  python3 "$here/analyzer_parity.py" --repo "$repo" --sha "$sha" --checkout "$checkout" \
    --config "$out/imported.yaml" --graph "$out/graph.json" --binary "$bin" \
    --analyzer "$RULEBEARING_ANALYZER" --project "$test_project" \
    "--msbuild=${msbuild_args[*]-}" --work "$out/analyzer" --out "$results/$slug.analyzer.json" ||
    analyzer_status=1
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
compare_status=$?
if [ "$compare_status" = 0 ] && [ "$plantuml_status" != 0 ]; then
  echo "dotnet-oracle: the plantuml round trip failed; see $results/$slug.plantuml.json and $out/plantuml" >&2
  exit 1
fi
# The analyzer's disagreement fails the row whatever the tests compared, nothing-compared included.
if [ "$analyzer_status" != 0 ]; then
  echo "dotnet-oracle: Rulebearing.Analyzer disagrees with the gate; see $results/$slug.analyzer.json and $out/analyzer" >&2
  exit 1
fi
exit "$compare_status"
