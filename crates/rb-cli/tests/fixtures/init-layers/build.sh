#!/usr/bin/env bash
# Rebuilds the init-layers fixture: deterministic, CI path-mapped, portable PDBs, the four
# assemblies copied to built/. Commit built/ with the new hashes in PROVENANCE.md.
# Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 15.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
rm -rf "$here/built"
dotnet build "$here/Shop.slnx" --configuration Release \
  -p:DebugType=portable -p:Deterministic=true -p:ContinuousIntegrationBuild=true \
  --nologo --verbosity quiet
mkdir -p "$here/built"
for project in Shop.Domain Shop.Application Shop.Infrastructure Shop.Web; do
  cp "$here/src/$project/bin/Release/net10.0/$project.dll" "$here/src/$project/bin/Release/net10.0/$project.pdb" "$here/built/"
  rm -rf "$here/src/$project/bin" "$here/src/$project/obj"
done
(cd "$here/built" && shasum -a 256 Shop.*.dll Shop.*.pdb)
echo "built with .NET SDK $(dotnet --version)"
