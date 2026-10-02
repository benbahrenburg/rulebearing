#!/usr/bin/env bash
# The guard's latency for NFR-PERF-03: generate the 5,500-module tree, start
# `rulebearing guard --watch` on it, save seeded files one at a time and read, from the findings
# file, how long each check took from the save to the answer written (`latencyMs`, the daemon's
# own timer). Prints the p50 and p95 and each stage's median, and exits 1 when the p95 is at or
# above the threshold.
#
# Usage: testbeds/synth/guard.sh [<tree directory>]   (default: a folder under the temp directory)
# $RB_GUARD_EDITS (default 40) and $RB_GUARD_SEED (default 42) set the edits,
# $RB_GUARD_THRESHOLD_MS (default 100) the threshold; the binary is $RULEBEARING_BIN, else
# target/release/rulebearing.
# Take the figure on a machine running nothing else of this repository's
# (docs/adr/0059-gates-run-by-tier-and-mutants-by-diff.md).
# Plan: docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md, Step 16 and section 1.7.
# Requirement: docs/prd.md#nfr-perf-03. Results: docs/perf.md.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
root="$(cd "$here/../.." && pwd -P)"
tree="${1:-${TMPDIR:-/tmp}/rulebearing-synth-guard}"
bin="${RULEBEARING_BIN:-$root/target/release/rulebearing}"
[ -x "$bin" ] || (cd "$root" && cargo build --quiet --release -p rb-cli)

node "$here/gen.mjs" "$tree"
cp "$here/dependency-cruiser.cjs" "$tree/.dependency-cruiser.cjs"
rm -rf "$tree/.graph"

python3 - "$bin" "$tree" "${RB_GUARD_EDITS:-40}" "${RB_GUARD_SEED:-42}" \
  "${RB_GUARD_THRESHOLD_MS:-100}" <<'PY'
import json, os, random, subprocess, sys, time

binary, tree = sys.argv[1], sys.argv[2]
edits, seed, threshold = int(sys.argv[3]), int(sys.argv[4]), int(sys.argv[5])
findings = os.path.join(tree, ".graph", "guard", "findings.json")


def read():
    try:
        with open(findings, encoding="utf-8") as handle:
            return json.load(handle)
    except (OSError, ValueError):
        return None


def wait(test, limit, what):
    started = time.monotonic()
    while True:
        found = read()
        if found is not None and test(found):
            return found
        if time.monotonic() - started > limit:
            sys.exit(f"guard: {what} did not arrive within {limit} s: {found}")
        time.sleep(0.002)


guard = subprocess.Popen(
    [binary, "guard", "--watch", "--interval", "5",
     "--config", ".dependency-cruiser.cjs", "apps", "packages"],
    cwd=tree, stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
)
try:
    wait(lambda f: True, 120, "the first answer")
    sources = sorted(
        os.path.join(folder, name)
        for part in ("apps", "packages")
        for folder, _, names in os.walk(os.path.join(tree, part))
        for name in names
        if name.endswith(".ts")
    )
    chosen = random.Random(seed).sample(sources, min(edits, len(sources)))
    latencies, extracts, answers = [], [], []
    for path in chosen:
        relative = os.path.relpath(path, tree)
        with open(path, "rb") as handle:
            original = handle.read()
        for content, measured in ((original + b"\n// edit\n", True), (original, False)):
            before = read()["writtenAt"]
            with open(path, "wb") as handle:
                handle.write(content)
            found = wait(
                lambda f: f["writtenAt"] > before and f.get("rechecked") == [relative],
                60, f"the check of {relative}",
            )
            if measured:
                if found.get("error"):
                    sys.exit(f"guard: the check of {relative} failed: {found['error']}")
                latencies.append(found["latencyMs"])
                extracts.append(found["timings"]["extract"])
                answers.append(found["timings"]["answer"])
finally:
    guard.stdin.close()
    guard.wait(timeout=60)


def at(values, quantile):
    ordered = sorted(values)
    return ordered[min(len(ordered) - 1, int(round(quantile * (len(ordered) - 1))))]


p50, p95 = at(latencies, 0.5), at(latencies, 0.95)
print(
    f"guard: p50 {p50} ms, p95 {p95} ms, max {max(latencies)} ms over {len(latencies)} saved files"
    f" (median extract {at(extracts, 0.5)} ms, answer {at(answers, 0.5)} ms);"
    f" threshold {threshold} ms"
)
sys.exit(1 if p95 >= threshold else 0)
PY
