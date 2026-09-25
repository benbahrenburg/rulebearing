# Copyright (c) 2026 Ben Bahrenburg. MIT License.
"""Fixtures for the Python packages' tests (wrappers/pip, adapters/python).

One file at the root rather than one per package: ``mypy .`` checks the whole tree and would report
two ``conftest`` modules as a duplicate. Plan:
docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md § 2.14, Step 14 (2H).
"""

from __future__ import annotations

import importlib.util
import os
import shutil
from pathlib import Path
from typing import TYPE_CHECKING

import pytest

if TYPE_CHECKING:
    from types import ModuleType

REPOSITORY = Path(__file__).resolve().parent
#: The test adapters' shared fixture repository (adapters/fixture/README.md).
FIXTURE = REPOSITORY / "adapters" / "fixture"


@pytest.fixture(scope="session")
def local_binary() -> Path:
    """The binary built from this checkout: ``RULEBEARING_BINARY``, else ``target/``."""
    name = "rulebearing.exe" if os.name == "nt" else "rulebearing"
    override = os.environ.get("RULEBEARING_BINARY", "")
    candidates = [Path(override)] if override else []
    candidates += [REPOSITORY / "target" / profile / name for profile in ("release", "debug")]
    for candidate in candidates:
        if candidate.is_file():
            return candidate
    pytest.fail(
        "no rulebearing binary: run `cargo build --release -p rb-cli`, or set RULEBEARING_BINARY",
    )


def load_hook(path: Path) -> ModuleType:
    """A package's hatchling hook, which hatchling loads by path, loaded the same way."""
    name = f"{path.stem}_{path.parent.name.replace('-', '_')}"
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        pytest.fail(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


@pytest.fixture(scope="session")
def pip_hatch_build() -> ModuleType:
    """``wrappers/pip/hatch_build.py``."""
    return load_hook(REPOSITORY / "wrappers" / "pip" / "hatch_build.py")


@pytest.fixture(scope="session")
def plugin_hatch_build() -> ModuleType:
    """``adapters/python/pytest-rulebearing/hatch_version.py``."""
    return load_hook(REPOSITORY / "adapters" / "python" / "pytest-rulebearing" / "hatch_version.py")


@pytest.fixture
def project(pytester: pytest.Pytester) -> Path:
    """adapters/fixture copied into pytester's directory, which the inner pytest runs in.

    A copy, so a run's cache never lands in the source tree, and the inner run's root directory,
    so the adapter and a direct ``rulebearing cruise`` see the same relative paths.
    """
    shutil.copytree(
        FIXTURE,
        pytester.path,
        ignore=shutil.ignore_patterns(".graph", "README.md"),
        dirs_exist_ok=True,
    )
    return pytester.path
