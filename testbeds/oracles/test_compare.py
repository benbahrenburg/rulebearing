# Copyright (c) 2026 Ben Bahrenburg. MIT licence: see LICENSE.
"""The oracle harness's join (compare.py) and table (table.py) on small, hand-written inputs.

Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 11 (the oracle
harness). Requirement: docs/prd.md#nfr-conf-03. Run with ``pytest testbeds/oracles``.
"""

from __future__ import annotations

import argparse
import json
from typing import TYPE_CHECKING, Any

import compare
import table

if TYPE_CHECKING:
    from pathlib import Path


def junit(tmp_path: Path, cases: str) -> compare.JUnit:
    """A JUnit report holding ``cases``, parsed."""
    path = tmp_path / "rulebearing.xml"
    path.write_text(
        f'<?xml version="1.0"?>\n<testsuites><testsuite name="rulebearing">{cases}'
        "</testsuite></testsuites>\n",
        encoding="utf-8",
    )
    return compare.JUnit(path)


def contract(name: str, verdict: str) -> dict[str, Any]:
    """An import-linter contract as lint_imports_json.py writes it."""
    return {"id": name.lower(), "name": name, "type": "protected", "importLinter": verdict}


IMPORTED: dict[str, Any] = {
    "rules": {
        "dependencies": {
            "forbidden": [
                {"name": "q-rule", "comment": "import-linter contract: Q"},
            ],
            "allowed": [
                {
                    "name": "p-allowed",
                    "comment": "import-linter contract: P",
                    "from": {"path": "^pkg/"},
                    "to": {"path": "^pkg/b\\.py$"},
                },
            ],
        },
    },
}


def test_a_violation_line_drops_the_position_and_the_known_marker(tmp_path: Path) -> None:
    report = junit(
        tmp_path,
        '<testcase name="not-in-allowed"><failure type="error" message="m">'
        "RB-1a2b3c4d pkg/a.py -&gt; pkg/b.py (line 3, column 1)\n"
        "pkg/c.py -&gt; pkg/b.py [known]\n"
        "pkg/d.py -&gt; pkg/b.py (line 9, column 5) [known]\n"
        "pkg/e.py -&gt; pkg/b.py"
        "</failure></testcase>",
    )
    assert report.violations == [
        ("not-in-allowed", "pkg/a.py", "pkg/b.py"),
        ("not-in-allowed", "pkg/c.py", "pkg/b.py"),
        ("not-in-allowed", "pkg/d.py", "pkg/b.py"),
        ("not-in-allowed", "pkg/e.py", "pkg/b.py"),
    ]


def test_a_protected_contract_is_credited_with_its_violation(tmp_path: Path) -> None:
    report = junit(
        tmp_path,
        '<testcase name="not-in-allowed"><failure type="error" message="m">'
        "RB-1 pkg/a.py -&gt; pkg/b.py (line 3, column 1)</failure></testcase>",
    )
    context = {"attribution": compare.Attribution(IMPORTED), "junit": report}
    rows = compare.python_rows(
        {"contracts": [contract("P", "broken")]},
        "",
        context,
    )
    assert rows[0]["violations"] == 1
    assert rows[0]["rulebearing"] == "broken"
    assert rows[0]["verdict"] == "agree"
    assert len(rows) == 1, "no stray row of violations no contract names"


def test_a_repeated_rule_name_still_belongs_to_its_contract() -> None:
    attribution = compare.Attribution(IMPORTED)
    assert attribution.rule("q-rule#2") == "q-rule"
    assert attribution.rule("not-in-allowed#2") == "not-in-allowed"
    assert attribution.rule("other#2") == "other#2"
    assert attribution.rule("q-rule") == "q-rule"


def test_a_rulebearing_error_is_the_verdict_error_not_kept(tmp_path: Path) -> None:
    report = junit(
        tmp_path,
        '<testcase name="q-rule"><error type="vacuous" message="m">m</error></testcase>',
    )
    context = {"attribution": compare.Attribution(IMPORTED), "junit": report}
    rows = compare.python_rows({"contracts": [contract("Q", "kept")]}, "", context)
    assert rows[0]["rulebearing"] == "error"
    assert rows[0]["verdict"] == "error"
    assert rows[0]["reason"] == "rulebearing could not check q-rule (vacuous)"
    assert rows[0]["vacuousRules"] == ["q-rule"]
    summary = compare.summarise(rows)
    assert compare.outcome(summary) == ("compared", False, compare.DISAGREES)


def test_junit_verdicts_let_an_error_win_over_a_failure(tmp_path: Path) -> None:
    junit(
        tmp_path,
        '<testcase name="both"><failure type="error" message="f">a -&gt; b</failure>'
        '<error type="expired" message="e">e</error></testcase>'
        '<testcase name="fails"><failure type="error" message="f">a -&gt; b</failure></testcase>'
        '<testcase name="passes"/>',
    )
    verdicts = compare.junit_verdicts(tmp_path / "rulebearing.xml")
    assert verdicts == {"both": "error", "fails": "fail", "passes": "pass"}
    test = {"test": "Ns.Arch.Layers", "outcome": "Failed"}
    rules = [{"name": "both", "active": True, "reason": "", "row": None, "file": None}]
    row = compare.dotnet_row(test, rules, verdicts)
    assert row["verdict"] == "error"
    assert row["rulebearing"] == "error"
    assert "both" in row["reason"]
    rules[0]["name"] = "fails"
    assert compare.dotnet_row(test, rules, verdicts)["verdict"] == "agree"


def test_the_outcome_needs_something_compared() -> None:
    def summary(**counts: int) -> dict[str, int]:
        base = {"total": 0, "agree": 0, "disagree": 0, "stays": 0, "not-imported": 0, "error": 0}
        return {**base, **counts}

    assert compare.outcome(summary(total=4, stays=1, **{"not-imported": 3})) == (
        "nothing-compared",
        False,
        compare.NOTHING_COMPARED,
    )
    assert compare.outcome(summary()) == ("nothing-compared", False, compare.NOTHING_COMPARED)
    assert compare.outcome(summary(total=2, agree=1, stays=1)) == ("compared", True, 0)
    assert compare.outcome(summary(total=2, agree=1, disagree=1)) == (
        "compared",
        False,
        compare.DISAGREES,
    )


def test_a_dotnet_row_where_every_test_is_not_imported_compares_nothing(tmp_path: Path) -> None:
    root = tmp_path.resolve()
    tests = root / "tests"
    tests.mkdir()
    (tests / "Arch.cs").write_text("using ArchUnitNET.Fluent;\nclass ArchTests {}\n")
    trx = root / "incumbent.trx"
    trx.write_text(
        f'<TestRun xmlns="{compare.TRX[1:-1]}"><TestDefinitions><UnitTest id="1">'
        '<TestMethod className="Ns.ArchTests" name="Layers" /></UnitTest></TestDefinitions>'
        '<Results><UnitTestResult testId="1" testName="Ns.ArchTests.Layers" outcome="Passed" />'
        "</Results></TestRun>\n",
    )
    imported = root / "imported.yaml"
    imported.write_text("# not imported: a custom predicate\n#  - name: layers\n")
    out = root / "result.json"
    args = argparse.Namespace(
        trx=str(trx),
        imported=str(imported),
        junit=str(root / "absent.xml"),
        tests_dir=str(tests),
        tests_shown="tests",
        cwd=str(root),
        repo="owner/name",
        sha="0" * 40,
        tool="archunitnet",
        out=str(out),
    )
    assert compare.dotnet_main(args) == compare.NOTHING_COMPARED
    document = json.loads(out.read_text())
    assert document["status"] == "nothing-compared"
    assert document["agrees"] is False
    assert document["summary"]["not-imported"] == 1
    assert table.outcome(document, document["results"]) == (
        "nothing compared: every row stays or is not imported"
    )


def test_the_table_never_shows_nothing_compared_as_agreement() -> None:
    legacy = {"status": "compared", "detail": ""}
    rows: list[dict[str, Any]] = [{"verdict": "not-imported"}, {"verdict": "stays"}]
    assert table.outcome(legacy, rows).startswith("nothing compared")
    assert table.outcome(legacy, [{"verdict": "agree"}, *rows]) == "agrees"
    assert table.outcome(legacy, [{"verdict": "error"}]) == "1 error"
    assert table.outcome({"status": "error", "detail": "no build"}, []) == "error: no build"
