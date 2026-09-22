#!/usr/bin/env bash
# Publishes this wrapper at the given tag. Filled in by the plan named in README.md.
set -euo pipefail
echo "publish skeleton for $(basename "$(dirname "$0")") at ${1:-<tag>}"
