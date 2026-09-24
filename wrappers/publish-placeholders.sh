#!/usr/bin/env bash
# Publishes the four 0.0.1 name reservations on one day, in the order docs/release.md gives:
# crates.io, npm, PyPI, NuGet (docs/adr/0020-single-name-across-registries.md).
# Plan: docs/plans/pending/0000-wave-0-spike.md, Step 11.
#
# Usage: wrappers/publish-placeholders.sh [--dry-run]
#   --dry-run  packages everything and runs each registry's own dry run; publishes nothing and
#              needs no credentials.
# A real run needs, from the maintainer's own session (never CI in wave 0):
#   crates.io  `cargo login` (or CARGO_REGISTRY_TOKEN)
#   npm        `npm login`
#   PyPI       TWINE_USERNAME=__token__ and TWINE_PASSWORD=<token>
#   NuGet      NUGET_API_KEY
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
dry=false
[ "${1:-}" = "--dry-run" ] && dry=true
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

echo "== crates.io"
(cd "$here/crates/rulebearing" && if $dry; then cargo publish --dry-run --allow-dirty --target-dir "$work/cargo"; else cargo publish --target-dir "$work/cargo"; fi)

echo "== npm"
(cd "$here/npm" && if $dry; then npm publish --dry-run --access public; else npm publish --access public; fi)
if ! $dry; then
  echo "npm: checking the @rulebearing scope from this signed-in session (ADR-0020)"
  npm org ls rulebearing >/dev/null 2>&1 && echo "npm: @rulebearing exists and is yours" || echo "npm: @rulebearing is not an organisation you hold; create it at https://www.npmjs.com/org/create"
fi

echo "== PyPI"
python3 -m venv "$work/venv"
"$work/venv/bin/pip" install --quiet build twine
"$work/venv/bin/python" -m build --outdir "$work/dist" "$here/pip"
"$work/venv/bin/twine" check "$work/dist/"*
$dry || "$work/venv/bin/twine" upload "$work/dist/"*

echo "== NuGet"
# The 0.0.1 reservation is published (2026-09-22). wrappers/nuget is now the real `Rulebearing`
# dotnet tool, packed from the release archives by wrappers/nuget/pack.sh and published by
# release.yml (docs/release.md), so this script no longer packs it.
echo "NuGet: reserved at 0.0.1; the dotnet tool and the test adapters are released by .github/workflows/release.yml"

echo "placeholders: $($dry && echo 'dry run complete; nothing published' || echo 'published; record the four URLs in the plan')"
