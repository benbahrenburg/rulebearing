# Copyright (c) 2026 Ben Bahrenburg. MIT licence: see LICENSE.
r"""The scale table: Rulebearing's wall-clock time and peak memory on the scale repositories.

Plan: docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md, Step 24 ("writes
testbeds/results/scale.md with wall-clock and peak memory per repository ... and the Stop-hook p95
from step 17, then rewrites the README between the scale-table markers ... A regression over 20%
against the previous published row fails the job"). Requirements: docs/prd.md#nfr-conf-03,
docs/prd.md#nfr-perf-03.

  python3 testbeds/bench/scale_table.py measure --rows testbeds/out \
      --stop-hook testbeds/results/stop-hook.json --run <id>
      read tonight's rows (each <slug>/rulebearing-timing.json, and for a .NET row also
      rulebearing-source-timing.json, from testbeds/scale.sh), compare them with the committed
      testbeds/results/scale.json, and write scale.json and scale.md; exit 1 on a regression
  python3 testbeds/bench/scale_table.py --readme README.md   write the table between the markers
  python3 testbeds/bench/scale_table.py --check README.md    exit 1 when the README's is stale

A row that did not complete tonight keeps its last measurement, marked with the run it came from,
so the table never shows a gap as zero. A regression is a time or peak more than 20% above the
previous published row's, and more than a floor (2 s, 100 MB), since hosted runners differ by a
few seconds on the same binary. Standard library only.
"""

from __future__ import annotations

import argparse
import json
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any

START = "<!-- scale-table:start -->"
END = "<!-- scale-table:end -->"
RESULTS = Path(__file__).resolve().parents[1] / "results"
RUNS = "https://github.com/benbahrenburg/rulebearing/actions/runs/"
THRESHOLD = 0.20
FLOOR_SECONDS = 2.0
FLOOR_MB = 100


@dataclass(frozen=True)
class Scale:
    """One row of the table: a repository, and the mode .NET is read in."""

    repo: str
    mode: str
    timing: str


ROWS = (
    Scale("n8n-io/n8n", "", "rulebearing-timing.json"),
    Scale("grafana/grafana", "", "rulebearing-timing.json"),
    Scale("elastic/kibana", "", "rulebearing-timing.json"),
    Scale("dotnet/aspnetcore", "compiled", "rulebearing-timing.json"),
    Scale("dotnet/aspnetcore", "source", "rulebearing-source-timing.json"),
    Scale("jellyfin/jellyfin", "compiled", "rulebearing-timing.json"),
    Scale("home-assistant/core", "", "rulebearing-timing.json"),
)


def key(repo: str, mode: str) -> str:
    """A row's key in scale.json."""
    return f"{repo} {mode}".strip()


def load(path: Path) -> Any:  # noqa: ANN401 - JSON
    """A JSON file, or None when it is absent or unreadable."""
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return None


def measured(rows: Path, scale: Scale, run: str) -> dict[str, Any] | None:
    """Tonight's measurement of a row, when it completed."""
    folder = rows / scale.repo.replace("/", "__")
    result = load(folder / "result.json") or {}
    timing = load(folder / scale.timing)
    if result.get("status") != "ok" or not timing or timing.get("exit_code") != 0:
        return None
    rss = timing.get("max_rss_kb")
    return {
        "repo": scale.repo,
        "mode": scale.mode,
        "sha": result.get("sha", ""),
        "detail": result.get("detail", ""),
        "seconds": timing["wall_seconds"],
        "peakMb": round(rss / 1024) if isinstance(rss, int) else None,
        "run": run,
    }


def regressions(current: dict[str, Any], previous: dict[str, Any] | None) -> list[str]:
    """What grew by more than the threshold and the floor against the previous row."""
    if not previous or previous.get("sha") != current.get("sha"):
        return []
    found = []
    for field, floor, unit in (("seconds", FLOOR_SECONDS, "s"), ("peakMb", FLOOR_MB, "MB")):
        now, before = current.get(field), previous.get(field)
        if (
            isinstance(now, (int, float))
            and isinstance(before, (int, float))
            and now > before * (1 + THRESHOLD)
            and now - before > floor
        ):
            found.append(
                f"{key(current['repo'], current['mode'])}: {field} {now} {unit}, "
                f"the previous row (run {previous.get('run', '?')}) {before} {unit}"
            )
    return found


def measure(
    rows: Path, stop_hook: Path, run: str, previous: dict[str, Any]
) -> tuple[dict[str, Any], list[str]]:
    """Tonight's table, carried rows marked, and the regressions against `previous`."""
    before = {key(r["repo"], r["mode"]): r for r in previous.get("rows", [])}
    table: list[dict[str, Any]] = []
    found: list[str] = []
    for scale in ROWS:
        now = measured(rows, scale, run)
        last = before.get(key(scale.repo, scale.mode))
        if now is None:
            if last is not None:
                table.append({**last, "stale": True})
            else:
                table.append({"repo": scale.repo, "mode": scale.mode, "stale": True})
            continue
        found += regressions(now, last)
        table.append(now)
    hook = load(stop_hook)
    document: dict[str, Any] = {"run": run, "rows": table}
    if hook:
        document["stopHook"] = {
            "repo": hook["repository"],
            "sha": hook["sha"],
            "p95": hook["seconds"]["p95"],
            "edits": hook["edits"],
            "cpus": hook["runner"]["cpus"],
            "run": run,
        }
    elif previous.get("stopHook"):
        document["stopHook"] = {**previous["stopHook"], "stale": True}
    return document, found


def number(value: Any, unit: str) -> str:  # noqa: ANN401 - JSON
    """A measurement with its unit, or a dash."""
    return f"{value} {unit}" if isinstance(value, (int, float)) else "-"


def run_link(run: str) -> str:
    """A nightly run's id, linked when it is one."""
    return f"[{run}]({RUNS}{run})" if run.isdigit() else run


def render(document: dict[str, Any]) -> str:
    """The Markdown table of a scale.json document."""
    lines = [
        "| Repository | .NET read as | Wall-clock | Peak memory | Modules | Measured in run |",
        "| --- | --- | --- | --- | --- | --- |",
    ]
    for row in document.get("rows", []):
        repo = row["repo"]
        sha = row.get("sha", "")
        link = f"[{repo}](https://github.com/{repo}/tree/{sha})" if sha else repo
        run = row.get("run", "")
        when = (
            "not measured yet"
            if not run
            else run_link(run) + (" (last completed)" if row.get("stale") else "")
        )
        detail = str(row.get("detail", "")).removeprefix("modules: ")
        lines.append(
            f"| {link} | {row.get('mode') or '-'} | {number(row.get('seconds'), 's')} | "
            f"{number(row.get('peakMb'), 'MB')} | {detail or '-'} | {when} |"
        )
    hook = document.get("stopHook")
    if hook:
        stale = " (last completed)" if hook.get("stale") else ""
        lines += [
            "",
            (
                f"Stop hook on {hook['repo']} in source mode: p95 {hook['p95']} s over "
                f"{hook['edits']} edits on {hook['cpus']} CPUs, run {run_link(hook['run'])}{stale}."
            ),
        ]
    return "\n".join(lines) + "\n"


def splice(text: str, rendered: str) -> str:
    """`text` with `rendered` between the markers."""
    start, end = text.find(START), text.find(END)
    if start < 0 or end < start:
        message = f"no {START} ... {END} block"
        raise ValueError(message)
    return text[: start + len(START)] + "\n" + rendered + text[end:]


def main(argv: list[str] | None = None) -> int:
    """Measure, or write or check the README's table."""
    parser = argparse.ArgumentParser(description=(__doc__ or "").split("\n\n", 1)[0])
    parser.add_argument("command", nargs="?", choices=["measure"])
    parser.add_argument("--rows", type=Path)
    parser.add_argument("--stop-hook", type=Path, default=RESULTS / "stop-hook.json")
    parser.add_argument("--run", default="")
    parser.add_argument("--results", type=Path, default=RESULTS)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--readme", type=Path)
    mode.add_argument("--check", type=Path)
    args = parser.parse_args(argv)
    data = args.results / "scale.json"
    if args.command == "measure":
        if args.rows is None:
            parser.error("measure needs --rows")
        previous = load(data) or {}
        document, found = measure(args.rows, args.stop_hook, args.run, previous)
        args.results.mkdir(parents=True, exist_ok=True)
        data.write_text(json.dumps(document, indent=2) + "\n", encoding="utf-8")
        (args.results / "scale.md").write_text(
            "# Scale table\n\nWritten by testbeds/bench/scale_table.py from the nightly "
            "(plan 0003, Step 24).\n\n" + render(document),
            encoding="utf-8",
        )
        for line in found:
            sys.stdout.write(f"::error::scale regression: {line}\n")
        sys.stdout.write(f"scale: {len(document['rows'])} rows, {len(found)} regressed\n")
        return 1 if found else 0
    target = args.readme or args.check
    if target is None:
        sys.stdout.write(render(load(data) or {}))
        return 0
    text = target.read_text(encoding="utf-8")
    try:
        updated = splice(text, render(load(data) or {}))
    except ValueError as error:
        sys.stderr.write(f"scale: {target}: {error}\n")
        return 2
    if args.check:
        if updated != text:
            sys.stderr.write(
                f"scale: the scale table in {target} is stale; run "
                f"python3 testbeds/bench/scale_table.py --readme {target}\n"
            )
            return 1
        return 0
    target.write_text(updated, encoding="utf-8")
    return 0


if __name__ == "__main__":
    sys.exit(main())
