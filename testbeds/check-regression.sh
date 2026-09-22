#!/usr/bin/env bash
# Fails on a wall-clock or peak-memory regression over the threshold; implemented in wave 0B.
set -euo pipefail
echo "regression check skeleton (threshold ${2:-20}%)"
