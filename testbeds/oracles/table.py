# Copyright (c) 2026 Ben Bahrenburg. MIT licence: see LICENSE.
"""The oracle agreement table, rendered from testbeds/results/*.json.

Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 11 ("writing
testbeds/results/<repo>.json with a per-test or per-contract agreement table that the README
table reads") and the 2F status rows. Requirement: docs/prd.md#nfr-conf-03.

Usage:
  python3 testbeds/oracles/table.py                    print the table
  python3 testbeds/oracles/table.py --readme README.md write it between the oracles markers
  python3 testbeds/oracles/table.py --check README.md  exit 1 when the committed table is stale

One row per result file: the contracts or tests compared, how many agree, disagree, stay with
the incumbent (a custom predicate or contract type), are not imported (with the importer's
reason in the file) or could not be compared, the graph comparison for a Python row, and the
first cause the file records for a disagreement. Standard library only, so the CI check needs
nothing installed.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path
from typing import Any

HERE = Path(__file__).resolve().parent
RESULTS = HERE.parent / "results"
START = "<!-- oracles:start -->"
END = "<!-- oracles:end -->"
DETAIL = 140


def cell(text: str) -> str:
    """Text safe inside a Markdown table cell."""
    return text.replace("|", "\\|").replace("\n", " ").strip()


def short(text: str) -> str:
    """The first sentence or so of a long explanation."""
    text = text.strip()
    return text if len(text) <= DETAIL else text[: DETAIL - 3].rstrip() + "..."


def graph_cell(document: dict[str, Any]) -> str:
    """The Python graph comparison: grimp's edges beside Rulebearing's."""
    graph = document.get("graph")
    if not graph:
        return ""
    theirs, ours = graph.get("importLinter", "?"), graph.get("rulebearing", "?")
    if graph.get("equal"):
        return f"{theirs} = {ours}"
    return f"{theirs} vs {ours}, {graph.get('unexplained', '?')} unexplained"


def outcome(document: dict[str, Any], rows: list[dict[str, Any]]) -> str:
    """Agrees, the first recorded cause of a disagreement, or why the row could not run."""
    if document.get("status") != "compared":
        return f"error: {short(str(document.get('detail', '')))}"
    disagreeing = [r for r in rows if r.get("verdict") == "disagree"]
    if not disagreeing:
        return "agrees"
    causes = [r["cause"] for r in disagreeing if r.get("cause")]
    explained = len(causes)
    lead = f"{len(disagreeing)} disagree"
    if explained:
        lead += f" ({explained} explained: {short(causes[0])})"
    return lead


def row(path: Path, readme_dir: Path) -> str:
    """One table row for one result file."""
    document = json.loads(path.read_text())
    rows = document.get("contracts", document.get("results", []))
    summary = document.get("summary", {})
    link = Path(os.path.relpath(path, readme_dir)).as_posix()
    counts = [
        str(summary.get(k, "")) if summary else ""
        for k in ("total", "agree", "disagree", "stays", "not-imported", "error")
    ]
    cells = [
        f"[{document['repo']}]({link})",
        document.get("tool", ""),
        *counts,
        graph_cell(document),
        outcome(document, rows),
    ]
    return "| " + " | ".join(cell(c) for c in cells) + " |"


def table(readme_dir: Path) -> str:
    """The whole table, rows in file-name order."""
    files = sorted(RESULTS.glob("*.json"), key=lambda p: p.name.lower())
    header = [
        (
            "| Repository | Incumbent | Compared | Agree | Disagree | Stays | Not imported "
            "| Errors | Graph (grimp vs Rulebearing) | Result |"
        ),
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
    ]
    return "\n".join(header + [row(p, readme_dir) for p in files]) + "\n"


def splice(text: str, rendered: str) -> str:
    """The README text with the table between the markers replaced."""
    start, end = text.find(START), text.find(END)
    if start < 0 or end < start:
        message = f"no {START} ... {END} markers"
        raise ValueError(message)
    return f"{text[: start + len(START)]}\n{rendered}{text[end:]}"


def main() -> int:
    """Print, write or check the table."""
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n", 1)[0])
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--readme", help="write the table between the markers in this file")
    mode.add_argument("--check", help="fail when this file's table is not the rendered one")
    args = parser.parse_args()
    target = args.readme or args.check
    if target is None:
        sys.stdout.write(table(Path.cwd()))
        return 0
    readme = Path(target)
    text = readme.read_text()
    try:
        updated = splice(text, table(readme.resolve().parent))
    except ValueError as error:
        sys.stderr.write(f"table: {readme}: {error}\n")
        return 2
    if args.check:
        if updated != text:
            sys.stderr.write(
                f"table: the oracle table in {readme} is stale; run "
                f"python3 testbeds/oracles/table.py --readme {readme}\n"
            )
            return 1
        return 0
    readme.write_text(updated)
    return 0


if __name__ == "__main__":
    sys.exit(main())
