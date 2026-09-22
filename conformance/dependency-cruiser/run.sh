#!/usr/bin/env bash
# Conformance gate 1. Wave 0 sub-wave 0B installs the pinned upstream and wires the five layers;
# until then this script reports the skeleton state and succeeds so CI shape is visible.
# See conformance/README.md and docs/plans/pending/0000-wave-0-spike.md.
set -euo pipefail
echo "gate 1 skeleton: dependency-cruiser@18.2.0 layers 1-5 are wired in wave 0B"
