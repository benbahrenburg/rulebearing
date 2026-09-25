#!/usr/bin/env bash
# Fails when Rulebearing's wall-clock time on any test bed regressed by more than the threshold
# against the build behind the previous committed summary (design § Test beds, item 3).
#
# Plan: docs/plans/pending/0000-wave-0-spike.md, Step 7 item 3. Requirement: docs/prd.md#nfr-conf-03.
# Usage: testbeds/check-regression.sh <summary.json> [threshold-percent]
#
# Each row's `regression` pair was timed in one job, on one runner: this build and the baseline
# (the commit the previous committed summary names), interleaved, each the median of three runs
# (testbeds/run.sh). Two nights' times from two runners are not compared, because hosted runners
# differ by more than the threshold on the same binary. A row without a pair (the first night, a
# baseline that did not build or run) is not compared, and with none the check passes saying so. A
# change under 0.1 s is not counted: most rows take well under a second, where noise exceeds 20%.
set -euo pipefail
summary="${1:?usage: check-regression.sh <summary.json> [threshold]}"
threshold="${2:-20}"
python3 - "$summary" "$threshold" <<'PY'
import json, sys
summary, threshold = sys.argv[1], float(sys.argv[2])
pairs = [(r["repo"], r["regression"]) for r in json.load(open(summary))
         if (r.get("regression") or {}).get("current") and r["regression"].get("baseline")]
if not pairs:
    print("regression: no row has a baseline timed beside this build; nothing to compare")
    sys.exit(0)
worse = [(repo, p) for repo, p in pairs
         if p["current"] > p["baseline"] * (1 + threshold / 100) and p["current"] - p["baseline"] >= 0.1]
for repo, p in worse:
    print(f"::error::{repo}: Rulebearing took {p['current']} s, the baseline "
          f"{p.get('baseline_sha', '')[:12]} took {p['baseline']} s on the same runner "
          f"(over {threshold:g}% slower)")
print(f"regression: compared {len(pairs)} rows, {len(worse)} regressed")
sys.exit(1 if worse else 0)
PY
