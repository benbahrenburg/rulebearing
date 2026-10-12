# Copyright (c) 2026 Ben Bahrenburg. MIT licence: see LICENSE.
"""The eval harness's results table and its place in docs/eval.md (plan 0004, Step 10)."""

from __future__ import annotations

import json
from pathlib import Path

import report

ROOT = Path(__file__).resolve().parents[1]

RESULT = {
    "date": "2026-10-11",
    "agent": "claude-code",
    "models": ["claude-x"],
    "budget": "usd:3.00",
    "rulebearing": "rulebearing 0.1.3",
    "suites": {"mutations": {"repo": "sverweij/dependency-cruiser", "sha": "7c22858071cc4491"}},
    "cases": [
        {
            "case": "m01-forbidden/RB-491004cb",
            "fix": "Do not import a | b.",
            "model": "claude-x",
            "turns": 3,
            "cleared": True,
            "widened": False,
            "budgetRaised": False,
            "costUsd": 0.4567,
            "note": None,
        }
    ],
    "totals": {
        "cases": 1,
        "cleared": 1,
        "clearedWithoutWidening": 1,
        "widened": 0,
        "budgetRaised": 0,
        "costUsd": 0.4567,
    },
}


def results(tmp_path: Path) -> Path:
    folder = tmp_path / "results"
    folder.mkdir()
    (folder / "2026-10-11-claude-code.json").write_text(json.dumps(RESULT))
    older = {**RESULT, "date": "2026-10-01", "cases": [], "suites": {}, "models": []}
    (folder / "2026-10-01-claude-code.json").write_text(json.dumps(older))
    return folder


def test_cells() -> None:
    assert report.cell(None) == "-"
    assert report.cell(True) == "yes"  # noqa: FBT003 - the value under test
    assert report.cell(False) == "no"  # noqa: FBT003 - the value under test
    assert report.cell(0.4567) == "0.46"
    assert report.cell("a | b\nc") == "a \\| b c"
    assert report.cell(3) == "3"


def test_each_run_is_a_section_newest_first(tmp_path: Path) -> None:
    text = report.render(report.load(results(tmp_path)))
    assert text.index("### 2026-10-11") < text.index("### 2026-10-01")
    assert "Models: `claude-x`. Budget per case: `usd:3.00`." in text
    assert "`mutations` (sverweij/dependency-cruiser at `7c22858071cc`)" in text
    assert "1 cases: 1 cleared, 1 cleared without widening" in text
    row = (
        "| `m01-forbidden/RB-491004cb` | Do not import a \\| b. | claude-x | 3 | yes | no | no "
        "| 0.46 | - |"
    )
    assert row in text
    assert "Models: -." in text
    assert report.render([]) == "No run has been recorded yet."


def test_the_page_is_written_and_checked(tmp_path: Path) -> None:
    folder = results(tmp_path)
    page = tmp_path / "eval.md"
    page.write_text(f"# Eval\n\n{report.START}\nold\n{report.END}\n\nAfter.\n")
    assert report.main(["--check", str(page)], folder) == 1
    assert report.main(["--write", str(page)], folder) == 0
    written = page.read_text()
    assert written.startswith(f"# Eval\n\n{report.START}\n### 2026-10-11")
    assert written.endswith(f"{report.END}\n\nAfter.\n")
    assert report.main(["--check", str(page)], folder) == 0
    page.write_text("no markers")
    assert report.main(["--check", str(page)], folder) == 2


def test_the_committed_page_is_current() -> None:
    assert report.main(["--check", str(ROOT / "docs" / "eval.md")]) == 0
