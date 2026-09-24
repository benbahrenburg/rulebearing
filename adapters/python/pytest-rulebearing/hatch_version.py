# Copyright (c) 2026 Ben Bahrenburg. MIT License.
"""The metadata hook of ``pytest-rulebearing``: the release version, ``rulebearing`` pinned to it.

- Decision: docs/adr/0020-single-name-across-registries.md (one version on every registry).
- Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md § 2.14, Step 14 (2H).
- Procedure: docs/release.md.

The plugin runs the binary the ``rulebearing`` wheel carries, so it depends on exactly the
``rulebearing`` release it was published with. ``RULEBEARING_VERSION`` is the version, with a
leading ``v`` removed and a semantic pre-release written as PEP 440; unset, the version is the
``[workspace.package]`` version of the repository's Cargo.toml, as for ``wrappers/pip``.
"""

from __future__ import annotations

import os
import tomllib
from pathlib import Path
from typing import TYPE_CHECKING, Any

from hatchling.metadata.plugin.interface import MetadataHookInterface
from packaging.version import InvalidVersion, Version

if TYPE_CHECKING:
    from collections.abc import Mapping

VERSION_ENV = "RULEBEARING_VERSION"


class VersionError(Exception):
    """No release version can be read; the message says where to set one."""


def release_version(env: Mapping[str, str], root: Path) -> str:
    """The PEP 440 version to stamp.

    Args:
        env: The environment.
        root: This package's directory (``adapters/python/pytest-rulebearing``).

    Returns:
        ``RULEBEARING_VERSION``, else the repository's workspace version, normalised.

    Raises:
        VersionError: Neither is available, or the version is not a release version.
    """
    version = env.get(VERSION_ENV, "").removeprefix("v")
    if not version:
        cargo_toml = root.parent.parent.parent / "Cargo.toml"
        try:
            manifest = tomllib.loads(cargo_toml.read_text(encoding="utf-8"))
        except OSError as error:
            message = f"set {VERSION_ENV}: cannot read {cargo_toml} for the release version"
            raise VersionError(message) from error
        found = manifest.get("workspace", {}).get("package", {}).get("version")
        if not isinstance(found, str):
            message = f"{cargo_toml} has no [workspace.package] version"
            raise VersionError(message)
        version = found
    try:
        return str(Version(version))
    except InvalidVersion as error:
        message = f"{VERSION_ENV}: not a release version: {version}"
        raise VersionError(message) from error


class CustomMetadataHook(MetadataHookInterface):
    """Stamps the version and pins ``rulebearing`` to it."""

    PLUGIN_NAME = "custom"

    def update(self, metadata: dict[str, Any]) -> None:
        """Set ``version`` and ``dependencies`` (both declared dynamic in pyproject.toml)."""
        version = release_version(os.environ, Path(self.root))
        metadata["version"] = version
        metadata["dependencies"] = ["pytest>=8", f"rulebearing=={version}"]
