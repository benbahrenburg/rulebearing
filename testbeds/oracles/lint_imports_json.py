# Copyright (c) 2026 Ben Bahrenburg. MIT licence: see LICENSE.
"""import-linter's verdict on each contract, and grimp's import graph, as JSON.

Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 11 (the oracle
harness) and section 1.4.5. Requirement: docs/prd.md#nfr-conf-03. Run by
testbeds/oracles/python.sh with the virtual environment's interpreter, in the folder that holds
the settings, with the repository's roots on PYTHONPATH.

It is `lint-imports` through import-linter's public use cases (`read_user_options`,
`create_report`), so each contract's verdict is read from the report instead of from the
rendered text. The one difference: a custom contract type whose module cannot be imported (a
plugin with a dependency the oracle's environment lacks) is recorded as an error for its own
contracts, and the other contracts still run, where `lint-imports` would stop at the first.

Usage: python lint_imports_json.py --roots DIR [--roots DIR ...] --out FILE
"""

from __future__ import annotations

import argparse
import importlib
import json
import sys
from importlib import metadata
from pathlib import Path
from typing import Any

from importlinter import configuration
from importlinter.application import use_cases
from importlinter.contracts.acyclic_siblings import AcyclicSiblingsContract
from importlinter.contracts.forbidden import ForbiddenContract
from importlinter.contracts.independence import IndependenceContract
from importlinter.contracts.layers import LayersContract
from importlinter.contracts.protected import ProtectedContract
from importlinter.domain.contract import Contract, registry

BUILT_IN: dict[str, type[Contract]] = {
    "forbidden": ForbiddenContract,
    "layers": LayersContract,
    "independence": IndependenceContract,
    "protected": ProtectedContract,
    "acyclic_siblings": AcyclicSiblingsContract,
}


def register(session: dict[str, Any]) -> dict[str, str]:
    """Register the built-in and plugin contract types; return the types that failed, and why."""
    for name, contract_class in BUILT_IN.items():
        registry.register(contract_class, name)
    failed: dict[str, str] = {}
    for entry in session.get("contract_types", []):
        name, _, dotted = (part.strip() for part in str(entry).partition(":"))
        module_name, _, class_name = dotted.rpartition(".")
        try:
            contract_class = getattr(importlib.import_module(module_name), class_name)
        except (ImportError, AttributeError) as error:
            failed[name] = f"contract type `{dotted}` cannot be imported: {error}"
            continue
        registry.register(contract_class, name)
    return failed


def module_file(module: str, roots: list[Path], base: Path) -> str | None:
    """The file of a module under one of the roots, relative to the base folder, else None."""
    parts = module.split(".")
    for root in roots:
        stem = root.joinpath(*parts)
        for candidate in (stem / "__init__.py", stem.with_suffix(".py")):
            if candidate.is_file():
                return candidate.resolve().relative_to(base).as_posix()
    return None


def root_folder(package: str, roots: list[Path], base: Path) -> str | None:
    """The folder (or single-file module) of a root package, relative to the base folder."""
    for root in roots:
        folder = root / package
        if folder.is_dir():
            return folder.resolve().relative_to(base).as_posix() + "/"
        if folder.with_suffix(".py").is_file():
            return folder.with_suffix(".py").resolve().relative_to(base).as_posix()
    return None


def flag(session: dict[str, Any], key: str) -> bool:
    """A session option import-linter reads as a boolean string."""
    return str(session.get(key, "")) in ("True", "true")


def main() -> int:
    """Write the report; exit 0 when it was written, 2 when import-linter could not run."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--roots", action="append", default=[], help="a folder on PYTHONPATH")
    parser.add_argument("--out", required=True, help="the JSON file to write")
    args = parser.parse_args()
    base = Path.cwd().resolve()
    roots = [Path(r).resolve() for r in args.roots] or [base]

    configuration.configure()
    options = use_cases.read_user_options()
    session = options.session_options
    failed = register(session)
    contracts = options.contracts_options
    runnable = [c for c in contracts if c["type"] not in failed]
    limit: tuple[str, ...] = ()
    if len(runnable) < len(contracts):
        limit = tuple(str(c["id"]) for c in runnable if "id" in c)
    results: dict[str, dict[str, Any]] = {}
    graph = None
    if runnable and (not failed or limit):
        report = use_cases.create_report(options, limit_to_contracts=limit, cache_dir=None)
        graph = report.graph
        for contract, check in report.get_contracts_and_checks():
            results[contract.name] = {
                "importLinter": "kept" if check.kept else "broken",
                "ignoredImports": check.ignored_import_count,
                "warnings": list(check.warnings),
                "metadata": check.metadata,
            }
        for name, error in report.invalid_contract_options.items():
            results[name] = {"importLinter": "error", "detail": f"invalid options: {error}"}

    rows = []
    for contract in contracts:
        ignored = contract.get("ignore_imports", [])
        row: dict[str, Any] = {
            "id": contract.get("id"),
            "name": contract["name"],
            "type": contract["type"],
            "ignoreImports": (
                [line.strip() for line in ignored.splitlines() if line.strip()]
                if isinstance(ignored, str)
                else [str(item) for item in ignored]
            ),
        }
        if contract["type"] in failed:
            row.update(importLinter="error", detail=failed[contract["type"]])
        else:
            row.update(
                results.get(
                    contract["name"],
                    {"importLinter": "error", "detail": "not checked (the report stopped early)"},
                )
            )
        rows.append(row)

    modules: dict[str, str | None] = {}
    edges: list[list[str]] = []
    if graph is not None:
        modules = {m: module_file(m, roots, base) for m in sorted(graph.modules)}
        edges = sorted(
            [importer, imported]
            for importer in modules
            if modules[importer] is not None
            for imported in graph.find_modules_directly_imported_by(importer)
            if modules.get(imported) is not None
        )
    document = {
        "importLinterVersion": metadata.version("import-linter"),
        "rootPackages": list(session.get("root_packages", [])),
        "rootFolders": {
            package: root_folder(package, roots, base)
            for package in session.get("root_packages", [])
        },
        "includeExternalPackages": flag(session, "include_external_packages"),
        "excludeTypeCheckingImports": flag(session, "exclude_type_checking_imports"),
        "contracts": rows,
        "modules": modules,
        "edges": edges,
    }
    Path(args.out).write_text(json.dumps(document, indent=2, default=str) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
