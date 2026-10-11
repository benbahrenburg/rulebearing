# Copyright (c) 2026 Ben Bahrenburg. MIT License.
"""Rulebearing's rule library: preset files read by ``extends: rulebearing-rules/<preset>``.

The package holds data, not code: each preset is ``<preset>.yaml`` beside this file, where
Rulebearing looks for it in a Python environment's ``site-packages`` (docs/config.md, "extends").
Source: presets/frameworks/ and presets/rulebearing/recommended.yaml of the Rulebearing
repository, added at build time by hatch_rules.py
(docs/adr/0071-the-rule-library-ships-from-this-repository.md).
"""
