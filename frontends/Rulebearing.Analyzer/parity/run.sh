#!/usr/bin/env bash
# Rulebearing.Analyzer against the gate on the extractor's sample fixture: the rules beside this
# script, as written and with every rule negated, judged by `cruise` over the fixture's compiled
# graph and by the analyzer attached to a build of the fixture's projects; any difference fails.
# CI's `analyzer` job runs it on every pull request; the nightly runs the same comparison on each
# .NET oracle (testbeds/oracles/dotnet.sh, analyzer_parity.py).
# Plan: docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md, Step 21.
# Requirement: docs/prd.md#fr-dist-04.
#
# Usage: frontends/Rulebearing.Analyzer/parity/run.sh
#   RULEBEARING_BIN  the binary, default target/release/rulebearing (built when absent)
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
root="$(cd "$here/../../.." && pwd -P)"
bin="${RULEBEARING_BIN:-$root/target/release/rulebearing}"
if [ ! -x "$bin" ]; then
  cargo build --release --locked -p rb-cli --manifest-path "$root/Cargo.toml"
fi
analyzer="$root/frontends/Rulebearing.Analyzer/src/Rulebearing.Analyzer"
dotnet build "$analyzer/Rulebearing.Analyzer.csproj" -c Release --nologo --verbosity quiet
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
cp -R "$root/crates/rb-extract-dotnet/tests/fixtures/sample/." "$work/"
cp "$here/rulebearing.yaml" "$work/rules.yaml"
(cd "$work" && "$bin" cruise --config rules.yaml -T json --no-progress .) > "$work/graph.json" 2> "$work/graph.err" || {
  cat "$work/graph.err" >&2
  exit 2
}
python3 "$root/testbeds/oracles/analyzer_parity.py" --repo fixtures/sample --sha local \
  --checkout "$work" --config "$work/rules.yaml" --graph "$work/graph.json" --binary "$bin" \
  --analyzer "$analyzer/bin/Release/netstandard2.0" --project src/Sample.csproj \
  --work "$work/analyzer" --out "$work/parity.json" || {
  cat "$work/parity.json" >&2
  exit 1
}
