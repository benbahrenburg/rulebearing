# Copyright (c) 2026 Ben Bahrenburg. MIT License.
"""Rulebearing on PyPI: the ``rulebearing`` binary on a Python project's path.

- Architecture: docs/architecture.md#distribution.
- Decision: docs/adr/0020-single-name-across-registries.md.
- Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md § 2.14, Step 14 (2H).
- Requirement: FR-DIST-01 (docs/prd.md).

The package is a launcher and nothing else (``rulebearing._launcher``). :func:`binary_path` is
public so that ``pytest-rulebearing`` runs the same binary the console script runs.
"""

from rulebearing._launcher import BINARY_OVERRIDE, BinaryNotFoundError, binary_path

__all__ = ["BINARY_OVERRIDE", "BinaryNotFoundError", "binary_path"]
