#!/usr/bin/env bash
# Conformance gate 2: every committed fixture assembly is present, matches its hash, carries a
# portable PDB and the Apache-2.0 notice; ported.json and unported.json are valid, and ported.json
# counts the cases in ported/*.yaml (all three written by conformance/archunitnet/tools/port.py).
#
# Plans: docs/plans/pending/0000-wave-0-spike.md, Step 6 item 3 (the `conformance-gate-2` job);
# docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 7 (the TestAssemblies and
# the ported cases).
# Decisions: docs/adr/0009-conformance-suites-as-specification.md, docs/adr/0019-mit-licence.md.
# The reader's own end-to-end test over the fixture is crates/rb-extract-dotnet/tests/test_assembly.rs.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)/conformance/archunitnet"

fail() { echo "gate2: $*" >&2; exit 1; }

for file in fixtures/LICENSE fixtures/NOTICE fixtures/SHA256SUMS ported.json unported.json PIN; do
  [ -f "$file" ] || fail "missing $file; run conformance/archunitnet/scripts/build-test-assembly.sh"
done
(cd fixtures && shasum -a 256 --check --status SHA256SUMS) || fail "fixture bytes do not match fixtures/SHA256SUMS"
while read -r hash file; do
  grep -q "$hash" fixtures/README.md || fail "fixtures/README.md does not record the hash of $file"
  case "$file" in
    *.pdb) [ "$(head -c 4 "fixtures/$file")" = "BSJB" ] || fail "$file is not a portable PDB" ;;
  esac
done < fixtures/SHA256SUMS
grep -q "TestAssembly.dll" fixtures/SHA256SUMS || fail "TestAssembly.dll is not among the fixtures"

grep -q "Apache" fixtures/NOTICE fixtures/LICENSE || fail "the Apache-2.0 notice is missing"

python3 - <<'PY' || fail "ported.json, unported.json or ported/*.yaml is invalid; rerun conformance/archunitnet/tools/port.py"
import glob, json, re, sys
pin = open("PIN").read().strip()
data = json.load(open("ported.json"))
for key in ("total", "ported", "customPredicate"):
    value = data.get(key)
    if not isinstance(value, int) or value < 0:
        sys.exit(f"{key} must be a non-negative integer, got {value!r}")
if data["total"] and data["ported"] > data["total"]:
    sys.exit("ported exceeds total")
if data.get("pin") != pin:
    sys.exit("ported.json: pin does not match PIN")

# unported.json: { $comment, pin, entries: [{ source, test, block, query, reason }] }.
unported = json.load(open("unported.json"))
if set(unported) != {"$comment", "pin", "entries"} or not isinstance(unported["entries"], list):
    sys.exit("unported.json must be { $comment, pin, entries: [...] }")
if unported["pin"] != pin:
    sys.exit("unported.json: pin does not match PIN")
reason = re.compile(r"^(custom-predicate|not-yet: \S.*|[a-z][a-z-]*(: \S.*)?)$")
for index, entry in enumerate(unported["entries"]):
    where = f"unported.json entries[{index}]"
    if not isinstance(entry, dict) or set(entry) != {"source", "test", "block", "query", "reason"}:
        sys.exit(f"{where} must have exactly source, test, block, query, reason")
    if not (isinstance(entry["source"], str) and entry["source"].startswith("ArchUnitNETTests/")):
        sys.exit(f"{where}: source must be an ArchUnitNETTests/ path")
    if not (isinstance(entry["test"], str) and entry["test"]):
        sys.exit(f"{where}: test must name the upstream test")
    block, query = entry["block"], entry["query"]
    if block is None:
        if query is not None:
            sys.exit(f"{where}: a whole-test entry (block null) has no query")
    elif not (isinstance(block, int) and not isinstance(block, bool) and block >= 1 and isinstance(query, str)):
        sys.exit(f"{where}: block must be a 1-based integer with its query, or null")
    if not (isinstance(entry["reason"], str) and reason.match(entry["reason"])):
        sys.exit(f"{where}: reason must be custom-predicate, not-yet: <what>, or a short reason")
custom = sum(1 for e in unported["entries"] if e["reason"] == "custom-predicate")
if data["customPredicate"] != custom:
    sys.exit(f"customPredicate is {data['customPredicate']}, but unported.json has {custom} custom-predicate entries")

# ported/*.yaml: one case per `- id:` line (the tool's fixed layout), each file with its header.
cases = 0
for path in sorted(glob.glob("ported/*.yaml")):
    text = open(path, encoding="utf-8").read()
    if not text.startswith(f"# Ported from ArchUnitNET {pin} "):
        sys.exit(f"{path} does not carry the port.py header for {pin}")
    count = len(re.findall(r"^- id: \S", text, re.MULTILINE))
    try:
        import yaml
    except ImportError:
        pass
    else:
        doc = yaml.safe_load(text)
        if not isinstance(doc, dict) or not isinstance(doc.get("cases"), list) or len(doc["cases"]) != count:
            sys.exit(f"{path}: cases do not parse as a list of {count}")
        for case in doc["cases"]:
            if not {"id", "query", "csharp", "rule", "expect"} <= set(case):
                sys.exit(f"{path}: case {case.get('id')!r} lacks id, query, csharp, rule or expect")
    cases += count
if data["ported"] != cases:
    sys.exit(f"ported is {data['ported']}, but ported/*.yaml holds {cases} cases")
if data["total"] != cases + len(unported["entries"]):
    sys.exit(f"total is {data['total']}, but {cases} cases and {len(unported['entries'])} unported entries make {cases + len(unported['entries'])}")
print(f"gate2: TestAssembly {data['pin']} verified; ported={data['ported']} total={data['total']} unported={len(unported['entries'])} customPredicate={custom}")
PY
