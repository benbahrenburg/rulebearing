#!/usr/bin/env bash
# Runs the Rulebearing.TestAdapter core tests and the six framework fixture projects, each held to
# the 70% line floor of docs/adr/0018-test-coverage-threshold.md for its own package.
# Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 14 (2H).
#
# Usage: adapters/dotnet/test.sh [--configuration Debug|Release]
#   RULEBEARING_BINARY  the binary the tests run; default target/release/rulebearing, built with
#                       `cargo build --release -p rb-cli` when it is absent.
#
# The VSTest projects (xUnit v2 and v3, NUnit, MSTest v2 and v4, and the core's) run under
# `dotnet test`, where coverlet.msbuild fails the run below the threshold (Directory.Build.props).
# TUnit runs only on Microsoft.Testing.Platform, which the .NET 10 SDK's `dotnet test` no longer
# drives through VSTest, so its fixture project runs as an executable with coverlet.MTP and this
# script checks the Cobertura line rate.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
root="$(cd "$here/../.." && pwd -P)"
configuration=Debug
if [ "${1:-}" = "--configuration" ]; then
  configuration="${2:?--configuration needs a value}"
fi
threshold=70

if [ -z "${RULEBEARING_BINARY:-}" ]; then
  RULEBEARING_BINARY="$root/target/release/rulebearing"
  case "$(uname -s)" in MINGW* | MSYS* | CYGWIN*) RULEBEARING_BINARY="$RULEBEARING_BINARY.exe" ;; esac
  if [ ! -x "$RULEBEARING_BINARY" ]; then
    echo "== building $RULEBEARING_BINARY"
    cargo build --release --locked -p rb-cli --manifest-path "$root/Cargo.toml"
  fi
fi
export RULEBEARING_BINARY
echo "== binary: $("$RULEBEARING_BINARY" --version)"

vstest=(
  Rulebearing.TestAdapter.Tests
  Rulebearing.TestAdapter.xUnit.Tests
  Rulebearing.TestAdapter.xUnitV3.Tests
  Rulebearing.TestAdapter.NUnit.Tests
  Rulebearing.TestAdapter.MSTestV2.Tests
  Rulebearing.TestAdapter.MSTestV4.Tests
)
for project in "${vstest[@]}"; do
  echo "== $project (coverlet.msbuild, line >= $threshold%)"
  dotnet test "$here/tests/$project/$project.csproj" --configuration "$configuration" --nologo \
    -p:Threshold="$threshold" -p:ThresholdType=line -p:ThresholdStat=total
done

project=Rulebearing.TestAdapter.TUnit.Tests
package=Rulebearing.TestAdapter.TUnit
echo "== $project (coverlet.MTP, line >= $threshold%)"
results="$root/target/coverage/dotnet/$project"
rm -rf "$results"
dotnet build "$here/tests/$project/$project.csproj" --configuration "$configuration" --nologo
dotnet "$here/tests/$project/bin/$configuration/net10.0/$project.dll" \
  --coverlet --coverlet-output-format cobertura --coverlet-include "[$package]*" \
  --results-directory "$results"
report="$(find "$results" -name 'coverage.cobertura.*.xml' | head -n 1)"
if [ -z "$report" ]; then
  echo "$project: coverlet.MTP wrote no Cobertura report under $results" >&2
  exit 1
fi
rate="$(sed -n 's/^<coverage line-rate="\([0-9.]*\)".*/\1/p' "$report" | head -n 1)"
percent="$(awk -v r="$rate" 'BEGIN { printf "%.2f", r * 100 }')"
if awk -v r="$rate" -v t="$threshold" 'BEGIN { exit !(r * 100 < t) }'; then
  echo "$package: line coverage $percent% is below the $threshold% floor (docs/adr/0018-test-coverage-threshold.md)" >&2
  exit 1
fi
echo "$package: line coverage $percent%"
echo "adapters/dotnet: every package at or above $threshold% line coverage"
