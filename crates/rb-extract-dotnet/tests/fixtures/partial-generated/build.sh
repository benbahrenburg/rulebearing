#!/usr/bin/env bash
# Rebuilds the partial-type fixture: deterministic, CI path-mapped (documents become /_/...,
# relative to the repository root), portable PDB. Generated/Square.g.cs is compiled from
# src/obj/Generator/, where a source generator's output lies, and removed with obj/ afterwards.
# Commit built/ with the new hashes in PROVENANCE.md.
# Decision: docs/adr/0061-a-type-is-attributed-to-a-file-its-developer-wrote.md.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
rm -rf "$here/src/obj" "$here/src/bin"
mkdir -p "$here/src/obj/Generator"
cp "$here/src/Generated/Square.g.cs" "$here/src/obj/Generator/Square.g.cs"
dotnet build "$here/src/PartialGenerated.csproj" --configuration Release --output "$here/built" \
  -p:DebugType=portable -p:Deterministic=true -p:ContinuousIntegrationBuild=true \
  -p:BaseIntermediateOutputPath="$here/intermediate/" --nologo --verbosity quiet
find "$here/built" -type f ! -name 'PartialGenerated.dll' ! -name 'PartialGenerated.pdb' -delete
rm -rf "$here/intermediate" "$here/src/obj" "$here/src/bin"
(cd "$here/built" && shasum -a 256 PartialGenerated.dll PartialGenerated.pdb)
echo "built with .NET SDK $(dotnet --version)"
