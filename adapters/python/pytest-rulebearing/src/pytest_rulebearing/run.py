# Copyright (c) 2026 Ben Bahrenburg. MIT License.
"""Runs ``rulebearing cruise --output-type json`` and reads what it wrote.

- Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md § 2.14, Step 14 (2H).
- Decisions: docs/adr/0008-exit-code-contract.md (exit 1 and 2 still write the JSON),
  docs/adr/0010-crate-layout-and-extractor-boundary.md rule 4 (the binary evaluates; the adapter
  reports).
- Requirement: FR-DIST-03 (docs/prd.md).
"""

from __future__ import annotations

import json
import os
import subprocess
from dataclasses import dataclass, field
from pathlib import Path
from typing import TYPE_CHECKING

import rulebearing

if TYPE_CHECKING:
    from collections.abc import Mapping, Sequence

__all__ = ["CruiseError", "Options", "command", "cruise", "resolve_binary"]


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


def resolve_binary(binary: str | None, env: Mapping[str, str] | None = None) -> str:
    """The binary to run: the option, else ``RULEBEARING_BINARY``, else the ``rulebearing`` wheel's.

    Args:
        binary: The ``--rulebearing-binary`` option or the ``rulebearing_binary`` ini value.
        env: The environment; ``os.environ`` when ``None``.

    Returns:
        The path to run.

    Raises:
        CruiseError: No binary can be found.
    """
    if binary:
        return binary
    try:
        return str(rulebearing.binary_path(os.environ if env is None else env))
    except rulebearing.BinaryNotFoundError as error:
        raise CruiseError(str(error)) from error


def command(options: Options, binary: str) -> list[str]:
    """The command line of the run.

    Args:
        options: How to run.
        binary: The resolved binary.

    Returns:
        ``[binary, "cruise", "--output-type", "json", "--no-progress", ...]``.
    """
    line = [binary, "cruise", "--output-type", "json", "--no-progress"]
    if options.config is not None:
        line += ["--config", options.config]
    if options.graph is not None:
        line += ["--graph", options.graph]
    return [*line, *options.args]


def cruise(options: Options, env: Mapping[str, str] | None = None) -> object:
    """Run the binary and parse its JSON.

    A run that finds violations (exit 1) or cannot be trusted (exit 2, a vacuous rule) still
    writes its result; only a run that wrote no result is an error.

    Args:
        options: How to run.
        env: The environment; ``os.environ`` when ``None``.

    Returns:
        The parsed result, an object with a ``summary``.

    Raises:
        CruiseError: The binary cannot be found or started, or wrote no result.
    """
    line = command(options, resolve_binary(options.binary, env))
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
    return result
