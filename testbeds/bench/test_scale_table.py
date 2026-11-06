# Copyright (c) 2026 Ben Bahrenburg. MIT licence: see LICENSE.
"""The scale table's rows, carried rows, regressions and README splice (plan 0003, Step 24).

A regression compares tonight's row with the baseline build timed beside it on the same runner.
"""

from __future__ import annotations

import json
from typing import TYPE_CHECKING

import pytest
import scale_table

if TYPE_CHECKING:
    from pathlib import Path

SHA = "b4be275a3b4fd83c304e7377403561a73eb4249e"
BASELINE_SHA = "9342f851c0ffee00000000000000000000000000"


def row(  # noqa: PLR0913 - a fixture writer, one keyword per file it may write
    rows: Path,
    repo: str,
    seconds: float,
    rss_kb: int,
    *,
    source: bool = False,
    baseline: tuple[float, int] | None = None,
) -> None:
    """Writes a completed scale row as testbeds/scale.sh does, with the baseline's when given."""
    folder = rows / repo.replace("/", "__")
    folder.mkdir(parents=True, exist_ok=True)
    (folder / "result.json").write_text(
        json.dumps({"status": "ok", "sha": SHA, "detail": "modules: dotnet 11487"})
    )
    timing = {"wall_seconds": seconds, "max_rss_kb": rss_kb, "exit_code": 0}
    (folder / "rulebearing-timing.json").write_text(json.dumps(timing))
    if source:
        source_timing = {"wall_seconds": seconds / 2, "max_rss_kb": rss_kb // 2, "exit_code": 0}
        (folder / "rulebearing-source-timing.json").write_text(json.dumps(source_timing))
    if baseline:
        before = {
            "wall_seconds": baseline[0],
            "max_rss_kb": baseline[1],
            "exit_code": 0,
            "sha": BASELINE_SHA,
        }
        (folder / "baseline-timing.json").write_text(json.dumps(before))
        if source:
            half = {**before, "wall_seconds": baseline[0] / 2, "max_rss_kb": baseline[1] // 2}
            (folder / "baseline-source-timing.json").write_text(json.dumps(half))


def hook(path: Path) -> None:
    """Writes a Stop-hook result as testbeds/bench/stop_hook.py does."""
    path.write_text(
        json.dumps(
            {
                "repository": "dotnet/aspnetcore",
                "sha": SHA,
                "seconds": {"p95": 1.606},
                "edits": 200,
                "runner": {"cpus": 4},
            }
        )
    )


def test_a_night_is_measured_and_rows_that_did_not_run_are_carried(tmp_path: Path) -> None:
    rows = tmp_path / "out"
    row(rows, "dotnet/aspnetcore", 800.0, 16_000_000, source=True)
    hook(tmp_path / "stop-hook.json")
    previous = {
        "rows": [
            {
                "repo": "jellyfin/jellyfin",
                "mode": "compiled",
                "sha": SHA,
                "seconds": 13.2,
                "peakMb": 5119,
                "run": "1",
            }
        ]
    }
    document, found = scale_table.measure(rows, tmp_path / "stop-hook.json", "2", previous)
    assert found == []
    by_key = {scale_table.key(r["repo"], r["mode"]): r for r in document["rows"]}
    assert by_key["dotnet/aspnetcore compiled"]["seconds"] == 800.0
    assert by_key["dotnet/aspnetcore compiled"]["peakMb"] == 15625
    assert by_key["dotnet/aspnetcore source"]["seconds"] == 400.0
    assert by_key["jellyfin/jellyfin compiled"]["stale"] is True
    assert by_key["n8n-io/n8n"] == {"repo": "n8n-io/n8n", "mode": "", "stale": True}
    assert document["stopHook"]["p95"] == 1.606
    rendered = scale_table.render(document)
    assert "| [dotnet/aspnetcore](https://github.com/dotnet/aspnetcore/tree/" in rendered
    assert (
        "| 13.2 s | 5119 MB | - | "
        "[1](https://github.com/benbahrenburg/rulebearing/actions/runs/1) (last completed) |"
        in rendered
    )
    assert "| n8n-io/n8n | - | - | - | - | not measured yet |" in rendered
    assert (
        "p95 1.606 s over 200 edits on 4 CPUs, run [2](https://github.com/benbahrenburg/rulebearing/actions/runs/2)."
        in rendered
    )


def test_a_regression_is_over_the_threshold_and_the_floor_against_the_baseline() -> None:
    def tonight(seconds: float, peak: int, baseline: tuple[float, int] | None) -> dict[str, object]:
        night: dict[str, object] = {"repo": "r", "mode": "", "seconds": seconds, "peakMb": peak}
        if baseline:
            night["baseline"] = {"sha": BASELINE_SHA, "seconds": baseline[0], "peakMb": baseline[1]}
        return night

    assert scale_table.regressions(tonight(11.9, 1000, (10.0, 1000))) == []
    assert scale_table.regressions(tonight(12.5, 1000, (10.0, 1000))) == [
        "r: seconds 12.5 s, the baseline 9342f851c0ff 10.0 s on the same runner"
    ]
    assert scale_table.regressions(tonight(2.9, 1000, (1.0, 1000))) == [], "under the floor"
    assert scale_table.regressions(tonight(10.0, 1300, (10.0, 1000))) == [
        "r: peakMb 1300 MB, the baseline 9342f851c0ff 1000 MB on the same runner"
    ]
    assert scale_table.regressions(tonight(10.0, 1150, (10.0, 1000))) == [], "under 20%"
    assert scale_table.regressions(tonight(99.0, 9000, None)) == [], "no pair, no comparison"
    assert scale_table.regressions(tonight(8.0, 900, (10.0, 1000))) == [], "faster is no regression"


def test_a_row_is_paired_with_the_baseline_timed_beside_it(tmp_path: Path) -> None:
    rows = tmp_path / "out"
    row(rows, "dotnet/aspnetcore", 800.0, 16_000_000, source=True, baseline=(790.0, 15_900_000))
    folder = rows / "dotnet__aspnetcore"
    compiled = scale_table.measured(rows, scale_table.ROWS[3], "2")
    source = scale_table.measured(rows, scale_table.ROWS[4], "2")
    assert compiled is not None
    assert source is not None
    assert compiled["baseline"] == {"sha": BASELINE_SHA, "seconds": 790.0, "peakMb": 15527}
    assert source["baseline"] == {"sha": BASELINE_SHA, "seconds": 395.0, "peakMb": 7764}
    failed = {"wall_seconds": 1.0, "max_rss_kb": 1, "exit_code": 2, "sha": BASELINE_SHA}
    (folder / "baseline-timing.json").write_text(json.dumps(failed))
    compiled = scale_table.measured(rows, scale_table.ROWS[3], "2")
    assert compiled is not None
    assert "baseline" not in compiled, "a baseline that did not complete is no pair"


def test_measure_writes_the_files_and_fails_on_a_regression(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    rows = tmp_path / "out"
    results = tmp_path / "results"
    results.mkdir()
    row(rows, "home-assistant/core", 30.0, 4_000_000, baseline=(12.0, 4_000_000))
    previous = {
        "rows": [
            {
                "repo": "home-assistant/core",
                "mode": "",
                "sha": SHA,
                "seconds": 12.2,
                "peakMb": 3906,
                "run": "1",
            }
        ],
        "stopHook": {"repo": "dotnet/aspnetcore", "p95": 1.5, "edits": 200, "cpus": 4, "run": "1"},
    }
    (results / "scale.json").write_text(json.dumps(previous))
    arguments = [
        "measure",
        "--rows",
        str(rows),
        "--stop-hook",
        str(tmp_path / "none.json"),
        "--run",
        "2",
        "--results",
        str(results),
    ]
    assert scale_table.main(arguments) == 1
    written = json.loads((results / "scale.json").read_text())
    assert written["stopHook"]["stale"] is True
    assert written["rows"][-1]["baseline"]["seconds"] == 12.0
    assert "# Scale table" in (results / "scale.md").read_text()
    output = capsys.readouterr().out
    assert "::error::scale regression: home-assistant/core: seconds 30.0 s" in output
    assert "scale: 7 rows, 1 compared with the baseline, 1 regressed" in output
    row(rows, "home-assistant/core", 12.0, 4_000_000, baseline=(12.5, 4_000_000))
    assert scale_table.main(arguments) == 0
    # The committed row from another night and runner is never the comparison: 30 s against the
    # 12.2 s published row passes when the baseline beside it took as long.
    (results / "scale.json").write_text(json.dumps(previous))
    row(rows, "home-assistant/core", 30.0, 4_000_000, baseline=(29.0, 4_000_000))
    assert scale_table.main(arguments) == 0
    (rows / "home-assistant__core" / "baseline-timing.json").unlink()
    capsys.readouterr()
    assert scale_table.main(arguments) == 0
    output = capsys.readouterr().out
    assert "scale: home-assistant/core has no baseline timed beside it; not compared" in output
    assert "scale: 7 rows, 0 compared with the baseline, 0 regressed" in output


def test_the_readme_is_written_and_checked(tmp_path: Path) -> None:
    results = tmp_path / "results"
    results.mkdir()
    (results / "scale.json").write_text(
        json.dumps({"rows": [{"repo": "n8n-io/n8n", "mode": "", "stale": True}]})
    )
    readme = tmp_path / "README.md"
    readme.write_text(f"intro\n{scale_table.START}\nold\n{scale_table.END}\noutro\n")
    common = ["--results", str(results)]
    assert scale_table.main(["--check", str(readme), *common]) == 1
    assert scale_table.main(["--readme", str(readme), *common]) == 0
    assert scale_table.main(["--check", str(readme), *common]) == 0
    assert "| n8n-io/n8n |" in readme.read_text()
    assert scale_table.main(common) == 0
    readme.write_text("no markers\n")
    assert scale_table.main(["--readme", str(readme), *common]) == 2
    with pytest.raises(SystemExit):
        scale_table.main(["measure", *common])


def test_an_unreadable_file_is_none(tmp_path: Path) -> None:
    (tmp_path / "bad.json").write_text("{")
    assert scale_table.load(tmp_path / "bad.json") is None
    assert scale_table.load(tmp_path / "absent.json") is None
