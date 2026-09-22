#!/usr/bin/env bash
# Runs one test-bed row: clones the repository at its pinned SHA (shallow), runs the incumbent tool
# with the repository's own configuration, and records the output, the wall-clock time and the
# peak memory. The Rulebearing column is added by wave 1.
#
# Plan: docs/plans/pending/0000-wave-0-spike.md, Step 7 item 2. Requirement: docs/prd.md#nfr-conf-03.
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
json.dump({"repo": repo, "sha": sha, "role": role, "tool": tool, "status": status,
           "detail": detail, "incumbent": timing, "rulebearing": None},
          open(path, "w"), indent=2)
open(path, "a").write("\n")
print(f"run: {repo}: {status} {detail}".rstrip())
PY
}

if [ "$tool" = "none" ]; then
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

# Runs a command in a directory under /usr/bin/time, recording wall-clock and peak RSS.
timed() { # dir, log, command...
  local dir="$1" log="$2"; shift 2
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
' "$started" "$ended" "${rss:-}" "$status" "$out/timing.json"
  return "$status"
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
          const v = (m.devDependencies || {})["dependency-cruiser"] || (m.dependencies || {})["dependency-cruiser"];
          if (v) { console.log(v.replace(/^[\^~]/, "")); break; }
        }
        if (d === path.dirname(d)) { console.log(fs.readFileSync(process.argv[1], "utf8").trim()); break; }
      }' "$here/../conformance/dependency-cruiser/PIN")"
    timed "$dir" "$out/incumbent.json" npx --yes "dependency-cruiser@$version" \
      --config "$(basename "$config")" --output-type json --no-progress .
    status=$?
    if python3 -c 'import json, sys; json.load(open(sys.argv[1]))' "$out/incumbent.json" 2>/dev/null; then
      # dependency-cruiser exits with the number of error-severity violations.
      if [ "$status" -eq 0 ]; then result ok "dependency-cruiser@$version"; else result failed "dependency-cruiser@$version reported $status errors"; fi
    else
      result error "dependency-cruiser@$version produced no JSON (exit $status, see incumbent.json.time)"
    fi
    ;;
  netarchtest | archunitnet)
    solution="$(field solution)"
    test_project="$(field test)"
    if ! (cd "$checkout" && dotnet build "$solution" -c Release -p:DebugType=portable) > "$out/build.log" 2>&1; then
      result error "dotnet build failed (see build.log)"
      exit 0
    fi
    timed "$checkout" "$out/incumbent.log" dotnet test "$test_project" -c Release --no-build \
      --logger "trx;LogFileName=incumbent.trx" --results-directory "$out"
    status=$?
    if [ -f "$out/incumbent.trx" ]; then
      if [ "$status" -eq 0 ]; then result ok "$tool tests passed"; else result failed "$tool tests failed (exit $status)"; fi
    else
      result error "dotnet test produced no results (exit $status, see incumbent.log)"
    fi
    ;;
  import-linter)
    venv="$checkout/.rb-venv"
    if ! { python3 -m venv "$venv" && "$venv/bin/pip" install --quiet import-linter &&
           (cd "$checkout" && "$venv/bin/pip" install --quiet -e .); } > "$out/install.log" 2>&1; then
      result error "installing the package or import-linter failed (see install.log)"
      exit 0
    fi
    timed "$checkout" "$out/incumbent.txt" "$venv/bin/lint-imports" --no-cache
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
