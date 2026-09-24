#!/usr/bin/env bash
# Runs one test-bed row: clones the repository at its pinned SHA (shallow), runs the incumbent tool
# with the repository's own configuration, and records the output, the wall-clock time and the
# peak memory. For a dependency-cruiser row it then times `rulebearing cruise` with the same
# configuration and roots (the median of three runs); the binary is $RULEBEARING_BIN, else
# target/release/rulebearing, and without one the Rulebearing column stays empty. With
# $RULEBEARING_BASELINE_BIN it times that build beside it, for check-regression.sh. The zero diff is
# not taken here: these clones have no dependencies installed, so the incumbent runs without
# TypeScript. Layer 5 installs them and diffs the three named oracles, and the summary takes their
# zero diff from it (conformance/dependency-cruiser/scripts/run-layer-5.sh).
#
# Plans: docs/plans/pending/0000-wave-0-spike.md, Step 7 item 2; docs/plans/pending/0001-wave-1-typescript-parity.md, Step 18.
# Requirement: docs/prd.md#nfr-conf-03.
# Usage: testbeds/run.sh <owner/repo> [out-dir]   (default out-dir: testbeds/out)
# Checkouts go to $RB_TESTBED_CHECKOUTS (default: rulebearing-testbeds under the temp directory).
#
# Writes <out-dir>/<owner>__<repo>/result.json with status one of:
#   ok      the incumbent ran and produced its output
#   failed  the incumbent ran and reported violations or failing tests (its output is kept)
#   error   the clone, the build or the tool itself failed; `detail` says which
#   idle    the row has no incumbent tool, so there is nothing to run until wave 1
# A row that errors never fails the script: the nightly reports it and carries on (§ 1.7, Reliability).
set -uo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
repo="${1:?usage: testbeds/run.sh <owner/repo> [out-dir]}"
out_root="${2:-$here/out}"
slug="${repo//\//__}"
out="$out_root/$slug"
# Outside this repository: MSBuild, eslint, tsconfig and editorconfig all search upward for their
# configuration, and a checkout inside this tree would build under this repository's
# Directory.Build.props (warnings as errors) instead of its own.
checkouts="${RB_TESTBED_CHECKOUTS:-${RUNNER_TEMP:-${TMPDIR:-/tmp}}/rulebearing-testbeds}"
checkout="$checkouts/$slug"
mkdir -p "$out"
out="$(cd "$out" && pwd -P)"

field() {
  python3 -c '
import sys, yaml
rows = yaml.safe_load(open(sys.argv[1]))["rows"]
match = [r for r in rows if r["repo"] == sys.argv[2]]
if not match:
    sys.exit(f"run: {sys.argv[2]} is not in the manifest")
value = match[0].get(sys.argv[3], "")
print(",".join(value) if isinstance(value, list) else value)
' "$here/manifest.yaml" "$repo" "$1"
}

sha="$(field sha)" || exit 2
role="$(field role)"
tool="$(field tool)"

result() { # status, detail
  python3 - "$out/result.json" "$repo" "$sha" "$role" "$tool" "$1" "$2" <<'PY'
import json, os, sys
path, repo, sha, role, tool, status, detail = sys.argv[1:8]
timing_path = os.path.join(os.path.dirname(path), "timing.json")
timing = json.load(open(timing_path)) if os.path.exists(timing_path) else None
def optional(name):
    p = os.path.join(os.path.dirname(path), name)
    return json.load(open(p)) if os.path.exists(p) else None
rulebearing = optional("rulebearing-timing.json")
row = {"repo": repo, "sha": sha, "role": role, "tool": tool, "status": status,
       "detail": detail, "incumbent": timing, "rulebearing": rulebearing}
regression = optional("regression.json")
if regression:
    row["regression"] = regression
json.dump(row, open(path, "w"), indent=2)
open(path, "a").write("\n")
print(f"run: {repo}: {status} {detail}".rstrip())
PY
}

if [ "$tool" = "none" ]; then
  # A greenfield or scale row with its own nightly job (testbeds/greenfield.sh, testbeds/scale.sh)
  # has written its result already; that result is kept.
  if [ -f "$out/result.json" ]; then
    echo "run: $repo: kept the result its own job wrote"
    exit 0
  fi
  result idle "no incumbent tool; the Rulebearing column starts in wave 1"
  exit 0
fi

# Clone the one commit, shallowly, read-only.
rm -rf "$checkout"
mkdir -p "$checkout"
if ! { git -C "$checkout" init --quiet &&
       git -C "$checkout" remote add origin "https://github.com/$repo.git" &&
       git -C "$checkout" fetch --quiet --depth 1 origin "$sha" &&
       git -C "$checkout" -c advice.detachedHead=false checkout --quiet FETCH_HEAD; } > "$out/clone.log" 2>&1; then
  result error "clone failed at $sha (see clone.log)"
  exit 0
fi

# Runs a command in a directory under /usr/bin/time, recording wall-clock and peak RSS in the
# timing file.
timed() { # timing file, dir, log, command...
  local timing="$1" dir="$2" log="$3"; shift 3
  local started ended status rss
  started="$(python3 -c 'import time; print(time.time())')"
  if [ "$(uname)" = "Darwin" ]; then
    (cd "$dir" && /usr/bin/time -l "$@") > "$log" 2> "$log.time"
    status=$?
    rss="$(awk '/maximum resident set size/ {print int($1/1024)}' "$log.time")"
  else
    (cd "$dir" && /usr/bin/time -v "$@") > "$log" 2> "$log.time"
    status=$?
    rss="$(awk -F': ' '/Maximum resident set size/ {print $2}' "$log.time")"
  fi
  ended="$(python3 -c 'import time; print(time.time())')"
  python3 -c '
import json, sys
json.dump({"wall_seconds": round(float(sys.argv[2]) - float(sys.argv[1]), 2),
           "max_rss_kb": int(sys.argv[3]) if sys.argv[3] else None,
           "exit_code": int(sys.argv[4])}, open(sys.argv[5], "w"), indent=2)
' "$started" "$ended" "${rss:-}" "$status" "$timing"
  return "$status"
}

# The Rulebearing time for a dependency-cruiser row: three timed runs with the incumbent's
# configuration and roots, the median kept. A run that cannot complete (exit 2 or 3, say an extended
# tsconfig from an uninstalled package) records no time and says why in rulebearing.err.
#
# With $RULEBEARING_BASELINE_BIN (the build behind the previous committed summary, its commit in
# $RULEBEARING_BASELINE_SHA), each run is followed by one of the baseline, so the pair that
# check-regression.sh compares comes from one runner. A baseline that cannot complete leaves no
# pair and says why in baseline.err; it never costs the row its Rulebearing time.
rulebearing_column() { # dir, config
  local dir="$1" config="$2" bin baseline run
  bin="${RULEBEARING_BIN:-$here/../target/release/rulebearing}"
  baseline="${RULEBEARING_BASELINE_BIN:-}"
  [ -x "$bin" ] || return 0
  [ -n "$baseline" ] && [ ! -x "$baseline" ] && baseline=""
  for run in 1 2 3; do
    # json does not gate (docs/adr/0030-the-reporter-decides-the-error-count-exit.md): 0 is a
    # completed run whatever it found.
    if ! timed "$out/rulebearing-timing-$run.json" "$dir" "$out/rulebearing.json" \
         "$bin" cruise --config "$config" --output-type json --no-progress --no-liveness .; then
      cp "$out/rulebearing.json.time" "$out/rulebearing.err"
      return 0
    fi
    if [ -n "$baseline" ] &&
       ! timed "$out/baseline-timing-$run.json" "$dir" "$out/baseline.json" \
         "$baseline" cruise --config "$config" --output-type json --no-progress --no-liveness .; then
      cp "$out/baseline.json.time" "$out/baseline.err"
      rm -f "$out"/baseline-timing-*.json
      baseline=""
    fi
  done
  python3 - "$out" "${RULEBEARING_BASELINE_SHA:-}" <<'PY'
import json, os, sys
out, baseline_sha = sys.argv[1], sys.argv[2]
def median(prefix):
    paths = [os.path.join(out, f"{prefix}-timing-{n}.json") for n in (1, 2, 3)]
    if not all(os.path.exists(p) for p in paths):
        return None
    runs = sorted((json.load(open(p)) for p in paths), key=lambda r: r["wall_seconds"])
    return runs[1]
current, baseline = median("rulebearing"), median("baseline")
json.dump(current, open(os.path.join(out, "rulebearing-timing.json"), "w"), indent=2)
if baseline:
    json.dump({"current": current["wall_seconds"], "baseline": baseline["wall_seconds"],
               "baseline_sha": baseline_sha},
              open(os.path.join(out, "regression.json"), "w"), indent=2)
PY
}

case "$tool" in
  dependency-cruiser)
    config="$(field config)"
    dir="$checkout/$(dirname "$config")"
    # The version the repository pins, else the version the conformance suite is pinned to.
    version="$(cd "$dir" && node -e '
      const fs = require("fs"), path = require("path");
      for (let d = process.cwd(); ; d = path.dirname(d)) {
        const p = path.join(d, "package.json");
        if (fs.existsSync(p)) {
          const m = JSON.parse(fs.readFileSync(p, "utf8"));
          // dependency-cruiser cruising itself runs the version its own manifest declares.
          const v = m.name === "dependency-cruiser" ? m.version
            : (m.devDependencies || {})["dependency-cruiser"] || (m.dependencies || {})["dependency-cruiser"];
          if (v) { console.log(v.replace(/^[\^~]/, "")); break; }
        }
        if (d === path.dirname(d)) { console.log(fs.readFileSync(process.argv[1], "utf8").trim()); break; }
      }' "$here/../conformance/dependency-cruiser/PIN")"
    # Installed into a tool folder rather than run through npx: inside dependency-cruiser's own
    # repository npx would pick up the local, uninstalled package.
    tools="$checkouts/.tools/dependency-cruiser-$version"
    if [ ! -x "$tools/node_modules/.bin/depcruise" ] &&
       ! npm install --silent --no-audit --no-fund --prefix "$tools" "dependency-cruiser@$version" > "$out/install.log" 2>&1; then
      result error "installing dependency-cruiser@$version failed (see install.log)"
      exit 0
    fi
    timed "$out/timing.json" "$dir" "$out/incumbent.json" "$tools/node_modules/.bin/depcruise" \
      --config "$(basename "$config")" --output-type json --no-progress .
    status=$?
    if python3 -c 'import json, sys; json.load(open(sys.argv[1]))' "$out/incumbent.json" 2>/dev/null; then
      rulebearing_column "$dir" "$(basename "$config")"
      # dependency-cruiser exits with the number of error-severity violations.
      if [ "$status" -eq 0 ]; then result ok "dependency-cruiser@$version"; else result failed "dependency-cruiser@$version reported $status errors"; fi
    else
      result error "dependency-cruiser@$version produced no JSON (exit $status, see incumbent.json.time)"
    fi
    ;;
  netarchtest | archunitnet)
    test_project="$(field test)"
    # Only the architecture-test project and what it references: that is all the incumbent runs,
    # and it keeps unrelated projects (a Visual Studio extension, say) from failing the row.
    # EnableWindowsTargeting lets projects that target Windows build on a Linux runner, as a user
    # building in CI would. NuGet signature verification is off for these throwaway clones only:
    # the Linux runner's certificate bundle rejects some valid author signatures (NU3012).
    if ! (cd "$checkout" && DOTNET_NUGET_SIGNATURE_VERIFICATION=false \
          dotnet build "$test_project" -c Release -p:DebugType=portable -p:EnableWindowsTargeting=true) > "$out/build.log" 2>&1; then
      result error "dotnet build failed (see build.log)"
      exit 0
    fi
    timed "$out/timing.json" "$checkout" "$out/incumbent.log" dotnet test "$test_project" -c Release --no-build \
      --logger "trx;LogFileName=incumbent.trx" --results-directory "$out"
    status=$?
    if [ -f "$out/incumbent.trx" ]; then
      if [ "$status" -eq 0 ]; then result ok "$tool tests passed"; else result failed "$tool tests failed (exit $status)"; fi
    else
      result error "dotnet test produced no results (exit $status, see incumbent.log)"
    fi
    ;;
  import-linter)
    # `dir` names the package folder that holds the contracts, for a repository with several.
    dir="$checkout/$(field dir)"
    venv="$checkout/.rb-venv"
    if ! { python3 -m venv "$venv" && "$venv/bin/pip" install --quiet import-linter &&
           (cd "$dir" && "$venv/bin/pip" install --quiet -e .); } > "$out/install.log" 2>&1; then
      result error "installing the package or import-linter failed (see install.log)"
      exit 0
    fi
    timed "$out/timing.json" "$dir" "$out/incumbent.txt" "$venv/bin/lint-imports" --no-cache
    status=$?
    case "$status" in
      0) result ok "import-linter contracts kept" ;;
      1) result failed "import-linter reported broken contracts" ;;
      *) result error "lint-imports exited $status (see incumbent.txt)" ;;
    esac
    ;;
  *)
    result error "unknown tool $tool"
    ;;
esac
exit 0
