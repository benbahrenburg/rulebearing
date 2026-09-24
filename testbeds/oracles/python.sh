#!/usr/bin/env bash
# The Python oracle harness: import-linter against Rulebearing, contract by contract, on one
# pinned repository.
#
# Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 11 (the oracle
# harness) and section 1.4.5. Requirement: docs/prd.md#nfr-conf-03. Design:
# docs/artifacts/design.md, "Test beds", item 1.
# Usage: testbeds/oracles/python.sh <owner/repo> [out-dir]   (default out-dir: testbeds/out)
#
# For a manifest row whose tool is import-linter: clones it at its SHA, reads the contracts with
# `rulebearing import import-linter` in the row's `dir` (the folder holding the settings, default
# the root), and runs import-linter (pinned below) through lint_imports_json.py in a throwaway
# virtual environment, with the roots the importer found on PYTHONPATH: grimp reads the source and
# only needs the packages findable, so the repository's own dependencies are not installed. A row
# may name extra packages in `pip` (a custom contract type's dependency). Then `rulebearing cruise
# -T junit` with the imported rules and `cruise -T json` with none, and compare.py joins the verdicts
# per contract and compares grimp's import graph with Rulebearing's local edges.
#
# A row's `translation` names a committed hand translation under testbeds/oracles/configs/ to use
# instead of the import, only where the importer writes a contract commented out that can be
# translated by hand; the file's header says why. No row needs one: every contract the importer
# comments out is a custom contract type, which stays with import-linter.
# configs/seddonym__import-linter.yaml is not one of these: it is the hand translation that
# crates/rb-cli/tests/import.rs holds the importer's output to.
#
# Writes testbeds/results/<owner>__<repo>.json (or under $RB_ORACLE_RESULTS), which
# testbeds/oracles/table.py renders, and keeps every intermediate file in <out-dir>/<owner>__<repo>.
# Exit 0 when every contract agrees, 1 when one disagrees, 2 when the row could not be compared.
set -uo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
# shellcheck source-path=SCRIPTDIR source=lib.sh
. "$here/lib.sh"
import_linter_version=2.15
repo="${1:?usage: testbeds/oracles/python.sh <owner/repo> [out-dir]}"
slug="${repo//\//__}"
out="${2:-$here/../out}/$slug"
results="${RB_ORACLE_RESULTS:-$here/../results}"
checkouts="${RB_TESTBED_CHECKOUTS:-${RUNNER_TEMP:-${TMPDIR:-/tmp}}/rulebearing-testbeds}"
checkout="$checkouts/$slug"
venv="$checkouts/$slug-venv"
bin="${RULEBEARING_BIN:-$here/../../target/release/rulebearing}"
manifest="$here/../manifest.yaml"
mkdir -p "$out" "$results" "$checkouts"
out="$(cd "$out" && pwd -P)"
result="$results/$slug.json"
[ -x "$bin" ] || { echo "python-oracle: no binary at $bin; cargo build --release -p rb-cli" >&2; exit 2; }
bin="$(cd "$(dirname "$bin")" && pwd -P)/$(basename "$bin")"
sha="$(oracle_field "$manifest" "$repo" sha)" || { echo "python-oracle: $repo is not in the manifest" >&2; exit 2; }
tool="$(oracle_field "$manifest" "$repo" tool)"
[ "$tool" = "import-linter" ] || { echo "python-oracle: $repo's tool is $tool, not import-linter" >&2; exit 2; }
dir_rel="$(oracle_field "$manifest" "$repo" dir)"
extras="$(oracle_field "$manifest" "$repo" pip)"
dir="$checkout/${dir_rel:-.}"

oracle_clone "$repo" "$sha" "$checkout" "$out/clone.log" ||
  { oracle_error "$result" "$repo" "$sha" "$tool" "clone failed at $sha"; exit 2; }

translation="$(oracle_field "$manifest" "$repo" translation)"
if [ -n "$translation" ]; then
  config="$here/configs/$translation"
  kind="hand translation (testbeds/oracles/configs/$translation)"
  [ -f "$config" ] || { oracle_error "$result" "$repo" "$sha" "$tool" "no hand translation $config"; exit 2; }
else
  config="$out/imported.yaml"
  kind="imported"
  if ! (cd "$dir" && "$bin" import import-linter --out "$config") 2> "$out/import.err"; then
    oracle_error "$result" "$repo" "$sha" "$tool" "rulebearing import import-linter failed: $(head -c 300 "$out/import.err")"
    exit 2
  fi
fi

# The interpreter that runs import-linter also runs compare.py, so it carries PyYAML.
if ! "$venv/bin/python" -c "import importlib.metadata as m, sys, yaml; sys.exit(m.version('import-linter') != '$import_linter_version')" 2> /dev/null; then
  rm -rf "$venv"
  # shellcheck disable=SC2046 # `pip` is a comma-separated list of package names, split on purpose
  if ! { python3 -m venv "$venv" &&
         "$venv/bin/pip" install --quiet "import-linter==$import_linter_version" pyyaml $(tr ',' ' ' <<< "$extras"); } > "$out/install.log" 2>&1; then
    oracle_error "$result" "$repo" "$sha" "$tool" "installing import-linter $import_linter_version failed (see install.log)"
    exit 2
  fi
fi

roots=()
while IFS= read -r root; do roots+=("$root"); done < <("$venv/bin/python" -c '
import sys, yaml
config = yaml.safe_load(open(sys.argv[1])) or {}
for root in (config.get("languages", {}).get("python", {}) or {}).get("roots", ["."]):
    print(root)
' "$config")
pythonpath=""
root_args=()
for root in "${roots[@]}"; do
  pythonpath="${pythonpath:+$pythonpath:}$dir/$root"
  root_args+=(--roots "$root")
done
if ! (cd "$dir" && PYTHONPATH="$pythonpath" "$venv/bin/python" "$here/lint_imports_json.py" "${root_args[@]}" --out "$out/incumbent.json") > "$out/incumbent.log" 2>&1; then
  oracle_error "$result" "$repo" "$sha" "$tool" "import-linter could not run: $(tail -n 1 "$out/incumbent.log" | head -c 300)"
  exit 2
fi

# Two cruises: the imported rules as JUnit (one line per violation, where JSON would carry a chain
# per violation, gigabytes on the largest oracles), and the graph alone, with no rules, for the
# graph comparison and the re-check of a disagreeing contract.
"$venv/bin/python" -c '
import sys, yaml
config = yaml.safe_load(open(sys.argv[1])) or {}
yaml.safe_dump({"languages": config.get("languages", {})}, open(sys.argv[2], "w"))
' "$config" "$out/graph-config.yaml"
(cd "$dir" && "$bin" cruise --config "$out/graph-config.yaml" -T json --no-progress .) > "$out/graph.json" 2> "$out/graph.err"
graph_status=$?
if ! "$venv/bin/python" -c 'import json, sys; json.load(open(sys.argv[1]))' "$out/graph.json" 2> /dev/null; then
  oracle_error "$result" "$repo" "$sha" "$tool" "rulebearing cruise (graph) exited $graph_status with no JSON: $(grep -v '^warning' "$out/graph.err" | head -c 300)"
  exit 2
fi
(cd "$dir" && "$bin" cruise --config "$config" -T junit --no-progress .) > "$out/rulebearing.xml" 2> "$out/rulebearing.err"
cruise_status=$?
if ! grep -q '<testsuites' "$out/rulebearing.xml"; then
  oracle_error "$result" "$repo" "$sha" "$tool" "rulebearing cruise exited $cruise_status with no JUnit report: $(grep -v '^warning' "$out/rulebearing.err" | head -c 300)"
  exit 2
fi

# The settings file, as the importer names it in its header.
settings="$(sed -n 's/^# Imported from \(.*\) by .rulebearing import import-linter.*/\1/p' "$config" | head -n 1)"
settings="${dir_rel:+$dir_rel/}${settings:-the settings in ${dir_rel:-the root}}"
"$venv/bin/python" "$here/compare.py" python --incumbent "$out/incumbent.json" --imported "$config" \
  --junit "$out/rulebearing.xml" --graph "$out/graph.json" --repo "$repo" --sha "$sha" \
  --settings "$settings" --config-kind "$kind" --cwd "$dir" --rulebearing "$bin" --work "$out" \
  --out "$result"
