# Copyright (c) 2026 Ben Bahrenburg. MIT licence: see LICENSE.
"""Source mode's edges against compiled mode's on one .NET oracle: precision and recall.

Plan: docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md, Step 14 ("a precision
test that runs source mode and compiled mode over each .NET oracle and reports precision and
recall of source edges against compiled edges", at or above 90% precision) and the 3D status
table (`source-mode-precision.json`, published by the nightly). Requirement:
docs/prd.md#fr-ext-dn-04. Decision: docs/adr/0011-read-dotnet-assemblies-not-source.md (source
mode is approximate, and how approximate is measured, not assumed). Run by
testbeds/oracles/dotnet.sh after the compiled cruise; `--aggregate` joins the oracles' results for
the nightly summary.

An edge is a (from file, to file) pair between two file modules (`followable`), whatever its
dependency kind; external modules are left out, since compiled mode names an assembly where
source mode names a namespace. Only files both graphs know count: compiled mode sees the projects
the oracle's build built, source mode every project of the solution. Precision is the share of
source edges compiled mode also has; recall the share of compiled edges source mode found. The
first edges each side has alone are listed, for the reader who wants to know why.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

TARGET = 0.9
SHOWN = 20


def file_edges(document: dict[str, object]) -> tuple[set[str], set[tuple[str, str]]]:
    """A cruise result's file modules and the edges between them."""
    modules = document.get("modules")
    if not isinstance(modules, list):
        return set(), set()
    files = {
        m["source"]
        for m in modules
        if isinstance(m, dict) and m.get("followable") is True and isinstance(m.get("source"), str)
    }
    edges: set[tuple[str, str]] = set()
    for module in modules:
        if not isinstance(module, dict) or module.get("source") not in files:
            continue
        dependencies = module.get("dependencies")
        for dependency in dependencies if isinstance(dependencies, list) else []:
            if isinstance(dependency, dict) and dependency.get("resolved") in files:
                edges.add((module["source"], dependency["resolved"]))
    return files, edges


def ratio(part: int, whole: int) -> float:
    """`part / whole`, 1.0 for an empty whole (nothing to miss)."""
    return 1.0 if whole == 0 else round(part / whole, 4)


def compare(
    compiled: dict[str, object],
    source: dict[str, object],
) -> dict[str, object]:
    """Precision and recall of `source`'s edges against `compiled`'s."""
    compiled_files, compiled_edges = file_edges(compiled)
    source_files, source_edges = file_edges(source)
    known = compiled_files & source_files

    def within(edges: set[tuple[str, str]]) -> set[tuple[str, str]]:
        return {(f, t) for f, t in edges if f in known and t in known}

    compiled_edges = within(compiled_edges)
    source_edges = within(source_edges)
    agreeing = compiled_edges & source_edges
    return {
        "files": len(known),
        "sourceEdges": len(source_edges),
        "compiledEdges": len(compiled_edges),
        "agreeing": len(agreeing),
        "precision": ratio(len(agreeing), len(source_edges)),
        "recall": ratio(len(agreeing), len(compiled_edges)),
        "target": TARGET,
        "onlySource": [f"{f} -> {t}" for f, t in sorted(source_edges - compiled_edges)[:SHOWN]],
        "onlyCompiled": [f"{f} -> {t}" for f, t in sorted(compiled_edges - source_edges)[:SHOWN]],
    }


def aggregate(results: list[Path]) -> dict[str, object]:
    """The oracles' results in one document, and whether each meets the target."""
    oracles = []
    for path in sorted(results):
        result = json.loads(path.read_text())
        precision = result.get("precision")
        result["meetsTarget"] = isinstance(precision, float | int) and precision >= TARGET
        oracles.append(result)
    return {"target": TARGET, "oracles": oracles}


def parse(argv: list[str]) -> argparse.Namespace:
    """The command line."""
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--compiled", type=Path, help="the compiled-mode cruise result")
    parser.add_argument("--source", type=Path, help="the source-mode cruise result")
    parser.add_argument("--repo", help="the oracle's owner/name")
    parser.add_argument("--sha", help="the commit")
    parser.add_argument("--aggregate", type=Path, nargs="*", help="join these results instead")
    parser.add_argument("--out", type=Path, required=True, help="where to write the result")
    return parser.parse_args(argv)


def main(argv: list[str]) -> int:
    """Writes the comparison (or the aggregate); 2 when an input cannot be read."""
    arguments = parse(argv)
    try:
        if arguments.aggregate is not None:
            result = aggregate(arguments.aggregate)
        else:
            if arguments.compiled is None or arguments.source is None:
                sys.stderr.write("precision: give --compiled and --source, or --aggregate\n")
                return 2
            result = {
                "repo": arguments.repo,
                "sha": arguments.sha,
                **compare(
                    json.loads(arguments.compiled.read_text()),
                    json.loads(arguments.source.read_text()),
                ),
            }
    except (OSError, json.JSONDecodeError) as error:
        sys.stderr.write(f"precision: {error}\n")
        return 2
    arguments.out.parent.mkdir(parents=True, exist_ok=True)
    arguments.out.write_text(json.dumps(result, indent=2) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
