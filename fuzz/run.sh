#!/usr/bin/env bash
# Runs a fuzz target for a time budget, seeded with the committed fixtures.
# Plan: docs/plans/pending/0000-wave-0-spike.md, Step 9. Workflow: .github/workflows/fuzz.yml.
# Usage: fuzz/run.sh [target] [seconds]   (default: metadata_reader, 600)
# Needs a nightly toolchain and cargo-fuzz (`cargo install cargo-fuzz`).
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"
target="${1:-metadata_reader}"
seconds="${2:-600}"
mkdir -p "corpus/$target"
cp ../conformance/archunitnet/fixtures/TestAssembly.dll ../conformance/archunitnet/fixtures/TestAssembly.pdb "corpus/$target/"
# The host triple explicitly: a prebuilt cargo-fuzz (as CI installs it) is a musl binary and would
# otherwise default to musl, where the address sanitizer cannot run.
host="$(rustc +nightly -vV | sed -n 's/^host: //p')"
cargo +nightly fuzz run --target "$host" "$target" "corpus/$target" -- -max_total_time="$seconds" -rss_limit_mb=2048
