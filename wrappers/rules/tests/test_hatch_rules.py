# Copyright (c) 2026 Ben Bahrenburg. MIT License.
"""The rule library's build hook: the presets it adds and the version it stamps.

Plan: docs/plans/implemented/0003-wave-3-operations-surface-inner-loop.md, Step 23.
Decision: docs/adr/0071-the-rule-library-ships-from-this-repository.md. Procedure: docs/release.md.
"""

from __future__ import annotations

from pathlib import Path
from typing import TYPE_CHECKING, Any

import pytest
import rulebearing_rules

if TYPE_CHECKING:
    from types import ModuleType

PACKAGE = Path(__file__).resolve().parent.parent
REPOSITORY = PACKAGE.parent.parent


def _library() -> list[str]:
    """The files the library must carry, read from the tree independently of the hook."""
    frameworks = [p.name for p in (REPOSITORY / "presets" / "frameworks").glob("*.yaml")]
    return sorted([*frameworks, "recommended.yaml"])


def test_the_library_is_every_framework_preset_and_recommended(
    rules_hatch_build: ModuleType,
) -> None:
    found = rules_hatch_build.presets(REPOSITORY)
    assert list(found) == _library()
    assert {"nextjs.yaml", "clean-architecture.yaml", "recommended.yaml"} <= set(found)
    assert found["recommended.yaml"] == REPOSITORY / "presets" / "rulebearing" / "recommended.yaml"
    assert found["nextjs.yaml"] == REPOSITORY / "presets" / "frameworks" / "nextjs.yaml"


def _repository(tmp_path: Path, frameworks: list[str], *, recommended: bool) -> Path:
    for name in frameworks:
        path = tmp_path / "presets" / "frameworks" / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text("rules: {}\n")
    if recommended:
        path = tmp_path / "presets" / "rulebearing" / "recommended.yaml"
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text("rules: {}\n")
    return tmp_path


def test_no_framework_presets_is_refused(rules_hatch_build: ModuleType, tmp_path: Path) -> None:
    repository = _repository(tmp_path, [], recommended=True)
    with pytest.raises(rules_hatch_build.RulesBuildError, match="no presets under"):
        rules_hatch_build.presets(repository)


def test_a_missing_recommended_is_refused(rules_hatch_build: ModuleType, tmp_path: Path) -> None:
    repository = _repository(tmp_path, ["a.yaml"], recommended=False)
    with pytest.raises(rules_hatch_build.RulesBuildError, match=r"recommended\.yaml is missing"):
        rules_hatch_build.presets(repository)


def test_a_framework_preset_cannot_replace_recommended(
    rules_hatch_build: ModuleType,
    tmp_path: Path,
) -> None:
    repository = _repository(tmp_path, ["recommended.yaml"], recommended=True)
    with pytest.raises(rules_hatch_build.RulesBuildError, match="would replace"):
        rules_hatch_build.presets(repository)


def test_the_presets_are_sorted_by_name(rules_hatch_build: ModuleType, tmp_path: Path) -> None:
    repository = _repository(tmp_path, ["zeta.yaml", "alpha.yaml"], recommended=True)
    assert list(rules_hatch_build.presets(repository)) == [
        "alpha.yaml",
        "recommended.yaml",
        "zeta.yaml",
    ]


@pytest.mark.parametrize(
    ("given", "expected"),
    [
        ("0.2.0", "0.2.0"),
        ("v0.2.0", "0.2.0"),
        ("0.2.0-rc.1", "0.2.0rc1"),
        ("v1.0.0-beta.2", "1.0.0b2"),
    ],
)
def test_pep440_spells_the_release_version_as_pypi_does(
    rules_hatch_build: ModuleType,
    given: str,
    expected: str,
) -> None:
    assert rules_hatch_build.pep440(given) == expected


def test_a_version_pep440_cannot_express_is_refused(rules_hatch_build: ModuleType) -> None:
    with pytest.raises(rules_hatch_build.RulesBuildError, match="not a release version: next"):
        rules_hatch_build.pep440("next")


def test_the_workspace_version_is_the_release_version(rules_hatch_build: ModuleType) -> None:
    cargo = (REPOSITORY / "Cargo.toml").read_text(encoding="utf-8")
    version = rules_hatch_build.release_version({}, PACKAGE)
    assert f'version = "{version}"' in cargo


def test_the_environment_overrides_the_workspace_version(rules_hatch_build: ModuleType) -> None:
    assert rules_hatch_build.release_version({"RULEBEARING_VERSION": "v7.0.0"}, PACKAGE) == "7.0.0"


def test_a_missing_manifest_asks_for_the_variable(
    rules_hatch_build: ModuleType,
    tmp_path: Path,
) -> None:
    with pytest.raises(rules_hatch_build.RulesBuildError, match="set RULEBEARING_VERSION"):
        rules_hatch_build.workspace_version(tmp_path / "Cargo.toml")


def test_a_manifest_without_a_workspace_version_is_refused(
    rules_hatch_build: ModuleType,
    tmp_path: Path,
) -> None:
    manifest = tmp_path / "Cargo.toml"
    manifest.write_text('[package]\nversion = "1.0.0"\n')
    with pytest.raises(rules_hatch_build.RulesBuildError, match=r"no \[workspace\.package\]"):
        rules_hatch_build.workspace_version(manifest)


def test_an_unpacked_source_distribution_keeps_its_version(
    rules_hatch_build: ModuleType,
    tmp_path: Path,
) -> None:
    (tmp_path / "PKG-INFO").write_text(
        "Metadata-Version: 2.4\nName: rulebearing-rules\nVersion: 0.3.0rc2\n",
    )
    env = {"RULEBEARING_VERSION": "9.9.9"}
    assert rules_hatch_build.release_version(env, tmp_path) == "0.3.0rc2"


def test_a_pkg_info_without_a_version_is_refused(
    rules_hatch_build: ModuleType,
    tmp_path: Path,
) -> None:
    (tmp_path / "PKG-INFO").write_text("Metadata-Version: 2.4\nName: rulebearing-rules\n")
    with pytest.raises(rules_hatch_build.RulesBuildError, match="has no Version"):
        rules_hatch_build.release_version({}, tmp_path)


def test_a_build_from_the_tree_adds_every_preset(rules_hatch_build: ModuleType) -> None:
    added = rules_hatch_build.force_include(PACKAGE)
    assert sorted(added.values()) == [f"rulebearing_rules/{name}" for name in _library()]
    for source, inside in added.items():
        assert Path(source).is_file()
        assert Path(source).name == Path(inside).name


def test_a_build_from_a_source_distribution_adds_nothing(
    rules_hatch_build: ModuleType,
    tmp_path: Path,
) -> None:
    (tmp_path / "PKG-INFO").write_text("Version: 1.0.0\n")
    assert rules_hatch_build.force_include(tmp_path) == {}


def test_the_metadata_hook_stamps_the_version(
    rules_hatch_build: ModuleType,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setenv("RULEBEARING_VERSION", "3.2.1-rc.4")
    metadata: dict[str, Any] = {"name": "rulebearing-rules"}
    rules_hatch_build.CustomMetadataHook(str(PACKAGE), {}).update(metadata)
    assert metadata["version"] == "3.2.1rc4"


@pytest.mark.parametrize("target_name", ["wheel", "sdist"])
def test_the_build_hook_adds_the_presets_to_both_distributions(
    rules_hatch_build: ModuleType,
    target_name: str,
) -> None:
    hook = rules_hatch_build.CustomBuildHook(
        str(PACKAGE),
        {},
        None,
        None,
        str(PACKAGE),
        target_name,
    )
    build_data: dict[str, Any] = {"force_include": {"kept": "kept"}}
    hook.initialize("standard", build_data)
    assert build_data["force_include"]["kept"] == "kept"
    assert len(build_data["force_include"]) == len(_library()) + 1


def test_the_presets_land_beside_the_package_module(rules_hatch_build: ModuleType) -> None:
    module_dir = Path(rulebearing_rules.__file__).parent
    assert module_dir.name == rules_hatch_build.MODULE
    assert module_dir == PACKAGE / "rulebearing_rules"
