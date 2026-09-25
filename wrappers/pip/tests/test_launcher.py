# Copyright (c) 2026 Ben Bahrenburg. MIT License.
"""The console script: resolve the binary, run it with the arguments unchanged, exit 2 without one.

Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md § 2.14, Step 14 (2H).
Exit codes: docs/adr/0008-exit-code-contract.md.
"""

from __future__ import annotations

import os
import signal
import sys
import threading
from pathlib import Path
from typing import TYPE_CHECKING

import pytest
import rulebearing
from rulebearing import _launcher

if TYPE_CHECKING:
    from collections.abc import Sequence


@pytest.mark.parametrize(
    ("platform", "name"),
    [("win32", "rulebearing.exe"), ("linux", "rulebearing"), ("darwin", "rulebearing")],
)
def test_binary_name_per_platform(platform: str, name: str) -> None:
    assert _launcher.binary_name(platform) == name
    assert _launcher.bundled_binary(platform).name == name


def test_bundled_binary_sits_in_bin_beside_the_package() -> None:
    bundled = _launcher.bundled_binary("linux")
    assert bundled.parent.name == "bin"
    assert bundled.parent.parent == Path(_launcher.__file__).resolve().parent


def test_the_override_wins_when_it_exists(tmp_path: Path) -> None:
    binary = tmp_path / "rb"
    binary.write_text("")
    assert _launcher.binary_path({"RULEBEARING_BINARY": str(binary)}) == binary


def test_an_override_that_does_not_exist_is_named(tmp_path: Path) -> None:
    missing = tmp_path / "absent"
    with pytest.raises(_launcher.BinaryNotFoundError, match="RULEBEARING_BINARY is set to"):
        _launcher.binary_path({"RULEBEARING_BINARY": str(missing)})


def test_a_missing_bundled_binary_says_how_to_get_one(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(_launcher, "bundled_binary", lambda _platform: tmp_path / "none")
    with pytest.raises(_launcher.BinaryNotFoundError, match="pip install rulebearing"):
        _launcher.binary_path({})


def test_the_bundled_binary_is_used_without_an_override(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    bundled = tmp_path / "rulebearing"
    bundled.write_text("")
    monkeypatch.setattr(_launcher, "bundled_binary", lambda _platform: bundled)
    monkeypatch.delenv("RULEBEARING_BINARY", raising=False)
    assert _launcher.binary_path() == bundled
    assert rulebearing.binary_path({"RULEBEARING_BINARY": ""}) == bundled


def test_posix_replaces_the_process_with_every_argument_unchanged(tmp_path: Path) -> None:
    binary = tmp_path / "rb"
    binary.write_text("")
    calls: list[tuple[str, list[str]]] = []

    def execv(path: str, args: Sequence[str]) -> None:
        calls.append((path, list(args)))

    code = _launcher.main(
        ["cruise", "-T", "err", "src dir"],
        {"RULEBEARING_BINARY": str(binary)},
        "linux",
        execv=execv,
        run=lambda _command: 99,
    )
    assert calls == [(str(binary), [str(binary), "cruise", "-T", "err", "src dir"])]
    # os.execv does not return; a stand-in that does falls through to the Windows path.
    assert code == 99


def test_windows_runs_the_binary_and_returns_its_exit_code(tmp_path: Path) -> None:
    binary = tmp_path / "rb.exe"
    binary.write_text("")
    seen: list[list[str]] = []

    def run(command: Sequence[str]) -> int:
        seen.append(list(command))
        return 3

    def execv(_path: str, _args: Sequence[str]) -> None:
        pytest.fail("execv must not be used on Windows")

    code = _launcher.main(
        ["--version"],
        {"RULEBEARING_BINARY": str(binary)},
        "win32",
        execv=execv,
        run=run,
    )
    assert code == 3
    assert seen == [[str(binary), "--version"]]


def test_no_binary_is_exit_2_with_one_line(
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    code = _launcher.main(["--version"], {"RULEBEARING_BINARY": str(tmp_path / "x")}, "linux")
    assert code == _launcher.EXIT_UNTRUSTWORTHY == 2
    err = capsys.readouterr().err
    assert err.count("\n") == 1
    assert err.startswith("rulebearing: RULEBEARING_BINARY is set to")


def test_a_binary_that_cannot_start_is_exit_2(
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    binary = tmp_path / "rb"
    binary.write_text("")

    def execv(path: str, _args: Sequence[str]) -> None:
        raise PermissionError(13, "Permission denied", path)

    code = _launcher.main([], {"RULEBEARING_BINARY": str(binary)}, "linux", execv=execv)
    assert code == 2
    assert "could not start" in capsys.readouterr().err


def test_argv_defaults_to_the_process_arguments(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    binary = tmp_path / "rb"
    binary.write_text("")
    monkeypatch.setattr(sys, "argv", ["rulebearing", "--help"])
    seen: list[list[str]] = []

    def run(command: Sequence[str]) -> int:
        seen.append(list(command))
        return 0

    assert _launcher.main(env={"RULEBEARING_BINARY": str(binary)}, platform="win32", run=run) == 0
    assert seen == [[str(binary), "--help"]]


def test_run_returns_the_child_exit_code() -> None:
    assert _launcher._run([sys.executable, "-c", "raise SystemExit(4)"]) == 4  # noqa: SLF001


def test_run_ignores_sigint_while_waiting_and_restores_the_handler() -> None:
    if sys.platform == "win32":
        pytest.skip("sending SIGINT to the own process is POSIX")
    before = signal.getsignal(signal.SIGINT)
    timer = threading.Timer(0.3, os.kill, (os.getpid(), signal.SIGINT))
    timer.start()
    try:
        code = _launcher._run(  # noqa: SLF001
            [sys.executable, "-c", "import time; time.sleep(1.5); raise SystemExit(5)"],
        )
    finally:
        timer.cancel()
    assert code == 5
    assert signal.getsignal(signal.SIGINT) is before


def test_run_off_the_main_thread_still_returns_the_code() -> None:
    codes: list[int] = []
    worker = threading.Thread(
        target=lambda: codes.append(
            _launcher._run([sys.executable, "-c", "raise SystemExit(6)"]),  # noqa: SLF001
        ),
    )
    worker.start()
    worker.join()
    assert codes == [6]


def test_console_exits_with_the_code_of_main(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(_launcher, "main", lambda: 5)
    with pytest.raises(SystemExit) as exited:
        _launcher.console()
    assert exited.value.code == 5


def test_the_real_binary_runs_through_main(local_binary: Path) -> None:
    # On Windows the launcher waits for the binary; on POSIX it would replace this process, so
    # the Windows path is taken explicitly to observe the exit code.
    code = _launcher.main(
        ["--version"],
        {"RULEBEARING_BINARY": str(local_binary)},
        "win32",
    )
    assert code == 0
