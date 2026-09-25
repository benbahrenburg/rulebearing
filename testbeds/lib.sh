#!/usr/bin/env bash
# Shared by the test-bed rows that run Rulebearing without an incumbent: testbeds/greenfield.sh
# (the init proof) and testbeds/scale.sh (the scale timings). Sourced, never run.
#
# Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 15.
# Source: docs/artifacts/design.md#test-beds-open-source-repositories-to-validate-against, items 2 and 3.
# The caller sets, before calling a function: here (testbeds/), repo, and for the rest sha, role,
# out (the row's output folder), checkout and bin (the rulebearing binary). Each function names
# what it reads, so a missing one stops it with the name rather than reading an empty string.

# A manifest field of $repo; a list is joined with commas.
manifest_field() { # field
  : "${here:?}" "${repo:?}"
  python3 -c '
import sys, yaml
rows = yaml.safe_load(open(sys.argv[1]))["rows"]
match = [r for r in rows if r["repo"] == sys.argv[2]]
if not match:
    sys.exit(f"{sys.argv[2]} is not in the manifest")
value = match[0].get(sys.argv[3], "")
print(",".join(value) if isinstance(value, list) else value)
' "$here/manifest.yaml" "$repo" "$1"
}

# Writes $out/result.json in the shape testbeds/run.sh writes, with the Rulebearing timing of
# $out/rulebearing-timing.json when there is one, and prints the status line.
row_result() { # status, detail
  : "${out:?}" "${repo:?}" "${sha:?}" "${role:?}"
  python3 - "$out/result.json" "$repo" "$sha" "$role" "$1" "$2" <<'PY'
import json, os, sys
path, repo, sha, role, status, detail = sys.argv[1:7]
timing = os.path.join(os.path.dirname(path), "rulebearing-timing.json")
json.dump({"repo": repo, "sha": sha, "role": role, "tool": "none", "status": status,
           "detail": detail, "incumbent": None,
           "rulebearing": json.load(open(timing)) if os.path.exists(timing) else None},
          open(path, "w"), indent=2)
open(path, "a").write("\n")
print(f"{repo}: {status} {detail}".rstrip())
PY
}

# Clones $repo at $sha into $checkout, shallowly. The Git LFS filter is switched off for the
# checkout, so LFS files stay pointers whether or not git-lfs is installed: no row reads them.
clone_row() {
  : "${checkout:?}" "${repo:?}" "${sha:?}"
  rm -rf "$checkout"
  mkdir -p "$checkout"
  git -C "$checkout" init --quiet &&
    git -C "$checkout" remote add origin "https://github.com/$repo.git" &&
    git -C "$checkout" fetch --quiet --depth 1 origin "$sha" &&
    git -C "$checkout" -c filter.lfs.smudge= -c filter.lfs.process= -c filter.lfs.required=false \
      -c advice.detachedHead=false checkout --quiet FETCH_HEAD
}

# The paths init's proposal is cruised over, one per line, from the last line `init` printed to
# $out/init.log ("wrote rulebearing.yaml; next: rulebearing cruise <paths>"); `.` when none.
init_paths() {
  : "${out:?}"
  local line
  line="$(sed -n 's/.*next: rulebearing cruise //p' "$out/init.log" | tail -1)"
  printf '%s\n' "${line:-.}" | tr ' ' '\n'
}

# Runs the row's manifest `build` command in the checkout, when it has one. NuGet signature
# verification is off for these throwaway clones only, as in testbeds/run.sh (NU3012 on Linux).
build_row() { # log
  : "${checkout:?}"
  local build
  build="$(manifest_field build)"
  [ -n "$build" ] || return 0
  (cd "$checkout" && DOTNET_NUGET_SIGNATURE_VERIFICATION=false bash -c "$build") > "$1" 2>&1
}

# Runs a command in a directory under /usr/bin/time, recording the wall-clock time, the peak
# resident set and the exit code in the timing file; returns the command's exit code.
timed() { # timing file, dir, log, command...
  local timing="$1" dir="$2" log="$3"
  shift 3
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

# Three timed cruises of the checkout with its rulebearing.yaml and the arguments given; the
# median is kept as $out/rulebearing-timing.json. Every run must succeed: the first that exits
# non-zero stops the loop and its exit code is returned, with its output in $out/cruise.out, so a
# failing first or second run is never hidden by a third that passes. Returns 0 when all three do.
median_cruise() { # cruise arguments...
  : "${out:?}" "${checkout:?}" "${bin:?}"
  local run status=0
  for run in 1 2 3; do
    timed "$out/cruise-timing-$run.json" "$checkout" "$out/cruise.out" "$bin" cruise --no-progress "$@"
    status=$?
    if [ "$status" -ne 0 ]; then
      echo "median_cruise: run $run of 3 exited $status" >&2
      return "$status"
    fi
  done
  python3 - "$out" <<'PY'
import json, os, sys
out = sys.argv[1]
runs = sorted((json.load(open(os.path.join(out, f"cruise-timing-{n}.json"))) for n in (1, 2, 3)),
              key=lambda r: r["wall_seconds"])
json.dump(runs[1], open(os.path.join(out, "rulebearing-timing.json"), "w"), indent=2)
PY
  return "$status"
}
