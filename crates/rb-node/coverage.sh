#!/usr/bin/env bash
# rb-node's Rust line coverage, measured through its JavaScript tests: the addon is built
# instrumented under `cargo llvm-cov show-env`, Node loads it while vitest runs __tests__/, and
# llvm-cov reports the crate. No Rust test binary can link the N-API symbols, which only a Node
# process has (Cargo.toml, [lib]). Prints llvm-cov's JSON summary, as `cargo llvm-cov --json` does.
# Called by scripts/coverage-per-crate.sh (docs/adr/0018-test-coverage-threshold.md).
# Plan: docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md, Step 22.
#
# Usage: crates/rb-node/coverage.sh   (needs Node 22, the root's `npm ci`, and cargo-llvm-cov)
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
root="$(cd "$here/../.." && pwd -P)"
cd "$root"
# A target directory of its own: the instrumentation reaches rustc through a wrapper cargo does not
# fingerprint, so instrumented builds in target/debug would be taken for plain ones afterwards (an
# instrumented binary writes default.profraw wherever it runs).
export CARGO_TARGET_DIR="$root/target/rb-node-coverage"
eval "$(cargo llvm-cov show-env --sh 2>/dev/null)"
# The tests build the addon and the binary with this environment, so both are instrumented; the
# addon writes its profile when vitest's workers exit (__tests__/flush-coverage.ts).
unset RULEBEARING_ADDON RULEBEARING_BINARY
cargo llvm-cov clean --workspace >&2
(cd "$here" && "$root/node_modules/.bin/vitest" run) >&2
cargo llvm-cov report --package rb-node --summary-only --json
