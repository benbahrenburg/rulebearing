#!/usr/bin/env bash
# The dotnet tool smoke test: packs `Rulebearing` with a locally built binary for this machine,
# installs it into an empty tool path from that package alone, and asserts that `rulebearing
# --version` prints `rulebearing <version>` (docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md,
# Step 14: "A smoke job installs each wrapper from the built package ... and runs
# `rulebearing --version`"). Installs nothing outside a temporary directory.
#
# Usage: wrappers/nuget/smoke.sh [--package <Rulebearing.X.Y.Z.nupkg>]
#   RULEBEARING_BINARY  the binary to pack; default target/release/rulebearing, built when absent.
#   --package           install this package (for example one release.yml packed) instead of
#                       packing one here.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
root="$(cd "$here/../.." && pwd -P)"
package=""
[ "${1:-}" = "--package" ] && package="${2:?--package needs a path}"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
# A private package cache, so no earlier install of the same version is reused.
export NUGET_PACKAGES="$work/packages"

version="$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"$/\1/p' "$root/Cargo.toml")"
if [ -z "$package" ]; then
  case "$(uname -s)-$(uname -m)" in
    Darwin-arm64) rid=osx-arm64 ;;
    Darwin-x86_64) rid=osx-x64 ;;
    Linux-x86_64)
      rid=linux-x64
      if ldd --version 2>&1 | grep -qi musl; then rid=linux-musl-x64; fi
      ;;
    Linux-aarch64 | Linux-arm64) rid=linux-arm64 ;;
    MINGW*-x86_64 | MSYS*-x86_64 | CYGWIN*-x86_64) rid=win-x64 ;;
    *) echo "smoke.sh: no release target for $(uname -s) $(uname -m)" >&2; exit 2 ;;
  esac
  binary="${RULEBEARING_BINARY:-$root/target/release/rulebearing}"
  case "$rid" in win-*) [ -f "$binary" ] || binary="$binary.exe" ;; esac
  if [ ! -f "$binary" ]; then
    echo "== building $binary"
    cargo build --release --locked -p rb-cli --manifest-path "$root/Cargo.toml"
  fi
  echo "== packing Rulebearing $version for $rid from $binary"
  package="$("$here/pack.sh" --binary "$binary" --rid "$rid" --version "$version" --out "$work/feed")"
else
  version="$(basename "$package" .nupkg)"
  version="${version#Rulebearing.}"
  mkdir -p "$work/feed"
  cp "$package" "$work/feed/"
fi

echo "== installing $(basename "$package") into an empty tool path"
dotnet tool install Rulebearing --version "$version" --tool-path "$work/tools" \
  --add-source "$work/feed" --ignore-failed-sources
printed="$("$work/tools/rulebearing" --version)"
echo "rulebearing --version: $printed"
if [ "$printed" != "rulebearing $version" ]; then
  echo "smoke.sh: expected \"rulebearing $version\", got \"$printed\"" >&2
  exit 1
fi
# The exit code passes through: an unknown subcommand is the binary's own usage error.
set +e
"$work/tools/rulebearing" no-such-subcommand >/dev/null 2>&1
code=$?
set -e
if [ "$code" -eq 0 ]; then
  echo "smoke.sh: an unknown subcommand exited 0 through the launcher" >&2
  exit 1
fi
echo "nuget smoke: dotnet tool install, then rulebearing --version, printed rulebearing $version (exit codes pass through: $code)"
