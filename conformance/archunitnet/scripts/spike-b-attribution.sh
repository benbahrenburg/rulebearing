#!/usr/bin/env bash
# Spike B: the share of the .NET oracle repositories' types the Rust reader attributes to a source
# file, the figure ADR-0003's 99% trigger is decided on.
#
# Plan: docs/plans/pending/0000-wave-0-spike.md, Step 9 (the pooled line) and § 1.6 (the rules for
# the denominator, .NET Framework projects and repositories that will not build).
# Decision: docs/adr/0003-dotnet-extractor-fallback.md.
#
# For every testbeds/manifest.yaml row whose tool is netarchtest or archunitnet: clone at the pinned
# SHA (outside this repository), `dotnet build <solution> -c Release -p:DebugType=portable`, run the
# `attribution` example, and write conformance/archunitnet/attribution/<owner>__<repo>.json with the
# counts per assembly, the unattributed types by name and the projects that produced no assembly. A
# repository that fails to clone, or where nothing builds, is written with its error and left out of
# the pooled figure (§ 1.9: "reported as error and excluded"). Prints:
#   spike-b: repos=N built=B error=E types=T excluded=X pdb=P inferred=I none=Z raw=R adjusted=A net4x_projects=K non_portable_pdb_types=M
# Usage: conformance/archunitnet/scripts/spike-b-attribution.sh [owner/repo ...]
set -uo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
root="$(cd "$here/../.." && pwd -P)"
out="$here/attribution"
checkouts="${RB_TESTBED_CHECKOUTS:-${RUNNER_TEMP:-${TMPDIR:-/tmp}}/rulebearing-testbeds}"
mkdir -p "$out" "$checkouts"
checkouts="$(cd "$checkouts" && pwd -P)"

(cd "$root" && cargo build --quiet --release -p rb-extract-dotnet --example attribution) || exit 2
example="$root/target/release/examples/attribution"

rows() {
  python3 -c '
import sys, yaml
rows = yaml.safe_load(open(sys.argv[1]))["rows"]
wanted = set(sys.argv[2:])
for r in rows:
    if r["tool"] in ("netarchtest", "archunitnet") and (not wanted or r["repo"] in wanted):
        print(r["repo"], r["sha"], r["solution"], sep="\t")
' "$root/testbeds/manifest.yaml" "$@"
}

write_error() { # file, repo, sha, reason
  python3 -c 'import json, sys; json.dump({"repo": sys.argv[2], "sha": sys.argv[3], "error": sys.argv[4]}, open(sys.argv[1], "w"), indent=2)' "$@"
}

while IFS=$'\t' read -r repo sha solution; do
  slug="${repo//\//__}"
  checkout="$checkouts/$slug"
  file="$out/$slug.json"
  echo "spike-b: $repo"
  if [ ! -d "$checkout/.git" ] || [ "$(git -C "$checkout" rev-parse HEAD 2>/dev/null)" != "$sha" ]; then
    rm -rf "$checkout" && mkdir -p "$checkout"
    if ! { git -C "$checkout" init --quiet &&
           git -C "$checkout" remote add origin "https://github.com/$repo.git" &&
           git -C "$checkout" fetch --quiet --depth 1 origin "$sha" &&
           git -C "$checkout" -c advice.detachedHead=false checkout --quiet FETCH_HEAD; } > "$out/$slug.log" 2>&1; then
      write_error "$file" "$repo" "$sha" "clone failed"
      continue
    fi
  fi
  # As testbeds/run.sh: Windows-targeting projects build on Linux, and NuGet signature verification
  # is off for these throwaway clones (the Linux certificate bundle rejects some valid signatures).
  # MSBuild carries on past a project that fails, so a solution with one unbuildable project (a
  # Visual Studio extension, say) is still measured: every project without output is listed in the
  # report's errors, and only a repository where nothing built is left out of the pooled figure.
  (cd "$checkout" && DOTNET_NUGET_SIGNATURE_VERIFICATION=false \
    dotnet build "$solution" -c Release -p:DebugType=portable -p:EnableWindowsTargeting=true) > "$out/$slug.log" 2>&1
  build_status=$?
  if ! "$example" --solution "$checkout/$solution" --configuration Release --repository "$checkout" > "$file.full" 2>> "$out/$slug.log"; then
    write_error "$file" "$repo" "$sha" "attribution failed (see the log)"
    rm -f "$file.full"
    continue
  fi
  if [ "$(python3 -c 'import json, sys; print(len(json.load(open(sys.argv[1]))["assemblies"]))' "$file.full")" = "0" ]; then
    reason="$(grep -m1 -E 'error [A-Z]+[0-9]+' "$out/$slug.log" | sed -E 's/^.*(error [A-Z]+[0-9]+:[^[]*).*$/\1/' | cut -c1-200)"
    write_error "$file" "$repo" "$sha" "nothing built: ${reason:-see the build log}"
    rm -f "$file.full"
    continue
  fi
  rm -f "$out/$slug.log"
  # Keep the counts and the unattributed types; every attributed type is reproducible from the build.
  python3 - "$file.full" "$file" "$repo" "$sha" "$build_status" <<'PY'
import json, sys
full = json.load(open(sys.argv[1]))
for assembly in full["assemblies"]:
    assembly["unattributed"] = [t["fullName"] for t in assembly.pop("types") if t.get("attribution") == "none"]
full = {"repo": sys.argv[3], "sha": sys.argv[4], "buildExitCode": int(sys.argv[5]), **full}
json.dump(full, open(sys.argv[2], "w"), indent=2)
open(sys.argv[2], "a").write("\n")
PY
  rm -f "$file.full"
done < <(rows "$@")

python3 - "$out" <<'PY'
import glob, json, os, sys
repos = built = errors = 0
totals = dict(typesTotal=0, typesExcluded=0, pdbAttributed=0, inferred=0, none=0)
net4x = non_portable = 0
for path in sorted(glob.glob(os.path.join(sys.argv[1], "*.json"))):
    data = json.load(open(path))
    repos += 1
    if "error" in data:
        errors += 1
        continue
    built += 1
    for key in totals:
        totals[key] += data["pooled"][key]
    net4x += data.get("net4xProjects", 0)
    non_portable += data.get("nonPortablePdbTypes", 0)
attributed = totals["pdbAttributed"] + totals["inferred"]
raw = attributed / totals["typesTotal"] if totals["typesTotal"] else 0
adjusted_base = totals["typesTotal"] - totals["typesExcluded"]
adjusted = attributed / adjusted_base if adjusted_base else 0
print(f"spike-b: repos={repos} built={built} error={errors} types={totals['typesTotal']} "
      f"excluded={totals['typesExcluded']} pdb={totals['pdbAttributed']} inferred={totals['inferred']} "
      f"none={totals['none']} raw={raw:.4f} adjusted={adjusted:.4f} net4x_projects={net4x} "
      f"non_portable_pdb_types={non_portable}")
PY
