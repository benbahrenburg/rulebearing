# Copyright (c) 2026 Ben Bahrenburg. MIT License.
"""Hatchling hooks for ``rulebearing-rules``: stamp the release version, add the presets.

- Decision: docs/adr/0071-the-rule-library-ships-from-this-repository.md (one source, the
  release version); docs/adr/0020-single-name-across-registries.md (one name on every registry).
- Plan: docs/plans/implemented/0003-wave-3-operations-surface-inner-loop.md, Step 23.
- Procedure: docs/release.md.
- Requirement: FR-REACH-04 (docs/prd.md).

The presets are the repository's ``presets/frameworks/*.yaml`` and
``presets/rulebearing/recommended.yaml``, the files the binary bundles as ``rulebearing:<name>``.
They are added to the package at ``rulebearing_rules/<preset>.yaml`` for a wheel and for a source
distribution, so the source tree holds no second copy. A build from an unpacked source
distribution (it has ``PKG-INFO``) takes the presets the package already holds and the version
``PKG-INFO`` records. Otherwise the version is ``RULEBEARING_VERSION``, a leading ``v`` removed and
a semantic pre-release written as PEP 440, else the ``[workspace.package]`` version of the
repository's Cargo.toml.
"""

from __future__ import annotations

import os
import tomllib
from email.parser import HeaderParser
from pathlib import Path
from typing import TYPE_CHECKING, Any

from hatchling.builders.hooks.plugin.interface import BuildHookInterface
from hatchling.metadata.plugin.interface import MetadataHookInterface
from packaging.version import InvalidVersion, Version

if TYPE_CHECKING:
    from collections.abc import Mapping

VERSION_ENV = "RULEBEARING_VERSION"
#: The import name of the package, the folder Rulebearing looks for in ``site-packages``.
MODULE = "rulebearing_rules"
#: The preset that is not a framework preset: ``rulebearing:recommended``.
RECOMMENDED = Path("presets") / "rulebearing" / "recommended.yaml"
FRAMEWORKS = Path("presets") / "frameworks"


class RulesBuildError(Exception):
    """The package cannot be built as asked; the message says why."""


def presets(repository: Path) -> dict[str, Path]:
    """The library's files, by the name each has in the package.

    Args:
        repository: The Rulebearing repository's root.

    Returns:
        ``<preset>.yaml`` to its source file, sorted by name: every framework preset and
        ``recommended.yaml``.

    Raises:
        RulesBuildError: The repository has no framework presets, no ``recommended.yaml``, or a
            framework preset named ``recommended``.
    """
    frameworks = sorted((repository / FRAMEWORKS).glob("*.yaml"))
    if not frameworks:
        message = f"no presets under {repository / FRAMEWORKS}; build from a Rulebearing checkout"
        raise RulesBuildError(message)
    recommended = repository / RECOMMENDED
    if not recommended.is_file():
        message = f"{recommended} is missing"
        raise RulesBuildError(message)
    found = {path.name: path for path in frameworks}
    if recommended.name in found:
        message = f"{found[recommended.name]} would replace rulebearing:recommended in the library"
        raise RulesBuildError(message)
    found[recommended.name] = recommended
    return dict(sorted(found.items()))


def pep440(version: str) -> str:
    """A release version as PyPI spells it (``v0.2.0-rc.1`` is ``0.2.0rc1``).

    Raises:
        RulesBuildError: The version is not one PEP 440 can express.
    """
    bare = version.removeprefix("v")
    try:
        return str(Version(bare))
    except InvalidVersion as error:
        message = f"{VERSION_ENV}: not a release version: {version}"
        raise RulesBuildError(message) from error


def workspace_version(cargo_toml: Path) -> str:
    """The ``[workspace.package]`` version of the repository's Cargo.toml.

    Raises:
        RulesBuildError: The manifest is missing or has no workspace version.
    """
    try:
        manifest = tomllib.loads(cargo_toml.read_text(encoding="utf-8"))
    except OSError as error:
        message = f"set {VERSION_ENV}: cannot read {cargo_toml} for the release version"
        raise RulesBuildError(message) from error
    version = manifest.get("workspace", {}).get("package", {}).get("version")
    if not isinstance(version, str):
        message = f"{cargo_toml} has no [workspace.package] version"
        raise RulesBuildError(message)
    return version


def sdist_version(pkg_info: Path) -> str:
    """The version an unpacked source distribution's ``PKG-INFO`` records.

    Raises:
        RulesBuildError: ``PKG-INFO`` has no ``Version``.
    """
    headers = HeaderParser().parsestr(pkg_info.read_text(encoding="utf-8"))
    version = headers.get("Version")
    if not version:
        message = f"{pkg_info} has no Version"
        raise RulesBuildError(message)
    return pep440(version)


def release_version(env: Mapping[str, str], root: Path) -> str:
    """The version to stamp.

    Args:
        env: The environment.
        root: The directory holding this package's pyproject.toml (``wrappers/rules``, or an
            unpacked source distribution).

    Returns:
        A PEP 440 version.
    """
    pkg_info = root / "PKG-INFO"
    if pkg_info.is_file():
        return sdist_version(pkg_info)
    stamped = env.get(VERSION_ENV, "")
    return pep440(stamped or workspace_version(root.parent.parent / "Cargo.toml"))


def force_include(root: Path) -> dict[str, str]:
    """What the build adds: each preset's source file to its path in the distribution.

    Args:
        root: The directory holding this package's pyproject.toml.

    Returns:
        Source path to ``rulebearing_rules/<preset>.yaml``; nothing for an unpacked source
        distribution, whose package already holds the presets.
    """
    if (root / "PKG-INFO").is_file():
        return {}
    return {str(source): f"{MODULE}/{name}" for name, source in presets(root.parent.parent).items()}


class CustomMetadataHook(MetadataHookInterface):
    """Stamps the release version into the package metadata."""

    PLUGIN_NAME = "custom"

    def update(self, metadata: dict[str, Any]) -> None:
        """Set ``version`` (declared dynamic in pyproject.toml)."""
        metadata["version"] = release_version(os.environ, Path(self.root))


class CustomBuildHook(BuildHookInterface):  # type: ignore[type-arg]
    """Adds the presets to a wheel or a source distribution."""

    PLUGIN_NAME = "custom"

    def initialize(self, version: str, build_data: dict[str, Any]) -> None:  # noqa: ARG002
        """Add every preset at ``rulebearing_rules/<preset>.yaml``."""
        build_data["force_include"].update(force_include(Path(self.root)))
