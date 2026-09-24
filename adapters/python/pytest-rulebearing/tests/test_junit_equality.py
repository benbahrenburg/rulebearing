# Copyright (c) 2026 Ben Bahrenburg. MIT License.
"""The proof the plan asks for: every failure message is the junit message for that rule.

On the shared fixture (adapters/fixture), ``rulebearing cruise -T junit`` writes one test case
per rule. For each, the ``message`` attribute of its ``<failure>`` and then of each ``<error>``,
joined by newlines, must equal the failure text pytest reports for that rule's item; a case with
neither must pass. Contract: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md § 1.5;
Step 14 (2H); ADR-0007 for the vacuous rule.
"""

from __future__ import annotations

import subprocess
import xml.etree.ElementTree as ET
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from pathlib import Path

    import pytest


def _junit_messages(binary: Path, project: Path) -> dict[str, str]:
    completed = subprocess.run(  # noqa: S603 - the locally built binary over the test's own copy
        [str(binary), "cruise", "-T", "junit", "--no-progress"],
        cwd=project,
        capture_output=True,
        check=False,
        encoding="utf-8",
    )
    root = ET.fromstring(completed.stdout)  # noqa: S314 - the binary's own output
    messages: dict[str, str] = {}
    for case in root.iter("testcase"):
        parts = [f.get("message", "") for f in case.findall("failure")]
        parts += [e.get("message", "") for e in case.findall("error")]
        messages[case.get("name", "")] = "\n".join(parts)
    return messages


def test_each_failure_message_is_the_junit_message(
    pytester: pytest.Pytester,
    local_binary: Path,
    project: Path,
) -> None:
    expected = _junit_messages(local_binary, project)
    assert set(expected) == {
        "handlers-not-to-util",
        "api-not-to-util",
        "nothing-matches",
        "util-is-a-leaf",
        "classes-are-pascal-case",
    }
    recorder = pytester.inline_run(
        "-p",
        "pytest_rulebearing.plugin",
        "--rulebearing",
        f"--rulebearing-binary={local_binary}",
    )
    reports = {
        r.nodeid.split("::")[-1]: r
        for r in recorder.getreports("pytest_runtest_logreport")
        if r.when == "call"
    }
    assert set(reports) == set(expected)
    for name, text in expected.items():
        report = reports[name]
        if text:
            assert report.failed, name
            assert report.longreprtext == text, name
        else:
            assert report.passed, name
    assert (
        "matched nothing, so it checks nothing (ADR-0007)"
        in reports["nothing-matches"].longreprtext
    )
    assert reports["handlers-not-to-util"].longreprtext.endswith("\n... and 2 more")
