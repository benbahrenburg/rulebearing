#!/usr/bin/env bash
# Builds ArchUnitNET's TestAssembly at the version in ../PIN with a portable PDB and copies it,
# with ArchUnitNET's LICENSE and NOTICE (Apache-2.0), into ../fixtures/.
#
# Plan: docs/plans/pending/0000-wave-0-spike.md, Step 6 item 1. Decisions:
# docs/adr/0009-conformance-suites-as-specification.md, docs/adr/0019-mit-licence.md (only the
# binary fixture and its notice are committed; the C# source is fetched here, never committed).
#
# The build is deterministic and CI-path-mapped (source paths become /_/...), so a rebuild with
# the same SDK gives the same bytes and fixtures/SHA256SUMS stays valid. Run it once, and again
# only when PIN changes; commit the diff under fixtures/ together with the new hashes.
# Needs git and a .NET SDK that can target the TestAssembly's framework (net10.0 at 0.13.4).
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
pin="$(tr -d '[:space:]' < "$here/PIN")"
# The physical path: on macOS the temp directory sits behind the /var -> /private/var symlink, and
# a path the compiler sees differently from git's SourceRoot escapes the /_/ mapping and leaks the
# temporary directory's name into the binaries.
work="$(cd "$(mktemp -d)" && pwd -P)"
trap 'rm -rf "$work"' EXIT

echo "build-test-assembly: cloning ArchUnitNET $pin"
git -c advice.detachedHead=false clone --quiet --depth 1 --branch "$pin" \
  https://github.com/TNG/ArchUnitNET.git "$work/ArchUnitNET"
# The upstream global.json pins the SDK the maintainers use; the fixture only needs one that can
# build net10.0, and the SDK actually used is recorded in fixtures/README.md.
rm -f "$work/ArchUnitNET/global.json"

dotnet build "$work/ArchUnitNET/TestAssembly/TestAssembly.csproj" \
  --configuration Release \
  --output "$work/out" \
  -p:DebugType=portable \
  -p:Deterministic=true \
  -p:ContinuousIntegrationBuild=true \
  --nologo --verbosity quiet

out="$here/fixtures"
cp "$work/out/TestAssembly.dll" "$work/out/TestAssembly.pdb" "$out/"
cp "$work/ArchUnitNET/LICENSE" "$out/LICENSE"
cp "$work/ArchUnitNET/NOTICE" "$out/NOTICE"
(cd "$out" && shasum -a 256 TestAssembly.dll TestAssembly.pdb > SHA256SUMS)
echo "build-test-assembly: built with .NET SDK $(dotnet --version)"
cat "$out/SHA256SUMS"
echo "build-test-assembly: update the SDK line in fixtures/README.md if it changed, then commit fixtures/"
