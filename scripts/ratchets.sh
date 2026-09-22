#!/usr/bin/env bash
# The conformance ratchets: compared with the base branch, conformance/excluded.json may not grow,
# conformance/archunitnet/ported.json's `ported` may not fall, and the layer 1 threshold in
# conformance/dependency-cruiser/threshold.json may not fall.
#
# Plan: docs/plans/pending/0000-wave-0-spike.md, Step 4 (the `ratchets` job).
# Decision: docs/adr/0009-conformance-suites-as-specification.md.
# Usage: scripts/ratchets.sh [base-ref]   (default: origin/$GITHUB_BASE_REF, else origin/main)
#
# Each file records the upstream `pin` it was measured against. The ratchet compares counts only
# when the base was recorded at the same pin; a base with no pin (the wave 0 skeleton) or another
# pin (a reviewed pin bump, which re-vendors and re-records) sets a new starting count, and the job
# says so in its log.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
base="${1:-origin/${GITHUB_BASE_REF:-main}}"

excluded_now="$(python3 -c 'import json; print(len(json.load(open("conformance/excluded.json"))["dependency-cruiser"]))')"
ported_now="$(python3 -c 'import json; print(json.load(open("conformance/archunitnet/ported.json"))["ported"])')"

# Prints the base's value when the base file exists and was recorded at the same pin, else nothing.
base_value() { # file, python expression over `d`
  local text
  text="$(git show "$base:$1" 2>/dev/null)" || return 0
  python3 -c '
import json, sys
base = json.loads(sys.stdin.read())
now = json.load(open(sys.argv[1]))
if base.get("pin") and base.get("pin") == now.get("pin"):
    d = base
    print(eval(sys.argv[2]))
' "$1" "$2" <<<"$text"
}

excluded_was="$(base_value conformance/excluded.json 'len(d["dependency-cruiser"])')"
if [ -z "$excluded_was" ]; then
  excluded_was="$excluded_now"
  echo "ratchets: $base has no excluded.json recorded at this pin; this change sets the starting count ($excluded_now)"
fi
ported_was="$(base_value conformance/archunitnet/ported.json 'd["ported"]')"
if [ -z "$ported_was" ]; then
  ported_was="$ported_now"
  echo "ratchets: $base has no ported.json recorded at this pin; this change sets the starting count ($ported_now)"
fi

threshold_now="$(python3 -c 'import json; print(json.load(open("conformance/dependency-cruiser/threshold.json"))["layer1"])')"
threshold_was="$(git show "$base:conformance/dependency-cruiser/threshold.json" 2>/dev/null | python3 -c 'import json, sys; print(json.load(sys.stdin)["layer1"])' 2>/dev/null || echo "$threshold_now")"
if python3 -c "import sys; sys.exit(0 if float('$threshold_now') < float('$threshold_was') else 1)"; then
  echo "::error::the layer 1 threshold fell from $threshold_was to $threshold_now; it may only rise (ADR-0009)"
  threshold_fell=1
fi
echo "ratchets: layer 1 threshold base=$threshold_was now=$threshold_now"
echo "ratchets: excluded.json base=$excluded_was now=$excluded_now; ported base=$ported_was now=$ported_now"
status="${threshold_fell:-0}"
if [ "$excluded_now" -gt "$excluded_was" ]; then
  echo "::error::conformance/excluded.json grew from $excluded_was to $excluded_now; it may only shrink (ADR-0009)"
  status=1
fi
if [ "$ported_now" -lt "$ported_was" ]; then
  echo "::error::ported fell from $ported_was to $ported_now; it may only rise (ADR-0009)"
  status=1
fi
exit "$status"
