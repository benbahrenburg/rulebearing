# Copyright (c) 2026 Ben Bahrenburg. MIT License.
"""The wheel build hook: the version it stamps, the binary it bundles, the platform tag it writes.

Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md § 2.14, Step 14 (2H).
Targets: docs/architecture.md#distribution. Procedure: docs/release.md.
"""

from __future__ import annotations

import platform
from pathlib import Path
from typing import TYPE_CHECKING, Any

import pytest

if TYPE_CHECKING:
    from types import ModuleType

PACKAGE = Path(__file__).resolve().parent.parent
REPOSITORY = PACKAGE.parent.parent


@pytest.mark.parametrize(
    ("given", "expected"),
    [
        ("0.2.0", "0.2.0"),
        ("v0.2.0", "0.2.0"),
        ("0.2.0-rc.1", "0.2.0rc1"),
        ("v1.0.0-beta.2", "1.0.0b2"),
        ("1.2.3-alpha.4", "1.2.3a4"),
    ],
)
def test_pep440_spells_the_release_version_as_pypi_does(
    pip_hatch_build: ModuleType,
    given: str,
    expected: str,
) -> None:
    assert pip_hatch_build.pep440(given) == expected


def test_a_version_pep440_cannot_express_is_refused(pip_hatch_build: ModuleType) -> None:
    with pytest.raises(pip_hatch_build.WheelConfigError, match="not a release version"):
        pip_hatch_build.pep440("next")


def test_the_workspace_version_is_the_release_version(pip_hatch_build: ModuleType) -> None:
    text = (REPOSITORY / "Cargo.toml").read_text(encoding="utf-8")
    version = pip_hatch_build.workspace_version(REPOSITORY / "Cargo.toml")
    assert f'version = "{version}"' in text
    assert pip_hatch_build.release_version({}, PACKAGE) == pip_hatch_build.pep440(version)


def test_the_environment_overrides_the_workspace_version(pip_hatch_build: ModuleType) -> None:
    assert pip_hatch_build.release_version({"RULEBEARING_VERSION": "v9.8.7"}, PACKAGE) == "9.8.7"


def test_a_missing_manifest_asks_for_the_variable(
    pip_hatch_build: ModuleType, tmp_path: Path
) -> None:
    with pytest.raises(pip_hatch_build.WheelConfigError, match="set RULEBEARING_VERSION"):
        pip_hatch_build.release_version({}, tmp_path / "a" / "b")


def test_a_manifest_without_a_workspace_version_is_refused(
    pip_hatch_build: ModuleType,
    tmp_path: Path,
) -> None:
    manifest = tmp_path / "Cargo.toml"
    manifest.write_text("[workspace]\nmembers = []\n", encoding="utf-8")
    with pytest.raises(pip_hatch_build.WheelConfigError, match=r"no \[workspace.package\] version"):
        pip_hatch_build.workspace_version(manifest)


@pytest.mark.parametrize(
    ("system", "machine", "libc", "target"),
    [
        ("Linux", "x86_64", "glibc", "x86_64-unknown-linux-gnu"),
        ("Linux", "aarch64", "glibc", "aarch64-unknown-linux-gnu"),
        ("Linux", "x86_64", "", "x86_64-unknown-linux-musl"),
        ("Darwin", "arm64", "", "aarch64-apple-darwin"),
        ("Darwin", "x86_64", "", "x86_64-apple-darwin"),
        ("Windows", "AMD64", "", "x86_64-pc-windows-msvc"),
    ],
)
def test_host_target_maps_the_six_release_hosts(
    pip_hatch_build: ModuleType,
    system: str,
    machine: str,
    libc: str,
    target: str,
) -> None:
    assert pip_hatch_build.host_target(system, machine, libc) == target


@pytest.mark.parametrize(
    ("system", "machine"),
    [("Linux", "riscv64"), ("FreeBSD", "amd64"), ("Windows", "ARM64")],
)
def test_a_host_without_a_release_target_is_named(
    pip_hatch_build: ModuleType,
    system: str,
    machine: str,
) -> None:
    with pytest.raises(pip_hatch_build.WheelConfigError, match="set RULEBEARING_WHEEL_TARGET"):
        pip_hatch_build.host_target(system, machine, "glibc")


def test_host_target_reads_this_host(pip_hatch_build: ModuleType) -> None:
    detected = pip_hatch_build.host_target()
    assert detected in pip_hatch_build.PLATFORM_TAGS
    arch = platform.machine().lower()
    assert detected.startswith("aarch64" if arch in {"arm64", "aarch64"} else "x86_64")


@pytest.mark.parametrize(
    ("target", "glibc", "tag"),
    [
        ("x86_64-unknown-linux-gnu", "2.34", "manylinux_2_34_x86_64"),
        ("aarch64-unknown-linux-gnu", "2.17", "manylinux_2_17_aarch64"),
        ("x86_64-unknown-linux-musl", None, "musllinux_1_1_x86_64"),
        ("aarch64-apple-darwin", None, "macosx_11_0_arm64"),
        ("x86_64-apple-darwin", None, "macosx_10_12_x86_64"),
        ("x86_64-pc-windows-msvc", None, "win_amd64"),
    ],
)
def test_platform_tag_per_release_target(
    pip_hatch_build: ModuleType,
    target: str,
    glibc: str | None,
    tag: str,
) -> None:
    assert pip_hatch_build.platform_tag(target, glibc) == tag


def test_every_release_target_has_a_tag(pip_hatch_build: ModuleType) -> None:
    targets = {
        "x86_64-unknown-linux-gnu",
        "x86_64-unknown-linux-musl",
        "aarch64-unknown-linux-gnu",
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
        "x86_64-pc-windows-msvc",
    }
    assert set(pip_hatch_build.PLATFORM_TAGS) == targets


@pytest.mark.parametrize("glibc", [None, "", "2", "3.1", "2.x"])
def test_a_gnu_target_needs_a_glibc_floor(pip_hatch_build: ModuleType, glibc: str | None) -> None:
    with pytest.raises(pip_hatch_build.WheelConfigError, match="glibc floor"):
        pip_hatch_build.platform_tag("x86_64-unknown-linux-gnu", glibc)


def test_an_unknown_target_lists_the_known_ones(pip_hatch_build: ModuleType) -> None:
    with pytest.raises(pip_hatch_build.WheelConfigError, match="x86_64-pc-windows-msvc"):
        pip_hatch_build.platform_tag("mips-unknown-linux-gnu", "2.17")


def test_a_wheel_needs_a_binary(pip_hatch_build: ModuleType) -> None:
    with pytest.raises(
        pip_hatch_build.WheelConfigError, match="RULEBEARING_WHEEL_BINARY must name"
    ):
        pip_hatch_build.wheel_plan({})


def test_a_binary_that_does_not_exist_is_named(pip_hatch_build: ModuleType, tmp_path: Path) -> None:
    with pytest.raises(pip_hatch_build.WheelConfigError, match="does not exist"):
        pip_hatch_build.wheel_plan({"RULEBEARING_WHEEL_BINARY": str(tmp_path / "rb")})


def test_the_plan_for_a_cross_built_target(pip_hatch_build: ModuleType, tmp_path: Path) -> None:
    binary = tmp_path / "rulebearing.exe"
    binary.write_text("")
    source, inside, tag = pip_hatch_build.wheel_plan(
        {
            "RULEBEARING_WHEEL_BINARY": str(binary),
            "RULEBEARING_WHEEL_TARGET": "x86_64-pc-windows-msvc",
        },
    )
    assert (source, inside, tag) == (
        binary,
        "rulebearing/bin/rulebearing.exe",
        "py3-none-win_amd64",
    )
    _, inside, tag = pip_hatch_build.wheel_plan(
        {
            "RULEBEARING_WHEEL_BINARY": str(binary),
            "RULEBEARING_WHEEL_TARGET": "aarch64-unknown-linux-gnu",
            "RULEBEARING_WHEEL_GLIBC": "2.39",
        },
    )
    assert (inside, tag) == ("rulebearing/bin/rulebearing", "py3-none-manylinux_2_39_aarch64")


def test_the_plan_for_the_host(pip_hatch_build: ModuleType, tmp_path: Path) -> None:
    binary = tmp_path / "rulebearing"
    binary.write_text("")
    _, _, tag = pip_hatch_build.wheel_plan({"RULEBEARING_WHEEL_BINARY": str(binary)})
    host = pip_hatch_build.host_target()
    glibc = platform.libc_ver()[1] if host.endswith("-linux-gnu") else None
    assert tag == f"py3-none-{pip_hatch_build.platform_tag(host, glibc)}"


def test_the_metadata_hook_stamps_the_version(
    pip_hatch_build: ModuleType,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setenv("RULEBEARING_VERSION", "3.2.1-rc.4")
    metadata: dict[str, Any] = {"name": "rulebearing"}
    pip_hatch_build.CustomMetadataHook(str(PACKAGE), {}).update(metadata)
    assert metadata["version"] == "3.2.1rc4"


def _build_hook(pip_hatch_build: ModuleType, target_name: str) -> Any:  # noqa: ANN401
    return pip_hatch_build.CustomBuildHook(str(PACKAGE), {}, None, None, str(PACKAGE), target_name)


def test_the_build_hook_bundles_and_tags_a_wheel(
    pip_hatch_build: ModuleType,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    binary = tmp_path / "rulebearing"
    binary.write_text("")
    monkeypatch.setenv("RULEBEARING_WHEEL_BINARY", str(binary))
    monkeypatch.setenv("RULEBEARING_WHEEL_TARGET", "x86_64-unknown-linux-musl")
    build_data: dict[str, Any] = {"force_include": {}, "pure_python": True, "infer_tag": True}
    _build_hook(pip_hatch_build, "wheel").initialize("standard", build_data)
    assert build_data == {
        "force_include": {str(binary): "rulebearing/bin/rulebearing"},
        "pure_python": False,
        "infer_tag": False,
        "tag": "py3-none-musllinux_1_1_x86_64",
    }


@pytest.mark.parametrize(("target_name", "version"), [("wheel", "editable"), ("sdist", "standard")])
def test_the_build_hook_leaves_editable_and_sdist_builds_alone(
    pip_hatch_build: ModuleType,
    monkeypatch: pytest.MonkeyPatch,
    target_name: str,
    version: str,
) -> None:
    monkeypatch.delenv("RULEBEARING_WHEEL_BINARY", raising=False)
    build_data: dict[str, Any] = {"force_include": {}}
    _build_hook(pip_hatch_build, target_name).initialize(version, build_data)
    assert build_data == {"force_include": {}}
