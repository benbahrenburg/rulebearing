# Copyright (c) 2026 Ben Bahrenburg. MIT License.
"""pytest-rulebearing: each Rulebearing architecture rule as a test in the pytest run.

- Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md § 2.14, Step 14 (2H).
- Coverage: docs/artifacts/archunitnet-0.13.4-coverage.md § Test framework adapters.
- Requirement: FR-DIST-03 (docs/prd.md).

The plugin itself is :mod:`pytest_rulebearing.plugin`, registered through the ``pytest11`` entry
point. :func:`cases` and :func:`message` are public for a runner that wants the same messages
without pytest.
"""

from pytest_rulebearing.cases import Case, Rule, cases, message

__all__ = ["Case", "Rule", "cases", "message"]
