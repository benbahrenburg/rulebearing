# Copyright (c) 2026 Ben Bahrenburg. MIT License.
"""pack.sh packs the rule library at one version, and each package resolves once installed.

The packages are npm's, NuGet's and PyPI's, built from the presets in the tree; installed, each is
where ``extends: rulebearing-rules/nextjs`` finds it. The release workflow's install checks do the
same with the packages a run built, on macOS, Linux and Windows (docs/release.md).

- Plan: docs/plans/implemented/0003-wave-3-operations-surface-inner-loop.md, Step 23 ("the release
  dry run packs the three library packages and installs each in an empty project, where
  ``extends: rulebearing-rules/nextjs`` resolves").
- Decision: docs/adr/0071-the-rule-library-ships-from-this-repository.md.
- Layouts: crates/rb-config/src/packages.rs.
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import tarfile
import venv
import zipfile
from pathlib import Path

import pytest

PACKAGE = Path(__file__).resolve().parent.parent
REPOSITORY = PACKAGE.parent.parent
PACK = PACKAGE / "pack.sh"
#: A version no release carries, so a package that shows it was stamped by this run.
VERSION = "9.8.7"
#: A rule of the nextjs preset, which a run that resolved the library reports.
NEXTJS_RULE = "nextjs-no-import-of-route-entries"


def _library() -> dict[str, bytes]:
    """Every file the library carries and its bytes, read from the tree."""
    files = {p.name: p.read_bytes() for p in (REPOSITORY / "presets" / "frameworks").glob("*.yaml")}
    recommended = REPOSITORY / "presets" / "rulebearing" / "recommended.yaml"
    files["recommended.yaml"] = recommended.read_bytes()
    return files


def _run(
    command: list[str],
    *,
    cwd: Path | None = None,
    env: dict[str, str] | None = None,
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(  # noqa: S603 - fixed commands over paths this test created
        command,
        capture_output=True,
        check=False,
        encoding="utf-8",
        cwd=cwd,
        env=env,
    )


def _ok(
    command: list[str],
    *,
    cwd: Path | None = None,
    env: dict[str, str] | None = None,
) -> str:
    completed = _run(command, cwd=cwd, env=env)
    assert completed.returncode == 0, completed.stdout + completed.stderr
    return completed.stdout


def _tool(name: str) -> str:
    found = shutil.which(name)
    if found is None:
        pytest.fail(f"{name} is not on the path: packing the rule library needs npm and dotnet")
    return found


@pytest.fixture(scope="module")
def packed(tmp_path_factory: pytest.TempPathFactory) -> dict[str, Path]:
    """The three packages, packed once by pack.sh at :data:`VERSION`."""
    _tool("npm")
    _tool("dotnet")
    out = tmp_path_factory.mktemp("packed")
    env = {**os.environ, "PYTHON": sys.executable}
    env.pop("RULEBEARING_VERSION", None)
    printed = _ok(["bash", str(PACK), f"v{VERSION}", str(out)], env=env).split()
    names = sorted(Path(p).name for p in printed)
    assert names == [
        f"Rulebearing.Rules.{VERSION}.nupkg",
        f"rulebearing-rules-{VERSION}.tgz",
        f"rulebearing_rules-{VERSION}-py3-none-any.whl",
        f"rulebearing_rules-{VERSION}.tar.gz",
    ]
    return {
        "npm": out / f"rulebearing-rules-{VERSION}.tgz",
        "nuget": out / f"Rulebearing.Rules.{VERSION}.nupkg",
        "wheel": out / f"rulebearing_rules-{VERSION}-py3-none-any.whl",
        "sdist": out / f"rulebearing_rules-{VERSION}.tar.gz",
    }


def _yaml_in_zip(path: Path, folder: str) -> dict[str, bytes]:
    with zipfile.ZipFile(path) as archive:
        return {
            name.removeprefix(folder): archive.read(name)
            for name in archive.namelist()
            if name.startswith(folder) and name.endswith(".yaml")
        }


def _yaml_in_tar(path: Path, folder: str) -> dict[str, bytes]:
    found: dict[str, bytes] = {}
    with tarfile.open(path) as archive:
        for member in archive.getmembers():
            if member.name.startswith(folder) and member.name.endswith(".yaml"):
                extracted = archive.extractfile(member)
                assert extracted is not None
                found[member.name.removeprefix(folder)] = extracted.read()
    return found


def test_the_npm_package_carries_the_presets_at_its_root(packed: dict[str, Path]) -> None:
    assert _yaml_in_tar(packed["npm"], "package/") == _library()
    with tarfile.open(packed["npm"]) as archive:
        names = set(archive.getnames())
        manifest = archive.extractfile("package/package.json")
        assert manifest is not None
        stamped = json.load(manifest)
    assert {"package/README.md", "package/LICENSE"} <= names
    assert stamped["name"] == "rulebearing-rules"
    assert stamped["version"] == VERSION
    # The source tree's manifest is copied, never edited.
    assert "version" not in json.loads((PACKAGE / "package.json").read_text(encoding="utf-8"))


def test_the_nuget_package_carries_the_presets_under_the_library_folder(
    packed: dict[str, Path],
) -> None:
    assert _yaml_in_zip(packed["nuget"], "rulebearing-rules/") == _library()
    with zipfile.ZipFile(packed["nuget"]) as archive:
        nuspec = archive.read("Rulebearing.Rules.nuspec").decode("utf-8")
        names = archive.namelist()
    assert f"<version>{VERSION}</version>" in nuspec
    assert '<license type="expression">MIT</license>' in nuspec
    assert not any(n.startswith("lib/") for n in names), "a content package carries no assembly"


def test_the_wheel_and_the_sdist_carry_the_presets_in_the_package(
    packed: dict[str, Path],
) -> None:
    assert _yaml_in_zip(packed["wheel"], "rulebearing_rules/") == _library()
    sdist = f"rulebearing_rules-{VERSION}/rulebearing_rules/"
    assert _yaml_in_tar(packed["sdist"], sdist) == _library()
    with zipfile.ZipFile(packed["wheel"]) as archive:
        metadata = archive.read(f"rulebearing_rules-{VERSION}.dist-info/METADATA").decode()
        names = archive.namelist()
    assert f"\nVersion: {VERSION}\n" in metadata
    assert not any(n.startswith(("tests/", "hatch_rules")) for n in names)


def test_a_wheel_built_from_the_sdist_alone_is_the_same(
    packed: dict[str, Path],
    tmp_path: Path,
) -> None:
    with tarfile.open(packed["sdist"]) as archive:
        archive.extractall(tmp_path, filter="data")
    unpacked = tmp_path / f"rulebearing_rules-{VERSION}"
    env = {k: v for k, v in os.environ.items() if k != "RULEBEARING_VERSION"}
    dist = tmp_path / "dist"
    _ok(
        [sys.executable, "-m", "build", "--wheel", "--no-isolation", "--outdir", str(dist)],
        cwd=unpacked,
        env=env,
    )
    wheel = dist / f"rulebearing_rules-{VERSION}-py3-none-any.whl"
    assert _yaml_in_zip(wheel, "rulebearing_rules/") == _library()


def test_an_unknown_registry_and_a_bad_version_are_refused(tmp_path: Path) -> None:
    unknown = _run(["bash", str(PACK), "1.0.0", str(tmp_path), "maven"])
    assert unknown.returncode == 2
    assert "unknown registry maven" in unknown.stderr
    bad = _run(["bash", str(PACK), "next", str(tmp_path), "npm"])
    assert bad.returncode == 2
    assert "not a release version: next" in bad.stderr
    usage = _run(["bash", str(PACK), "1.0.0"])
    assert usage.returncode == 2
    assert "usage:" in usage.stderr
    assert not any(tmp_path.iterdir())


def _repository(tmp_path: Path) -> Path:
    """A repository whose configuration extends the library, with a folder for its packages."""
    repo = tmp_path / "repo"
    repo.mkdir()
    (repo / "rulebearing.yaml").write_text("extends: rulebearing-rules/nextjs\n")
    return repo


def _test_rules(binary: Path, repo: Path, env: dict[str, str]) -> subprocess.CompletedProcess[str]:
    """``rulebearing test`` in ``repo`` with only the package locations ``env`` names."""
    clean = {
        k: v for k, v in os.environ.items() if k not in {"NUGET_PACKAGES", "VIRTUAL_ENV", "HOME"}
    }
    clean["HOME"] = str(repo / "no-home")
    clean["USERPROFILE"] = str(repo / "no-home")
    return _run([str(binary), "test"], cwd=repo, env={**clean, **env})


def _assert_resolves(binary: Path, repo: Path, env: dict[str, str]) -> None:
    completed = _test_rules(binary, repo, env)
    assert completed.returncode == 0, completed.stdout + completed.stderr
    assert NEXTJS_RULE in completed.stdout
    assert "0 failing" in completed.stdout


def test_without_the_package_the_library_is_named_as_missing(
    local_binary: Path,
    tmp_path: Path,
) -> None:
    completed = _test_rules(local_binary, _repository(tmp_path), {})
    assert completed.returncode == 3
    assert "rulebearing-rules/nextjs" in completed.stderr


def test_the_npm_package_resolves_from_node_modules(
    local_binary: Path,
    packed: dict[str, Path],
    tmp_path: Path,
) -> None:
    repo = _repository(tmp_path)
    (repo / "package.json").write_text('{ "name": "fixture", "private": true }\n')
    _ok([_tool("npm"), "install", "--no-audit", "--no-fund", str(packed["npm"])], cwd=repo)
    _assert_resolves(local_binary, repo, {})


def test_the_nuget_package_resolves_from_the_global_packages_folder(
    local_binary: Path,
    packed: dict[str, Path],
    tmp_path: Path,
) -> None:
    repo = _repository(tmp_path)
    (repo / "nuget.config").write_text(
        '<?xml version="1.0" encoding="utf-8"?>\n<configuration>\n  <packageSources>\n'
        f'    <clear />\n    <add key="built" value="{packed["nuget"].parent}" />\n'
        "  </packageSources>\n</configuration>\n",
    )
    (repo / "Consumer.csproj").write_text(
        '<Project Sdk="Microsoft.NET.Sdk">\n'
        "  <PropertyGroup><TargetFramework>net10.0</TargetFramework></PropertyGroup>\n"
        f'  <ItemGroup><PackageReference Include="Rulebearing.Rules" Version="{VERSION}" />'
        "</ItemGroup>\n</Project>\n",
    )
    packages = tmp_path / "nuget-packages"
    env = {**os.environ, "NUGET_PACKAGES": str(packages)}
    _ok([_tool("dotnet"), "restore", "--configfile", str(repo / "nuget.config")], cwd=repo, env=env)
    assert (
        packages / "rulebearing.rules" / VERSION / "rulebearing-rules" / "nextjs.yaml"
    ).is_file()
    _assert_resolves(local_binary, repo, {"NUGET_PACKAGES": str(packages)})


def test_the_wheel_resolves_from_site_packages(
    local_binary: Path,
    packed: dict[str, Path],
    tmp_path: Path,
) -> None:
    repo = _repository(tmp_path)
    environment = tmp_path / "env"
    venv.create(environment, with_pip=True)
    scripts = environment / ("Scripts" if os.name == "nt" else "bin")
    python = scripts / ("python.exe" if os.name == "nt" else "python")
    _ok([str(python), "-m", "pip", "install", "--quiet", "--no-index", str(packed["wheel"])])
    _assert_resolves(local_binary, repo, {"VIRTUAL_ENV": str(environment)})
