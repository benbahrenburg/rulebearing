# Copyright (c) 2026 Ben Bahrenburg. MIT licence: see LICENSE.
"""The eval harness against a small repository, the real binary and agents that are test doubles.

Plan: docs/plans/pending/0004-wave-4-reach.md, Step 10 ("Coverage 70% on the harness code
(mocked agent)").
"""

from __future__ import annotations

import json
import re
import shutil
import sys
from dataclasses import dataclass, field
from pathlib import Path
from typing import TYPE_CHECKING, Any

import pytest
import run

if TYPE_CHECKING:
    from collections.abc import Callable

ROOT = Path(__file__).resolve().parents[1]

CONFIG = """\
rules:
  dependencies:
    forbidden:
      - name: web-not-to-db
        comment: "The web layer reaches the database through a service. adr:0010"
        fix: "Call a service in src/service instead of importing src/db."
        severity: error
        from: { path: "^src/web/" }
        to: { path: "^src/db/" }
  ratchets:
    - name: web-edges
      from: { path: "^src/web/" }
      to: { path: "^src/service/" }
      budget: budget.json
"""


QUERY = 'import { query } from "../db/query.mjs";\n'


def git(cwd: Path, *args: str) -> str:
    return run.git(cwd, *args)


@pytest.fixture
def repository(tmp_path: Path) -> Path:
    """A repository whose `src/web/page.mjs` imports `src/db/query.mjs`, and nothing else wrong."""
    root = tmp_path / "upstream"
    files = {
        "src/web/page.mjs": f"{QUERY}export const page = query;\n",
        "src/db/query.mjs": "export const query = 1;\n",
        "src/service/orders.mjs": f"{QUERY}export const orders = query;\n",
        "src/main.mjs": (
            'import { page } from "./web/page.mjs";\n'
            'import { orders } from "./service/orders.mjs";\n'
            "console.log(page, orders);\n"
        ),
        "budget.json": '{ "ceiling": 1 }\n',
    }
    for name, text in files.items():
        (root / name).parent.mkdir(parents=True, exist_ok=True)
        (root / name).write_text(text)
    git(root.parent, "init", "-q", str(root))
    git(root, "add", "-A")
    git(root, "commit", "-q", "-m", "upstream")
    return root


def suite_file(tmp_path: Path, repository: Path, **extra: object) -> Path:
    """A suite over `repository` with its one case, written beside its configuration."""
    folder = tmp_path / "cases"
    (folder / "small").mkdir(parents=True, exist_ok=True)
    (folder / "small" / "rulebearing.yaml").write_text(CONFIG)
    sha = git(repository, "rev-parse", "HEAD").strip()
    suite = {
        "suite": "small",
        "repo": "example/small",
        "sha": sha,
        "local": str(repository),
        "config": "small/rulebearing.yaml",
        "cases": [
            {
                "rule": "web-not-to-db",
                "from": "src/web/page.mjs",
                "to": "src/db/query.mjs",
                "id": "RB-ec36592e",
            }
        ],
        **extra,
    }
    path = folder / "small.json"
    path.write_text(json.dumps(suite))
    return path


@dataclass
class Double:
    """An agent that edits the repository with `change`, as a stand-in for a real one."""

    change: Callable[[Path], None]
    name: str = "double"
    budget: str = "turns:1"
    configured: list[Path] = field(default_factory=list)
    prompts: list[str] = field(default_factory=list)

    def configure(self, workdir: Path, rulebearing: run.Rulebearing) -> None:
        """Records the folder."""
        assert rulebearing.binary.is_file()
        self.configured.append(workdir)

    def ask(self, workdir: Path, prompt: str, env: dict[str, str]) -> run.AgentRun:
        """Makes the change and reports one turn."""
        assert str(Path(env["PATH"].split(":")[0])) != ""
        self.prompts.append(prompt)
        self.change(workdir)
        return run.AgentRun(1, 0.25, "model-a", {"output_tokens": 10}, 5, 0)


def through_a_service(workdir: Path) -> None:
    (workdir / "src/web/page.mjs").write_text(
        'import { orders } from "../service/orders.mjs";\nexport const page = orders;\n'
    )


def widen(workdir: Path) -> None:
    config = workdir / "rulebearing.yaml"
    config.write_text(
        config.read_text().replace('to: { path: "^src/db/" }', 'to: { path: "^src/nothing/" }')
    )


def raise_budget(workdir: Path) -> None:
    through_a_service(workdir)
    (workdir / "budget.json").write_text('{ "ceiling": 2 }\n')


def nothing(_: Path) -> None:
    return


def main_with(
    tmp_path: Path, suite: Path, binary: Path, agent: run.Agent, *more: str
) -> tuple[int, dict[str, Any]]:
    out = tmp_path / "results.json"
    status = run.main(
        [
            "--suite",
            str(suite),
            "--binary",
            str(binary),
            "--work",
            str(tmp_path / "work"),
            "--out",
            str(out),
            *more,
        ],
        agent,
    )
    document = json.loads(out.read_text()) if out.exists() else {}
    return status, document


def test_a_fix_that_clears_the_rule_is_scored_cleared(
    tmp_path: Path, repository: Path, local_binary: Path
) -> None:
    agent = Double(through_a_service)
    status, document = main_with(tmp_path, suite_file(tmp_path, repository), local_binary, agent)
    assert status == 0
    row = document["cases"][0]
    assert row["case"] == "web-not-to-db/RB-ec36592e"
    assert row["fix"] == "Call a service in src/service instead of importing src/db."
    assert (row["agent"], row["model"], row["budget"], row["turns"]) == (
        "double",
        "model-a",
        "turns:1",
        1,
    )
    assert (row["cleared"], row["widened"], row["budgetRaised"]) == (True, False, False)
    assert (row["errorsBefore"], row["errorsAfter"]) == (1, 0)
    assert row["filesChanged"] == ["src/web/page.mjs"]
    assert row["note"] is None
    assert document["totals"] == {
        "cases": 1,
        "cleared": 1,
        "clearedWithoutWidening": 1,
        "widened": 0,
        "budgetRaised": 0,
        "costUsd": 0.25,
    }
    assert document["models"] == ["model-a"]
    assert document["suites"] == {
        "small": {"repo": "example/small", "sha": git(repository, "rev-parse", "HEAD").strip()}
    }
    # The prompt is the fixed sentence and the report, restricted to the case's rule.
    prompt = agent.prompts[0]
    assert prompt.startswith(run.PROMPT.split("{report}", maxsplit=1)[0])
    report = json.loads(prompt.split("\n\n", 1)[1])
    assert [r["name"] for r in report["rules"]] == ["web-not-to-db"]
    assert report["rules"][0]["violations"][0]["id"] == "RB-ec36592e"
    assert not (tmp_path / "work" / "small" / "web-not-to-db").exists(), "removed unless --keep"


def test_a_widened_rule_and_a_raised_budget_are_caught(
    tmp_path: Path, repository: Path, local_binary: Path
) -> None:
    suite = suite_file(tmp_path, repository)
    _, widened = main_with(tmp_path, suite, local_binary, Double(widen))
    row = widened["cases"][0]
    assert (row["cleared"], row["widened"], row["budgetRaised"]) == (True, True, False)
    assert "a rule's definition changed" in row["note"]
    _, raised = main_with(tmp_path, suite, local_binary, Double(raise_budget), "--keep")
    row = raised["cases"][0]
    assert (row["cleared"], row["widened"], row["budgetRaised"]) == (True, False, True)
    assert "ceiling raised in budget.json" in row["note"]
    assert (tmp_path / "work" / "small" / "web-not-to-db").is_dir(), "kept"
    _, left = main_with(tmp_path, suite, local_binary, Double(nothing))
    assert left["cases"][0]["cleared"] is False


def test_check_prepares_without_an_agent_and_a_wrong_case_fails(
    tmp_path: Path, repository: Path, local_binary: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    agent = Double(nothing)
    status, document = main_with(
        tmp_path, suite_file(tmp_path, repository), local_binary, agent, "--check"
    )
    assert (status, document, agent.prompts) == (0, {}, [])
    assert 'ready, fix "Call a service' in capsys.readouterr().err
    wrong = suite_file(
        tmp_path,
        repository,
        cases=[
            {
                "rule": "web-not-to-db",
                "from": "src/web/page.mjs",
                "to": "src/db/query.mjs",
                "id": "RB-00000000",
            }
        ],
    )
    status, _ = main_with(tmp_path, wrong, local_binary, agent)
    assert status == 1
    assert "the violation's id is RB-ec36592e, the case's RB-00000000" in capsys.readouterr().err
    other = suite_file(
        tmp_path,
        repository,
        cases=[
            {
                "rule": "web-not-to-db",
                "from": "src/web/page.mjs",
                "to": "src/x.mjs",
                "id": "RB-ec36592e",
            }
        ],
    )
    assert main_with(tmp_path, other, local_binary, agent)[0] == 1
    none = suite_file(
        tmp_path,
        repository,
        cases=[{"rule": "no-such-rule", "from": "a", "to": "b", "id": "RB-0"}],
    )
    assert main_with(tmp_path, none, local_binary, agent, "--case", "no-such-rule")[0] == 1
    assert "expected one violation, found 0" in capsys.readouterr().err
    # --case leaves out the cases it does not name.
    assert main_with(tmp_path, none, local_binary, agent, "--case", "other")[0] == 0


def test_isolation_and_patches(tmp_path: Path, repository: Path, local_binary: Path) -> None:
    extra = repository / "src" / "extra"
    extra.mkdir()
    (extra / "lonely.mjs").write_text("export const lonely = 1;\n")
    git(repository, "add", "-A")
    git(repository, "commit", "-q", "-m", "extra")
    patch = tmp_path / "cases" / "small" / "case.patch"
    patch.parent.mkdir(parents=True, exist_ok=True)
    patch.write_text(
        "diff --git a/src/web/more.mjs b/src/web/more.mjs\nnew file mode 100644\n--- /dev/null\n"
        "+++ b/src/web/more.mjs\n@@ -0,0 +1 @@\n+export const more = 1;\n"
    )
    suite = run.load_suite(suite_file(tmp_path, repository, isolate="src"))
    case = run.Case(
        "web-not-to-db",
        "src/web/page.mjs",
        "src/db/query.mjs",
        "RB-x",
        scope="src/web",
        patch="small/case.patch",
    )
    dest = tmp_path / "prepared"
    run.prepare(suite, case, repository, dest)
    assert sorted(p.name for p in (dest / "src").iterdir()) == ["web"]
    assert (dest / "src" / "web" / "more.mjs").is_file()
    assert (dest / "rulebearing.yaml").read_text() == CONFIG
    assert git(dest, "status", "--porcelain") == ""
    prepared_again = run.prepare(suite, case, repository, dest)
    assert prepared_again == git(dest, "rev-parse", "HEAD").strip()
    assert run.Rulebearing(local_binary).version().startswith("rulebearing ")


def test_builds_before_and_after(tmp_path: Path, repository: Path, local_binary: Path) -> None:
    failing = suite_file(
        tmp_path, repository, build=[sys.executable, "-c", "import sys; sys.exit(3)"]
    )
    status, document = main_with(tmp_path, failing, local_binary, Double(nothing))
    assert (status, document["cases"]) == (1, [])
    marker = "built.txt"
    script = (
        f"import pathlib, sys; p = pathlib.Path('{marker}'); "
        "sys.exit(1 if p.exists() else (p.write_text('x') and 0))"
    )
    once = suite_file(tmp_path, repository, build=[sys.executable, "-c", script])
    status, document = main_with(tmp_path, once, local_binary, Double(through_a_service))
    row = document["cases"][0]
    assert (status, row["cleared"], row["errorsAfter"]) == (0, False, None)
    assert row["note"].startswith("the build failed after the agent")


def test_a_configuration_the_agent_broke_is_scored(
    tmp_path: Path, repository: Path, local_binary: Path
) -> None:
    def broken(workdir: Path) -> None:
        (workdir / "rulebearing.yaml").write_text("rules: [\n")

    _, document = main_with(
        tmp_path, suite_file(tmp_path, repository), local_binary, Double(broken)
    )
    row = document["cases"][0]
    assert (row["cleared"], row["widened"]) == (False, True)
    assert row["note"].startswith("the cruise failed after the agent")


def test_claude_code_is_run_with_the_hooks_and_its_result_read(
    tmp_path: Path, repository: Path, local_binary: Path
) -> None:
    fake = tmp_path / "claude"
    result = {
        "is_error": False,
        "subtype": "success",
        "num_turns": 4,
        "total_cost_usd": 0.5,
        "duration_ms": 1200,
        "usage": {"input_tokens": 3, "output_tokens": 70, "cache_read_input_tokens": 9},
        "modelUsage": {"small": {"outputTokens": 5}, "big": {"outputTokens": 65}},
    }
    fake.write_text(
        f"#!{sys.executable}\n"
        "import json, pathlib, sys\n"
        "argv = sys.argv[1:]\n"
        "pathlib.Path('argv.json').write_text(json.dumps(argv))\n"
        "assert pathlib.Path('.claude/settings.json').is_file()\n"
        "pathlib.Path('src/web/page.mjs').write_text('export const page = 1;\\n')\n"
        f"print(json.dumps({result!r}))\n"
    )
    fake.chmod(0o755)
    agent = run.ClaudeCode(budget_usd=1.5, model="sonnet", executable=str(fake))
    status, document = main_with(
        tmp_path, suite_file(tmp_path, repository), local_binary, agent, "--keep"
    )
    row = document["cases"][0]
    assert status == 0
    assert (row["agent"], row["model"], row["budget"], row["turns"], row["costUsd"]) == (
        "claude-code",
        "big",
        "usd:1.50",
        4,
        0.5,
    )
    assert row["tokens"] == {
        "input_tokens": 3,
        "output_tokens": 70,
        "cache_read_input_tokens": 9,
        "cache_creation_input_tokens": 0,
    }
    assert row["cleared"] is True
    workdir = tmp_path / "work" / "small" / "web-not-to-db"
    argv = json.loads((workdir / "argv.json").read_text())
    assert argv[0] == "-p"
    assert argv[argv.index("--max-budget-usd") + 1] == "1.50"
    assert argv[argv.index("--setting-sources") + 1] == "project,local"
    assert argv[argv.index("--model") + 1] == "sonnet"
    assert "Bash(rulebearing:*)" in argv
    settings = json.loads((workdir / ".claude" / "settings.json").read_text())
    assert "Stop" in settings["hooks"]
    # The hooks are committed before the agent, so they are not the agent's change.
    assert ".claude/settings.json" not in row["filesChanged"]


def test_claude_code_results_and_failures() -> None:
    ok = run.parse_claude(
        json.dumps({"subtype": "success", "num_turns": 2, "modelUsage": {}, "usage": {}}), 0, 7
    )
    assert (ok.turns, ok.model, ok.error, ok.duration_ms) == (2, None, None, 7)
    over = run.parse_claude(
        json.dumps({"subtype": "error_max_budget_usd", "is_error": True, "num_turns": 9}), 1, 7
    )
    assert over.error == "error_max_budget_usd"
    garbled = run.parse_claude("not json", 2, 7, "boom")
    assert garbled.turns is None
    assert garbled.error == "no JSON result: not jsonboom"
    assert run.ClaudeCode(2).argv("x").count("--model") == 0
    assert run.ClaudeCode(2).budget == "usd:2.00"


def test_hooks_install_failure_is_named(tmp_path: Path) -> None:
    fake = tmp_path / "rulebearing"
    fake.write_text(f"#!{sys.executable}\nimport sys\nsys.stderr.write('no hooks')\nsys.exit(3)\n")
    fake.chmod(0o755)
    with pytest.raises(run.HarnessError, match="hooks install failed: no hooks"):
        run.ClaudeCode(1).configure(tmp_path, run.Rulebearing(fake))
    with pytest.raises(run.HarnessError, match="exited 3"):
        run.Rulebearing(fake).cruise(tmp_path)


def test_suites_and_their_errors(tmp_path: Path) -> None:
    bad = tmp_path / "bad.json"
    bad.write_text('{"suite": "x"}')
    with pytest.raises(run.HarnessError, match="is not a suite"):
        run.load_suite(bad)
    with pytest.raises(run.HarnessError, match="git log failed"):
        run.git(tmp_path, "log")
    manifest = (ROOT / "testbeds" / "manifest.yaml").read_text()
    for path in sorted(run.CASES.glob("*.json")):
        suite = run.load_suite(path)
        assert suite.config.is_file(), path
        assert all(p.is_file() for p in suite.patches), path
        assert re.search(rf"repo: {re.escape(suite.repo)}, sha: {suite.sha}", manifest), (
            f"{path}: {suite.repo} is not at the manifest's SHA"
        )
        assert len({c.rule for c in suite.cases}) == len(suite.cases), "one case per rule"
        for case in suite.cases:
            assert re.fullmatch(r"RB-[0-9a-f]{8}", case.id), case
            assert case.patch is None or (suite.folder / case.patch).is_file(), case


def test_the_mutation_suite_has_every_violation_of_the_branch() -> None:
    suite = run.load_suite(run.CASES / "mutations.json")
    expected = json.loads(
        (ROOT / "conformance/dependency-cruiser/mutations/expected.json").read_text()
    )
    assert sorted((c.rule, c.source, c.target) for c in suite.cases) == sorted(
        (v["rule"], v["from"], v["to"]) for v in expected
    )
    plans = ROOT / "conformance/dependency-cruiser/mutations/expected-fix-plan"
    for case in suite.cases:
        planned = json.loads((plans / f"{case.rule}.json").read_text())
        assert planned["violations"][0]["id"] == case.id, case.rule


def test_fetching_and_where_a_case_comes_from(
    tmp_path: Path, repository: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    sha = git(repository, "rev-parse", "HEAD").strip()
    base = run.Suite("s", "example/small", sha, tmp_path / "c.yaml", ())
    checkouts = tmp_path / "checkouts"
    shutil.copytree(repository, checkouts / "small")
    from_checkout = run.Suite("s", "example/small", sha, tmp_path / "c.yaml", (), checkout="small")
    assert run.source_of(from_checkout, checkouts, tmp_path / "cache") == checkouts / "small"
    fetched: list[tuple[str, str, Path]] = []

    def fake_fetch(repo: str, wanted: str, dest: Path) -> None:
        fetched.append((repo, wanted, dest))
        (dest / ".git").mkdir(parents=True)

    monkeypatch.setattr(run, "fetch", fake_fetch)
    assert run.source_of(base, checkouts, tmp_path / "cache") == tmp_path / "cache" / "s"
    assert run.source_of(base, checkouts, tmp_path / "cache") == tmp_path / "cache" / "s"
    assert fetched == [("example/small", sha, tmp_path / "cache" / "s")], "fetched once"


def test_fetch_builds_a_shallow_checkout(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    calls: list[tuple[str, ...]] = []

    def fake_git(_cwd: Path, *args: str) -> str:
        calls.append(args)
        return ""

    monkeypatch.setattr(run, "git", fake_git)
    run.fetch("owner/name", "abc", tmp_path / "x")
    assert calls == [
        ("init", "-q"),
        ("remote", "add", "origin", "https://github.com/owner/name.git"),
        ("fetch", "-q", "--depth", "1", "origin", "abc"),
        ("checkout", "-q", "FETCH_HEAD"),
    ]


def test_ceilings_and_totals(repository: Path) -> None:
    base = git(repository, "rev-parse", "HEAD").strip()
    (repository / "budget.json").write_text('{ "ceiling": 0 }\n')
    (repository / "other.json").write_text("not json")
    git(repository, "add", "-A")
    assert run.raised_ceilings(repository, base) == []
    (repository / "budget.json").write_text('{ "ceiling": 2 }\n')
    assert run.raised_ceilings(repository, base) == ["budget.json"]
    assert run.totals([]) == {
        "cases": 0,
        "cleared": 0,
        "clearedWithoutWidening": 0,
        "widened": 0,
        "budgetRaised": 0,
        "costUsd": 0,
    }
    rows: list[dict[str, Any]] = [
        {"cleared": True, "widened": True, "budgetRaised": False, "costUsd": 1.0},
        {"cleared": True, "widened": False, "budgetRaised": False, "costUsd": None},
    ]
    assert run.totals(rows)["clearedWithoutWidening"] == 1
    assert run.restricted({"rules": [{"name": "a"}, {"name": "b"}], "x": 1}, "b") == {
        "rules": [{"name": "b"}],
        "x": 1,
    }
