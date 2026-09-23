#!/usr/bin/env bash
# Writes the workspace's crate graph as a Rulebearing graph document on stdout: one module per
# workspace crate (named by its manifest, crates/<name>/Cargo.toml), one edge per normal
# dependency (not dev, not build) on another workspace crate. The repository's own rulebearing.yaml
# is evaluated over it by `rulebearing cruise --graph`, because Rust is not an extracted language.
# Plan: docs/plans/pending/0001-wave-1-typescript-parity.md, Step 9. Decision: docs/adr/0010-crate-layout-and-extractor-boundary.md.
# Usage: scripts/cargo-graph.sh > target/cargo-graph.json
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
cargo metadata --format-version 1 --no-deps | python3 -c '
import json, os, sys
metadata = json.load(sys.stdin)
root = metadata["workspace_root"]
members = set(metadata["workspace_members"])
packages = [p for p in metadata["packages"] if p["id"] in members]
manifest = {p["name"]: os.path.relpath(p["manifest_path"], root).replace(os.sep, "/") for p in packages}
modules = []
for package in sorted(packages, key=lambda p: manifest[p["name"]]):
    dependencies = []
    for dependency in package["dependencies"]:
        if dependency.get("kind") is not None or dependency["name"] not in manifest:
            continue
        dependencies.append({
            "module": dependency["name"], "resolved": manifest[dependency["name"]],
            "coreModule": False, "dependencyTypes": ["local"], "followable": True, "dynamic": False,
            "exoticallyRequired": False, "couldNotResolve": False, "circular": False,
            "moduleSystem": "cjs", "valid": True,
        })
    dependencies.sort(key=lambda d: d["resolved"])
    modules.append({"source": manifest[package["name"]], "dependencies": dependencies, "valid": True})
summary = {"violations": [], "error": 0, "warn": 0, "info": 0, "totalCruised": len(modules), "optionsUsed": {}}
json.dump({"modules": modules, "summary": summary}, sys.stdout, indent=2)
print()
'
