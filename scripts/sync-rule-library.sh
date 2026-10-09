#!/usr/bin/env bash
# presets/frameworks/ regenerated from a checkout of the rule library, rulebearing-rules, which is
# where the five framework presets are maintained; and the library's recommended.yaml checked to be
# this repository's presets/rulebearing/recommended.yaml, which it mirrors.
#
# Plan: docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md, Step 23 ("presets/frameworks/
# is regenerated from the library by a script"). Requirement: docs/prd.md#fr-reach-04.
#
# Usage: scripts/sync-rule-library.sh <rulebearing-rules checkout> [--check]
#   --check  change nothing; exit 1 when presets/frameworks/ or the mirror differs from the library
set -euo pipefail
library="${1:?usage: scripts/sync-rule-library.sh <rulebearing-rules checkout> [--check]}"
check="${2:-}"
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
presets=(clean-architecture django fastapi nextjs vertical-slices)
status=0
for preset in "${presets[@]}"; do
  from="$library/$preset.yaml"
  to="$root/presets/frameworks/$preset.yaml"
  if [ ! -f "$from" ]; then
    echo "sync-rule-library: $from is missing; is $library a rulebearing-rules checkout?" >&2
    exit 2
  fi
  if ! cmp -s "$from" "$to"; then
    if [ "$check" = "--check" ]; then
      echo "sync-rule-library: presets/frameworks/$preset.yaml differs from the library's" >&2
      status=1
    else
      cp "$from" "$to"
      echo "sync-rule-library: presets/frameworks/$preset.yaml updated from the library"
    fi
  fi
done
if ! cmp -s "$library/recommended.yaml" "$root/presets/rulebearing/recommended.yaml"; then
  echo "sync-rule-library: the library's recommended.yaml is not presets/rulebearing/recommended.yaml; copy this repository's into the library" >&2
  status=1
fi
exit "$status"
