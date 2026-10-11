#!/usr/bin/env bash
# Packs the rule library, rulebearing-rules, for npm, NuGet and PyPI from presets/frameworks/ and
# presets/rulebearing/recommended.yaml, at one version. Nothing in the source tree is edited: the
# npm package is staged in a temporary folder with the version stamped into its copy of
# package.json, and NuGet and PyPI take the version as a property and an environment variable.
# Decision: docs/adr/0071-the-rule-library-ships-from-this-repository.md. Procedure: docs/release.md.
# Plan: docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md, Step 23.
#
# Usage: wrappers/rules/pack.sh <version> <out-dir> [npm] [nuget] [pypi]
#   <version>  the release version; a leading v is removed
#   npm        <out-dir>/rulebearing-rules-<version>.tgz, the presets at the package root
#   nuget      <out-dir>/Rulebearing.Rules.<version>.nupkg, the presets under rulebearing-rules/
#   pypi       the rulebearing_rules wheel and source distribution, the presets in the package
# No registry named packs all three. Needs npm, the .NET SDK, and a python (PYTHON names it) with
# build, hatchling and packaging, for the registries named. Prints each package's path.
set -euo pipefail

usage="usage: wrappers/rules/pack.sh <version> <out-dir> [npm] [nuget] [pypi]"
if [ "$#" -lt 2 ]; then echo "$usage" >&2; exit 2; fi
given="$1"
version="${given#v}"
out="$2"
shift 2
registries=("$@")
if [ "${#registries[@]}" -eq 0 ]; then registries=(npm nuget pypi); fi
for registry in "${registries[@]}"; do
  case "$registry" in
    npm | nuget | pypi) ;;
    *) echo "pack.sh: unknown registry $registry; $usage" >&2; exit 2 ;;
  esac
done
case "$version" in
  [0-9]*.[0-9]*.[0-9]*) ;;
  *) echo "pack.sh: not a release version: $given" >&2; exit 2 ;;
esac

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
root="$(cd "$here/../.." && pwd -P)"
python="${PYTHON:-python3}"
mkdir -p "$out"
out="$(cd "$out" && pwd -P)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# The library: every framework preset and rulebearing:recommended, the files the binary bundles.
shopt -s nullglob
presets=("$root"/presets/frameworks/*.yaml)
shopt -u nullglob
if [ "${#presets[@]}" -eq 0 ]; then
  echo "pack.sh: no presets under $root/presets/frameworks" >&2
  exit 1
fi
presets+=("$root/presets/rulebearing/recommended.yaml")

for registry in "${registries[@]}"; do
  case "$registry" in
    npm)
      stage="$work/npm/rulebearing-rules"
      mkdir -p "$stage"
      cp "${presets[@]}" "$here/package.json" "$here/README.md" "$root/LICENSE" "$stage/"
      (cd "$stage" && npm pkg set "version=$version")
      npm pack "$stage" --pack-destination "$out" --silent >&2
      packages=("$out/rulebearing-rules-$version.tgz")
      ;;
    nuget)
      dotnet pack "$here/Rulebearing.Rules.csproj" --configuration Release --output "$out" \
        --nologo --verbosity quiet -p:Version="$version" >&2
      packages=("$out/Rulebearing.Rules.$version.nupkg")
      ;;
    pypi)
      RULEBEARING_VERSION="$version" "$python" -m build --no-isolation --outdir "$work/pypi" "$here" >&2
      packages=()
      for built in "$work"/pypi/*.whl "$work"/pypi/*.tar.gz; do
        cp "$built" "$out/"
        packages+=("$out/$(basename "$built")")
      done
      ;;
  esac
  for file in "${packages[@]}"; do
    [ -f "$file" ] || { echo "pack.sh: $registry wrote no $file" >&2; exit 1; }
    echo "$file"
  done
done
