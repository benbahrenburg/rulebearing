# Copyright (c) 2026 Ben Bahrenburg. MIT licence: see LICENSE.
r"""The eval harness's results table, written into docs/eval.md for the docs site.

Plan: docs/plans/pending/0004-wave-4-reach.md, Step 10 ("`report.py` (the table) ... results
committed under `eval/results/<date>-<agent>.json` and rendered on the docs site"). Decision:
docs/adr/0077-fix-plan-candidates-are-verified-and-the-eval-harness-scores-fix-text.md.
Requirement: docs/prd.md#fr-reach-03.

  python3 eval/report.py --write docs/eval.md   write the tables between the markers
  python3 eval/report.py --check docs/eval.md   exit 1 when the page's tables are stale

One section per results file, newest first: the run's parameters (agent, models, budget, binary,
suites), its totals, and one row per case. The site renders docs/eval.md like every other page
under docs/. Standard library only.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any

START = "<!-- eval-results:start -->"
END = "<!-- eval-results:end -->"
RESULTS = Path(__file__).resolve().parent / "results"
REPOSITORY = "https://github.com/benbahrenburg/rulebearing/blob/main/eval/results/"


def load(folder: Path) -> list[tuple[str, dict[str, Any]]]:
    """Each results file's name and document, newest first."""
    files = sorted(folder.glob("*.json"), reverse=True)
    return [(f.name, json.loads(f.read_text(encoding="utf-8"))) for f in files]


def cell(value: object) -> str:
    """A value as a table cell: text escaped, booleans as yes or no, nothing as a dash."""
    if value is None:
        return "-"
    if isinstance(value, bool):
        return "yes" if value else "no"
    if isinstance(value, float):
        return f"{value:.2f}"
    return str(value).replace("|", "\\|").replace("\n", " ")


def section(name: str, document: dict[str, Any]) -> str:
    """One run: its parameters, its totals and its rows."""
    totals = document.get("totals", {})
    suites = ", ".join(
        f"`{suite}` ({info['repo']} at `{info['sha'][:12]}`)"
        for suite, info in sorted(document.get("suites", {}).items())
    )
    models = ", ".join(f"`{m}`" for m in document.get("models", []))
    names = ", ".join(sorted(document.get("suites", {}))) or "no suite"
    lines = [
        f"### {document.get('date')}, {document.get('agent')}: {names}",
        "",
        (
            f"Results file: [{name}]({REPOSITORY}{name}). Models: {models or '-'}. "
            f"Budget per case: `{document.get('budget')}`. "
            f"Binary: `{document.get('rulebearing')}`. Suites: {suites or '-'}."
        ),
        "",
        (
            f"{totals.get('cases', 0)} cases: {totals.get('cleared', 0)} cleared, "
            f"{totals.get('clearedWithoutWidening', 0)} cleared without widening a rule or "
            f"raising a budget, {totals.get('widened', 0)} widened, "
            f"{totals.get('budgetRaised', 0)} raised a budget. "
            f"Cost: {totals.get('costUsd', 0):.2f} USD."
        ),
        "",
        "| Case | Fix | Model | Turns | Cleared | Widened | Budget raised | Cost (USD) | Note |",
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- |",
    ]
    lines.extend(
        "| "
        + " | ".join(
            [
                f"`{cell(row.get('case'))}`",
                cell(row.get("fix")),
                cell(row.get("model")),
                cell(row.get("turns")),
                cell(row.get("cleared")),
                cell(row.get("widened")),
                cell(row.get("budgetRaised")),
                cell(row.get("costUsd")),
                cell(row.get("note")),
            ]
        )
        + " |"
        for row in document.get("cases", [])
    )
    return "\n".join(lines)


def render(results: list[tuple[str, dict[str, Any]]]) -> str:
    """Every run's section, newest first, or a line saying there is none."""
    if not results:
        return "No run has been recorded yet."
    return "\n\n".join(section(name, document) for name, document in results)


def splice(page: str, block: str) -> str:
    """`page` with the text between the markers replaced by `block`.

    Raises:
        ValueError: the page lacks a marker.
    """
    start, end = page.find(START), page.find(END)
    if start < 0 or end < start:
        message = f"the page has no {START} ... {END} block"
        raise ValueError(message)
    return f"{page[: start + len(START)]}\n{block}\n{page[end:]}"


def main(argv: list[str] | None = None, results: Path = RESULTS) -> int:
    """Writes or checks the page; 1 when `--check` finds it stale, 2 for a page without markers."""
    parser = argparse.ArgumentParser(description="The eval harness's results table")
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--write", type=Path)
    mode.add_argument("--check", type=Path)
    args = parser.parse_args(argv)
    page_path: Path = args.write or args.check
    page = page_path.read_text(encoding="utf-8")
    try:
        wanted = splice(page, render(load(results)))
    except ValueError as error:
        sys.stderr.write(f"eval report: {page_path}: {error}\n")
        return 2
    if args.write:
        page_path.write_text(wanted, encoding="utf-8")
        return 0
    if wanted != page:
        sys.stderr.write(
            f"eval report: {page_path} is stale; run python3 eval/report.py --write {page_path}\n"
        )
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
