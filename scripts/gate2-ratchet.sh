#!/usr/bin/env bash
# The gate 2 ratchet: compared with the base branch, conformance/archunitnet/unported.json may not
# grow, and once the wave 2 plan has moved to docs/plans/implemented/ no entry may still be
# `not-yet`. Every other reason is custom-predicate or one argued in review.
#
# Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 7 (the `gate2-ratchet`
# check). Decision: docs/adr/0009-conformance-suites-as-specification.md.
# Usage: scripts/gate2-ratchet.sh [base-ref]   (default: origin/$GITHUB_BASE_REF, else origin/main)
#
# As in scripts/ratchets.sh, counts are compared only when the base was recorded at the same pin.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
base="${1:-origin/${GITHUB_BASE_REF:-main}}"
file=conformance/archunitnet/unported.json

now="$(python3 -c 'import json, sys; print(len(json.load(open(sys.argv[1]))["entries"]))' "$file")"
was="$(git show "$base:$file" 2>/dev/null | python3 -c '
import json, sys
base = json.loads(sys.stdin.read())
now = json.load(open(sys.argv[1]))
if base.get("pin") == now.get("pin"):
    print(len(base["entries"]))
' "$file" 2>/dev/null || true)"
status=0
if [ -z "$was" ]; then
  echo "gate2-ratchet: $base has no unported.json at this pin; this change sets the starting count ($now)"
elif [ "$now" -gt "$was" ]; then
  echo "::error::$file grew from $was to $now entries; it may only shrink (ADR-0009)"
  status=1
fi
echo "gate2-ratchet: unported base=${was:-none} now=$now"

if compgen -G "docs/plans/implemented/0002-*.md" > /dev/null; then
  left="$(python3 -c '
import json, sys
entries = json.load(open(sys.argv[1]))["entries"]
for e in entries:
    if e["reason"].startswith("not-yet"):
        print(e["source"], e["test"] + ":", e["reason"])
' "$file")"
  if [ -n "$left" ]; then
    echo "::error::the wave 2 plan is implemented but unported.json still has not-yet entries:"
    echo "$left"
    status=1
  fi
fi
exit "$status"
