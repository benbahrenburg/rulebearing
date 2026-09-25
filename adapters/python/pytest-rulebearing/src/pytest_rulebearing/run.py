# Copyright (c) 2026 Ben Bahrenburg. MIT License.
"""Runs ``rulebearing cruise --output-type json`` and reads what it wrote.

- Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md § 2.14, Step 14 (2H).
- Decisions: docs/adr/0008-exit-code-contract.md (exit 1 and 2 still write the JSON),
  docs/adr/0010-crate-layout-and-extractor-boundary.md rule 4 (the binary evaluates; the adapter
  reports).
- Requirement: FR-DIST-03 (docs/prd.md).

The run always passes ``--output-to -``, so a configuration that sets ``options.outputTo`` neither
hides the JSON from the adapter nor has a file of the user's overwritten with it. A binary named
by a bare name (``rulebearing``) is looked up on ``PATH``; one named by a relative path is taken
against the directory it was given in (see :func:`locate`), never against the run's directory.
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
from dataclasses import dataclass, field
from pathlib import Path
from typing import TYPE_CHECKING

import rulebearing

from pytest_rulebearing.cases import cases

if TYPE_CHECKING:
    from collections.abc import Mapping, Sequence

__all__ = ["CruiseError", "Options", "command", "cruise", "locate", "resolve_binary"]


#: The exit code of a run that cannot be trusted (docs/adr/0008-exit-code-contract.md).
EXIT_UNTRUSTWORTHY = 2


class CruiseError(Exception):
    """The run wrote no result to read; the message names the command, exit code and stderr."""


@dataclass(frozen=True)
class Options:
    """How to run the binary.

    Attributes:
        binary: The binary; ``None`` resolves it as :func:`resolve_binary` says.
        config: ``--config``; ``None`` lets the binary find the configuration.
        graph: ``--graph``: read a graph document instead of extracting.
        args: Further arguments, such as the files or directories to cruise.
        cwd: The directory to run in: pytest's root directory.
    """

    binary: str | None = None
    config: str | None = None
    graph: str | None = None
    args: Sequence[str] = field(default_factory=tuple)
    cwd: Path = field(default_factory=Path.cwd)


def _bare(value: str) -> bool:
    """Whether a binary is named without a directory, so ``PATH`` finds it."""
    return os.sep not in value and (os.altsep is None or os.altsep not in value)


def locate(value: str, base: Path, path: str | None = None) -> str:
    """A binary as the user named it, made independent of the directory the run happens in.

    Args:
        value: The binary: a bare name, a relative path or an absolute path.
        base: The directory a relative path was given in.
        path: The ``PATH`` to search for a bare name; the process's when ``None``.

    Returns:
        A bare name's ``PATH`` entry (the name itself when none has it, so starting it reports
        the failure), a relative path joined to ``base``, or an absolute path unchanged.
    """
    if _bare(value):
        found = shutil.which(value, path=path)
        return value if found is None else found
    return str(base / value)


def resolve_binary(
    binary: str | None,
    env: Mapping[str, str] | None = None,
    base: Path | None = None,
) -> str:
    """The binary to run: the option, else ``RULEBEARING_BINARY``, else the ``rulebearing`` wheel's.

    Args:
        binary: The ``--rulebearing-binary`` option or the ``rulebearing_binary`` ini value, already
            located against the directory it was given in (the plugin does so).
        env: The environment; ``os.environ`` when ``None``.
        base: The directory a relative ``RULEBEARING_BINARY`` is taken against: the invocation
            directory, the current one when ``None``.

    Returns:
        The path to run.

    Raises:
        CruiseError: No binary can be found.
    """
    if binary:
        return binary
    environment = os.environ if env is None else env
    override = environment.get(rulebearing.BINARY_OVERRIDE, "")
    where = Path.cwd() if base is None else base
    if override:
        located = locate(override, where, environment.get("PATH"))
        environment = {**environment, rulebearing.BINARY_OVERRIDE: located}
    try:
        found = rulebearing.binary_path(environment)
    except rulebearing.BinaryNotFoundError as error:
        raise CruiseError(str(error)) from error
    return str(found if found.is_absolute() else where / found)


def command(options: Options, binary: str) -> list[str]:
    """The command line of the run.

    Args:
        options: How to run.
        binary: The resolved binary.

    Returns:
        ``[binary, "cruise", "--output-type", "json", "--output-to", "-", "--no-progress", ...]``.
    """
    line = [binary, "cruise", "--output-type", "json", "--output-to", "-", "--no-progress"]
    if options.config is not None:
        line += ["--config", options.config]
    if options.graph is not None:
        line += ["--graph", options.graph]
    return [*line, *options.args]


def cruise(
    options: Options,
    env: Mapping[str, str] | None = None,
    base: Path | None = None,
) -> object:
    """Run the binary and parse its JSON.

    A run that finds violations (exit 1) or cannot be trusted (exit 2, a vacuous rule) still
    writes its result; a run that wrote no result is an error, and so is an exit 2 whose result
    carries no failing case, since the reason the run cannot be trusted is then not among the
    rules (the .NET adapter's rule).

    Args:
        options: How to run.
        env: The environment; ``os.environ`` when ``None``.
        base: The directory a relative ``RULEBEARING_BINARY`` is taken against (pytest's
            invocation directory); the current one when ``None``.

    Returns:
        The parsed result, an object with a ``summary``.

    Raises:
        CruiseError: The binary cannot be found or started, wrote no result, or exited 2 with
            every rule passing.
    """
    line = command(options, resolve_binary(options.binary, env, base))
    shown = " ".join(line)
    try:
        # The command is the resolved binary and the configured arguments, never a shell string.
        completed = subprocess.run(  # noqa: S603
            line,
            cwd=options.cwd,
            capture_output=True,
            check=False,
            encoding="utf-8",
            env=None if env is None else dict(env),
        )
    except OSError as error:
        message = f"`{shown}` could not start: {error}"
        raise CruiseError(message) from error
    try:
        result: object = json.loads(completed.stdout)
    except json.JSONDecodeError:
        result = None
    if not isinstance(result, dict) or "summary" not in result:
        message = (
            f"`{shown}` exited {completed.returncode} without a result:\n{completed.stderr.strip()}"
        )
        raise CruiseError(message)
    if completed.returncode == EXIT_UNTRUSTWORTHY and not any(c.failed for c in cases(result)):
        message = (
            f"`{shown}` cannot be trusted (exit 2), and no rule says why:\n"
            f"{completed.stderr.strip()}\nFix: the message above names the cause (zero modules, "
            "an unsupported file, an assembly without a portable PDB)."
        )
        raise CruiseError(message)
    return result
