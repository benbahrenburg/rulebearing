# Copyright (c) 2026 Ben Bahrenburg. MIT License.
"""The pytest plugin: one test item per rule of ``rulebearing cruise``.

- Architecture: docs/architecture.md#distribution; design § Hooks, test runners, an MCP server,
  an LSP (docs/artifacts/design.md).
- Contract: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md § 1.5 (the adapters'
  failure message is the ``junit`` message text).
- Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md § 2.14, Step 14 (2H).
- Decisions: docs/adr/0007-vacuous-rules-fail-by-default.md,
  docs/adr/0010-crate-layout-and-extractor-boundary.md rule 4,
  docs/adr/0018-test-coverage-threshold.md.
- Requirement: FR-DIST-03 (docs/prd.md).

Off unless ``--rulebearing`` is passed or ``rulebearing = true`` is set in
``[tool.pytest.ini_options]``. When on, the plugin runs the binary once during collection and
adds one item per rule, marked ``rulebearing``, whose failure message is exactly the ``junit``
reporter's message for that rule. The plugin never evaluates a rule.
"""

from __future__ import annotations

from typing import TYPE_CHECKING, Any

import pytest

from pytest_rulebearing.cases import Case, cases, message
from pytest_rulebearing.run import CruiseError, Options, cruise

if TYPE_CHECKING:
    from collections.abc import Iterator
    from pathlib import Path

    from _pytest._code.code import ExceptionInfo, TerminalRepr

__all__ = ["CruiseItem", "RuleFailedError", "RuleItem", "RulebearingCollector", "enabled"]

_MARKER = "rulebearing"


def pytest_addoption(parser: pytest.Parser) -> None:
    """Register the command-line options and the ini keys."""
    group = parser.getgroup("rulebearing", "architecture rules from rulebearing cruise")
    group.addoption(
        "--rulebearing",
        action="store_true",
        default=False,
        help="Add one test per rule of `rulebearing cruise`.",
    )
    group.addoption("--rulebearing-config", metavar="FILE", help="The rulebearing configuration.")
    group.addoption("--rulebearing-graph", metavar="FILE", help="Read this graph document.")
    group.addoption("--rulebearing-binary", metavar="FILE", help="The rulebearing binary to run.")
    group.addoption(
        "--rulebearing-arg",
        action="append",
        default=[],
        metavar="ARG",
        help="Pass ARG to `rulebearing cruise` (repeatable), such as a directory to cruise.",
    )
    parser.addini("rulebearing", "Add one test per rule of `rulebearing cruise`.", type="bool")
    parser.addini("rulebearing_config", "The rulebearing configuration.", default="")
    parser.addini(
        "rulebearing_graph", "Read this graph document instead of extracting.", default=""
    )
    parser.addini("rulebearing_binary", "The rulebearing binary to run.", default="")
    parser.addini("rulebearing_args", "Further arguments to `rulebearing cruise`.", type="args")


def pytest_configure(config: pytest.Config) -> None:
    """Register the marker every rule item carries."""
    config.addinivalue_line("markers", f"{_MARKER}: a rule of `rulebearing cruise`")


def enabled(config: pytest.Config) -> bool:
    """Whether the plugin adds rule items: ``--rulebearing`` or ``rulebearing = true``."""
    return bool(config.getoption("rulebearing")) or bool(config.getini("rulebearing"))


def _ini_base(config: pytest.Config) -> Path:
    return config.rootpath if config.inipath is None else config.inipath.parent


def _path(config: pytest.Config, option: str, ini: str) -> str | None:
    """A path from the command line (against the invocation directory) or the ini file."""
    given = config.getoption(option)
    if isinstance(given, str) and given:
        return str(config.invocation_params.dir / given)
    configured = config.getini(ini)
    if isinstance(configured, str) and configured:
        return str(_ini_base(config) / configured)
    return None


def options(config: pytest.Config) -> Options:
    """How to run the binary, from the options and the ini keys.

    Args:
        config: The pytest configuration.

    Returns:
        The run options; the run's directory is the ini file's, else pytest's root directory.
    """
    ini_args = config.getini("rulebearing_args")
    cli_args = config.getoption("rulebearing_arg")
    return Options(
        binary=_path(config, "rulebearing_binary", "rulebearing_binary"),
        config=_path(config, "rulebearing_config", "rulebearing_config"),
        graph=_path(config, "rulebearing_graph", "rulebearing_graph"),
        args=(*[str(a) for a in ini_args], *[str(a) for a in cli_args]),
        cwd=_ini_base(config),
    )


class RuleFailedError(Exception):
    """A rule failed; the message is the ``junit`` message text, reported as is."""


class _ReportedItem(pytest.Item):
    """An item whose failure message is reported as is, with no traceback."""

    def repr_failure(
        self,
        excinfo: ExceptionInfo[BaseException],
        style: str | None = None,
    ) -> str | TerminalRepr:
        """The message of a :class:`RuleFailedError` exactly; pytest's report for anything else."""
        if isinstance(excinfo.value, RuleFailedError):
            return str(excinfo.value)
        return super().repr_failure(excinfo, style=style)  # type: ignore[arg-type]


class RuleItem(_ReportedItem):
    """One rule of the run."""

    def __init__(self, *, case: Case, **kwargs: Any) -> None:  # noqa: ANN401 - pytest's node kwargs
        """Create the item for ``case``."""
        super().__init__(**kwargs)
        self.case = case
        self.add_marker(_MARKER)

    def runtest(self) -> None:
        """Fail with the rule's message when the case failed; report output either way."""
        if self.case.output:
            self.add_report_section("call", "rulebearing", "\n".join(self.case.output))
        if self.case.failed:
            raise RuleFailedError(message(self.case))

    def reportinfo(self) -> tuple[Path, int | None, str]:
        """Where the item comes from: the run's directory and the rule's family and name."""
        return self.path, None, f"rulebearing.{self.case.rule.family}: {self.case.rule.name}"


class CruiseItem(_ReportedItem):
    """Stands for a run that wrote no result, so its failure shows among the tests."""

    def __init__(self, *, error: str, **kwargs: Any) -> None:  # noqa: ANN401 - pytest's node kwargs
        """Create the item for a run that failed with ``error``."""
        super().__init__(**kwargs)
        self.error = error
        self.add_marker(_MARKER)

    def runtest(self) -> None:
        """Fail with the reason the run wrote no result."""
        raise RuleFailedError(self.error)

    def reportinfo(self) -> tuple[Path, int | None, str]:
        """Where the item comes from."""
        return self.path, None, "rulebearing cruise"


class RulebearingCollector(pytest.Collector):
    """Runs the binary once and yields its rule items."""

    def collect(self) -> Iterator[pytest.Item]:
        """One :class:`RuleItem` per case, or one :class:`CruiseItem` when the run failed."""
        try:
            result = cruise(options(self.config))
        except CruiseError as error:
            yield CruiseItem.from_parent(self, name="cruise", error=str(error))
            return
        for case in cases(result):
            yield RuleItem.from_parent(self, name=case.rule.name, case=case)


@pytest.hookimpl(tryfirst=True)
def pytest_collection_modifyitems(
    session: pytest.Session,
    config: pytest.Config,
    items: list[pytest.Item],
) -> None:
    """Add the rule items before selection (``-k``, ``-m``) and deselection run."""
    if not enabled(config):
        return
    collector = RulebearingCollector.from_parent(session, name=_MARKER, nodeid=_MARKER)
    items.extend(collector.collect())
