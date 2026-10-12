# Copyright (c) 2026 Ben Bahrenburg. MIT licence: see LICENSE.
r"""The live coverage tables: each coverage-tab row's status, decided by the conformance results.

Plan: docs/plans/pending/0004-wave-4-reach.md, Step 4 ("reads the conformance results ... and the
two coverage tabs, and writes site/data/coverage.json with a status and an evidence link per row
... a row whose evidence is missing renders as unproven regardless of the tab's text").
Decisions: docs/adr/0009-conformance-suites-as-specification.md (a row may not say Parity until
the pinned suite says so),
docs/adr/0074-the-coverage-ledger-is-tracked-and-its-status-is-computed.md (the tracked ledger
and the status rule). Requirement: docs/prd.md#nfr-conf-03.

The rows come from conformance/ledger/<tab>.json, one tracked file per coverage tab: each row's
cells as the tab exports them, and the evidence that proves it. The tabs themselves stay local
(docs/adr/0039-plans-and-design-kept-local.md); `rows` derives the ledger's rows from them and
keeps each row's evidence. A row's status is never written by hand: `write` computes it from the
results the gates write under target/conformance/.

  python3 testbeds/scripts/coverage_status.py rows [--tabs docs/artifacts] [--check]
      derive the ledger's rows from the local tabs, keeping each row's evidence
  python3 testbeds/scripts/coverage_status.py check
      every evidence reference names something the repository holds
  python3 testbeds/scripts/coverage_status.py tests [--results target/conformance]
      run the Rust test targets the ledger names and write tests.json
  python3 testbeds/scripts/coverage_status.py jobs --from <gh run view --json json> --run <id>
      record a CI run's job conclusions as jobs.json
  python3 testbeds/scripts/coverage_status.py write [--results target/conformance] [--run <id>]
      write site/data/coverage.json

A row is proven when it names evidence and every piece of it is proven: the results file it names
exists and, for an item, records it as passed, or, for a whole results file, records every item
passed and at least one. Anything else, a missing file included, leaves the row unproven. Two runs
over the same inputs write the same bytes. Standard library only.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[2]
LEDGER = ROOT / "conformance" / "ledger"
RESULTS = ROOT / "target" / "conformance"
OUT = ROOT / "site" / "data" / "coverage.json"
TABS = ROOT / "docs" / "artifacts"
REPO_URL = "https://github.com/benbahrenburg/rulebearing"
RUNS = f"{REPO_URL}/actions/runs/"
GOOD = "pass"
BAD = "fail"
PROVEN = "proven"
UNPROVEN = "unproven"


@dataclass(frozen=True)
class Tab:
    """One coverage tab: its exported file, its ledger and the column that holds its claim."""

    id: str
    title: str
    claim_columns: tuple[str, ...]


TAB_LIST = (
    Tab("dependency-cruiser-18.2.0", "dependency-cruiser 18.2.0 coverage", ("Rulebearing",)),
    Tab("archunitnet-0.13.4", "ArchUnitNET 0.13.4 coverage", ("Status",)),
)

# Results that are read from tracked files rather than from target/conformance.
UNPORTED = {
    "unported-archunitnet": ROOT / "conformance" / "archunitnet" / "unported.json",
    "unported-netarchtest": ROOT / "conformance" / "netarchtest" / "unported.json",
}


def load(path: Path) -> Any:  # noqa: ANN401 - JSON
    """A JSON file, or None when it is absent or unreadable."""
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return None


def dump(document: Any) -> str:  # noqa: ANN401 - JSON
    """The one serialisation every file this script writes uses: sorted keys, two spaces."""
    return json.dumps(document, indent=2, sort_keys=True, ensure_ascii=False) + "\n"


# The tabs ------------------------------------------------------------------------------------


def split_cells(line: str) -> list[str]:
    r"""The cells of a Markdown table line, with a pipe inside a code span kept in its cell.

    A backslash-escaped backtick (`\``) is text, not a code-span delimiter.
    """
    body = line.strip()
    body = body.removeprefix("|")
    body = body.removesuffix("|")
    cells: list[str] = []
    current: list[str] = []
    in_code = False
    index = 0
    while index < len(body):
        char = body[index]
        if char == "\\" and index + 1 < len(body):
            current.append(body[index : index + 2])
            index += 2
            continue
        if char == "`":
            in_code = not in_code
        if char == "|" and not in_code:
            cells.append("".join(current).strip())
            current = []
        else:
            current.append(char)
        index += 1
    cells.append("".join(current).strip())
    return cells


def is_rule(line: str) -> bool:
    """True for a table's delimiter row (`| --- | --- |`)."""
    return line.strip().startswith("|") and all(
        re.fullmatch(r":?-{3,}:?", cell) for cell in split_cells(line)
    )


def parse_tab(text: str) -> list[dict[str, Any]]:
    """The tables of a coverage tab, by `##` section: headers and the rows' cells."""
    sections: list[dict[str, Any]] = []
    title = ""
    lines = text.splitlines()
    index = 0
    while index < len(lines):
        line = lines[index]
        if line.startswith("## "):
            title = line[3:].strip()
        elif line.startswith("|") and index + 1 < len(lines) and is_rule(lines[index + 1]):
            headers = split_cells(line)
            rows = []
            index += 2
            while index < len(lines) and lines[index].startswith("|"):
                rows.append(split_cells(lines[index]))
                index += 1
            sections.append({"title": title, "headers": headers, "rows": rows})
            continue
        index += 1
    return sections


def claim_of(tab: Tab, headers: list[str], cells: list[str]) -> str:
    """The tab's claim for a row: the cell under its status column.

    A section with no status column (ArchUnitNET's selectors) lists each method with the key it
    maps to, which the tab's lead defines as Parity; a row naming no upstream method is an
    addition. A cell the export lost is an empty claim, never a guessed one.
    """
    for column in tab.claim_columns:
        if column in headers:
            position = headers.index(column)
            return cells[position] if position < len(cells) else ""
    return "Addition" if cells and cells[0] == "(none)" else "Parity"


def tab_digest(text: str) -> str:
    """The sha256 of a tab's text, recorded so a later export is noticed."""
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def derive(tab: Tab, text: str, previous: dict[str, Any] | None) -> dict[str, Any]:
    """The ledger of `tab` from its text, each row keeping the evidence `previous` gave it."""
    kept: dict[tuple[str, str], list[Any]] = {}
    for section in (previous or {}).get("sections", []):
        for row in section.get("rows", []):
            kept[(section["title"], row["cells"][0])] = row.get("evidence", [])
    sections = []
    for section in parse_tab(text):
        rows = [
            {
                "cells": cells,
                "claim": claim_of(tab, section["headers"], cells),
                "evidence": kept.get((section["title"], cells[0]), []),
            }
            for cells in section["rows"]
        ]
        sections.append({"title": section["title"], "headers": section["headers"], "rows": rows})
    return {
        "$comment": (
            f"The rows of the {tab.title} tab (docs/artifacts/{tab.id}-coverage.md, local only), "
            "derived by testbeds/scripts/coverage_status.py rows, and the evidence that proves "
            "each. The cells and claims are the tab's and are never edited here; a status is "
            "never written here (docs/adr/0009-conformance-suites-as-specification.md, "
            "docs/adr/0074-the-coverage-ledger-is-tracked-and-its-status-is-computed.md)."
        ),
        "tab": tab.id,
        "title": tab.title,
        "tabSha256": tab_digest(text),
        "sections": sections,
    }


def ledger_path(ledger: Path, tab: Tab) -> Path:
    """Where a tab's ledger is kept."""
    return ledger / f"{tab.id}.json"


def rows_command(tabs: Path, ledger: Path, *, check: bool) -> int:
    """Derive (or, with `check`, compare) every ledger's rows from the local tabs."""
    stale = []
    for tab in TAB_LIST:
        source = tabs / f"{tab.id}-coverage.md"
        if not source.is_file():
            sys.stderr.write(f"coverage: {source} is absent; the tabs are local (ADR-0039)\n")
            return 2
        path = ledger_path(ledger, tab)
        current = path.read_text(encoding="utf-8") if path.is_file() else ""
        derived = dump(derive(tab, source.read_text(encoding="utf-8"), load(path)))
        if derived == current:
            continue
        stale.append(path)
        if not check:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(derived, encoding="utf-8")
    for path in stale:
        verb = "differs from its tab" if check else "rewritten"
        sys.stdout.write(f"coverage: {path.relative_to(ledger.parent.parent)} {verb}\n")
    return 1 if check and stale else 0


# The evidence --------------------------------------------------------------------------------


def evidence_of(ledger: Path) -> list[dict[str, Any]]:
    """Every piece of evidence in every ledger."""
    found = []
    for tab in TAB_LIST:
        document = load(ledger_path(ledger, tab)) or {}
        for section in document.get("sections", []):
            for row in section.get("rows", []):
                found.extend(row.get("evidence", []))
    return found


def named_targets(evidence: list[dict[str, Any]]) -> list[tuple[str, str]]:
    """The (package, target) pairs the `tests` evidence names, sorted."""
    pairs = set()
    for item in evidence:
        if item.get("results") == "tests" and item.get("item"):
            package, target = item["item"].split("::", 1)[0].split("/", 1)
            pairs.add((package, target))
    return sorted(pairs)


TEST_LINE = re.compile(r"^test (\S+) \.\.\. (ok|FAILED|ignored)", re.MULTILINE)


def parse_test_output(package: str, target: str, output: str, code: int) -> dict[str, str]:
    """The items one `cargo test` run proves: each test, and the target when all passed."""
    items: dict[str, str] = {}
    for name, verdict in TEST_LINE.findall(output):
        if verdict == "ignored":
            continue
        items[f"{package}/{target}::{name}"] = GOOD if verdict == "ok" else BAD
    whole = code == 0 and bool(items) and all(v == GOOD for v in items.values())
    items[f"{package}/{target}"] = GOOD if whole else BAD
    return items


def cargo_test(package: str, target: str) -> tuple[str, int]:
    """Runs one test target with every feature; its standard output and exit code."""
    selector = ["--lib"] if target == "lib" else ["--test", target]
    command = [
        "cargo",
        "test",
        "-p",
        package,
        *selector,
        "--all-features",
        "--",
        "--format",
        "pretty",
    ]
    done = subprocess.run(  # noqa: S603 - a fixed command, arguments from the tracked ledger
        command, cwd=ROOT, capture_output=True, text=True, check=False
    )
    return done.stdout, done.returncode


def results_document(name: str, items: dict[str, str]) -> dict[str, Any]:
    """A results file in the one shape every gate writes."""
    passed = sum(1 for v in items.values() if v == GOOD)
    return {
        "results": name,
        "ok": bool(items) and passed == len(items),
        "passed": passed,
        "total": len(items),
        "items": dict(sorted(items.items())),
    }


def run_tests_command(ledger: Path, results: Path, runner: Any = cargo_test) -> int:  # noqa: ANN401
    """Runs the targets the ledger names and writes tests.json; exit 1 when one failed."""
    items: dict[str, str] = {}
    for package, target in named_targets(evidence_of(ledger)):
        output, code = runner(package, target)
        found = parse_test_output(package, target, output, code)
        items.update(found)
        verdict = found[f"{package}/{target}"]
        sys.stdout.write(f"coverage: {package}/{target}: {verdict}\n")
    results.mkdir(parents=True, exist_ok=True)
    document = results_document("tests", items)
    (results / "tests.json").write_text(dump(document), encoding="utf-8")
    return 0 if document["ok"] else 1


def jobs_command(source: Path, run: str, results: Path) -> int:
    """Records the conclusions of a CI run's jobs (from `gh run view --json jobs`)."""
    document = load(source)
    if not isinstance(document, dict) or not isinstance(document.get("jobs"), list):
        sys.stderr.write(f"coverage: {source} holds no jobs\n")
        return 2
    items = {
        str(job["name"]): GOOD if job.get("conclusion") == "success" else BAD
        for job in document["jobs"]
        if isinstance(job, dict) and "name" in job
    }
    written = results_document("jobs", items)
    written["run"] = run
    results.mkdir(parents=True, exist_ok=True)
    (results / "jobs.json").write_text(dump(written), encoding="utf-8")
    return 0


# The results ---------------------------------------------------------------------------------


def layer5_results(folder: Path) -> dict[str, Any] | None:
    """Layer 5's verdicts from run-layer-5.sh's folders: each oracle and the mutation branch."""
    if not folder.is_dir():
        return None
    items: dict[str, str] = {}
    for child in sorted(p for p in folder.iterdir() if p.is_dir()):
        row = load(child / "row.json")
        if isinstance(row, dict) and "repo" in row:
            items[str(row["repo"])] = GOOD if row.get("zeroDiff") == "yes" else BAD
        result = load(child / "result.json")
        if isinstance(result, dict) and "steps" in result:
            for step, verdict in result["steps"].items():
                items[f"{child.name}:{step}"] = GOOD if verdict == GOOD else BAD
    return results_document("layer5", items) if items else None


def unported_results(name: str, path: Path) -> dict[str, Any] | None:
    """A suite's unported tests by reason: an item passes when the reason has an entry."""
    document = load(path)
    if not isinstance(document, dict):
        return None
    items = {
        str(entry["reason"]): GOOD
        for entry in document.get("entries", [])
        if isinstance(entry, dict) and "reason" in entry
    }
    return results_document(name, items)


def load_results(results: Path, name: str) -> dict[str, Any] | None:
    """The results named `name`: a gate's file under `results`, or one read from the tree."""
    if name == "layer5":
        return layer5_results(results / "layer5")
    if name in UNPORTED:
        return unported_results(name, UNPORTED[name])
    document = load(results / f"{name}.json")
    return document if isinstance(document, dict) else None


def proven(evidence: dict[str, Any], document: dict[str, Any] | None) -> bool:
    """Whether one piece of evidence is proven by its results."""
    if document is None:
        return False
    item = evidence.get("item")
    if item:
        return bool(document.get("items", {}).get(item) == GOOD)
    items = document.get("items", {})
    return bool(document.get("ok") is True and items and all(v == GOOD for v in items.values()))


UPSTREAM_URL = "https://github.com/sverweij/dependency-cruiser/blob/v18.2.0/"
SUITES = {"gate2-archunitnet": "ArchUnitNET", "gate2-netarchtest": "NetArchTest"}
WHOLE = {
    "layer1": "gate 1 layer 1: every recorded test/extract case",
    "layer2": "gate 1 layer 2: every test/validate and test/graph-utl spec",
    "layer3": "gate 1 layer 3: every test/report spec and reporter comparison",
    "layer5": "gate 1 layer 5: every oracle and the mutation branch",
    "gate2-archunitnet": "gate 2: every ported ArchUnitNET case",
    "gate2-netarchtest": "gate 2: every ported NetArchTest case",
}
LAYER5 = {
    "mutations:mutations": "gate 1 layer 5: the mutation branch's twelve violations",
    "mutations:affected": "gate 1 layer 5: the mutation branch with --affected",
}
HARNESS = "conformance/dependency-cruiser/"
WHOLE_SOURCES = {
    "layer1": "crates/rb-extract-ts/tests/extract_fixtures.rs",
    "layer2": HARNESS + "harness/run-layer-2.mjs",
    "layer3": HARNESS + "harness/run-layer-3.mjs",
    "layer5": HARNESS + "scripts/run-layer-5.sh",
}


def blob(path: str) -> str:
    """A file of this repository on GitHub."""
    return f"{REPO_URL}/blob/main/{path}"


def tests_item_source(item: str) -> str:
    """The file (or the crate's sources) a `tests` item names."""
    package, _, target = item.split("::", 1)[0].partition("/")
    folder = f"crates/{package}"
    return blob(f"{folder}/src" if target == "lib" else f"{folder}/tests/{target}.rs")


def described(evidence: dict[str, Any]) -> tuple[str, str, bool]:
    """The label, the source link and whether the proof is the upstream suite's own."""
    name = str(evidence["results"])
    item = str(evidence.get("item") or "")
    if name == "tests":
        package_target, _, test = item.partition("::")
        return f"{package_target}: {test or 'every test'}", tests_item_source(item), False
    if name == "jobs":
        return f"CI job {item}", blob(".github/workflows/ci.yml"), False
    if name in UNPORTED:
        source = UNPORTED[name].relative_to(ROOT).as_posix()
        suite = "ArchUnitNET" if name.endswith("archunitnet") else "NetArchTest"
        return f"{suite} tests left upstream: {item}", blob(source), True
    if name in SUITES:
        folder = GATE_FILES[name].relative_to(ROOT).as_posix()
        if not item:
            return WHOLE[name], blob(folder), True
        return f"gate 2: {SUITES[name]} {item}", blob(f"{folder}/{item}.yaml"), True
    return described_gate1(name, item)


def described_gate1(name: str, item: str) -> tuple[str, str, bool]:
    """The label and source link of a gate 1 item."""
    if not item:
        return WHOLE.get(name, name), blob(WHOLE_SOURCES.get(name, HARNESS + "run.sh")), True
    if item.startswith("oracle:"):
        output = item.removeprefix("oracle:")
        label = f"gate 1 layer 3: {output} compared with upstream's reporter"
        return label, blob(WHOLE_SOURCES["layer3"]), True
    if name == "layer5":
        label = LAYER5.get(item, f"gate 1 layer 5: zero difference on {item}")
        return label, blob(WHOLE_SOURCES["layer5"]), True
    layer = name.removeprefix("layer")
    return f"gate 1 layer {layer}: {item}", UPSTREAM_URL + item.split("#", 1)[0], True


def run_href(run: str) -> str:
    """The link to a run: an Actions run for a numeric id, else nothing."""
    return f"{RUNS}{run}" if run.isdigit() else ""


def judged(evidence: dict[str, Any], document: dict[str, Any] | None, run: str) -> dict[str, Any]:
    """One piece of evidence as the site shows it: label, links and whether it holds."""
    holds = proven(evidence, document)
    label, source, upstream = described(evidence)
    shown: dict[str, Any] = {
        "label": label,
        "proven": holds,
        "source": source,
        "upstream": upstream,
    }
    own_run = str((document or {}).get("run", "")) or run
    if holds and own_run:
        shown["run"] = own_run
        href = run_href(own_run)
        if href:
            shown["runUrl"] = href
    return shown


def status_of(evidence: list[dict[str, Any]]) -> str:
    """A row's status: proven only when it names evidence and all of it holds."""
    return PROVEN if evidence and all(e["proven"] for e in evidence) else UNPROVEN


def coverage(ledger: Path, results: Path, run: str) -> dict[str, Any]:
    """The coverage document the site renders."""
    loaded: dict[str, dict[str, Any] | None] = {}
    tabs = []
    for tab in TAB_LIST:
        document = load(ledger_path(ledger, tab)) or {}
        sections = []
        counts = {PROVEN: 0, UNPROVEN: 0}
        for section in document.get("sections", []):
            rows = []
            for row in section.get("rows", []):
                shown = []
                for evidence in row.get("evidence", []):
                    name = evidence["results"]
                    if name not in loaded:
                        loaded[name] = load_results(results, name)
                    shown.append(judged(evidence, loaded[name], run))
                status = status_of(shown)
                counts[status] += 1
                rows.append(
                    {
                        "cells": row["cells"],
                        "claim": row["claim"],
                        "status": status,
                        "evidence": shown,
                    }
                )
            sections.append(
                {"title": section["title"], "headers": section["headers"], "rows": rows}
            )
        tabs.append(
            {
                "id": tab.id,
                "title": tab.title,
                "tabSha256": document.get("tabSha256", ""),
                "counts": counts,
                "sections": sections,
            }
        )
    return {
        "$comment": (
            "Written by testbeds/scripts/coverage_status.py write from conformance/ledger/ and the "
            "conformance results; never edited by hand (docs/adr/0009-conformance-suites-as-"
            "specification.md, docs/adr/0074-the-coverage-ledger-is-tracked-and-its-status-is-"
            "computed.md)."
        ),
        "run": run,
        "tabs": tabs,
    }


def write_command(ledger: Path, results: Path, run: str, out: Path) -> int:
    """Writes the coverage document and prints the counts."""
    document = coverage(ledger, results, run)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(dump(document), encoding="utf-8")
    for tab in document["tabs"]:
        counts = tab["counts"]
        sys.stdout.write(
            f"coverage: {tab['id']}: {counts[PROVEN]} proven, {counts[UNPROVEN]} unproven\n"
        )
    return 0


# The ledger's own check ----------------------------------------------------------------------

GATE_FILES = {
    "gate2-archunitnet": ROOT / "conformance" / "archunitnet" / "ported",
    "gate2-netarchtest": ROOT / "conformance" / "netarchtest" / "ported",
}
KNOWN = {
    "layer1",
    "layer2",
    "layer3",
    "layer5",
    "tests",
    "jobs",
    *GATE_FILES,
    *UNPORTED,
}


def tests_item_problem(item: str, root: Path) -> str | None:
    """What is wrong with a `tests` item, if anything: the crate, target and test must exist."""
    head, _, name = item.partition("::")
    package, _, target = head.partition("/")
    crate = root / "crates" / package
    if not (crate / "Cargo.toml").is_file():
        return f"no crate {package}"
    files = (
        sorted((crate / "src").rglob("*.rs"))
        if target == "lib"
        else [crate / "tests" / f"{target}.rs"]
    )
    if not all(f.is_file() for f in files) or not files:
        return f"no test target {package}/{target}"
    if name:
        function = name.rsplit("::", 1)[-1]
        pattern = re.compile(rf"\bfn {re.escape(function)}\s*\(")
        if not any(pattern.search(f.read_text(encoding="utf-8")) for f in files):
            return f"no test {name} in {package}/{target}"
    return None


def evidence_problem(evidence: dict[str, Any], root: Path) -> str | None:
    """What is wrong with one piece of evidence, if anything."""
    name = evidence.get("results")
    item = evidence.get("item")
    if name not in KNOWN:
        return f"unknown results {name!r}"
    if name in GATE_FILES and item and not (GATE_FILES[name] / f"{item}.yaml").is_file():
        return f"no ported case file {item}"
    if name == "tests":
        return (
            tests_item_problem(str(item or ""), root) if item else "a tests item must name a target"
        )
    source = evidence.get("source")
    if source and not (root / source).exists():
        return f"no file {source}"
    return None


def check_command(ledger: Path, root: Path = ROOT) -> int:
    """Every ledger is present, and every evidence reference names something the tree holds."""
    problems = []
    for tab in TAB_LIST:
        path = ledger_path(ledger, tab)
        document = load(path)
        if not isinstance(document, dict):
            problems.append(f"{path}: missing or not JSON")
            continue
        for section in document.get("sections", []):
            for row in section.get("rows", []):
                for evidence in row.get("evidence", []):
                    problem = evidence_problem(evidence, root)
                    if problem:
                        problems.append(
                            f"{tab.id}: {section['title']}: {row['cells'][0]}: {problem}"
                        )
    for problem in problems:
        sys.stderr.write(f"coverage: {problem}\n")
    return 1 if problems else 0


def main(argv: list[str] | None = None) -> int:
    """Run one command."""
    parser = argparse.ArgumentParser(description=(__doc__ or "").split("\n\n", 1)[0])
    parser.add_argument("command", choices=["rows", "check", "tests", "jobs", "write"])
    parser.add_argument("--ledger", type=Path, default=LEDGER)
    parser.add_argument("--tabs", type=Path, default=TABS)
    parser.add_argument("--results", type=Path, default=RESULTS)
    parser.add_argument("--out", type=Path, default=OUT)
    parser.add_argument("--run", default="")
    parser.add_argument("--from", dest="source", type=Path)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args(argv)
    if args.command == "rows":
        return rows_command(args.tabs, args.ledger, check=args.check)
    if args.command == "check":
        return check_command(args.ledger)
    if args.command == "tests":
        return run_tests_command(args.ledger, args.results)
    if args.command == "jobs":
        if args.source is None:
            parser.error("jobs needs --from")
        return jobs_command(args.source, args.run, args.results)
    return write_command(args.ledger, args.results, args.run, args.out)


if __name__ == "__main__":
    sys.exit(main())
