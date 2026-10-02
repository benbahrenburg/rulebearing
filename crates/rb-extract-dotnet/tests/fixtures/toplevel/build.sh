#!/usr/bin/env bash
# Rebuilds the top-level statements fixture: deterministic, CI path-mapped (documents become
# /_/..., relative to the repository root), portable PDB. Commit built/ with the new hashes in
# PROVENANCE.md. Plan: docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md, Step 14.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
dotnet build "$here/src/TopLevel.csproj" --configuration Release --output "$here/built" \
  -p:DebugType=portable -p:Deterministic=true -p:ContinuousIntegrationBuild=true \
  -p:BaseIntermediateOutputPath="$here/obj/" --nologo --verbosity quiet
find "$here/built" -type f ! -name 'TopLevel.dll' ! -name 'TopLevel.pdb' -delete
rm -rf "$here/obj" "$here/src/obj" "$here/src/bin"
(cd "$here/built" && shasum -a 256 TopLevel.dll TopLevel.pdb)
echo "built with .NET SDK $(dotnet --version)"
