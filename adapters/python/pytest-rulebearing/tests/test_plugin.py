# Copyright (c) 2026 Ben Bahrenburg. MIT License.
"""The plugin in a real pytest run: off by default, on by flag or ini, one item per rule.

Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md § 2.14, Step 14 (2H):
"collects one item per rule from ``rulebearing cruise --output-type json`` when ``--rulebearing``
is passed or ``[tool.pytest.ini_options] rulebearing = true``".
"""

from __future__ import annotations

import subprocess
import sys
from typing import TYPE_CHECKING

import pytest
from pytest_rulebearing import run

if TYPE_CHECKING:
    from pathlib import Path

PLUGIN = ("-p", "pytest_rulebearing.plugin")


def test_off_by_default(pytester: pytest.Pytester, project: Path) -> None:
    del project
    pytester.makepyfile(test_one="def test_one():\n    pass\n")
    result = pytester.runpytest(*PLUGIN)
    result.assert_outcomes(passed=1)


def test_on_by_flag_one_item_per_rule_in_junit_order(
    pytester: pytest.Pytester,
    project: Path,
    local_binary: Path,
) -> None:
    del project
    result = pytester.runpytest(*PLUGIN, "--rulebearing", f"--rulebearing-binary={local_binary}")
    result.assert_outcomes(passed=2, failed=3)
    collected = pytester.runpytest(
        *PLUGIN,
        "--rulebearing",
        f"--rulebearing-binary={local_binary}",
        "--collect-only",
        "-q",
    )
    assert collected.outlines[:5] == [
        "rulebearing::handlers-not-to-util",
        "rulebearing::api-not-to-util",
        "rulebearing::nothing-matches",
        "rulebearing::util-is-a-leaf",
        "rulebearing::classes-are-pascal-case",
    ]


def test_on_by_ini_with_a_configured_binary_and_arguments(
    pytester: pytest.Pytester,
    project: Path,
    local_binary: Path,
) -> None:
    del project
    pytester.makeini(
        f"""
        [pytest]
        rulebearing = true
        rulebearing_binary = {local_binary}
        rulebearing_config = rulebearing.yaml
        rulebearing_args = py
        """,
    )
    result = pytester.runpytest(*PLUGIN, "-m", "rulebearing", "-k", "leaf or api")
    result.assert_outcomes(passed=2, deselected=3)


def test_warn_findings_are_reported_in_a_section(
    pytester: pytest.Pytester,
    project: Path,
    local_binary: Path,
) -> None:
    del project
    recorder = pytester.inline_run(
        *PLUGIN,
        "--rulebearing",
        f"--rulebearing-binary={local_binary}",
        "--rulebearing-arg=py",
        "-k",
        "api",
    )
    (report,) = [r for r in recorder.getreports("pytest_runtest_logreport") if r.when == "call"]
    assert report.passed
    sections = dict(report.sections)
    assert sections["Captured rulebearing call"].startswith("warn: RB-")
    assert (
        "py/api/view.py -> py/app/util.py (line 1, column 17)"
        in sections["Captured rulebearing call"]
    )


def test_a_graph_document_gives_the_same_items_as_extraction(
    pytester: pytest.Pytester,
    project: Path,
    local_binary: Path,
) -> None:
    written = subprocess.run(  # noqa: S603 - the locally built binary over the test's own copy
        [str(local_binary), "cruise", "-T", "json", "--no-progress"],
        cwd=project,
        capture_output=True,
        check=False,
        encoding="utf-8",
    )
    (project / "graph.json").write_text(written.stdout, encoding="utf-8")
    outcomes = []
    for extra in ((), ("--rulebearing-graph=graph.json",)):
        recorder = pytester.inline_run(
            *PLUGIN,
            "--rulebearing",
            f"--rulebearing-binary={local_binary}",
            *extra,
        )
        reports = recorder.getreports("pytest_runtest_logreport")
        outcomes.append(
            [(r.nodeid, r.outcome, r.longreprtext) for r in reports if r.when == "call"]
        )
    assert outcomes[0] == outcomes[1]
    assert len(outcomes[0]) == 5


def test_a_binary_that_cannot_start_is_one_failing_item(
    pytester: pytest.Pytester,
    project: Path,
) -> None:
    del project
    recorder = pytester.inline_run(*PLUGIN, "--rulebearing", "--rulebearing-binary=no-such-rb")
    (report,) = [r for r in recorder.getreports("pytest_runtest_logreport") if r.when == "call"]
    assert report.failed
    assert report.nodeid == "rulebearing::cruise"
    assert "could not start" in report.longreprtext


def test_a_run_without_a_result_shows_the_binary_stderr(
    pytester: pytest.Pytester,
    project: Path,
    local_binary: Path,
) -> None:
    del project
    recorder = pytester.inline_run(
        *PLUGIN,
        "--rulebearing",
        f"--rulebearing-binary={local_binary}",
        "--rulebearing-config=missing.yaml",
    )
    (report,) = [r for r in recorder.getreports("pytest_runtest_logreport") if r.when == "call"]
    assert report.failed
    assert "without a result" in report.longreprtext
    assert "missing.yaml" in report.longreprtext


def test_resolve_binary_prefers_the_option_then_the_environment(tmp_path: Path) -> None:
    assert run.resolve_binary("given") == "given"
    binary = tmp_path / "rb"
    binary.write_text("")
    assert run.resolve_binary(None, {"RULEBEARING_BINARY": str(binary)}) == str(binary)
    with pytest.raises(run.CruiseError, match="RULEBEARING_BINARY is set to"):
        run.resolve_binary(None, {"RULEBEARING_BINARY": str(tmp_path / "absent")})


def test_the_command_line(tmp_path: Path) -> None:
    options = run.Options(config="c.yaml", graph="g.json", args=("src",), cwd=tmp_path)
    assert run.command(options, "rb") == [
        "rb",
        "cruise",
        "--output-type",
        "json",
        "--no-progress",
        "--config",
        "c.yaml",
        "--graph",
        "g.json",
        "src",
    ]
    assert run.command(run.Options(), "rb") == [
        "rb",
        "cruise",
        "--output-type",
        "json",
        "--no-progress",
    ]


def _fake_binary(tmp_path: Path, body: str) -> str:
    """A stand-in for the binary: a Python script run through its shebang (POSIX only)."""
    if sys.platform == "win32":
        pytest.skip("a shebang script is not executable on Windows")
    fake = tmp_path / "rb"
    fake.write_text(f"#!{sys.executable}\nimport sys\n{body}\n", encoding="utf-8")
    fake.chmod(0o755)
    return str(fake)


def test_cruise_reads_the_json_of_a_run_that_exits_2(tmp_path: Path) -> None:
    fake = _fake_binary(tmp_path, "print('{\"summary\": {}}')\nsys.exit(2)")
    assert run.cruise(run.Options(binary=fake, cwd=tmp_path), {"PATH": ""}) == {"summary": {}}


def test_cruise_refuses_json_without_a_summary(tmp_path: Path) -> None:
    fake = _fake_binary(tmp_path, "print('[1, 2]')\nsys.stderr.write('why')")
    with pytest.raises(run.CruiseError, match="exited 0 without a result:\nwhy"):
        run.cruise(run.Options(binary=fake, cwd=tmp_path))


def test_an_unexpected_error_keeps_pytest_report(
    pytester: pytest.Pytester,
    project: Path,
    local_binary: Path,
) -> None:
    del project
    pytester.makeconftest(
        """
        import pytest

        @pytest.hookimpl(hookwrapper=True)
        def pytest_runtest_call(item):
            raise ValueError("not a rule failure")
            yield
        """,
    )
    recorder = pytester.inline_run(
        *PLUGIN,
        "--rulebearing",
        f"--rulebearing-binary={local_binary}",
        "-k",
        "leaf",
    )
    (report,) = [r for r in recorder.getreports("pytest_runtest_logreport") if r.when == "call"]
    assert report.failed
    assert "ValueError: not a rule failure" in report.longreprtext
