#!/usr/bin/env bash
# Rebuilds the extraction fixture: deterministic, CI path-mapped (documents become /_/..., relative
# to the repository root), portable PDB. Commit built/ with the new hashes in PROVENANCE.md.
# Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 3.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
dotnet build "$here/src/Sample.csproj" --configuration Release --output "$here/built" \
  -p:DebugType=portable -p:Deterministic=true -p:ContinuousIntegrationBuild=true \
  -p:BaseIntermediateOutputPath="$here/obj/" --nologo --verbosity quiet
find "$here/built" -type f ! -name 'Sample*.dll' ! -name 'Sample*.pdb' -delete
rm -rf "$here/obj" "$here/src/obj" "$here/src/bin" "$here/core/obj" "$here/core/bin"
(cd "$here/built" && shasum -a 256 Sample*.dll Sample*.pdb)
echo "built with .NET SDK $(dotnet --version)"
