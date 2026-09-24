#!/usr/bin/env bash
# Conformance gate 2, NetArchTest half: every committed fixture assembly is present, matches its
# hash, carries a portable PDB and NetArchTest's MIT licence; ported.json and unported.json are
# valid, and ported.json counts the cases in ported/*.yaml (all three written by
# conformance/netarchtest/tools/Port). scripts/gate2-check.sh runs it after the ArchUnitNET half.
#
# Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 7 (NetArchTest's
# `Types.InAssembly(...)` tests mapped through the element rules).
# Decisions: docs/adr/0009-conformance-suites-as-specification.md, docs/adr/0019-mit-licence.md.
# The cases themselves run in crates/rb-rules/tests/gate2_netarchtest.rs.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)/conformance/netarchtest"

fail() { echo "gate2-netarchtest: $*" >&2; exit 1; }

for file in fixtures/LICENSE fixtures/SHA256SUMS fixtures/README.md ported.json unported.json PIN; do
  [ -f "$file" ] || fail "missing $file; run conformance/netarchtest/scripts/build-test-assemblies.sh"
done
(cd fixtures && shasum -a 256 --check --status SHA256SUMS) || fail "fixture bytes do not match fixtures/SHA256SUMS"
while read -r hash file; do
  grep -q "$hash" fixtures/README.md || fail "fixtures/README.md does not record the hash of $file"
  case "$file" in
    *.pdb) [ "$(head -c 4 "fixtures/$file")" = "BSJB" ] || fail "$file is not a portable PDB" ;;
  esac
done < fixtures/SHA256SUMS
for name in NetArchTest.TestStructure NetArchTest.CrossAssemblyTest.A NetArchTest.CrossAssemblyTest.B; do
  grep -q "$name.dll" fixtures/SHA256SUMS || fail "$name.dll is not among the fixtures"
done
grep -q "MIT License" fixtures/LICENSE || fail "NetArchTest's MIT licence is missing"

python3 - <<'PY' || fail "ported.json, unported.json or ported/*.yaml is invalid; rerun conformance/netarchtest/tools/Port"
import glob, json, re, sys
pin = open("PIN").read().strip()
data = json.load(open("ported.json"))
for key in ("total", "ported", "customPredicate"):
    value = data.get(key)
    if not isinstance(value, int) or value < 0:
        sys.exit(f"{key} must be a non-negative integer, got {value!r}")
if data.get("pin") != pin:
    sys.exit("ported.json: pin does not match PIN")

# unported.json: { $comment, pin, entries: [{ source, test, block, query, reason }] }.
unported = json.load(open("unported.json"))
if set(unported) != {"$comment", "pin", "entries"} or not isinstance(unported["entries"], list):
    sys.exit("unported.json must be { $comment, pin, entries: [...] }")
if unported["pin"] != pin:
    sys.exit("unported.json: pin does not match PIN")
# No `not-yet`: every NetArchTest test is either ported or unported for a stated reason.
reason = re.compile(r"^(custom-predicate|[a-z][a-z-]*: \S.*)$")
for index, entry in enumerate(unported["entries"]):
    where = f"unported.json entries[{index}]"
    if not isinstance(entry, dict) or set(entry) != {"source", "test", "block", "query", "reason"}:
        sys.exit(f"{where} must have exactly source, test, block, query, reason")
    if not (isinstance(entry["source"], str) and entry["source"].startswith("test/NetArchTest.Rules.UnitTests/")):
        sys.exit(f"{where}: source must be a test/NetArchTest.Rules.UnitTests/ path")
    if not (isinstance(entry["test"], str) and entry["test"]):
        sys.exit(f"{where}: test must name the upstream test")
    block, query = entry["block"], entry["query"]
    if block is None:
        if query is not None:
            sys.exit(f"{where}: a whole-test entry (block null) has no query")
    elif not (isinstance(block, int) and not isinstance(block, bool) and block >= 1 and isinstance(query, str)):
        sys.exit(f"{where}: block must be a 1-based integer with its query, or null")
    if not (isinstance(entry["reason"], str) and reason.match(entry["reason"])) or entry["reason"].startswith("not-yet"):
        sys.exit(f"{where}: reason must be custom-predicate or <kind>: <why>, never not-yet")
custom = sum(1 for e in unported["entries"] if e["reason"] == "custom-predicate")
if data["customPredicate"] != custom:
    sys.exit(f"customPredicate is {data['customPredicate']}, but unported.json has {custom} custom-predicate entries")

# ported/*.yaml: one case per `- id:` line (the tool's fixed layout), each file with its header.
cases = 0
for path in sorted(glob.glob("ported/*.yaml")):
    text = open(path, encoding="utf-8").read()
    if not text.startswith(f"# Ported from NetArchTest {pin} "):
        sys.exit(f"{path} does not carry the Port tool's header for {pin}")
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
            if not {"id", "csharp", "rule", "expect"} <= set(case):
                sys.exit(f"{path}: case {case.get('id')!r} lacks id, csharp, rule or expect")
    cases += count
if data["ported"] != cases:
    sys.exit(f"ported is {data['ported']}, but ported/*.yaml holds {cases} cases")
if data["total"] != cases + len(unported["entries"]):
    sys.exit(f"total is {data['total']}, but {cases} cases and {len(unported['entries'])} unported entries make {cases + len(unported['entries'])}")
print(f"gate2-netarchtest: NetArchTest {pin} fixtures verified; ported={data['ported']} total={data['total']} unported={len(unported['entries'])} customPredicate={custom}")
PY
