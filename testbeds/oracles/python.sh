#!/usr/bin/env bash
# The Python oracle harness: import-linter against Rulebearing on one pinned repository.
#
# Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 11 (the oracle
# harness) and sub-wave 2B's gating metric. Requirement: docs/prd.md#nfr-conf-03.
# Usage: testbeds/oracles/python.sh <owner/repo> [out-dir]   (default out-dir: testbeds/out)
#
# Clones the manifest row at its SHA, installs it into a throwaway virtual environment (grimp,
# which import-linter uses, needs the package importable), runs `lint-imports`, then
# `rulebearing cruise` with testbeds/oracles/configs/<owner>__<repo>.yaml. Writes
# <out-dir>/<owner>__<repo>/oracle.json with each tool's verdict, the Rulebearing violations, and
# the difference between grimp's direct import graph of the root package and Rulebearing's local
# edges. The row agrees when import-linter keeps every contract, Rulebearing reports no error, and
# the two graphs are equal. Exit 0 when it agrees, 1 when it does not, 2 when it could not run.
set -uo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
repo="${1:?usage: testbeds/oracles/python.sh <owner/repo> [out-dir]}"
slug="${repo//\//__}"
out="${2:-$here/../out}/$slug"
checkouts="${RB_TESTBED_CHECKOUTS:-${RUNNER_TEMP:-${TMPDIR:-/tmp}}/rulebearing-testbeds}"
checkout="$checkouts/oracle-$slug"
bin="${RULEBEARING_BIN:-$here/../../target/release/rulebearing}"
config="$here/configs/$slug.yaml"
mkdir -p "$out"
[ -f "$config" ] || { echo "python-oracle: no $config" >&2; exit 2; }
[ -x "$bin" ] || { echo "python-oracle: no binary at $bin; cargo build --release -p rb-cli" >&2; exit 2; }
sha="$(python3 -c 'import sys, yaml
rows = [r for r in yaml.safe_load(open(sys.argv[1]))["rows"] if r["repo"] == sys.argv[2]]
print(rows[0]["sha"] if rows else "")' "$here/../manifest.yaml" "$repo")"
[ -n "$sha" ] || { echo "python-oracle: $repo is not in the manifest" >&2; exit 2; }
if [ "$(git -C "$checkout" rev-parse HEAD 2>/dev/null)" != "$sha" ]; then
  rm -rf "$checkout" && mkdir -p "$checkout"
  git -C "$checkout" init --quiet && git -C "$checkout" remote add origin "https://github.com/$repo.git" &&
    git -C "$checkout" fetch --quiet --depth 1 origin "$sha" &&
    git -C "$checkout" -c advice.detachedHead=false checkout --quiet FETCH_HEAD || exit 2
fi
venv="$checkouts/oracle-$slug-venv"
[ -x "$venv/bin/lint-imports" ] || {
  python3 -m venv "$venv" && "$venv/bin/pip" install --quiet import-linter && (cd "$checkout" && "$venv/bin/pip" install --quiet -e .)
} > "$out/install.log" 2>&1 || { echo "python-oracle: install failed (see install.log)" >&2; exit 2; }
(cd "$checkout" && "$venv/bin/lint-imports") > "$out/lint-imports.txt" 2>&1
lint_status=$?
cp "$config" "$checkout/rulebearing.yaml"
(cd "$checkout" && "$bin" cruise -T json --no-progress src .) > "$out/rulebearing.json" 2> "$out/rulebearing.err"
"$venv/bin/python" - "$checkout" "$out" "$lint_status" "$repo" "$sha" <<'PY'
import configparser, json, os, sys
checkout, out, lint_status, repo, sha = sys.argv[1], sys.argv[2], int(sys.argv[3]), sys.argv[4], sys.argv[5]
import grimp
settings = configparser.ConfigParser()
settings.read(os.path.join(checkout, ".importlinter"))
root = settings.get("importlinter", "root_package", fallback=None)
result = json.load(open(os.path.join(out, "rulebearing.json")))
def path(module):
    for base in ("src/", ""):
        stem = os.path.join(checkout, base + module.replace(".", "/"))
        if os.path.isdir(stem):
            return base + module.replace(".", "/") + "/__init__.py"
        if os.path.isfile(stem + ".py"):
            return base + module.replace(".", "/") + ".py"
    return module
graph = grimp.build_graph(root, include_external_packages=False)
theirs = {(path(a), path(b)) for a in graph.modules for b in graph.find_modules_directly_imported_by(a)}
prefix = path(root).rsplit("/", 1)[0]
ours = {(m["source"], d["resolved"]) for m in result["modules"] if m.get("language") == "python"
        if m["source"].startswith(prefix)
        for d in m["dependencies"] if "local" in d["dependencyTypes"] and d["resolved"].startswith(prefix)}
errors = [v for v in result["summary"]["violations"] if v["rule"]["severity"] == "error"]
report = {
    "repo": repo, "sha": sha,
    "importLinter": {"kept": lint_status == 0},
    "rulebearing": {"errors": len(errors), "violations": [v["rule"]["name"] for v in errors]},
    "graph": {"importLinter": len(theirs), "rulebearing": len(ours),
              "onlyImportLinter": sorted(map(list, theirs - ours)), "onlyRulebearing": sorted(map(list, ours - theirs))},
}
report["agrees"] = (lint_status == 0) == (not errors) and theirs == ours
json.dump(report, open(os.path.join(out, "oracle.json"), "w"), indent=2)
print(f"python-oracle: {repo}: import-linter {'kept' if lint_status == 0 else 'broken'}, "
      f"rulebearing {len(errors)} errors, graph {len(theirs)} vs {len(ours)} edges, "
      f"{'agrees' if report['agrees'] else 'DIFFERS'}")
sys.exit(0 if report["agrees"] else 1)
PY
