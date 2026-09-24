# Copyright (c) 2026 Ben Bahrenburg. MIT License.
"""Smoke test: build the wheel with the local binary, install it in a new venv, run the command.

The release workflow's ``pypi-install-check`` job does the same with the wheels it built, on each
platform (docs/release.md). Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md
§ 2.14, Step 14 (2H): "a smoke job installs each wrapper from the built package on each platform
and runs ``rulebearing --version``".
"""

from __future__ import annotations

import os
import subprocess
import sys
import venv
import zipfile
from pathlib import Path
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from types import ModuleType

PACKAGE = Path(__file__).resolve().parent.parent


def _run(command: list[str], env: dict[str, str] | None = None) -> str:
    completed = subprocess.run(  # noqa: S603 - fixed commands over paths this test created
        command,
        capture_output=True,
        check=False,
        encoding="utf-8",
        env=env,
    )
    assert completed.returncode == 0, completed.stderr
    return completed.stdout.strip()


def test_the_built_wheel_installs_and_runs(
    local_binary: Path,
    pip_hatch_build: ModuleType,
    tmp_path: Path,
) -> None:
    version = pip_hatch_build.release_version({}, PACKAGE)
    env = {**os.environ, "RULEBEARING_WHEEL_BINARY": str(local_binary)}
    env.pop("RULEBEARING_VERSION", None)
    env.pop("RULEBEARING_BINARY", None)
    dist = tmp_path / "dist"
    _run(
        [
            sys.executable,
            "-m",
            "build",
            "--wheel",
            "--no-isolation",
            "--outdir",
            str(dist),
            str(PACKAGE),
        ],
        env,
    )
    wheels = list(dist.glob("*.whl"))
    assert len(wheels) == 1
    _, _, tag = pip_hatch_build.wheel_plan(env)
    assert wheels[0].name == f"rulebearing-{version}-{tag}.whl"
    with zipfile.ZipFile(wheels[0]) as archive:
        names = archive.namelist()
        inside = next(n for n in names if n.startswith("rulebearing/bin/"))
        mode = archive.getinfo(inside).external_attr >> 16
    assert inside in {"rulebearing/bin/rulebearing", "rulebearing/bin/rulebearing.exe"}
    if os.name != "nt":
        assert mode & 0o111, "the bundled binary must be executable"
    assert not any(n.startswith(("tests/", "hatch_build")) for n in names)

    environment = tmp_path / "venv"
    venv.create(environment, with_pip=True)
    scripts = environment / ("Scripts" if os.name == "nt" else "bin")
    python = scripts / ("python.exe" if os.name == "nt" else "python")
    _run([str(python), "-m", "pip", "install", "--quiet", "--no-index", str(wheels[0])])
    clean = {k: v for k, v in os.environ.items() if k != "RULEBEARING_BINARY"}
    command = scripts / ("rulebearing.exe" if os.name == "nt" else "rulebearing")
    expected = _run([str(local_binary), "--version"])
    assert expected == f"rulebearing {version}"
    assert _run([str(command), "--version"], clean) == expected
    assert _run([str(python), "-m", "rulebearing", "--version"], clean) == expected
