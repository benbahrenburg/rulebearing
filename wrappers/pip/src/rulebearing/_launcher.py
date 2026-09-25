# Copyright (c) 2026 Ben Bahrenburg. MIT License.
"""The ``rulebearing`` console script: find the bundled binary and run it, nothing else.

- Architecture: docs/architecture.md#distribution.
- Decisions: docs/adr/0020-single-name-across-registries.md (one name on every registry),
  docs/adr/0008-exit-code-contract.md (2 when no binary can be found or started).
- Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md § 2.14, Step 14 (2H).
- Requirement: FR-DIST-01 (docs/prd.md).

The wheel for each platform carries the binary at ``rulebearing/bin/rulebearing`` (``.exe`` on
Windows). The launcher never re-implements a subcommand: every argument goes to the binary
unchanged. On POSIX the launcher process becomes the binary (``os.execv``), so the exit code and
any signal are the binary's own; on Windows it waits for the binary and returns its exit code,
ignoring Ctrl+C while it waits (the console delivers it to the binary too, which decides how to
stop), so an interrupted run still returns the binary's own code.
``RULEBEARING_BINARY`` names a binary to run instead, as it does for the npm wrapper.
"""

from __future__ import annotations

import os
import signal
import subprocess
import sys
from pathlib import Path
from typing import TYPE_CHECKING, NoReturn

if TYPE_CHECKING:
    from collections.abc import Callable, Mapping, Sequence

__all__ = [
    "BINARY_OVERRIDE",
    "EXIT_UNTRUSTWORTHY",
    "BinaryNotFoundError",
    "binary_name",
    "binary_path",
    "bundled_binary",
    "console",
    "main",
]

#: Environment variable naming a binary to run instead of the bundled one.
BINARY_OVERRIDE = "RULEBEARING_BINARY"

#: The exit code for a run that could not start the binary (ADR-0008).
EXIT_UNTRUSTWORTHY = 2

_PROJECT = "https://github.com/benbahrenburg/rulebearing"


class BinaryNotFoundError(Exception):
    """No binary to run; the message is the one line the launcher prints."""


def binary_name(platform: str = sys.platform) -> str:
    """The binary's file name on a platform.

    Args:
        platform: A ``sys.platform`` value.

    Returns:
        ``rulebearing.exe`` on Windows, ``rulebearing`` elsewhere.
    """
    return "rulebearing.exe" if platform == "win32" else "rulebearing"


def bundled_binary(platform: str = sys.platform) -> Path:
    """Where the wheel puts the binary: ``bin/`` beside this module.

    Args:
        platform: A ``sys.platform`` value.

    Returns:
        The path, whether or not the file exists.
    """
    return Path(__file__).resolve().parent / "bin" / binary_name(platform)


def binary_path(
    env: Mapping[str, str] | None = None,
    platform: str = sys.platform,
) -> Path:
    """The binary to run: ``RULEBEARING_BINARY`` when set, otherwise the bundled one.

    Args:
        env: The environment; ``os.environ`` when ``None``.
        platform: A ``sys.platform`` value.

    Returns:
        An existing file.

    Raises:
        BinaryNotFoundError: The override names a missing file, or the wheel carries no binary.
    """
    environment = os.environ if env is None else env
    override = environment.get(BINARY_OVERRIDE, "")
    if override:
        path = Path(override)
        if not path.is_file():
            message = (
                f"rulebearing: {BINARY_OVERRIDE} is set to {override}, which does not exist; "
                "unset it or point it at a rulebearing binary."
            )
            raise BinaryNotFoundError(message)
        return path
    bundled = bundled_binary(platform)
    if not bundled.is_file():
        message = (
            f"rulebearing: this installation carries no binary at {bundled}. Install the wheel "
            f"for your platform with `pip install rulebearing`, or build from source ({_PROJECT}) "
            f"and set {BINARY_OVERRIDE} to the binary."
        )
        raise BinaryNotFoundError(message)
    return bundled


def _run(command: Sequence[str]) -> int:
    """Start the binary, wait with SIGINT ignored, and return the binary's exit code."""
    # The command is the resolved binary and the caller's own arguments, never a shell string.
    process = subprocess.Popen(command)  # noqa: S603
    try:
        previous = signal.signal(signal.SIGINT, signal.SIG_IGN)
    except ValueError:  # not the main thread: signals cannot be changed, and do not arrive here
        return process.wait()
    try:
        return process.wait()
    finally:
        signal.signal(signal.SIGINT, previous)


def main(
    argv: Sequence[str] | None = None,
    env: Mapping[str, str] | None = None,
    platform: str = sys.platform,
    execv: Callable[[str, list[str]], object] = os.execv,
    run: Callable[[Sequence[str]], int] = _run,
) -> int:
    """Resolve the binary and run it with the arguments unchanged.

    Args:
        argv: The arguments after the program name; ``sys.argv[1:]`` when ``None``.
        env: The environment; ``os.environ`` when ``None``.
        platform: A ``sys.platform`` value.
        execv: Replaces the process with the binary on POSIX (``os.execv``).
        run: Runs the binary to completion and returns its exit code, on Windows.

    Returns:
        The binary's exit code on Windows; 2 when no binary could be found or started. On POSIX
        a successful ``execv`` does not return.
    """
    args = list(sys.argv[1:] if argv is None else argv)
    try:
        path = str(binary_path(env, platform))
    except BinaryNotFoundError as error:
        print(error, file=sys.stderr)  # noqa: T201 - the launcher's one line of output
        return EXIT_UNTRUSTWORTHY
    try:
        if platform != "win32":
            execv(path, [path, *args])
        return run([path, *args])
    except OSError as error:
        print(f"rulebearing: could not start {path}: {error}", file=sys.stderr)  # noqa: T201
        return EXIT_UNTRUSTWORTHY


def console() -> NoReturn:
    """The console script's entry point: exits with :func:`main`'s code."""
    sys.exit(main())
