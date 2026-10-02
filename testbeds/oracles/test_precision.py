# Copyright (c) 2026 Ben Bahrenburg. MIT licence: see LICENSE.
"""Source mode's precision against compiled mode (plan 0003, Step 14)."""

from __future__ import annotations

import json
from typing import TYPE_CHECKING

import precision

if TYPE_CHECKING:
    from pathlib import Path


def module(source: str, *targets: str, followable: bool = True) -> dict[str, object]:
    return {
        "source": source,
        "followable": followable,
        "dependencies": [{"resolved": t} for t in targets],
    }


COMPILED: dict[str, object] = {
    "modules": [
        module("A.cs", "B.cs", "C.cs", "System.Runtime"),
        module("B.cs", "C.cs"),
        module("C.cs"),
        module("Only.cs", "A.cs"),
        module("System.Runtime", followable=False),
    ],
}
SOURCE: dict[str, object] = {
    "modules": [
        module("A.cs", "B.cs", "System.Text"),
        module("B.cs", "C.cs", "A.cs"),
        module("C.cs"),
        module("Unbuilt.cs", "A.cs"),
        module("System.Text", followable=False),
    ],
}


def test_only_edges_between_files_both_graphs_know_count() -> None:
    result = precision.compare(COMPILED, SOURCE)
    assert result["files"] == 3
    assert result["compiledEdges"] == 3
    assert result["sourceEdges"] == 3
    assert result["agreeing"] == 2
    assert result["precision"] == 0.6667
    assert result["recall"] == 0.6667
    assert result["onlySource"] == ["B.cs -> A.cs"]
    assert result["onlyCompiled"] == ["A.cs -> C.cs"]


def test_an_empty_side_has_nothing_to_miss() -> None:
    assert precision.ratio(0, 0) == 1.0
    empty = precision.compare({"modules": []}, {})
    assert empty["precision"] == 1.0
    assert empty["recall"] == 1.0


def test_the_command_line_writes_one_result_and_the_aggregate(tmp_path: Path) -> None:
    compiled = tmp_path / "graph.json"
    source = tmp_path / "source.json"
    compiled.write_text(json.dumps(COMPILED))
    source.write_text(json.dumps(SOURCE))
    one = tmp_path / "results" / "o__r.source-mode.json"
    arguments = ["--compiled", str(compiled), "--source", str(source), "--out", str(one)]
    assert precision.main([*arguments, "--repo", "o/r", "--sha", "abc"]) == 0
    written = json.loads(one.read_text())
    assert written["repo"] == "o/r"
    assert written["sha"] == "abc"
    good = tmp_path / "results" / "g__r.source-mode.json"
    good.write_text(json.dumps({"repo": "g/r", "precision": 0.95}))
    joined = tmp_path / "source-mode-precision.json"
    assert precision.main(["--aggregate", str(one), str(good), "--out", str(joined)]) == 0
    aggregate = json.loads(joined.read_text())
    assert aggregate["target"] == precision.TARGET
    assert [(o["repo"], o["meetsTarget"]) for o in aggregate["oracles"]] == [
        ("g/r", True),
        ("o/r", False),
    ]


def test_missing_inputs_are_named() -> None:
    assert precision.main(["--out", "unused.json"]) == 2
    assert precision.main(["--compiled", "nope.json", "--source", "nope.json", "--out", "x"]) == 2
