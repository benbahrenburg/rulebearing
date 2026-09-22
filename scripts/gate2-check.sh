#!/usr/bin/env bash
# Conformance gate 2 in wave 0: the committed TestAssembly fixture is present, matches its hashes,
# carries a portable PDB and the Apache-2.0 notice, and ported.json is valid.
#
# Plan: docs/plans/pending/0000-wave-0-spike.md, Step 6 item 3 (the `conformance-gate-2` job).
# Decisions: docs/adr/0009-conformance-suites-as-specification.md, docs/adr/0019-mit-licence.md.
# The reader's own end-to-end test over the fixture is crates/rb-extract-dotnet/tests/test_assembly.rs.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)/conformance/archunitnet"

fail() { echo "gate2: $*" >&2; exit 1; }

for file in fixtures/TestAssembly.dll fixtures/TestAssembly.pdb fixtures/LICENSE fixtures/NOTICE fixtures/SHA256SUMS ported.json PIN; do
  [ -f "$file" ] || fail "missing $file; run conformance/archunitnet/scripts/build-test-assembly.sh"
done
(cd fixtures && shasum -a 256 --check --status SHA256SUMS) || fail "fixture bytes do not match fixtures/SHA256SUMS"
for file in TestAssembly.dll TestAssembly.pdb; do
  hash="$(grep " $file\$" fixtures/SHA256SUMS | cut -d' ' -f1)"
  grep -q "$hash" fixtures/README.md || fail "fixtures/README.md does not record the hash of $file"
done
[ "$(head -c 4 fixtures/TestAssembly.pdb)" = "BSJB" ] || fail "TestAssembly.pdb is not a portable PDB"
grep -q "Apache" fixtures/NOTICE fixtures/LICENSE || fail "the Apache-2.0 notice is missing"

python3 - <<'PY' || fail "ported.json is invalid"
import json, sys
data = json.load(open("ported.json"))
for key in ("total", "ported", "customPredicate"):
    value = data.get(key)
    if not isinstance(value, int) or value < 0:
        sys.exit(f"{key} must be a non-negative integer, got {value!r}")
if data["total"] and data["ported"] > data["total"]:
    sys.exit("ported exceeds total")
if data.get("pin") != open("PIN").read().strip():
    sys.exit("pin does not match PIN")
print(f"gate2: TestAssembly {data['pin']} verified; ported={data['ported']} total={data['total']}")
PY
