#!/usr/bin/env bash
# The guard's latency for NFR-PERF-03: generate the 5,500-module tree, start
# `rulebearing guard --watch` on it, save seeded files one at a time and read, from the findings
# file, how long each check took from the save to the answer written (`latencyMs`, the daemon's
# own timer). Each file is saved twice: once with a comment added, which leaves the graph as it
# was, and once with an import added, which changes it and so has the rules evaluated again.
# Prints the p50 and p95 and each stage's median for both kinds of save, and exits 1 when either
# p95 is at or above its threshold. Both thresholds are 100 ms unless set: a developer machine
# must meet both; the CI runner holds the graph-changing save to a recorded ceiling instead
# (docs/adr/0060-the-guards-latency-target-is-set-for-a-developer-machine.md).
#
# Usage: testbeds/synth/guard.sh [<tree directory>]   (default: a folder under the temp directory)
# $RB_GUARD_EDITS (default 40) and $RB_GUARD_SEED (default 42) set the edits,
# $RB_GUARD_THRESHOLD_MS (default 100) the threshold for a save that leaves the graph as it was,
# $RB_GUARD_CHANGED_THRESHOLD_MS (default 100) the one for a save that changes it; the binary is
# $RULEBEARING_BIN, else target/release/rulebearing.
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
  "${RB_GUARD_THRESHOLD_MS:-100}" "${RB_GUARD_CHANGED_THRESHOLD_MS:-100}" <<'PY'
import json, os, random, subprocess, sys, time

binary, tree = sys.argv[1], sys.argv[2]
edits, seed = int(sys.argv[3]), int(sys.argv[4])
thresholds = {"a comment added": int(sys.argv[5]), "an import added": int(sys.argv[6])}
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
    kinds = {
        "a comment added": b"\n// edit\n",
        "an import added": b'\nimport "node:path";\n',
    }
    measured = {kind: {"latency": [], "extract": [], "answer": []} for kind in kinds}
    for path in chosen:
        relative = os.path.relpath(path, tree)
        with open(path, "rb") as handle:
            original = handle.read()
        # Each save is followed by the original again, which is not measured.
        saves = [step for kind, added in kinds.items() for step in ((original + added, kind), (original, None))]
        for content, kind in saves:
            with open(path, "wb") as handle:
                handle.write(content)
            # A scan that started after the save has seen it; an earlier answer for the same
            # file, or the heartbeat's rewrite of one, is not taken for this save's.
            saved = time.time() * 1000
            found = wait(
                lambda f: f["seenUpTo"] >= saved and f.get("rechecked") == [relative],
                60, f"the check of {relative}",
            )
            if kind is not None:
                if found.get("error"):
                    sys.exit(f"guard: the check of {relative} failed: {found['error']}")
                measured[kind]["latency"].append(found["latencyMs"])
                measured[kind]["extract"].append(found["timings"]["extract"])
                measured[kind]["answer"].append(found["timings"]["answer"])
finally:
    guard.stdin.close()
    guard.wait(timeout=60)


def at(values, quantile):
    ordered = sorted(values)
    return ordered[min(len(ordered) - 1, int(round(quantile * (len(ordered) - 1))))]


over = False
for kind, taken in measured.items():
    p50, p95 = at(taken["latency"], 0.5), at(taken["latency"], 0.95)
    threshold = thresholds[kind]
    over = over or p95 >= threshold
    print(
        f"guard, {kind}: p50 {p50} ms, p95 {p95} ms, max {max(taken['latency'])} ms over"
        f" {len(taken['latency'])} saved files (median extract {at(taken['extract'], 0.5)} ms,"
        f" answer {at(taken['answer'], 0.5)} ms); threshold {threshold} ms"
    )
sys.exit(1 if over else 0)
PY
