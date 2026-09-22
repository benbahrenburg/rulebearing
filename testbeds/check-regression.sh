#!/usr/bin/env bash
# Fails when Rulebearing's wall-clock time on any test bed regressed by more than the threshold
# against the previous nightly summary (design § Test beds, item 3).
#
# Plan: docs/plans/pending/0000-wave-0-spike.md, Step 7 item 3. Requirement: docs/prd.md#nfr-conf-03.
# Usage: testbeds/check-regression.sh <previous summary.json> <current summary.json> [threshold-percent]
#
# Only rows with a Rulebearing timing in both summaries are compared, so until wave 1 records one
# the check reports that there is nothing to compare and passes. A missing previous summary (the
# first night) passes the same way.
set -euo pipefail
previous="${1:?usage: check-regression.sh <previous summary.json> <current summary.json> [threshold]}"
current="${2:?usage: check-regression.sh <previous summary.json> <current summary.json> [threshold]}"
threshold="${3:-20}"
if [ ! -f "$previous" ]; then
  echo "regression: no previous summary; nothing to compare"
  exit 0
fi
python3 - "$previous" "$current" "$threshold" <<'PY'
import json, sys
previous, current, threshold = sys.argv[1], sys.argv[2], float(sys.argv[3])
def timings(path):
    rows = json.load(open(path))
    return {r["repo"]: r["rulebearing"]["wall_seconds"] for r in rows
            if (r.get("rulebearing") or {}).get("wall_seconds")}
before, after = timings(previous), timings(current)
common = sorted(set(before) & set(after))
if not common:
    print("regression: no row has a Rulebearing timing in both summaries; nothing to compare")
    sys.exit(0)
worse = [(repo, before[repo], after[repo]) for repo in common
         if after[repo] > before[repo] * (1 + threshold / 100)]
for repo, was, now in worse:
    print(f"::error::{repo}: Rulebearing took {now} s, was {was} s (over {threshold:g}% slower)")
print(f"regression: compared {len(common)} rows, {len(worse)} regressed")
sys.exit(1 if worse else 0)
PY
