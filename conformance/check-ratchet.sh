#!/usr/bin/env bash
# Fails when excluded.json grows or the ArchUnitNET unported count rises, compared with the base branch.
# ADR-0009. Usage: conformance/check-ratchet.sh <dependency-cruiser|archunitnet>
set -euo pipefail
gate="$1"
base="${GITHUB_BASE_REF:-main}"
case "$gate" in
  dependency-cruiser)
    now="$(python3 -c 'import json; print(len(json.load(open("conformance/excluded.json"))["dependency-cruiser"]))')"
    was="$(git show "origin/$base:conformance/excluded.json" 2>/dev/null | python3 -c 'import json,sys; print(len(json.load(sys.stdin)["dependency-cruiser"]))' || echo "$now")"
    ;;
  archunitnet)
    now="$(wc -l < conformance/archunitnet/ported/unported.txt 2>/dev/null || echo 0)"
    was="$(git show "origin/$base:conformance/archunitnet/ported/unported.txt" 2>/dev/null | wc -l || echo "$now")"
    ;;
  *) echo "unknown gate $gate"; exit 2;;
esac
echo "$gate ratchet: base=$was now=$now"
[ "$now" -le "$was" ] || { echo "::error::$gate ratchet grew from $was to $now"; exit 1; }
