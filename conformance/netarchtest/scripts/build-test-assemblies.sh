#!/usr/bin/env bash
# Builds the assemblies NetArchTest's own unit tests load, at the version in ../PIN, with portable
# PDBs, and copies them with NetArchTest's LICENSE (MIT) into ../fixtures/: NetArchTest.TestStructure
# (every predicate, condition and dependency-search test) and the two CrossAssemblyTest projects
# (`Inherit` across an assembly boundary).
#
# Plans: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 7 (NetArchTest's
# `Types.InAssembly(...)` tests mapped through the element rules).
# Decisions: docs/adr/0009-conformance-suites-as-specification.md, docs/adr/0019-mit-licence.md (only
# the binary fixtures and the licence are committed; the C# source is fetched here, never committed).
#
# Debug, as upstream builds and tests (`dotnet test` with no configuration in its CI): the
# dependency-search tests read method bodies, locals and compiler-generated closures as Debug IL
# lays them out. The upstream target frameworks (netstandard2.1 and netstandard2.0) build with the
# .NET 10 SDK unchanged. The build is deterministic and CI-path-mapped (source paths become /_/...),
# so a rebuild with the same SDK gives the same bytes and fixtures/SHA256SUMS stays valid. Run it
# once, and again only when PIN changes; commit the diff under fixtures/ with the new hashes.
# Needs git and a .NET SDK.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
pin="$(tr -d '[:space:]' < "$here/PIN")"
# The physical path: on macOS the temporary directory sits behind the /var -> /private/var symlink,
# and a path the compiler sees differently from git's SourceRoot escapes the /_/ mapping.
work="$(cd "$(mktemp -d)" && pwd -P)"
trap 'rm -rf "$work"' EXIT

echo "build-test-assemblies: cloning NetArchTest v$pin"
git -c advice.detachedHead=false clone --quiet --depth 1 --branch "v$pin" \
  https://github.com/BenMorris/NetArchTest.git "$work/NetArchTest"

projects=(
  test/NetArchTest.TestStructure/NetArchTest.TestStructure.csproj
  test/NetArchTest.CrossAssemblyTest.A/NetArchTest.CrossAssemblyTest.A.csproj
  test/NetArchTest.CrossAssemblyTest.B/NetArchTest.CrossAssemblyTest.B.csproj
)
out="$here/fixtures"
names=()
for project in "${projects[@]}"; do
  name="$(basename "$project" .csproj)"
  names+=("$name")
  dotnet build "$work/NetArchTest/$project" \
    --configuration Debug \
    --output "$work/out/$name" \
    -p:DebugType=portable \
    -p:Deterministic=true \
    -p:ContinuousIntegrationBuild=true \
    --nologo --verbosity quiet
  cp "$work/out/$name/$name.dll" "$work/out/$name/$name.pdb" "$out/"
done
cp "$work/NetArchTest/LICENSE" "$out/LICENSE"
(cd "$out" && for name in "${names[@]}"; do shasum -a 256 "$name.dll" "$name.pdb"; done > SHA256SUMS)
echo "build-test-assemblies: built at commit $(git -C "$work/NetArchTest" rev-parse HEAD) with .NET SDK $(dotnet --version)"
cat "$out/SHA256SUMS"
echo "build-test-assemblies: update the SDK line and hashes in fixtures/README.md if they changed, then commit fixtures/"
