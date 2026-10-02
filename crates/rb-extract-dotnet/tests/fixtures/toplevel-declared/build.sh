#!/usr/bin/env bash
# Rebuilds the top-level statements fixture whose Program is declared in another file: deterministic, CI path-mapped (documents become
# /_/..., relative to the repository root), portable PDB. Commit built/ with the new hashes in
# PROVENANCE.md. Plan: docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md, Step 14.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
dotnet build "$here/src/TopLevelDeclared.csproj" --configuration Release --output "$here/built" \
  -p:DebugType=portable -p:Deterministic=true -p:ContinuousIntegrationBuild=true \
  -p:BaseIntermediateOutputPath="$here/obj/" --nologo --verbosity quiet
find "$here/built" -type f ! -name 'TopLevelDeclared.dll' ! -name 'TopLevelDeclared.pdb' -delete
rm -rf "$here/obj" "$here/src/obj" "$here/src/bin"
(cd "$here/built" && shasum -a 256 TopLevelDeclared.dll TopLevelDeclared.pdb)
echo "built with .NET SDK $(dotnet --version)"
