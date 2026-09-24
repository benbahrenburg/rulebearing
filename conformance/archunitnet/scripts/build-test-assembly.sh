#!/usr/bin/env bash
# Builds ArchUnitNET's test fixtures at the version in ../PIN with portable PDBs and copies them,
# with ArchUnitNET's LICENSE and NOTICE (Apache-2.0), into ../fixtures/: TestAssembly (slices,
# PlantUML, the reader's own tests), the purpose-built assemblies under TestAssemblies/ that the
# element tests' snapshots were recorded against, and ArchUnitNETTests itself.
#
# Plans: docs/plans/pending/0000-wave-0-spike.md, Step 6 item 1 (TestAssembly);
# docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 7 (the TestAssemblies).
# Decisions: docs/adr/0009-conformance-suites-as-specification.md, docs/adr/0019-mit-licence.md (only
# the binary fixtures and the notice are committed; the C# source is fetched here, never committed).
#
# Debug, as upstream's CI runs the tests (`dotnet test -c Debug` in .github/workflows/build.yaml):
# the snapshots describe Debug IL, whose locals and class state machines the element rules read.
# The build is deterministic and CI-path-mapped (source paths become /_/...), so a rebuild with
# the same SDK gives the same bytes and fixtures/SHA256SUMS stays valid. Run it once, and again
# only when PIN changes; commit the diff under fixtures/ together with the new hashes.
# Needs git and a .NET SDK that can target net10.0.
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
# The upstream global.json pins the SDK the maintainers use; the fixtures only need one that can
# build net10.0, and the SDK actually used is recorded in fixtures/README.md.
rm -f "$work/ArchUnitNET/global.json"

# The assemblies the element snapshots load (ArchUnitNETTests/AssemblyTestHelper), the two that
# share full names across assemblies, and the test project itself, which is
# `ArchUnitNETTestArchitecture` for the combinator and rule-evaluation tests.
projects=(
  TestAssembly/TestAssembly.csproj
  ArchUnitNETTests/ArchUnitNETTests.csproj
  TestAssemblies/AttributeAssembly/AttributeAssembly.csproj
  TestAssemblies/ClassAssembly/ClassAssembly.csproj
  TestAssemblies/DuplicateFullNameAssembly/DuplicateFullNameAssembly.csproj
  TestAssemblies/OtherDuplicateFullNameAssembly/OtherDuplicateFullNameAssembly.csproj
  TestAssemblies/MethodDependencyAssembly/MethodDependencyAssembly.csproj
  TestAssemblies/MethodMemberAssembly/MethodMemberAssembly.csproj
  TestAssemblies/PropertyMemberAssembly/PropertyMemberAssembly.csproj
  TestAssemblies/TypeAssembly/TypeAssembly.csproj
  TestAssemblies/TypeDependencyAssembly/TypeDependencyAssembly.csproj
  TestAssemblies/VisibilityAssembly/VisibilityAssembly.csproj
)
out="$here/fixtures"
names=()
for project in "${projects[@]}"; do
  name="$(basename "$project" .csproj)"
  names+=("$name")
  # Verify, a package of the test project, turns deterministic source paths off and writes the
  # project directory into an assembly attribute; global properties put both back, so the test
  # project's bytes do not depend on where it was cloned. The others map through SourceRoot.
  extra=()
  if [ "$name" = ArchUnitNETTests ]; then
    extra=(-p:DeterministicSourcePaths=true "-p:PathMap=$work/ArchUnitNET/=/_/" "-p:ProjectDir=/_/ArchUnitNETTests/")
  fi
  dotnet build "$work/ArchUnitNET/$project" ${extra[@]+"${extra[@]}"} \
    --configuration Debug \
    --output "$work/out/$name" \
    -p:DebugType=portable \
    -p:Deterministic=true \
    -p:ContinuousIntegrationBuild=true \
    --nologo --verbosity quiet
  cp "$work/out/$name/$name.dll" "$work/out/$name/$name.pdb" "$out/"
done
cp "$work/ArchUnitNET/LICENSE" "$out/LICENSE"
cp "$work/ArchUnitNET/NOTICE" "$out/NOTICE"
(cd "$out" && for name in "${names[@]}"; do shasum -a 256 "$name.dll" "$name.pdb"; done > SHA256SUMS)
echo "build-test-assembly: built with .NET SDK $(dotnet --version)"
cat "$out/SHA256SUMS"
echo "build-test-assembly: update the SDK line and hashes in fixtures/README.md if they changed, then commit fixtures/"
