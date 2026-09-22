#!/usr/bin/env bash
# Vendors dependency-cruiser's test inputs at the version in ../PIN.
#
# Plan: docs/plans/pending/0000-wave-0-spike.md, Step 5 item 1 (and Step 3 item 7 for the report
# fixtures the rb-model round-trip test reads). Decision: docs/adr/0009-conformance-suites-as-specification.md.
# Licence: dependency-cruiser is MIT; its LICENSE is copied beside the fixtures (docs/adr/0019-mit-licence.md).
#
# Writes, under conformance/dependency-cruiser/:
#   LICENSE                  upstream licence
#   fixtures/report/         test/report verbatim (inputs and expected reporter output, wave 1)
#   fixtures/report-json/    every cruise-result mock in test/report, as JSON, with INDEX.json
#                            sorting them by whether the pinned schema accepts them (rb-model round trip)
#   fixtures/schemas/        the pinned cruise-result and configuration schemas (layer 4, --strict-schema)
#   fixtures/extract/        test/extract inputs plus the recorded layer 1 expectations (INDEX.json)
#
# Needs git and Node 22 or later; runs `npm ci` inside the clone (never in this repository).
# Re-run after bumping PIN, commit the diff, and expect the layer 1 ratio to move.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
pin="$(tr -d '[:space:]' < "$here/PIN")"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# The clone lives in a directory named dependency-cruiser because some upstream fixtures address
# files as ../dependency-cruiser/test/...
upstream="$work/dependency-cruiser"
echo "vendor: cloning dependency-cruiser v$pin"
git -c advice.detachedHead=false clone --quiet --depth 1 --branch "v$pin" \
  https://github.com/sverweij/dependency-cruiser.git "$upstream"

echo "vendor: installing the checkout's dependencies (ajv validates the report mocks)"
(cd "$upstream" && npm ci --no-audit --no-fund --ignore-scripts --silent)

rm -rf "$here/fixtures/report" "$here/fixtures/report-json" "$here/fixtures/schemas"
mkdir -p "$here/fixtures/report" "$here/fixtures/report-json" "$here/fixtures/schemas"
cp "$upstream/LICENSE" "$here/LICENSE"
cp -R "$upstream/test/report/." "$here/fixtures/report/"
cp "$upstream/src/schema/cruise-result.schema.json" "$upstream/src/schema/configuration.schema.json" \
  "$here/fixtures/schemas/"
node "$here/harness/export-report-mocks.mjs" "$upstream" "$here/fixtures/report-json"

if [ -f "$here/harness/export-expectations.mjs" ]; then
  echo "vendor: recording layer 1 expectations"
  rm -rf "$here/fixtures/extract"
  mkdir -p "$here/fixtures/extract"
  node "$here/harness/export-expectations.mjs" "$upstream" "$here/fixtures/extract"
fi

echo "vendor: done; review the diff under conformance/dependency-cruiser/ and commit it"
