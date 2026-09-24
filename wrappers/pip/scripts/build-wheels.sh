#!/usr/bin/env bash
# Builds the PyPI packages of one release from the archives release.yml builds: one `rulebearing`
# wheel per release target (wrappers/pip/hatch_build.py bundles the binary and tags the wheel) and
# the `pytest-rulebearing` wheel, all at the release version.
# Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 14 (2H).
# Procedure: docs/release.md. Decision: docs/adr/0020-single-name-across-registries.md.
#
# Usage: wrappers/pip/scripts/build-wheels.sh <dist-dir> <version> <out-dir> [--partial]
#   <dist-dir>  holds rulebearing-<target>.tar.gz for the six targets
#   --partial   skip absent archives instead of failing (a local check on one host)
# Needs python with build, hatchling and packaging (PYTHON names the interpreter), and readelf
# for the two *-linux-gnu targets, whose manylinux tag is the newest glibc symbol the binary needs.
set -euo pipefail

usage="usage: wrappers/pip/scripts/build-wheels.sh <dist-dir> <version> <out-dir> [--partial]"
[ "$#" -ge 3 ] && [ "$#" -le 4 ] || { echo "$usage" >&2; exit 2; }
dist="$1"
version="$2"
out="$3"
partial=false
if [ "$#" -eq 4 ]; then
  [ "$4" = "--partial" ] || { echo "$usage" >&2; exit 2; }
  partial=true
fi
python="${PYTHON:-python3}"
here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
repository="$(cd "$here/../.." && pwd -P)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$out"

# The release targets of docs/architecture.md#distribution, as release.yml names its archives.
targets=(
  x86_64-unknown-linux-gnu
  x86_64-unknown-linux-musl
  aarch64-unknown-linux-gnu
  aarch64-apple-darwin
  x86_64-apple-darwin
  x86_64-pc-windows-msvc
)

built=0
for target in "${targets[@]}"; do
  archive="$dist/rulebearing-$target.tar.gz"
  if [ ! -f "$archive" ]; then
    if $partial; then
      echo "build-wheels: skipping $target, no $archive" >&2
      continue
    fi
    echo "build-wheels: missing $archive" >&2
    exit 1
  fi
  mkdir -p "$work/$target"
  tar -xzf "$archive" -C "$work/$target"
  binary="$work/$target/rulebearing"
  case "$target" in *-windows-msvc) binary="$binary.exe" ;; esac
  [ -f "$binary" ] || { echo "build-wheels: $archive does not contain $(basename "$binary")" >&2; exit 1; }
  glibc=""
  case "$target" in
    *-linux-gnu)
      glibc="$(readelf --version-info --wide "$binary" | grep -o 'GLIBC_2\.[0-9]*' | sed 's/^GLIBC_//' | sort -t. -k2,2n | tail -n 1)"
      [ -n "$glibc" ] || { echo "build-wheels: no GLIBC_2.N symbol version in $binary" >&2; exit 1; }
      ;;
  esac
  echo "build-wheels: $target${glibc:+ (glibc $glibc)}" >&2
  RULEBEARING_VERSION="$version" \
    RULEBEARING_WHEEL_BINARY="$binary" \
    RULEBEARING_WHEEL_TARGET="$target" \
    RULEBEARING_WHEEL_GLIBC="$glibc" \
    "$python" -m build --wheel --no-isolation --outdir "$out" "$here" >&2
  built=$((built + 1))
done
[ "$built" -gt 0 ] || { echo "build-wheels: no rulebearing-<target>.tar.gz archives in $dist" >&2; exit 1; }

RULEBEARING_VERSION="$version" \
  "$python" -m build --wheel --no-isolation --outdir "$out" "$repository/adapters/python/pytest-rulebearing" >&2
ls -1 "$out"
