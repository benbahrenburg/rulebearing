# Copyright (c) 2026 Ben Bahrenburg. MIT licence: see LICENSE.
"""The Stop-hook benchmark's choice, timing, checks and results file (plan 0003, Step 17)."""

from __future__ import annotations

import json
import shutil
import subprocess
import sys
from typing import TYPE_CHECKING

import pytest
import stop_hook

if TYPE_CHECKING:
    from pathlib import Path

# A stand-in hook: reads the hook input, answers as `cruise --from-hook` does, and records the
# files it saw edited, so the test can check each run saw exactly its own edit.
HOOK = """
import json, pathlib, sys
data = json.loads(sys.stdin.read())
assert data == {"stop_hook_active": False}
edited = [p.name for p in pathlib.Path(".").rglob("*.cs") if b"benchmark" in p.read_bytes()]
with open("seen.log", "a") as log:
    log.write(",".join(sorted(edited)) + "\\n")
if len(sys.argv) > 1 and sys.argv[1] == "fail":
    sys.exit(3)
if len(sys.argv) > 1 and sys.argv[1] == "block":
    print(json.dumps({"decision": "block", "reason": "x"}))
"""


def repository(tmp_path: Path) -> Path:
    git = shutil.which("git")
    assert git is not None
    root = tmp_path / "repo"
    (root / "src" / "bin").mkdir(parents=True)
    for name in ["a.cs", "b.cs", "c.cs", "d.cs", "notes.txt"]:
        (root / "src" / name).write_text(f"// {name}\n")
    (root / "src" / "bin" / "e.cs").write_text("// build output\n")
    (root / "hook.py").write_text(HOOK)
    for command in (
        ["init", "-q"],
        ["add", "-A"],
        [
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "x",
        ],
    ):
        subprocess.run([git, *command], cwd=root, check=True)  # noqa: S603 - fixed argv
    return root


def test_percentile_is_the_nearest_rank() -> None:
    values = [float(v) for v in range(1, 101)]
    assert stop_hook.percentile(values, 0.5) == 51.0
    assert stop_hook.percentile(values, 0.95) == 95.0
    assert stop_hook.percentile(values, 0.99) == 99.0
    assert stop_hook.percentile([3.0], 0.95) == 3.0
    with pytest.raises(stop_hook.BenchmarkError):
        stop_hook.percentile([], 0.5)


def test_the_choice_is_the_seeds_and_skips_build_output(tmp_path: Path) -> None:
    root = repository(tmp_path)
    files = stop_hook.tracked(root, ".cs")
    assert files == ["src/a.cs", "src/b.cs", "src/c.cs", "src/d.cs"]
    assert stop_hook.choose(files, 2, 42) == stop_hook.choose(files, 2, 42)
    assert sorted(stop_hook.choose(files, 10, 1)) == files
    with pytest.raises(stop_hook.BenchmarkError):
        stop_hook.choose([], 1, 42)


def test_each_run_sees_its_own_edit_and_every_file_is_restored(tmp_path: Path) -> None:
    root = repository(tmp_path)
    out = tmp_path / "results" / "stop-hook.json"
    code = stop_hook.main(
        [
            "--repo",
            str(root),
            "--name",
            "owner/repo",
            "--bin",
            sys.executable,
            "--edits",
            "3",
            "--out",
            str(out),
            "--",
            "hook.py",
            "block",
        ],
    )
    assert code == 0
    seen = (root / "seen.log").read_text().splitlines()
    # Two warm-up runs with nothing edited, then one run per edit with only that file edited.
    assert seen[:2] == ["", ""]
    assert len(seen) == 5
    assert all(len(line.split(",")) == 1 and line.endswith(".cs") for line in seen[2:])
    for name in ["a.cs", "b.cs", "c.cs", "d.cs"]:
        assert (root / "src" / name).read_text() == f"// {name}\n"
    result = json.loads(out.read_text())
    assert result["repository"] == "owner/repo"
    assert result["edits"] == 3
    assert result["seed"] == 42
    assert result["extension"] == ".cs"
    assert set(result["seconds"]) == {"p50", "p95", "p99", "mean", "min", "max"}
    assert result["runner"]["cpus"] >= 1
    assert result["runner"]["cpu"]
    assert len(result["sha"]) == 40
    assert result["tool"].startswith("Python")


def test_a_p95_at_the_threshold_fails_and_a_failed_hook_is_no_figure(tmp_path: Path) -> None:
    root = repository(tmp_path)
    arguments = ["--repo", str(root), "--bin", sys.executable, "--edits", "2"]
    assert stop_hook.main([*arguments, "--threshold", "0", "--", "hook.py"]) == 1
    assert stop_hook.main([*arguments, "--", "hook.py", "fail"]) == 2
    assert (root / "src" / "a.cs").read_text() == "// a.cs\n"


def test_the_runner_is_described() -> None:
    memory = stop_hook.memory_bytes()
    assert memory is None or memory > 0
    assert stop_hook.cpu_model()
