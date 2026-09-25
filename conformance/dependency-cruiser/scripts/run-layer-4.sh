#!/usr/bin/env bash
# Conformance gate 1, layer 4: Rulebearing's JSON output (with --strict-schema) and every accepted
# dependency-cruiser configuration validate against the vendored upstream schemas.
#
# The checks are crates/rb-cli/tests/layer4.rs, so they also run in `cargo test`; this script runs
# them alone and prints the counts. Oracle configurations join when scripts/fetch-oracle-configs.sh
# has run, and layer 5's results when scripts/run-layer-5.sh has.
# Plan: docs/plans/pending/0001-wave-1-typescript-parity.md, Step 12.
# Decision: docs/adr/0004-graph-document-is-cruise-result-superset.md.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd -P)"
cd "$root"
cargo test --quiet -p rb-cli --test layer4 -- --nocapture --test-threads 1
