#!/usr/bin/env bash
# Stages the rulebearing binaries under runtimes/<rid>/native/ and packs the `Rulebearing` dotnet
# tool at one version (docs/release.md; docs/adr/0020-single-name-across-registries.md).
# Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 14 (2H).
#
# Usage:
#   wrappers/nuget/pack.sh <dist-dir> <version> <out-dir>
#       the release: the contract release.yml's nuget-pack job calls (docs/release.md). Packs the
#       tool from the six archives rulebearing-<target>.tar.gz, and the seven
#       Rulebearing.TestAdapter packages of adapters/dotnet, all at <version>, into <out-dir>.
#   wrappers/nuget/pack.sh --archives <dist-dir> --version <version> --out <dir> [--partial]
#       the tool alone from the archives; all six targets are required unless --partial is given.
#   wrappers/nuget/pack.sh --binary <path> --rid <rid> --version <version> --out <dir>
#       the tool alone from one local binary, for the smoke test (wrappers/nuget/smoke.sh).
# Prints the path of the tool's .nupkg.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
root="$(cd "$here/../.." && pwd -P)"

# The runtime identifier for each release target: the matrix of .github/workflows/release.yml.
targets=(
  "linux-x64 x86_64-unknown-linux-gnu"
  "linux-musl-x64 x86_64-unknown-linux-musl"
  "linux-arm64 aarch64-unknown-linux-gnu"
  "osx-arm64 aarch64-apple-darwin"
  "osx-x64 x86_64-apple-darwin"
  "win-x64 x86_64-pc-windows-msvc"
)

archives="" binary="" rid="" version="" out="" partial=false adapters=false
if [ $# -eq 3 ] && [ "${1#--}" = "$1" ]; then
  archives="$1" version="$2" out="$3" adapters=true
  set --
fi
while [ $# -gt 0 ]; do
  case "$1" in
    --archives) archives="${2:?}"; shift 2 ;;
    --binary) binary="${2:?}"; shift 2 ;;
    --rid) rid="${2:?}"; shift 2 ;;
    --version) version="${2:?}"; shift 2 ;;
    --out) out="${2:?}"; shift 2 ;;
    --partial) partial=true; shift ;;
    *) echo "pack.sh: unknown argument $1" >&2; exit 2 ;;
  esac
done
[ -n "$version" ] || { echo "pack.sh: --version is required" >&2; exit 2; }
[ -n "$out" ] || { echo "pack.sh: --out is required" >&2; exit 2; }
if [ -z "$archives" ] && [ -z "$binary" ]; then
  echo "pack.sh: give --archives <dist-dir> or --binary <path> --rid <rid>" >&2
  exit 2
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
runtimes="$work/runtimes"
require_all=true

if [ -n "$binary" ]; then
  [ -n "$rid" ] || { echo "pack.sh: --binary needs --rid" >&2; exit 2; }
  name=rulebearing
  case "$rid" in win-*) name=rulebearing.exe ;; esac
  mkdir -p "$runtimes/$rid/native"
  cp "$binary" "$runtimes/$rid/native/$name"
  chmod +x "$runtimes/$rid/native/$name"
  require_all=false
else
  $partial && require_all=false
  for entry in "${targets[@]}"; do
    read -r each target <<<"$entry"
    archive="$archives/rulebearing-$target.tar.gz"
    if [ ! -f "$archive" ]; then
      if $partial; then continue; fi
      echo "pack.sh: $archive is missing; a release packs all six targets (--partial for a local check)" >&2
      exit 1
    fi
    mkdir -p "$work/unpack/$each" "$runtimes/$each/native"
    tar -xzf "$archive" -C "$work/unpack/$each"
    for name in rulebearing rulebearing.exe; do
      if [ -f "$work/unpack/$each/$name" ]; then
        cp "$work/unpack/$each/$name" "$runtimes/$each/native/$name"
        chmod +x "$runtimes/$each/native/$name"
      fi
    done
  done
fi

mkdir -p "$out"
dotnet pack "$here/Rulebearing.csproj" --configuration Release --output "$out" --nologo --verbosity quiet \
  -p:Version="$version" -p:RulebearingRuntimes="$runtimes/" -p:RulebearingRequireAllRuntimes="$require_all" >&2
if $adapters; then
  # The test adapters, at the same version (docs/adr/0020-single-name-across-registries.md).
  for project in Rulebearing.TestAdapter Rulebearing.TestAdapter.xUnit Rulebearing.TestAdapter.xUnitV3 \
    Rulebearing.TestAdapter.NUnit Rulebearing.TestAdapter.MSTestV2 Rulebearing.TestAdapter.MSTestV4 \
    Rulebearing.TestAdapter.TUnit; do
    dotnet pack "$root/adapters/dotnet/$project/$project.csproj" --configuration Release --output "$out" \
      --nologo --verbosity quiet -p:Version="$version" >&2
    [ -f "$out/$project.$version.nupkg" ] || { echo "pack.sh: dotnet pack wrote no $out/$project.$version.nupkg" >&2; exit 1; }
  done
fi
package="$out/Rulebearing.$version.nupkg"
[ -f "$package" ] || { echo "pack.sh: dotnet pack wrote no $package" >&2; exit 1; }
echo "$package"
