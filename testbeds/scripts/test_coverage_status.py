# Copyright (c) 2026 Ben Bahrenburg. MIT licence: see LICENSE.
"""The live coverage tables: the status rule, the ledger, the results and determinism.

Plan: docs/plans/pending/0004-wave-4-reach.md, Step 4.

Decision: docs/adr/0074-the-coverage-ledger-is-tracked-and-its-status-is-computed.md.
"""

from __future__ import annotations

import json
from typing import TYPE_CHECKING, Any

import coverage_status as cs
import pytest

if TYPE_CHECKING:
    from pathlib import Path

TAB = """# A coverage tab

Lead text with a | pipe outside a table.

## Rules

| Attribute | Applies to | Rulebearing | Wave |
| --- | --- | --- | --- |
| `forbidden[]` | all | Parity+ | 1 |
| `to.dynamic` | all | Parity+ (.NET: `a | b`) | 1 |

## Stays

- a list, not a table
"""

SELECTORS = """## Selectors (`select.kind`)

| ArchUnitNET | Rulebearing | Languages |
| --- | --- | --- |
| `Types()` | `kind: type` | all |
| (none) | `kind: function` | TS |

## Plantuml

| ArchUnitNET | Rulebearing | Status |
| --- | --- | --- |
| `Lost` | a cell \\`--from a | b |
"""


def write_json(path: Path, document: Any) -> None:  # noqa: ANN401 - JSON
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(document), encoding="utf-8")


def results(folder: Path, name: str, items: dict[str, str], **extra: Any) -> None:  # noqa: ANN401
    document = cs.results_document(name, items)
    document.update(extra)
    write_json(folder / f"{name}.json", document)


def ledger_with(folder: Path, rows: list[dict[str, Any]]) -> Path:
    """A ledger folder whose first tab has one section holding `rows`; the second is empty."""
    ledger = folder / "ledger"
    first, second = cs.TAB_LIST
    section = {"title": "Rules", "headers": ["Attribute", "Rulebearing"], "rows": rows}
    write_json(cs.ledger_path(ledger, first), {"tabSha256": "x", "sections": [section]})
    write_json(cs.ledger_path(ledger, second), {"sections": []})
    return ledger


def test_cells_split_on_pipes_outside_code_spans_only() -> None:
    assert cs.split_cells("| `a | b` | c |") == ["`a | b`", "c"]
    assert cs.split_cells("| a \\`b | c |") == ["a \\`b", "c"]
    assert cs.split_cells("a | b") == ["a", "b"]
    assert cs.is_rule("| --- | :---: |")
    assert not cs.is_rule("| a | --- |")
    assert not cs.is_rule("--- | ---")


def test_a_tab_is_its_sections_tables_and_nothing_else() -> None:
    sections = cs.parse_tab(TAB)
    assert [s["title"] for s in sections] == ["Rules"]
    assert sections[0]["headers"] == ["Attribute", "Applies to", "Rulebearing", "Wave"]
    assert sections[0]["rows"][1] == ["`to.dynamic`", "all", "Parity+ (.NET: `a | b`)", "1"]


def test_the_claim_is_the_status_cell_or_the_tabs_lead() -> None:
    dc, archunit = cs.TAB_LIST
    headers = ["Attribute", "Applies to", "Rulebearing", "Wave"]
    assert cs.claim_of(dc, headers, ["x", "all", "Kept", "1"]) == "Kept"
    assert cs.claim_of(dc, headers, ["x", "all"]) == "", "a lost cell is never guessed"
    selectors = ["ArchUnitNET", "Rulebearing", "Languages"]
    assert cs.claim_of(archunit, selectors, ["`Types()`", "k", "all"]) == "Parity"
    assert cs.claim_of(archunit, selectors, ["(none)", "k", "TS"]) == "Addition"
    sections = cs.parse_tab(SELECTORS)
    lost = sections[1]["rows"][0]
    assert cs.claim_of(archunit, sections[1]["headers"], lost) == "b"


def test_derive_keeps_each_rows_evidence_and_records_the_tab() -> None:
    dc = cs.TAB_LIST[0]
    evidence = [{"results": "layer2", "item": "test/validate/index.spec.mjs"}]
    previous = {
        "sections": [
            {"title": "Rules", "rows": [{"cells": ["`forbidden[]`"], "evidence": evidence}]},
            {"title": "Gone", "rows": [{"cells": ["`old`"], "evidence": evidence}]},
        ]
    }
    derived = cs.derive(dc, TAB, previous)
    rows = derived["sections"][0]["rows"]
    assert rows[0] == {
        "cells": ["`forbidden[]`", "all", "Parity+", "1"],
        "claim": "Parity+",
        "evidence": evidence,
    }
    assert rows[1]["evidence"] == []
    assert derived["tabSha256"] == cs.tab_digest(TAB)
    assert "status" not in json.dumps(derived["sections"])


def test_rows_writes_the_ledger_and_check_reports_drift(tmp_path: Path) -> None:
    tabs = tmp_path / "tabs"
    tabs.mkdir()
    ledger = tmp_path / "conformance" / "ledger"
    assert cs.rows_command(tabs, ledger, check=False) == 2, "the tabs are local"
    for tab in cs.TAB_LIST:
        (tabs / f"{tab.id}-coverage.md").write_text(TAB, encoding="utf-8")
    assert cs.rows_command(tabs, ledger, check=True) == 1
    assert cs.rows_command(tabs, ledger, check=False) == 0
    assert cs.rows_command(tabs, ledger, check=True) == 0
    (tabs / f"{cs.TAB_LIST[0].id}-coverage.md").write_text(TAB + SELECTORS, encoding="utf-8")
    assert cs.rows_command(tabs, ledger, check=True) == 1


def test_a_row_is_proven_only_when_all_its_evidence_passed(tmp_path: Path) -> None:
    folder = tmp_path / "results"
    results(
        folder, "layer2", {"test/validate/a.spec.mjs": "pass", "test/validate/b.spec.mjs": "fail"}
    )
    results(folder, "tests", {"rb-cli/cli": "pass", "rb-cli/cli::t": "pass"})
    results(folder, "layer3", {})
    ledger = ledger_with(
        tmp_path,
        [
            {
                "cells": ["all pass", "Parity"],
                "claim": "Parity",
                "evidence": [
                    {"results": "layer2", "item": "test/validate/a.spec.mjs"},
                    {"results": "tests", "item": "rb-cli/cli::t"},
                ],
            },
            {
                "cells": ["one fails", "Parity"],
                "claim": "Parity",
                "evidence": [
                    {"results": "layer2", "item": "test/validate/a.spec.mjs"},
                    {"results": "layer2", "item": "test/validate/b.spec.mjs"},
                ],
            },
            {
                "cells": ["missing file", "Parity"],
                "claim": "Parity",
                "evidence": [
                    {"results": "layer1"},
                ],
            },
            {
                "cells": ["whole file with a failure", "Parity"],
                "claim": "Parity",
                "evidence": [
                    {"results": "layer2"},
                ],
            },
            {
                "cells": ["whole file with nothing in it", "Parity"],
                "claim": "Parity",
                "evidence": [
                    {"results": "layer3"},
                ],
            },
            {
                "cells": ["whole file passing", "Parity"],
                "claim": "Parity",
                "evidence": [
                    {"results": "tests"},
                ],
            },
            {"cells": ["no evidence", "Parity"], "claim": "Parity", "evidence": []},
        ],
    )
    document = cs.coverage(ledger, folder, "123")
    rows = document["tabs"][0]["sections"][0]["rows"]
    assert [r["status"] for r in rows] == [
        "proven",
        "unproven",
        "unproven",
        "unproven",
        "unproven",
        "proven",
        "unproven",
    ]
    assert document["tabs"][0]["counts"] == {"proven": 2, "unproven": 5}
    first = rows[0]["evidence"][0]
    assert first == {
        "label": "gate 1 layer 2: test/validate/a.spec.mjs",
        "proven": True,
        "run": "123",
        "runUrl": "https://github.com/benbahrenburg/rulebearing/actions/runs/123",
        "source": "https://github.com/sverweij/dependency-cruiser/blob/v18.2.0/test/validate/a.spec.mjs",
        "upstream": True,
    }
    failed = rows[1]["evidence"][1]
    assert failed["proven"] is False
    assert "run" not in failed
    assert rows[0]["claim"] == "Parity", "the claim is kept; the site shows it only when proven"


def test_a_results_file_names_its_own_run(tmp_path: Path) -> None:
    folder = tmp_path / "results"
    results(folder, "jobs", {"dotnet-adapters": "pass"}, run="9")
    ledger = ledger_with(
        tmp_path,
        [
            {
                "cells": ["x", "Parity"],
                "claim": "Parity",
                "evidence": [
                    {"results": "jobs", "item": "dotnet-adapters"},
                ],
            }
        ],
    )
    evidence = cs.coverage(ledger, folder, "")["tabs"][0]["sections"][0]["rows"][0]["evidence"][0]
    assert evidence["run"] == "9"
    assert evidence["label"] == "CI job dotnet-adapters"
    local = cs.judged(
        {"results": "jobs", "item": "a"}, cs.results_document("jobs", {"a": "pass"}), "local"
    )
    assert local["run"] == "local"
    assert "runUrl" not in local


def test_two_runs_write_the_same_bytes(tmp_path: Path) -> None:
    folder = tmp_path / "results"
    results(
        folder, "layer2", {"test/validate/b.spec.mjs": "pass", "test/validate/a.spec.mjs": "pass"}
    )
    first = tmp_path / "one.json"
    second = tmp_path / "two.json"
    assert cs.main(["write", "--results", str(folder), "--run", "7", "--out", str(first)]) == 0
    assert cs.main(["write", "--results", str(folder), "--run", "7", "--out", str(second)]) == 0
    assert first.read_bytes() == second.read_bytes()
    assert first.read_text(encoding="utf-8").endswith("}\n")
    written = json.loads(first.read_text(encoding="utf-8"))
    assert written["run"] == "7"
    assert [t["id"] for t in written["tabs"]] == [t.id for t in cs.TAB_LIST]


def test_layer_5_is_read_from_its_folders(tmp_path: Path) -> None:
    folder = tmp_path / "layer5"
    assert cs.layer5_results(folder) is None
    write_json(folder / "a__b" / "row.json", {"repo": "a/b", "zeroDiff": "yes"})
    write_json(folder / "c__d" / "row.json", {"repo": "c/d", "zeroDiff": "3 differences"})
    write_json(
        folder / "mutations" / "result.json", {"steps": {"mutations": "pass", "affected": "x"}}
    )
    (folder / "empty").mkdir()
    document = cs.layer5_results(folder)
    assert document is not None
    assert document["items"] == {
        "a/b": "pass",
        "c/d": "fail",
        "mutations:affected": "fail",
        "mutations:mutations": "pass",
    }
    assert cs.load_results(tmp_path, "layer5") == document
    assert cs.layer5_results(tmp_path / "nothing") is None
    (tmp_path / "bare").mkdir()
    assert cs.layer5_results(tmp_path / "bare") is None


def test_unported_reasons_are_read_from_the_tracked_file(tmp_path: Path) -> None:
    path = tmp_path / "unported.json"
    write_json(path, {"entries": [{"reason": "custom-predicate"}, {"test": "no reason"}]})
    document = cs.unported_results("unported-archunitnet", path)
    assert document is not None
    assert document["items"] == {"custom-predicate": "pass"}
    assert cs.unported_results("x", tmp_path / "absent.json") is None
    real = cs.load_results(tmp_path, "unported-archunitnet")
    assert real is not None
    assert real["items"]["custom-predicate"] == "pass"


def test_test_output_is_read_line_by_line() -> None:
    output = (
        "running 3 tests\n"
        "test a::b ... ok\n"
        "test c ... FAILED\n"
        "test d ... ignored\n"
        "test result: FAILED. 1 passed; 1 failed\n"
    )
    items = cs.parse_test_output("rb-x", "lib", output, 101)
    assert items == {"rb-x/lib::a::b": "pass", "rb-x/lib::c": "fail", "rb-x/lib": "fail"}
    passing = cs.parse_test_output("rb-x", "t", "test a ... ok\n", 0)
    assert passing["rb-x/t"] == "pass"
    assert cs.parse_test_output("rb-x", "t", "running 0 tests\n", 0) == {"rb-x/t": "fail"}


def test_the_tests_command_runs_each_named_target_once(tmp_path: Path) -> None:
    ledger = ledger_with(
        tmp_path,
        [
            {
                "cells": ["x", "P"],
                "claim": "P",
                "evidence": [
                    {"results": "tests", "item": "rb-a/lib::m::tests::one"},
                    {"results": "tests", "item": "rb-a/lib::m::tests::two"},
                    {"results": "tests", "item": "rb-b/cli"},
                    {"results": "layer1"},
                ],
            }
        ],
    )
    calls: list[tuple[str, str]] = []

    def runner(package: str, target: str) -> tuple[str, int]:
        calls.append((package, target))
        if package == "rb-a":
            return "test m::tests::one ... ok\ntest m::tests::two ... ok\n", 0
        return "test x ... FAILED\n", 101

    folder = tmp_path / "results"
    assert cs.run_tests_command(ledger, folder, runner) == 1
    assert calls == [("rb-a", "lib"), ("rb-b", "cli")]
    written = json.loads((folder / "tests.json").read_text(encoding="utf-8"))
    assert written["items"]["rb-a/lib::m::tests::two"] == "pass"
    assert written["items"]["rb-b/cli"] == "fail"
    assert written["ok"] is False


def test_jobs_are_recorded_from_the_run(tmp_path: Path) -> None:
    source = tmp_path / "run.json"
    write_json(
        source,
        {
            "jobs": [
                {"name": "dotnet-adapters", "conclusion": "success"},
                {"name": "rb-node (ubuntu-latest)", "conclusion": "failure"},
                "not a job",
            ]
        },
    )
    folder = tmp_path / "results"
    assert cs.main(["jobs", "--from", str(source), "--run", "42", "--results", str(folder)]) == 0
    written = json.loads((folder / "jobs.json").read_text(encoding="utf-8"))
    assert written["items"] == {"dotnet-adapters": "pass", "rb-node (ubuntu-latest)": "fail"}
    assert written["run"] == "42"
    write_json(source, {"jobs": "none"})
    assert cs.jobs_command(source, "1", folder) == 2


def test_every_kind_of_evidence_is_labelled_and_linked() -> None:
    blob = "https://github.com/benbahrenburg/rulebearing/blob/main/"
    cases = [
        (
            {"results": "tests", "item": "rb-cli/cache"},
            "rb-cli/cache: every test",
            blob + "crates/rb-cli/tests/cache.rs",
            False,
        ),
        (
            {"results": "tests", "item": "rb-rules/lib::a::b"},
            "rb-rules/lib: a::b",
            blob + "crates/rb-rules/src",
            False,
        ),
        (
            {"results": "unported-netarchtest", "item": "r"},
            "NetArchTest tests left upstream: r",
            blob + "conformance/netarchtest/unported.json",
            True,
        ),
        (
            {"results": "gate2-archunitnet"},
            "gate 2: every ported ArchUnitNET case",
            blob + "conformance/archunitnet/ported",
            True,
        ),
        (
            {"results": "gate2-netarchtest", "item": "F"},
            "gate 2: NetArchTest F",
            blob + "conformance/netarchtest/ported/F.yaml",
            True,
        ),
        (
            {"results": "layer1"},
            "gate 1 layer 1: every recorded test/extract case",
            blob + "crates/rb-extract-ts/tests/extract_fixtures.rs",
            True,
        ),
        (
            {"results": "layer3", "item": "oracle:d2"},
            "gate 1 layer 3: d2 compared with upstream's reporter",
            blob + "conformance/dependency-cruiser/harness/run-layer-3.mjs",
            True,
        ),
        (
            {"results": "layer5", "item": "mutations:affected"},
            "gate 1 layer 5: the mutation branch with --affected",
            blob + "conformance/dependency-cruiser/scripts/run-layer-5.sh",
            True,
        ),
        (
            {"results": "layer5", "item": "a/b"},
            "gate 1 layer 5: zero difference on a/b",
            blob + "conformance/dependency-cruiser/scripts/run-layer-5.sh",
            True,
        ),
        (
            {"results": "layer1", "item": "test/extract/x.spec.mjs#2"},
            "gate 1 layer 1: test/extract/x.spec.mjs#2",
            cs.UPSTREAM_URL + "test/extract/x.spec.mjs",
            True,
        ),
    ]
    for evidence, label, source, upstream in cases:
        assert cs.described(evidence) == (label, source, upstream)


def test_the_committed_ledgers_name_only_what_the_tree_holds() -> None:
    assert cs.check_command(cs.LEDGER) == 0
    keys = set()
    for tab in cs.TAB_LIST:
        document = cs.load(cs.ledger_path(cs.LEDGER, tab))
        for section in document["sections"]:
            for row in section["rows"]:
                key = (tab.id, section["title"], row["cells"][0])
                assert key not in keys, f"two rows share the key {key}"
                keys.add(key)
                assert row["evidence"], f"{key} names no evidence"
                assert "status" not in row, "a status is never written into the ledger"
    assert len(keys) == 190


def test_a_bad_reference_is_named(tmp_path: Path, capsys: pytest.CaptureFixture[str]) -> None:
    bad = [
        {"results": "nonsense"},
        {"results": "gate2-archunitnet", "item": "NoSuchFile"},
        {"results": "tests"},
        {"results": "tests", "item": "rb-nothing/cli"},
        {"results": "tests", "item": "rb-cli/no_such_target"},
        {"results": "tests", "item": "rb-cli/cli::no_such_test"},
        {"results": "tests", "item": "rb-rules/lib::x::no_such_unit_test"},
        {"results": "layer1", "source": "no/such/file"},
    ]
    ledger = ledger_with(tmp_path, [{"cells": ["x", "P"], "claim": "P", "evidence": bad}])
    assert cs.check_command(ledger) == 1
    err = capsys.readouterr().err
    for text in [
        "unknown results 'nonsense'",
        "no ported case file NoSuchFile",
        "a tests item must name a target",
        "no crate rb-nothing",
        "no test target rb-cli/no_such_target",
        "no test no_such_test in rb-cli/cli",
        "no test x::no_such_unit_test in rb-rules/lib",
        "no file no/such/file",
    ]:
        assert text in err
    (tmp_path / "ledger" / f"{cs.TAB_LIST[1].id}.json").write_text("not json", encoding="utf-8")
    assert cs.check_command(tmp_path / "ledger") == 1


def test_the_ledger_matches_the_local_tabs() -> None:
    if not all((cs.TABS / f"{tab.id}-coverage.md").is_file() for tab in cs.TAB_LIST):
        pytest.skip("the coverage tabs are local (docs/adr/0039-plans-and-design-kept-local.md)")
    assert cs.rows_command(cs.TABS, cs.LEDGER, check=True) == 0


def test_the_command_line_dispatches(tmp_path: Path) -> None:
    assert cs.main(["check"]) == 0
    assert cs.main(["rows", "--tabs", str(tmp_path), "--ledger", str(tmp_path / "l")]) == 2
    with pytest.raises(SystemExit):
        cs.main(["jobs"])
    with pytest.raises(SystemExit):
        cs.main(["publish"])
