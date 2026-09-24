# Copyright (c) 2026 Ben Bahrenburg. MIT License.
"""The plugin's metadata hook: the release version, and ``rulebearing`` pinned to exactly it.

Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md § 2.14, Step 14 (2H).
Decision: docs/adr/0020-single-name-across-registries.md (one version on every registry).
"""

from __future__ import annotations

from pathlib import Path
from typing import TYPE_CHECKING, Any

import pytest

if TYPE_CHECKING:
    from types import ModuleType

PACKAGE = Path(__file__).resolve().parent.parent
REPOSITORY = PACKAGE.parents[2]


def test_the_workspace_version_by_default(plugin_hatch_build: ModuleType) -> None:
    text = (REPOSITORY / "Cargo.toml").read_text(encoding="utf-8")
    version = plugin_hatch_build.release_version({}, PACKAGE)
    assert f'version = "{version}"' in text


@pytest.mark.parametrize(
    ("given", "expected"),
    [("v1.2.3", "1.2.3"), ("1.2.3-rc.2", "1.2.3rc2"), ("0.1.0", "0.1.0")],
)
def test_the_environment_version_normalised(
    plugin_hatch_build: ModuleType, given: str, expected: str
) -> None:
    assert plugin_hatch_build.release_version({"RULEBEARING_VERSION": given}, PACKAGE) == expected


def test_an_invalid_version_is_refused(plugin_hatch_build: ModuleType) -> None:
    with pytest.raises(plugin_hatch_build.VersionError, match="not a release version"):
        plugin_hatch_build.release_version({"RULEBEARING_VERSION": "soon"}, PACKAGE)


def test_no_manifest_asks_for_the_variable(plugin_hatch_build: ModuleType, tmp_path: Path) -> None:
    with pytest.raises(plugin_hatch_build.VersionError, match="set RULEBEARING_VERSION"):
        plugin_hatch_build.release_version({}, tmp_path / "a" / "b" / "c")


def test_a_manifest_without_a_workspace_version(
    plugin_hatch_build: ModuleType, tmp_path: Path
) -> None:
    package = tmp_path / "a" / "b" / "c"
    package.mkdir(parents=True)
    (tmp_path / "Cargo.toml").write_text("[package]\nname = 'x'\n", encoding="utf-8")
    with pytest.raises(plugin_hatch_build.VersionError, match=r"no \[workspace.package\] version"):
        plugin_hatch_build.release_version({}, package)


def test_the_hook_pins_rulebearing_to_the_same_version(
    plugin_hatch_build: ModuleType,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setenv("RULEBEARING_VERSION", "0.4.0-rc.1")
    metadata: dict[str, Any] = {}
    plugin_hatch_build.CustomMetadataHook(str(PACKAGE), {}).update(metadata)
    assert metadata == {
        "version": "0.4.0rc1",
        "dependencies": ["pytest>=8", "rulebearing==0.4.0rc1"],
    }
