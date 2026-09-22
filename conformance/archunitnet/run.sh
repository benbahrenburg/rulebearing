#!/usr/bin/env bash
# Conformance gate 2. Wave 0 sub-wave 0B builds TestAssembly with a portable PDB and commits it;
# wave 2C ports the fluent tests. Until then this script reports the skeleton state and succeeds.
# See conformance/README.md and docs/plans/pending/0000-wave-0-spike.md.
set -euo pipefail
echo "gate 2 skeleton: ArchUnitNET 0.13.4 TestAssembly and ported cases are wired in wave 0B and 2C"
