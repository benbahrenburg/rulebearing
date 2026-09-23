#!/usr/bin/env bash
# Conformance gate 1 (dependency-cruiser, the version in PIN): the `conformance-gate-1` job.
#
#   layer 1  crates/rb-extract-ts/tests/extract_fixtures.rs replays the recorded test/extract
#            cases against the Rust extractor; fails under threshold.json
#   layer 2  harness/run-layer-2.mjs runs upstream's test/validate and test/graph-utl specs with the
#            unit under test forwarded to `rulebearing validate`; fails on a failure not listed in
#            ../excluded.json
#   layer 3  harness/run-layer-3.mjs runs upstream's test/report specs for the wave 1 reporters with
#            each reporter forwarded to `rulebearing report`; the specs byte-compare the output
#
# Layers 4 and 5 run from scripts/run-layer-4.sh and scripts/run-layer-5.sh
# (docs/plans/pending/0001-wave-1-typescript-parity.md, Steps 12 and 18).
# Plan: docs/plans/pending/0000-wave-0-spike.md, Step 5. Decision: docs/adr/0009-conformance-suites-as-specification.md.
# Needs Node 22 or later and network access for the upstream clone (kept in upstream/, git-ignored).
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
root="$(cd "$here/../.." && pwd -P)"
pin="$(tr -d '[:space:]' < "$here/PIN")"
upstream="$here/upstream/dependency-cruiser"

echo "gate 1, layer 1: recorded test/extract cases"
(cd "$root" && cargo test --quiet -p rb-extract-ts --test extract_fixtures -- --nocapture)

if [ ! -f "$upstream/package.json" ] || [ "$(node -p "require('$upstream/package.json').version")" != "$pin" ]; then
  echo "gate 1: cloning dependency-cruiser v$pin into upstream/"
  rm -rf "$upstream"
  mkdir -p "$here/upstream"
  git -c advice.detachedHead=false clone --quiet --depth 1 --branch "v$pin" \
    https://github.com/sverweij/dependency-cruiser.git "$upstream"
  (cd "$upstream" && npm ci --no-audit --no-fund --ignore-scripts --silent)
fi

echo "gate 1, layer 2: upstream specs through the shim"
if [ -z "${RULEBEARING_BIN:-}" ]; then
  (cd "$root" && cargo build --quiet --release -p rb-cli)
fi
node "$here/harness/run-layer-2.mjs" "$upstream"

echo "gate 1, layer 3: upstream report specs through the shim"
node "$here/harness/run-layer-3.mjs" "$upstream"
