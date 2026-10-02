# Copyright (c) 2026 Ben Bahrenburg. MIT licence: see LICENSE.
"""The Stop hook's wall-clock over seeded one-line edits: the p95 NFR-PERF-02 promises under 2 s.

Plan: docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md, Step 17 ("for a repository
and a seed, choose 200 files, for each apply a one-line no-op edit, run the hook command,
restore, record wall-clock; print p50, p95, p99, the runner CPU and memory, and write
testbeds/results/stop-hook.json") and section 1.7 (Performance: Stop hook p95). Requirement:
docs/prd.md#nfr-perf-02. Run by .github/workflows/nightly-testbeds.yml on dotnet/aspnetcore in
source mode, with the configuration testbeds/bench/aspnetcore.yaml; docs/perf.md records the
figures.

The hook command is what `rulebearing hooks install --claude-code` would run, narrowed as the
design's recipe narrows it: `cruise --from-hook --mode source --cache --affected HEAD`. Its
input is the hook JSON Claude Code sends (`stop_hook_active` false), and it must exit 0 and
print either nothing or the hook's block decision; a run that does not is an error and the
benchmark fails rather than timing it. Before the timed runs the command runs twice untimed, so
the cache is warm, as it is in a session. Each timed run follows one edit: a comment line
appended to a tracked file of the chosen extension, chosen with the seed from `git ls-files`,
and the file restored after the run, so every run sees the edit of its own file and the
restore of the one before.
"""

from __future__ import annotations

import argparse
import json
import os
import platform
import random
import shutil
import statistics
import subprocess
import sys
import time
from pathlib import Path

HOOK_INPUT = b'{"stop_hook_active": false}'
EDIT = b"\n// rulebearing stop-hook benchmark: a no-op edit\n"
DEFAULT_COMMAND = ["cruise", "--from-hook", "--mode", "source", "--cache", "--affected", "HEAD"]


class BenchmarkError(Exception):
    """The benchmark cannot give a trustworthy figure."""


def percentile(values: list[float], fraction: float) -> float:
    """The nearest-rank percentile of `values`, as testbeds/synth/bench.sh computes its p95."""
    ordered = sorted(values)
    if not ordered:
        message = "no timings"
        raise BenchmarkError(message)
    index = min(len(ordered) - 1, round(fraction * (len(ordered) - 1)))
    return ordered[index]


def tracked(repository: Path, extension: str) -> list[str]:
    """The tracked files with `extension`, outside build output, in git's order."""
    git = shutil.which("git")
    if git is None:
        message = "git is not on the path"
        raise BenchmarkError(message)
    listed = subprocess.run(  # noqa: S603 - fixed argv, no shell
        [git, "ls-files", "-z", f"*{extension}"],
        cwd=repository,
        capture_output=True,
        check=True,
    ).stdout.decode()
    skipped = {"bin", "obj", "node_modules"}
    return [
        name
        for name in listed.split("\0")
        if name and not skipped.intersection(Path(name).parts[:-1])
    ]


def choose(files: list[str], count: int, seed: int) -> list[str]:
    """`count` of `files`, the same ones for the same seed and file list."""
    if not files:
        message = "no tracked file with the chosen extension"
        raise BenchmarkError(message)
    return random.Random(seed).sample(files, min(count, len(files)))  # noqa: S311 - a seeded choice, not a secret


def run_hook(repository: Path, command: list[str]) -> float:
    """Runs the hook once and returns its wall-clock in seconds; a failed run raises."""
    started = time.perf_counter()
    completed = subprocess.run(  # noqa: S603 - the binary under test, arguments this script built
        command,
        cwd=repository,
        input=HOOK_INPUT,
        capture_output=True,
        check=False,
    )
    elapsed = time.perf_counter() - started
    answer = completed.stdout.strip()
    answered = not answer or json.loads(answer).get("decision") == "block"
    if completed.returncode != 0 or not answered:
        stderr = completed.stderr.decode(errors="replace").strip().splitlines()[-3:]
        message = f"the hook exited {completed.returncode}: {' | '.join(stderr)}"
        raise BenchmarkError(message)
    return elapsed


def measure(repository: Path, command: list[str], files: list[str]) -> list[float]:
    """Warms the cache, then times the hook after each file's edit, restoring each file."""
    run_hook(repository, command)
    run_hook(repository, command)
    timings = []
    for name in files:
        path = repository / name
        original = path.read_bytes()
        try:
            path.write_bytes(original + EDIT)
            timings.append(run_hook(repository, command))
        finally:
            path.write_bytes(original)
    return timings


def memory_bytes() -> int | None:
    """The machine's memory: /proc/meminfo on Linux, sysctl on macOS."""
    meminfo = Path("/proc/meminfo")
    if meminfo.is_file():
        for line in meminfo.read_text().splitlines():
            if line.startswith("MemTotal:"):
                return int(line.split()[1]) * 1024
    sysctl = shutil.which("sysctl")
    if sysctl is not None:
        found = subprocess.run(  # noqa: S603 - fixed argv, no shell
            [sysctl, "-n", "hw.memsize"],
            capture_output=True,
            check=False,
        )
        if found.returncode == 0 and found.stdout.strip().isdigit():
            return int(found.stdout.strip())
    return None


def cpu_model() -> str:
    """The processor's name, as the kernel reports it."""
    cpuinfo = Path("/proc/cpuinfo")
    if cpuinfo.is_file():
        for line in cpuinfo.read_text().splitlines():
            if line.startswith("model name"):
                return line.split(":", 1)[1].strip()
    sysctl = shutil.which("sysctl")
    if sysctl is not None:
        found = subprocess.run(  # noqa: S603 - fixed argv, no shell
            [sysctl, "-n", "machdep.cpu.brand_string"],
            capture_output=True,
            check=False,
        )
        if found.returncode == 0:
            return found.stdout.decode().strip()
    return platform.processor() or "unknown"


def revision(repository: Path) -> str:
    """The commit the benchmark ran at."""
    git = shutil.which("git")
    if git is None:
        return "unknown"
    found = subprocess.run(  # noqa: S603 - fixed argv, no shell
        [git, "rev-parse", "HEAD"],
        cwd=repository,
        capture_output=True,
        check=False,
    )
    return found.stdout.decode().strip() or "unknown"


def report(
    arguments: argparse.Namespace,
    command: list[str],
    timings: list[float],
) -> dict[str, object]:
    """The results file's content."""
    tool = subprocess.run(  # noqa: S603 - the binary under test
        [command[0], "--version"],
        capture_output=True,
        check=False,
    )
    return {
        "repository": arguments.name or arguments.repo.name,
        "sha": revision(arguments.repo),
        "tool": tool.stdout.decode().strip(),
        "command": " ".join(["rulebearing", *command[1:]]),
        "extension": arguments.extension,
        "edits": len(timings),
        "seed": arguments.seed,
        "seconds": {
            "p50": round(percentile(timings, 0.50), 3),
            "p95": round(percentile(timings, 0.95), 3),
            "p99": round(percentile(timings, 0.99), 3),
            "mean": round(statistics.fmean(timings), 3),
            "min": round(min(timings), 3),
            "max": round(max(timings), 3),
        },
        "thresholdSeconds": arguments.threshold,
        "runner": {
            "os": f"{platform.system()} {platform.release()}",
            "machine": platform.machine(),
            "cpus": os.cpu_count(),
            "cpu": cpu_model(),
            "memoryBytes": memory_bytes(),
        },
    }


def parse(argv: list[str]) -> argparse.Namespace:
    """The command line."""
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--repo", type=Path, required=True, help="the checkout to edit")
    parser.add_argument("--name", help="the repository's owner/name, for the results file")
    parser.add_argument("--bin", default="rulebearing", help="the rulebearing binary")
    parser.add_argument("--edits", type=int, default=200, help="how many files to edit")
    parser.add_argument("--seed", type=int, default=42, help="the seed that chooses them")
    parser.add_argument("--extension", default=".cs", help="the extension of the files edited")
    parser.add_argument(
        "--threshold",
        type=float,
        default=2.0,
        help="fail (exit 1) when the p95 is at or above this many seconds",
    )
    parser.add_argument("--out", type=Path, help="write the results as JSON here")
    parser.add_argument(
        "hook",
        nargs="*",
        help="the hook's arguments after the binary (default: %(default)s)",
        default=DEFAULT_COMMAND,
    )
    return parser.parse_args(argv)


def main(argv: list[str]) -> int:
    """Runs the benchmark: 0 under the threshold, 1 at or above it, 2 when it cannot run."""
    arguments = parse(argv)
    binary = shutil.which(arguments.bin) or arguments.bin
    command = [binary, *arguments.hook]
    try:
        files = choose(
            tracked(arguments.repo, arguments.extension), arguments.edits, arguments.seed
        )
        timings = measure(arguments.repo, command, files)
        result = report(arguments, command, timings)
    except (BenchmarkError, OSError, subprocess.CalledProcessError, json.JSONDecodeError) as error:
        sys.stderr.write(f"stop-hook benchmark: {error}\n")
        return 2
    if arguments.out is not None:
        arguments.out.parent.mkdir(parents=True, exist_ok=True)
        arguments.out.write_text(json.dumps(result, indent=2) + "\n")
    seconds = result["seconds"]
    sys.stdout.write(
        f"stop hook over {len(timings)} edits: {json.dumps(seconds)} on {result['runner']}\n",
    )
    return 1 if percentile(timings, 0.95) >= arguments.threshold else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
