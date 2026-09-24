# Copyright (c) 2026 Ben Bahrenburg. MIT License.
"""Hatchling hooks for the ``rulebearing`` wheel: stamp the release version, bundle the binary.

- Architecture: docs/architecture.md#distribution (six release targets).
- Decision: docs/adr/0020-single-name-across-registries.md (one version on every registry).
- Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md § 2.14, Step 14 (2H).
- Procedure: docs/release.md.
- Requirement: FR-DIST-01 (docs/prd.md).

One wheel per platform tag, each carrying one binary. The environment says which:

====================== ==================================================================
Variable               Meaning
====================== ==================================================================
RULEBEARING_VERSION    The release version; a leading ``v`` is removed and a semantic
                       pre-release (``-rc.1``) becomes PEP 440 (``rc1``). Unset: the
                       ``[workspace.package]`` version of the repository's Cargo.toml.
RULEBEARING_WHEEL_BINARY  The binary to bundle. Required for a wheel (not an editable one).
RULEBEARING_WHEEL_TARGET  The Rust target triple the binary was built for. Unset: the host's.
RULEBEARING_WHEEL_GLIBC   For a ``*-linux-gnu`` target, the newest glibc symbol version the
                       binary needs (``2.34``), which names its ``manylinux`` tag. Unset:
                       the host's glibc, which is correct for a binary built on this host.
====================== ==================================================================
"""

from __future__ import annotations

import os
import platform
import re
import tomllib
from pathlib import Path
from typing import TYPE_CHECKING, Any

from hatchling.builders.hooks.plugin.interface import BuildHookInterface
from hatchling.metadata.plugin.interface import MetadataHookInterface
from packaging.version import InvalidVersion, Version

if TYPE_CHECKING:
    from collections.abc import Mapping

VERSION_ENV = "RULEBEARING_VERSION"
BINARY_ENV = "RULEBEARING_WHEEL_BINARY"
TARGET_ENV = "RULEBEARING_WHEEL_TARGET"
GLIBC_ENV = "RULEBEARING_WHEEL_GLIBC"

#: The release targets of docs/architecture.md#distribution and the wheel platform tag of each.
#: ``{glibc}`` is the binary's glibc floor. The musl build is static, so any musl system runs it.
#: The macOS floors are Rust's default deployment targets for the two Apple targets.
PLATFORM_TAGS: dict[str, str] = {
    "x86_64-unknown-linux-gnu": "manylinux_{glibc}_x86_64",
    "aarch64-unknown-linux-gnu": "manylinux_{glibc}_aarch64",
    "x86_64-unknown-linux-musl": "musllinux_1_1_x86_64",
    "aarch64-apple-darwin": "macosx_11_0_arm64",
    "x86_64-apple-darwin": "macosx_10_12_x86_64",
    "x86_64-pc-windows-msvc": "win_amd64",
}

_GLIBC = re.compile(r"^2\.(\d+)$")


class WheelConfigError(Exception):
    """The environment does not describe a wheel that can be built; the message says why."""


def pep440(version: str) -> str:
    """A release version as PyPI spells it.

    Args:
        version: A semantic version, optionally with a leading ``v`` (``v0.2.0-rc.1``).

    Returns:
        The normalised PEP 440 version (``0.2.0rc1``).

    Raises:
        WheelConfigError: The version is not one PEP 440 can express.
    """
    bare = version.removeprefix("v")
    try:
        return str(Version(bare))
    except InvalidVersion as error:
        message = f"{VERSION_ENV}: not a release version: {version}"
        raise WheelConfigError(message) from error


def workspace_version(cargo_toml: Path) -> str:
    """The ``[workspace.package]`` version of a Cargo manifest: the release version.

    Args:
        cargo_toml: The repository's root Cargo.toml.

    Returns:
        The version string.

    Raises:
        WheelConfigError: The manifest is missing or has no workspace version.
    """
    try:
        manifest = tomllib.loads(cargo_toml.read_text(encoding="utf-8"))
    except OSError as error:
        message = f"set {VERSION_ENV}: cannot read {cargo_toml} for the release version"
        raise WheelConfigError(message) from error
    version = manifest.get("workspace", {}).get("package", {}).get("version")
    if not isinstance(version, str):
        message = f"{cargo_toml} has no [workspace.package] version"
        raise WheelConfigError(message)
    return version


def release_version(env: Mapping[str, str], root: Path) -> str:
    """The version to stamp: ``RULEBEARING_VERSION``, else the workspace version.

    Args:
        env: The environment.
        root: The directory holding this package's pyproject.toml (``wrappers/pip``).

    Returns:
        A PEP 440 version.
    """
    stamped = env.get(VERSION_ENV, "")
    return pep440(stamped or workspace_version(root.parent.parent / "Cargo.toml"))


def host_target(
    system: str | None = None,
    machine: str | None = None,
    libc: str | None = None,
) -> str:
    """The Rust target triple of the host, among the release targets.

    Args:
        system: ``platform.system()``; the host's when ``None``.
        machine: ``platform.machine()``; the host's when ``None``.
        libc: ``platform.libc_ver()[0]`` on Linux; the host's when ``None``.

    Returns:
        A key of :data:`PLATFORM_TAGS`.

    Raises:
        WheelConfigError: The host is not a release target; set ``RULEBEARING_WHEEL_TARGET``.
    """
    system = platform.system() if system is None else system
    machine = (platform.machine() if machine is None else machine).lower()
    arch = {"amd64": "x86_64", "x86_64": "x86_64", "arm64": "aarch64", "aarch64": "aarch64"}.get(
        machine,
        machine,
    )
    if system == "Linux":
        found = platform.libc_ver()[0] if libc is None else libc
        target = f"{arch}-unknown-linux-{'gnu' if found == 'glibc' else 'musl'}"
    elif system == "Darwin":
        target = f"{arch}-apple-darwin"
    elif system == "Windows":
        target = f"{arch}-pc-windows-msvc"
    else:
        target = f"{arch}-{system.lower()}"
    if target not in PLATFORM_TAGS:
        message = f"no release target for this host ({target}); set {TARGET_ENV}"
        raise WheelConfigError(message)
    return target


def platform_tag(target: str, glibc: str | None) -> str:
    """The wheel platform tag of a release target.

    Args:
        target: A key of :data:`PLATFORM_TAGS`.
        glibc: The binary's glibc floor (``2.34``), for a ``*-linux-gnu`` target.

    Returns:
        The platform tag (``manylinux_2_34_x86_64``).

    Raises:
        WheelConfigError: An unknown target, or a gnu target without a valid glibc floor.
    """
    template = PLATFORM_TAGS.get(target)
    if template is None:
        known = ", ".join(sorted(PLATFORM_TAGS))
        message = f"{TARGET_ENV}: {target} is not a release target ({known})"
        raise WheelConfigError(message)
    if "{glibc}" not in template:
        return template
    matched = _GLIBC.match(glibc or "")
    if matched is None:
        message = f"{GLIBC_ENV}: {target} needs the binary's glibc floor as 2.N, got {glibc!r}"
        raise WheelConfigError(message)
    return template.format(glibc=f"2_{matched.group(1)}")


def wheel_plan(env: Mapping[str, str]) -> tuple[Path, str, str]:
    """What to bundle and how to tag it.

    Args:
        env: The environment.

    Returns:
        ``(binary, path inside the wheel, wheel tag)``.

    Raises:
        WheelConfigError: No binary, an unknown target, or a gnu target without a glibc floor.
    """
    binary = env.get(BINARY_ENV, "")
    if not binary:
        message = (
            f"{BINARY_ENV} must name the rulebearing binary to bundle; a wheel without one "
            "would install a command that cannot run"
        )
        raise WheelConfigError(message)
    source = Path(binary)
    if not source.is_file():
        message = f"{BINARY_ENV}: {binary} does not exist"
        raise WheelConfigError(message)
    target = env.get(TARGET_ENV, "") or host_target()
    glibc = env.get(GLIBC_ENV, "") or None
    if glibc is None and target.endswith("-linux-gnu") and target == host_target():
        glibc = platform.libc_ver()[1] or None
    name = "rulebearing.exe" if target.endswith("-windows-msvc") else "rulebearing"
    return source, f"rulebearing/bin/{name}", f"py3-none-{platform_tag(target, glibc)}"


class CustomMetadataHook(MetadataHookInterface):
    """Stamps the release version into the package metadata."""

    PLUGIN_NAME = "custom"

    def update(self, metadata: dict[str, Any]) -> None:
        """Set ``version`` (declared dynamic in pyproject.toml)."""
        metadata["version"] = release_version(os.environ, Path(self.root))


class CustomBuildHook(BuildHookInterface):  # type: ignore[type-arg]
    """Bundles the binary and tags the wheel with its platform."""

    PLUGIN_NAME = "custom"

    def initialize(self, version: str, build_data: dict[str, Any]) -> None:
        """Add the binary and the platform tag to a wheel build.

        An editable install (``pip install -e``) carries no binary: the launcher then runs the
        one ``RULEBEARING_BINARY`` names. An sdist is not published and is left alone.
        """
        if self.target_name != "wheel" or version == "editable":
            return
        source, inside, tag = wheel_plan(os.environ)
        build_data["force_include"][str(source)] = inside
        build_data["pure_python"] = False
        build_data["infer_tag"] = False
        build_data["tag"] = tag
