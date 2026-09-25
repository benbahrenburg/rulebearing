# Copyright (c) 2026 Ben Bahrenburg. MIT licence: see LICENSE.
"""The join of an incumbent's verdicts with Rulebearing's, per contract or per test.

Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 11 (the oracle
harness) and section 1.4.5. Requirement: docs/prd.md#nfr-conf-03. Design:
docs/artifacts/design.md, "Test beds", item 1 (oracle zero-diff). Run by
testbeds/oracles/python.sh and testbeds/oracles/dotnet.sh; writes testbeds/results/<slug>.json,
which testbeds/oracles/table.py reads.

`python`: import-linter's verdict on each contract (lint_imports_json.py) beside Rulebearing's
over the rules `rulebearing import import-linter` wrote from it. A rule belongs to the contract
its comment names ("import-linter contract: <name>"); a violation of the `allowed` list
(`not-in-allowed`) belongs to each protected contract whose `to.path` matches the imported
module. The violations come from `cruise -T junit` (one line each, no chain), and only
error-severity failures count: an `ignore_imports` entry is a known violation, which the
reporter lists without failing the rule. A disagreeing contract is re-checked over
Rulebearing's graph with the imports import-linter removes before it follows chains
(`ignore_imports`, and `TYPE_CHECKING` imports under `exclude_type_checking_imports`) removed
too; when that re-check keeps the contract, the row's `cause` says so. The verdict stays
`disagree`: the import cannot express either filter, so the imported rules do disagree.

The graph comparison is grimp's direct imports between the root packages' modules beside
Rulebearing's local edges between the same files (from a cruise with no rules, which is also
what the re-check edits), self-imports left out. An edge on one side
only is counted by kind: `notGrimpModule` (a file in a folder grimp does not walk, having no
`__init__.py`), `dynamic` (a literal `importlib.import_module`, which grimp does not read),
`allReexport` (a submodule an `__init__.py` lists in `__all__`, a dependency by design, plan
0002 Step 4), `ancestorOfMissing` (grimp gives an import of a module it does not know, missing
or in a folder it does not walk, to its nearest package; Rulebearing reports it unresolved or
resolves it to its file), else `unexplained`.

`dotnet`: each architecture test in the incumbent's TRX file (a test whose class is declared in
a file under the imported folder that uses ArchUnitNET or NetArchTest) beside the verdict of the
rules `rulebearing import archunit` named from its method (`kebab(method)`, then `-2`, `-3` for more
chains in the same method), read from the JUnit report where each rule is one test case. A
test whose chains the importer wrote commented out is `stays` (custom predicate) or
`not-imported` (with the importer's reason), recorded and not counted as a disagreement.

A rule Rulebearing reports as a JUnit `<error>` (vacuous, expired, a ratchet without a budget)
could not be checked, so a contract or test with one has the verdict `error`, never a pass or a
fail. A result where nothing was compared (every row stays, is not imported, or neither tool
has a verdict for it) is `nothing-compared`: reported, and never shown as agreement.

Exit 0 when something was compared and nothing disagrees or errors, 1 when something disagrees
or errors, 2 when the inputs are unusable, 3 when nothing was compared.
"""

from __future__ import annotations

import argparse
import ast
import json
import re
import subprocess
import sys
import xml.etree.ElementTree as ET
from pathlib import Path
from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:
    from collections.abc import Callable, Iterator

SAMPLE = 20
CONTRACT = "import-linter contract: "
ALLOWED = "not-in-allowed"
UNPROTECTED = "import-linter:unprotected"
NO_CONTRACT = "(violations of rules no contract names)"
Violation = tuple[str, str, str]  # (rule, from, to)
# The ways import-linter's graph differs from the imported rules' (compare.explain).
IGNORE = "ignore_imports"
TYPE_ONLY = "TYPE_CHECKING imports (exclude_type_checking_imports)"
NAMESPACE = "namespace portions below a root package"
OUTSIDE = "imports by modules outside the root packages"
TRX = "{http://microsoft.com/schemas/VisualStudio/TeamTest/2010}"
# The exit codes (module documentation).
DISAGREES = 1
UNUSABLE = 2
NOTHING_COMPARED = 3


def write(path: Path, document: dict[str, Any]) -> None:
    """Write a result file: sorted keys where order carries no meaning, two-space indent."""
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(document, indent=2, ensure_ascii=False) + "\n")


def outcome(summary: dict[str, int]) -> tuple[str, bool, int]:
    """The status, whether the result agrees, and the exit code, from the verdict counts.

    Agreement needs at least one agreeing row: a result whose rows all stay or are not imported
    compared nothing, which is `nothing-compared` and not agreement.
    """
    if summary["agree"] == 0 and summary["disagree"] == 0 and summary["error"] == 0:
        return "nothing-compared", False, NOTHING_COMPARED
    agrees = summary["disagree"] == 0 and summary["error"] == 0
    return "compared", agrees, 0 if agrees else DISAGREES


def summarise(rows: list[dict[str, Any]]) -> dict[str, int]:
    """Count the rows per verdict."""
    counts = {"agree": 0, "disagree": 0, "stays": 0, "not-imported": 0, "error": 0}
    for row in rows:
        counts[row["verdict"]] = counts.get(row["verdict"], 0) + 1
    return {"total": len(rows), **counts}


# Python -------------------------------------------------------------------------------------


def walk_rules(node: object) -> Iterator[dict[str, Any]]:
    """Every mapping in the rules tree that carries a rule name."""
    if isinstance(node, dict):
        if isinstance(node.get("name"), str):
            yield node
        for value in node.values():
            yield from walk_rules(value)
    elif isinstance(node, list):
        for item in node:
            yield from walk_rules(item)


def patterns(side: object) -> list[re.Pattern[str]]:
    """A rule side's `path` as compiled expressions."""
    if not isinstance(side, dict):
        return []
    value = side.get("path", [])
    return [re.compile(p) for p in ([value] if isinstance(value, str) else value)]


def contract_notes(text: str) -> dict[str, str]:
    """The reason the importer wrote under a contract's heading comment, by id and by name."""
    notes: dict[str, str] = {}
    heading = re.compile(r"^\s*# import-linter contract `([^`]+)` \([^)]*\): (.*)$")
    current: list[str] = []
    for line in text.splitlines():
        found = heading.match(line)
        if found:
            current = [found.group(1), found.group(2).strip()]
            continue
        stripped = line.strip()
        if not current or not stripped.startswith("#"):
            current = []
            continue
        body = stripped[1:].strip()
        if body and not stripped.startswith("#   ") and not body.startswith("- name:"):
            for key in current:
                notes.setdefault(key, body)
    return notes


class Attribution:
    """Which contract each imported rule, and so each violation, belongs to."""

    def __init__(self, imported: dict[str, Any]) -> None:
        """Read the rules' comments and the protected contracts' `allowed` rules."""
        self.by_rule: dict[str, str] = {}
        self.rules_per_contract: dict[str, int] = {}
        self.protected: list[tuple[str, list[re.Pattern[str]]]] = []
        rules = imported.get("rules", {}) or {}
        for rule in walk_rules(rules):
            comment = str(rule.get("comment", ""))
            if comment.startswith(CONTRACT):
                contract = comment[len(CONTRACT) :]
                self.by_rule[rule["name"]] = contract
                self.rules_per_contract[contract] = self.rules_per_contract.get(contract, 0) + 1
        for rule in (rules.get("dependencies", {}) or {}).get("allowed", []) or []:
            comment = str(rule.get("comment", ""))
            if rule.get("name") != UNPROTECTED and comment.startswith(CONTRACT):
                self.protected.append((comment[len(CONTRACT) :], patterns(rule.get("to"))))

    def rule(self, case: str) -> str:
        """The rule a JUnit case is for: its name, less the `#n` the reporter adds to a repeat."""
        if case in self.by_rule or case == ALLOWED:
            return case
        base = re.sub(r"#\d+\Z", "", case)
        return base if base in self.by_rule or base == ALLOWED else case

    def violations(self, report: JUnit) -> dict[str, list[Violation]]:
        """The error-severity violations of a JUnit report, by contract name."""
        out: dict[str, list[Violation]] = {}
        for violation in report.violations:
            name = self.rule(violation[0])
            owners = [self.by_rule[name]] if name in self.by_rule else []
            if name == ALLOWED:
                owners = [c for c, ps in self.protected if any(p.search(violation[2]) for p in ps)]
            for owner in owners or [NO_CONTRACT]:
                out.setdefault(owner, []).append(violation)
        return out


class JUnit:
    """A `cruise -T junit` report: each failing rule's violations and each rule that errored.

    The report is used rather than `-T json` because it carries one line per violation and no
    chain: on an oracle with a hundred thousand reachability violations the JSON result, with a
    path per violation in the modules and again in the summary, runs to gigabytes.
    """

    def __init__(self, path: Path) -> None:
        """Parse the report."""
        self.violations: list[Violation] = []
        self.errors: dict[str, list[str]] = {}
        root = ET.parse(path).getroot()  # noqa: S314 (the file is Rulebearing's own output)
        # `[RB-id ]from -> to[ (line N, column M)][ [known]]`: the position and the known marker
        # are the reporter's, not part of the module name a protected contract's pattern matches.
        line = re.compile(
            r"^(?:RB-\S+ )?(.+?) -> (.+?)(?: \(line \d+, column \d+\))?(?: \[known\])?$"
        )
        for case in root.iter("testcase"):
            name = case.get("name", "")
            for failure in case.findall("failure"):
                if failure.get("type") != "error":
                    continue
                for text in (failure.text or "").splitlines():
                    found = line.match(text.strip())
                    if found:
                        self.violations.append((name, found.group(1), found.group(2)))
            for error in case.findall("error"):
                self.errors.setdefault(name, []).append(error.get("type", "error"))


def module_pattern(expression: str) -> re.Pattern[str]:
    """An import-linter module expression (`a.*.b`, `a.**`) as a pattern over dotted names."""
    parts = []
    for part in expression.strip().split("."):
        if part == "**":
            parts.append(r"[^.]+(?:\.[^.]+)*")
        elif part == "*":
            parts.append(r"[^.]+")
        else:
            parts.append(re.escape(part))
    return re.compile(r"\.".join(parts) + r"\Z")


def ignored_edges(contract: dict[str, Any], incumbent: dict[str, Any]) -> set[tuple[str, str]]:
    """A contract's `ignore_imports` as (importer file, imported file or external name) pairs."""
    modules: dict[str, str | None] = incumbent["modules"]
    edges = set()
    for entry in contract.get("ignoreImports", []):
        importer, _, imported = entry.partition("->")
        source_pattern, target_pattern = module_pattern(importer), module_pattern(imported)
        sources = [m for m in modules if source_pattern.match(m)]
        targets = [m for m in modules if target_pattern.match(m)]
        if not targets and "*" not in imported:
            targets = [imported.strip()]
        for source in sources:
            for target in targets:
                edges.add((modules.get(source) or source, modules.get(target) or target))
    return edges


def without(
    graph: dict[str, Any],
    edges: set[tuple[str, str]],
    unwalked: set[str],
    outside: set[str],
    *,
    type_only: bool,
) -> dict[str, Any]:
    """The graph document without the imports import-linter's graph does not have.

    These are the ignored imports, type-only ones when asked, every import of or by a file
    grimp does not walk, and every import by a file outside the root packages.
    """
    document: dict[str, Any] = json.loads(json.dumps(graph))
    by_importer: dict[str, set[str]] = {}
    for importer, imported in edges:
        by_importer.setdefault(importer, set()).add(imported)
    for module in document["modules"]:
        ignored = by_importer.get(module["source"], set())
        kept = []
        for dependency in module.get("dependencies", []):
            resolved = dependency["resolved"]
            # An ignored external package covers its submodules, as grimp squashes them.
            parts = resolved.split(".")
            dropped = (
                any(".".join(parts[:n]) in ignored for n in range(1, len(parts) + 1))
                or module["source"] in unwalked
                or resolved in unwalked
                or module["source"] in outside
            )
            if not dropped and not (type_only and "type-only" in dependency["dependencyTypes"]):
                kept.append(dependency)
        module["dependencies"] = kept
    return document


def outside_files(incumbent: dict[str, Any], graph: dict[str, Any]) -> set[str]:
    """Python files outside the root packages, which import-linter's graph never leaves from.

    grimp builds the graph of the root packages only; any other module, even one in the same
    repository, is at most an external package, squashed and with no imports of its own, so no
    chain import-linter follows passes through it.
    """
    inside = root_inside(incumbent)
    return {
        m["source"]
        for m in graph["modules"]
        if m.get("language") == "python" and not inside(m["source"])
    }


def unwalked_files(incumbent: dict[str, Any], graph: dict[str, Any]) -> set[str]:
    """Python files under the root packages that grimp does not read as modules.

    grimp walks a root package's regular subpackages only: a folder without `__init__.py` below
    it (a namespace portion) is not analysed unless it is a root package itself, so no chain
    import-linter follows passes through it.
    """
    inside = root_inside(incumbent)
    known = {f for f in incumbent["modules"].values() if f is not None}
    return {
        m["source"]
        for m in graph["modules"]
        if m.get("language") == "python" and inside(m["source"]) and m["source"] not in known
    }


def junit_run(context: dict[str, Any], graph: Path, out: Path) -> int:
    """`rulebearing cruise --graph` with the imported rules, the JUnit report written to `out`."""
    with out.open("w") as sink:
        return subprocess.run(  # noqa: S603 (the harness's own binary, arguments it built)
            [
                context["bin"],
                "cruise",
                "--config",
                context["config"],
                "--graph",
                str(graph),
                "-T",
                "junit",
                "--no-progress",
            ],
            cwd=context["cwd"],
            stdout=sink,
            stderr=subprocess.DEVNULL,
            check=False,
        ).returncode


def explain(
    row: dict[str, Any],
    contract: dict[str, Any],
    context: dict[str, Any],
) -> None:
    """For a disagreeing contract, whether import-linter's graph filters account for it.

    import-linter removes each `ignore_imports` import, and with
    `exclude_type_checking_imports` every `TYPE_CHECKING` import, from the graph before it
    follows chains; grimp does not read a namespace portion below a root package, and a module
    outside the root packages has no imports in its graph. The import expresses none of these.
    The contract is re-checked by `rulebearing cruise --graph` over Rulebearing's own graph with
    the same imports removed.
    """
    incumbent = context["incumbent"]
    edges = ignored_edges(contract, incumbent)
    filters = {
        IGNORE: bool(edges),
        TYPE_ONLY: bool(incumbent["excludeTypeCheckingImports"]),
        NAMESPACE: bool(unwalked_files(incumbent, context["graph"])),
        OUTSIDE: bool(outside_files(incumbent, context["graph"])),
    }
    mechanisms = [name for name, applies in filters.items() if applies]
    if not mechanisms or context.get("bin") is None:
        return
    index = context["incumbent"]["contracts"].index(contract)
    left = recheck(row, contract, context, set(mechanisms), f"{index}-all")
    if left is None:
        return
    row["filtered"] = {"mechanisms": mechanisms, "violations": left}
    if left != 0:
        return
    # Which filter accounts for it on its own, when there is more than one.
    alone = mechanisms
    if len(mechanisms) > 1:
        alone = [
            m
            for m in mechanisms
            if recheck(row, contract, context, {m}, f"{index}-{mechanisms.index(m)}") == 0
        ]
    row["filtered"]["sufficientAlone"] = alone
    which = ", ".join(alone) if alone else f"{', '.join(mechanisms)}, together"
    row["cause"] = (
        f"import-linter's graph has no {which}; with the same imports removed from "
        "Rulebearing's graph the contract is kept too. The import writes ignore_imports as "
        "knownViolations, which excuse a violation but cut no chain, and has no form for the "
        "other filters"
    )


def recheck(
    row: dict[str, Any],
    contract: dict[str, Any],
    context: dict[str, Any],
    active: set[str],
    stem: str,
) -> int | None:
    """The contract's violations over Rulebearing's graph with these filters applied."""
    incumbent = context["incumbent"]
    filtered = without(
        context["graph"],
        ignored_edges(contract, incumbent) if IGNORE in active else set(),
        unwalked_files(incumbent, context["graph"]) if NAMESPACE in active else set(),
        outside_files(incumbent, context["graph"]) if OUTSIDE in active else set(),
        type_only=TYPE_ONLY in active,
    )
    work = Path(context["work"])
    graph = work / f"recheck-{stem}.json"
    graph.write_text(json.dumps(filtered))
    report = work / f"recheck-{stem}.xml"
    status = junit_run(context, graph, report)
    try:
        rerun = JUnit(report)
    except ET.ParseError:
        row["cause"] = f"not diagnosed: the re-check exited {status} with no JUnit report"
        return None
    return len(context["attribution"].violations(rerun).get(row["name"], []))


def root_inside(incumbent: dict[str, Any]) -> Callable[[str], bool]:
    """Whether a file lies in one of the root packages' folders (or is a single-file root)."""
    folders = [f for f in incumbent.get("rootFolders", {}).values() if f]
    return lambda path: any(path == f or (f.endswith("/") and path.startswith(f)) for f in folders)


def our_edges(
    incumbent: dict[str, Any], cruise: dict[str, Any]
) -> tuple[dict[tuple[str, str], set[str]], dict[str, list[tuple[str, str]]]]:
    """Rulebearing's local edges under the roots with their types, and each file's imports.

    The second map holds, per file, each import's module name and what it resolved to, for
    telling apart the edges grimp gives to a package because it does not know the module.
    """
    inside = root_inside(incumbent)
    skip_type_only = bool(incumbent["excludeTypeCheckingImports"])
    # The folder each root package's folder sits in (`src/` in a src layout), for module names.
    prefixes = sorted(
        {
            folder.removesuffix(package.replace(".", "/") + "/")
            for package, folder in incumbent.get("rootFolders", {}).items()
            if folder and folder.endswith(package.replace(".", "/") + "/")
        },
        key=len,
        reverse=True,
    )
    types: dict[tuple[str, str], set[str]] = {}
    unresolved: dict[str, list[tuple[str, str]]] = {}
    for module in cruise["modules"]:
        if module.get("language") != "python" or not inside(module["source"]):
            continue
        for dependency in module.get("dependencies", []):
            edge = (module["source"], dependency["resolved"])
            kinds = set(dependency["dependencyTypes"])
            unresolved.setdefault(module["source"], []).append(
                (dotted(module["source"], dependency, prefixes), dependency["resolved"])
            )
            local = "local" in kinds and inside(edge[1]) and edge[0] != edge[1]
            if local and not (skip_type_only and kinds == {"local", "type-only"}):
                types.setdefault(edge, set()).update(kinds)
    return types, unresolved


def module_name(path: str, prefixes: list[str]) -> str:
    """The dotted module name of a Python file under a root."""
    for prefix in prefixes:
        if path.startswith(prefix):
            path = path[len(prefix) :]
            break
    return path.removesuffix(".py").removesuffix("/__init__").replace("/", ".")


def dotted(source: str, dependency: dict[str, Any], prefixes: list[str]) -> str:
    """An import's absolute module name.

    From the file it resolved to, else as written, a relative import made absolute against the
    importing file's package.
    """
    resolved = str(dependency["resolved"])
    if resolved.endswith(".py"):
        return module_name(resolved, prefixes)
    written = str(dependency["module"])
    level = len(written) - len(written.lstrip("."))
    if level == 0:
        return written
    package = module_name(source, prefixes).split(".")
    if not source.endswith("__init__.py"):
        package = package[:-1]
    base = package[: len(package) - (level - 1)] if level > 1 else package
    rest = written[level:]
    return ".".join([*base, rest] if rest else base)


def kinds_summary(groups: dict[str, list[tuple[str, str]]]) -> dict[str, Any]:
    """Each kind of one-sided edge with its count and a sample."""
    return {
        k: {"count": len(v), "sample": [list(e) for e in v[:SAMPLE]]}
        for k, v in sorted(groups.items())
    }


def python_graph(incumbent: dict[str, Any], cruise: dict[str, Any], cwd: Path) -> dict[str, Any]:
    """The graph comparison: grimp's direct imports beside Rulebearing's local edges."""
    modules: dict[str, str | None] = incumbent["modules"]
    by_file = {f: m for m, f in modules.items() if f is not None}
    theirs = {
        (str(modules[a]), str(modules[b]))
        for a, b in incumbent["edges"]
        if modules.get(a) is not None and modules.get(b) is not None and a != b
    }
    types, unresolved = our_edges(incumbent, cruise)
    ours = set(types)
    # Edges Rulebearing has for reasons the design records: a file in a folder grimp does not
    # walk (no __init__.py), a literal importlib call, a submodule an __init__.py's __all__ names.
    only_ours: dict[str, list[tuple[str, str]]] = {}
    for edge in sorted(ours - theirs):
        if edge[0] not in by_file or edge[1] not in by_file:
            kind = "notGrimpModule"
        elif "dynamic" in types[edge]:
            kind = "dynamic"
        elif edge[0].endswith("__init__.py") and in_all(cwd / edge[0], edge[1]):
            kind = "allReexport"
        else:
            kind = "unexplained"
        only_ours.setdefault(kind, []).append(edge)
    # Edges grimp has to a package where the import names a module grimp does not know: missing
    # (a generated `_version`, a deleted module), which Rulebearing reports unresolved, or in a
    # folder grimp does not walk, which Rulebearing resolves to its file. grimp gives the import
    # to the nearest package it knows.
    only_theirs: dict[str, list[tuple[str, str]]] = {}
    for edge in sorted(theirs - ours):
        target = by_file.get(edge[1], "")
        missing = any(
            target and name.startswith(target + ".") and resolved not in by_file
            for name, resolved in unresolved.get(edge[0], [])
        )
        only_theirs.setdefault("ancestorOfMissing" if missing else "unexplained", []).append(edge)
    unexplained = len(only_ours.get("unexplained", [])) + len(only_theirs.get("unexplained", []))
    return {
        "rootPackages": incumbent["rootPackages"],
        "importLinter": len(theirs),
        "rulebearing": len(ours),
        "equal": theirs == ours,
        "unexplained": unexplained,
        "onlyRulebearing": kinds_summary(only_ours),
        "onlyImportLinter": kinds_summary(only_theirs),
    }


def in_all(init: Path, target: str) -> bool:
    """Whether the package's __init__.py lists the target's module name in `__all__`."""
    try:
        tree = ast.parse(init.read_text(errors="replace"))
    except (OSError, SyntaxError, ValueError):
        return False
    stem = target.removesuffix("/__init__.py").removesuffix(".py").rsplit("/", 1)[-1]
    for node in ast.walk(tree):
        targets = []
        if isinstance(node, ast.Assign):
            targets, value = node.targets, node.value
        elif isinstance(node, (ast.AnnAssign, ast.AugAssign)) and node.value is not None:
            targets, value = [node.target], node.value
        if any(isinstance(t, ast.Name) and t.id == "__all__" for t in targets) and isinstance(
            value, (ast.List, ast.Tuple)
        ):
            names = [e.value for e in value.elts if isinstance(e, ast.Constant)]
            if stem in names:
                return True
    return False


def rulebearing_errors(
    attribution: Attribution, report: JUnit
) -> tuple[dict[str, list[str]], dict[str, list[str]]]:
    """Per contract, its vacuous rules, and every `<error>` of its rules as `rule (kind)`."""
    vacuous: dict[str, list[str]] = {}
    errored: dict[str, list[str]] = {}
    for case, kinds in report.errors.items():
        rule = attribution.rule(case)
        owner = attribution.by_rule.get(rule, NO_CONTRACT)
        if "vacuous" in kinds:
            vacuous.setdefault(owner, []).append(rule)
        errored.setdefault(owner, []).extend(f"{rule} ({kind})" for kind in kinds)
    return vacuous, errored


def python_rows(
    incumbent: dict[str, Any],
    imported_text: str,
    context: dict[str, Any],
) -> list[dict[str, Any]]:
    """One row per contract."""
    attribution: Attribution = context["attribution"]
    violations = attribution.violations(context["junit"])
    vacuous, errored = rulebearing_errors(attribution, context["junit"])
    notes = contract_notes(imported_text)
    rows = []
    for contract in incumbent["contracts"]:
        name = contract["name"]
        found = violations.get(name, [])
        row: dict[str, Any] = {
            "id": contract["id"],
            "name": name,
            "type": contract["type"],
            "importLinter": contract["importLinter"],
            "rules": attribution.rules_per_contract.get(name, 0),
            "violations": len(found),
        }
        note = notes.get(str(contract["id"]), notes.get(name, ""))
        if row["rules"] == 0:
            row["rulebearing"] = None
            row["verdict"] = "stays" if note.startswith("stays in") else "not-imported"
            row["reason"] = note or "the importer wrote no rule for this contract"
        elif contract["importLinter"] == "error":
            row["rulebearing"] = "broken" if found else "kept"
            row["verdict"] = "error"
            row["reason"] = contract.get("detail", "")
        elif name in errored:
            # A rule Rulebearing could not check has no verdict to compare.
            row["rulebearing"] = "error"
            row["verdict"] = "error"
            row["reason"] = f"rulebearing could not check {', '.join(sorted(errored[name]))}"
        else:
            row["rulebearing"] = "broken" if found else "kept"
            agree = row["rulebearing"] == contract["importLinter"]
            row["verdict"] = "agree" if agree else "disagree"
            if not agree and found:
                explain(row, contract, context)
        if name in vacuous and row["rules"]:
            row["vacuousRules"] = sorted(vacuous[name])
        if found:
            row["sample"] = sorted({f"{r}: {a} -> {b}" for r, a, b in found})[:SAMPLE]
        rows.append(row)
    stray = violations.get(NO_CONTRACT, [])
    if stray:
        rows.append(
            {
                "id": None,
                "name": NO_CONTRACT,
                "type": "",
                "importLinter": None,
                "rules": 0,
                "violations": len(stray),
                "rulebearing": "broken",
                "verdict": "disagree",
                "sample": sorted({f"{r}: {a} -> {b}" for r, a, b in stray})[:SAMPLE],
            }
        )
    return rows


def python_main(args: argparse.Namespace) -> int:
    """Compare one Python oracle."""
    import yaml  # noqa: PLC0415 (only the python mode needs it; table.py's CI job has no PyYAML)

    incumbent = json.loads(Path(args.incumbent).read_text())
    imported_text = Path(args.imported).read_text()
    imported = yaml.safe_load(imported_text) or {}
    graph = json.loads(Path(args.graph).read_text())
    context = {
        "incumbent": incumbent,
        "graph": graph,
        "junit": JUnit(Path(args.junit)),
        "attribution": Attribution(imported),
        "bin": args.rulebearing,
        "config": args.imported,
        "cwd": args.cwd,
        "work": args.work or str(Path(args.junit).parent),
    }
    rows = python_rows(incumbent, imported_text, context)
    comparison = python_graph(incumbent, graph, Path(args.cwd))
    summary = summarise(rows)
    status, agrees, code = outcome(summary)
    document = {
        "repo": args.repo,
        "sha": args.sha,
        "tool": "import-linter",
        "importLinterVersion": incumbent["importLinterVersion"],
        "settings": args.settings,
        "config": args.config_kind,
        "status": status,
        "summary": summary,
        "contracts": rows,
        "graph": comparison,
        "agrees": agrees,
    }
    if status == "nothing-compared":
        document["detail"] = "every contract stays or is not imported: nothing to compare"

    write(Path(args.out), document)
    sys.stdout.write(
        f"python-oracle: {args.repo}: {summary['total']} contracts, {summary['agree']} agree, "
        f"{summary['disagree']} disagree, {summary['stays']} stay, "
        f"{summary['not-imported']} not imported, {summary['error']} error; graph "
        f"{comparison['importLinter']} vs {comparison['rulebearing']} edges, "
        f"{comparison['unexplained']} unexplained; {status}\n"
    )
    return code


# .NET ---------------------------------------------------------------------------------------


def kebab(name: str) -> str:
    """The importer's rule name for a test method (crates/rb-cli/src/cmd/import/archunit.rs)."""
    out: list[str] = []
    for i, c in enumerate(name):
        if c in "_- ":
            if out and out[-1] != "-":
                out.append("-")
            continue
        if c.isupper() and i > 0:
            previous = name[i - 1]
            next_lower = i + 1 < len(name) and name[i + 1].islower()
            if (
                previous.islower() or previous.isdigit() or (previous.isupper() and next_lower)
            ) and (not out or out[-1] != "-"):
                out.append("-")
        out.append(c.lower())
    return "".join(out).strip("-")


def split_top(text: str) -> list[str]:
    """Split on commas outside quotes and brackets."""
    parts, depth, quote, current = [], 0, "", []
    for c in text:
        if quote:
            current.append(c)
            if c == quote:
                quote = ""
        elif c in "\"'":
            quote = c
            current.append(c)
        elif c in "([{":
            depth += 1
            current.append(c)
        elif c in ")]}":
            depth -= 1
            current.append(c)
        elif c == "," and depth == 0:
            parts.append("".join(current).strip())
            current = []
        else:
            current.append(c)
    if "".join(current).strip():
        parts.append("".join(current).strip())
    return parts


def literal(value: str) -> str:
    """A data-row value as a comparable string: quotes, `typeof`, `nameof` and suffixes dropped."""
    value = value.strip()
    found = re.fullmatch(r"(?:typeof|nameof)\((.*)\)", value)
    if found:
        value = found.group(1).rsplit(".", 1)[-1]
    if len(value) >= 2 and value[0] == value[-1] and value[0] in "\"'":  # noqa: PLR2004
        return value[1:-1]
    return value.rstrip("mMfFdDlLuU") if re.fullmatch(r"-?\d[\d.]*[mMfFdDlLuU]?", value) else value


def row_values(row: str) -> list[str]:
    """`[InlineData("a", 1)]` as ["a", "1"]."""
    found = re.search(r"\((.*)\)\s*\]?\s*$", row)
    return [literal(v) for v in split_top(found.group(1))] if found else []


def trx_values(display: str) -> list[str]:
    """`Ns.Class.Method(a: "x", b: 1)` as ["x", "1"]."""
    if "(" not in display or not display.endswith(")"):
        return []
    inner = display[display.index("(") + 1 : -1]
    return [literal(p.split(":", 1)[1] if ":" in p else p) for p in split_top(inner)]


def imported_rules(text: str) -> list[dict[str, Any]]:
    """Every rule the importer wrote, active or commented out, with its file, row and reason."""
    rules: list[dict[str, Any]] = []
    pending: list[str] = []
    name_line = re.compile(r"^\s*(#\s)?\s*- name: (.+?)\s*$")
    for line in text.splitlines():
        found = name_line.match(line)
        if found:
            reason = ""
            row = None
            for note in pending:
                body = note.strip().lstrip("#").strip()
                if body.startswith(("stays in ", "not imported: ")):
                    reason = body
                if body.startswith("with "):
                    row = body[len("with ") :]
            rules.append(
                {
                    "name": found.group(2).strip("\"'"),
                    "active": found.group(1) is None,
                    "row": row,
                    "reason": reason,
                    "file": None,
                }
            )
            pending = []
            continue
        file_line = re.match(r'^\s*#?\s*comment: "imported from (.+):(\d+)"', line)
        if file_line and rules and rules[-1]["file"] is None:
            rules[-1]["file"] = file_line.group(1)
            continue
        if line.lstrip().startswith("#"):
            pending.append(line)
    return rules


def junit_verdicts(path: Path) -> dict[str, str]:
    """Each rule's verdict in the JUnit report: pass, fail (a failure) or error (vacuous, ...).

    An `<error>` wins over a `<failure>`: a rule that could not be checked has no pass or fail.
    """
    verdicts: dict[str, str] = {}
    root = ET.parse(path).getroot()  # noqa: S314 (the file is Rulebearing's own output)
    for case in root.iter("testcase"):
        name = case.get("name", "")
        if case.find("error") is not None:
            verdicts[name] = "error"
        elif case.find("failure") is not None:
            verdicts[name] = "fail"
        else:
            verdicts[name] = "pass"
    return verdicts


def trx_tests(path: Path) -> list[dict[str, str]]:
    """Each test result in a TRX file: class, method, display name and outcome."""
    root = ET.parse(path).getroot()  # noqa: S314 (the file is dotnet test's output on this runner)
    methods: dict[str, tuple[str, str]] = {}
    for test in root.iter(f"{TRX}UnitTest"):
        element = test.find(f"{TRX}TestMethod")
        if element is not None:
            methods[test.get("id", "")] = (element.get("className", ""), element.get("name", ""))
    tests = []
    for result in root.iter(f"{TRX}UnitTestResult"):
        class_name, method = methods.get(result.get("testId", ""), ("", ""))
        display = result.get("testName", "")
        if not method:
            method = display.split("(", 1)[0].rsplit(".", 1)[-1]
        # xUnit writes the method with its data row; MSTest and NUnit write the bare name.
        method = method.split("(", 1)[0].rsplit(".", 1)[-1]
        tests.append(
            {
                "class": class_name.split(",", 1)[0].strip(),
                "method": method,
                "test": display,
                "outcome": result.get("outcome", ""),
            }
        )
    return sorted(tests, key=lambda t: (t["class"], t["test"]))


def class_files(tests_dir: Path, cwd: Path) -> dict[str, set[str]]:
    """The architecture-test files under the test folder that declare each class, by name.

    An architecture test is one whose file uses ArchUnitNET or NetArchTest, or any file of a
    folder where a `global using` brings one in: a test project also holds unit tests, and a
    repository can name the libraries only as data (a package whose licence it checks), so a
    test in any other file is out of scope.
    """
    declared: dict[str, set[str]] = {}
    pattern = re.compile(r"\b(?:class|record)\s+([A-Za-z_]\w*)")
    uses = re.compile(r"\b(?:using\s+(?:static\s+)?|global::)(?:ArchUnitNET|NetArchTest)\b")
    files = [
        (path, path.read_text(errors="replace"))
        for path in sorted(tests_dir.rglob("*.cs"))
        if not {"bin", "obj"} & set(path.relative_to(tests_dir).parts)
    ]
    everywhere = any(re.search(r"\bglobal\s+" + uses.pattern, text) for _, text in files)
    for path, text in files:
        if not everywhere and not uses.search(text):
            continue
        shown = path.resolve().relative_to(cwd).as_posix()
        for name in pattern.findall(text):
            declared.setdefault(name, set()).add(shown)
    return declared


def rules_for(
    test: dict[str, str],
    rules: list[dict[str, Any]],
    declared: dict[str, set[str]],
) -> list[dict[str, Any]]:
    """The rules the importer named from this test's method, in its class's files."""
    base = kebab(test["method"])
    named = [r for r in rules if re.fullmatch(re.escape(base) + r"(-\d+)?", r["name"])]
    simple = re.split(r"[.+]", test["class"])[-1]
    files = declared.get(simple, set())
    in_class = [r for r in named if r["file"] in files]
    if in_class:
        named = in_class
    elif len({r["file"] for r in named}) > 1:
        return []
    with_rows = [r for r in named if r["row"]]
    wanted = trx_values(test["test"])
    if with_rows and wanted:
        same_row = [r for r in with_rows if row_values(r["row"]) == wanted]
        if same_row:
            return same_row
    return named


def dotnet_row(
    test: dict[str, str], rules: list[dict[str, Any]], verdicts: dict[str, str]
) -> dict[str, Any]:
    """One test's row."""
    outcome = test["outcome"].lower()
    row: dict[str, Any] = {
        "test": test["test"],
        "dotnet": outcome,
        "rules": [r["name"] for r in rules],
    }
    active = [r for r in rules if r["active"]]
    inactive = [r for r in rules if not r["active"]]
    if outcome not in ("passed", "failed"):
        row.update(rulebearing=None, verdict="not-imported", reason=f"dotnet test: {outcome}")
        return row
    if not rules:
        row.update(
            rulebearing=None,
            verdict="not-imported",
            reason="no fluent rule the importer reads (the test queries the architecture in C#)",
        )
        return row
    if not active:
        reason = next((r["reason"] for r in inactive if r["reason"].startswith("stays")), "")
        reason = reason or next((r["reason"] for r in inactive if r["reason"]), "")
        verdict = "stays" if reason.startswith("stays") else "not-imported"
        row.update(rulebearing=None, verdict=verdict, reason=reason)
        return row
    missing = [r["name"] for r in active if r["name"] not in verdicts]
    if missing:
        row.update(
            rulebearing=None, verdict="error", reason=f"no JUnit case for {', '.join(missing)}"
        )
        return row
    errored = [r["name"] for r in active if verdicts[r["name"]] == "error"]
    if errored:
        row.update(
            rulebearing="error",
            verdict="error",
            reason=f"rulebearing could not check {', '.join(errored)} (a JUnit <error>)",
        )
        return row
    failing = [r["name"] for r in active if verdicts[r["name"]] == "fail"]
    row["rulebearing"] = "failed" if failing else "passed"
    if failing:
        row["failing"] = failing
    if inactive and outcome == "failed" and not failing:
        row["verdict"] = "not-imported"
        row["reason"] = (
            "the test fails and the chains the importer translated pass; the rest are not "
            f"imported ({inactive[0]['reason']})"
        )
    else:
        row["verdict"] = "agree" if row["rulebearing"] == outcome else "disagree"
    return row


def dotnet_main(args: argparse.Namespace) -> int:
    """Compare one .NET oracle."""
    cwd = Path(args.cwd).resolve()
    rules = imported_rules(Path(args.imported).read_text())
    verdicts = junit_verdicts(Path(args.junit)) if Path(args.junit).is_file() else {}
    tests = trx_tests(Path(args.trx))
    declared = class_files(Path(args.tests_dir).resolve(), cwd)
    rows: list[dict[str, Any]] = []
    used: set[str] = set()
    out_of_scope = 0
    for test in tests:
        simple = re.split(r"[.+]", test["class"])[-1]
        if simple not in declared:
            out_of_scope += 1
            continue
        matched = rules_for(test, rules, declared)
        used.update(r["name"] for r in matched)
        rows.append(dotnet_row(test, matched, verdicts))
    summary = summarise(rows)
    unmatched = sorted(r["name"] for r in rules if r["active"] and r["name"] not in used)
    status, agrees, code = outcome(summary)
    detail = ""
    if status == "nothing-compared":
        detail = "every test stays or is not imported: nothing to compare"
    if not tests:
        status, detail = "error", "dotnet test ran no tests (see incumbent.log)"
    elif not rows:
        status = "error"
        detail = (
            f"no test dotnet test ran is in a file under {args.tests_shown} that uses "
            "ArchUnitNET or NetArchTest: nothing to compare"
        )
    document = {
        "repo": args.repo,
        "sha": args.sha,
        "tool": args.tool,
        "tests": args.tests_shown,
        "status": status,
        "detail": detail,
        "summary": summary,
        "rules": {
            "imported": sum(1 for r in rules if r["active"]),
            "commentedOut": sum(1 for r in rules if not r["active"]),
            "withoutTest": unmatched,
        },
        "outOfScope": out_of_scope,
        "results": rows,
        "agrees": status == "compared" and agrees,
    }
    write(Path(args.out), document)
    sys.stdout.write(
        f"dotnet-oracle: {args.repo}: {status}{' (' + detail + ')' if detail else ''}; "
        f"{summary['total']} tests, {summary['agree']} agree, {summary['disagree']} disagree, "
        f"{summary['stays']} stay, {summary['not-imported']} not imported, "
        f"{summary['error']} error\n"
    )
    return UNUSABLE if status == "error" else code


def main() -> int:
    """Parse the arguments and run one mode."""
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n", 1)[0])
    modes = parser.add_subparsers(dest="mode", required=True)
    python = modes.add_parser("python", help="one import-linter oracle")
    for flag in ("incumbent", "imported", "junit", "graph", "repo", "sha", "settings", "out"):
        python.add_argument(f"--{flag}", required=True)
    python.add_argument("--config-kind", default="imported")
    python.add_argument("--cwd", default=".", help="the folder cruise ran in")
    python.add_argument("--rulebearing", help="the binary, to re-check a disagreeing contract")
    python.add_argument("--work", help="where the re-check writes its graphs")
    dotnet = modes.add_parser("dotnet", help="one NetArchTest or ArchUnitNET oracle")
    for flag in ("trx", "imported", "junit", "tests-dir", "tests-shown", "cwd", "repo", "sha"):
        dotnet.add_argument(f"--{flag}", required=True)
    dotnet.add_argument("--tool", required=True)
    dotnet.add_argument("--out", required=True)
    args = parser.parse_args()
    return python_main(args) if args.mode == "python" else dotnet_main(args)


if __name__ == "__main__":
    sys.exit(main())
