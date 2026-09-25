#!/usr/bin/env bash
# The synthetic benchmark for NFR-PERF-01: generate the 5,500-module tree, then time the design's
# command, `rulebearing cruise --config .dependency-cruiser.cjs --output-type json apps packages`,
# with two warm-up runs and ten measured runs. hyperfine is used when it is installed (CI installs
# it); otherwise the same runs are timed here. Prints the mean and p95 in seconds, then the stage
# split of one run from --progress performance-log.
#
# Usage: testbeds/synth/bench.sh [<tree directory>]   (default: a folder under the temp directory)
# Plan: docs/plans/pending/0001-wave-1-typescript-parity.md, Step 20. Results: docs/perf.md.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
root="$(cd "$here/../.." && pwd -P)"
tree="${1:-${TMPDIR:-/tmp}/rulebearing-synth}"
bin="${RULEBEARING_BIN:-$root/target/release/rulebearing}"
[ -x "$bin" ] || (cd "$root" && cargo build --quiet --release -p rb-cli)

node "$here/gen.mjs" "$tree"
cp "$here/dependency-cruiser.cjs" "$tree/.dependency-cruiser.cjs"
cd "$tree"
command=("$bin" cruise --config .dependency-cruiser.cjs --output-type json apps packages)

if command -v hyperfine > /dev/null; then
  hyperfine --warmup 2 --runs 10 --ignore-failure --export-json "$tree/hyperfine.json" \
    "${command[*]} > /dev/null"
  python3 - "$tree/hyperfine.json" <<'PY'
import json, sys
result = json.load(open(sys.argv[1]))["results"][0]
times = sorted(result["times"])
p95 = times[min(len(times) - 1, int(round(0.95 * (len(times) - 1))))]
print(f"bench: mean {result['mean']:.3f} s, p95 {p95:.3f} s over {len(times)} runs")
PY
else
  python3 - "${command[@]}" <<'PY'
import subprocess, sys, time
command = sys.argv[1:]
def once():
    started = time.perf_counter()
    subprocess.run(command, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False)
    return time.perf_counter() - started
for _ in range(2):
    once()
times = sorted(once() for _ in range(10))
p95 = times[min(len(times) - 1, int(round(0.95 * (len(times) - 1))))]
print(f"bench: mean {sum(times) / len(times):.3f} s, p95 {p95:.3f} s over {len(times)} runs (timed without hyperfine)")
PY
fi
echo "bench: stage split of one run"
"${command[@]}" --progress performance-log 2>&1 > /dev/null | tail -n 12 || true
