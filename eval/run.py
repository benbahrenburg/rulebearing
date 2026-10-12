# Copyright (c) 2026 Ben Bahrenburg. MIT licence: see LICENSE.
r"""The `fix`-text eval harness: a benchmark of violations, an agent per violation, a score.

Plan: docs/plans/pending/0004-wave-4-reach.md, Step 10 ("checks out the case, writes the `agent`
reporter output, invokes the configured agent with a fixed prompt that includes only that output,
then runs `cruise --exit-code`, `config lint` and `count` to score `cleared`, `widened` and
`budgetRaised`") and section 1.7 (the agent under test and its token budget are parameters of the
run, recorded in every result row; one configuration ships: Claude Code with the repository's
`hooks install` output). Decision:
docs/adr/0076-fix-plan-candidates-are-verified-and-the-eval-harness-scores-fix-text.md.
Requirement: docs/prd.md#fr-reach-03. Design: docs/artifacts/design.md, "The agentic engineering
hat".

  python3 eval/run.py [--suite eval/cases/mutations.json ...] [--case RULE ...]
      [--agent claude-code] [--model MODEL] [--budget-usd 3] [--binary target/release/rulebearing]
      [--checkouts DIR] [--work DIR] [--keep] [--out eval/results/<date>-<agent>.json]

A suite (eval/cases/<suite>.json) names a repository at a SHA, the patches that put the
violations in it, the configuration the cases run under (with each rule's `fix`), the build a
compiled language needs, and the cases: one violation each, by rule and ends. For each case the
harness copies the repository from the testbed checkouts (or fetches the SHA), applies the
patches, keeps only the case's own violation, commits that state, builds it, installs the agent's
configuration and asks `rulebearing cruise --output-type agent` for the report. The agent gets a
fixed prompt and that report, restricted to the case's rule, and nothing else. Afterwards the
harness builds again and scores, from the binary's own answers:

  cleared       the case's rule reports no violation (`cruise`)
  widened       a rule's definition changed, a finding was softened to `ignore` by a known
                violation, or `config lint` reports a finding it did not report before
  budgetRaised  a ratchet budget file's `ceiling` went up (what `count --budget` checks)

The harness runs outside CI, on demand. Every row records the agent, the model the agent
reported, the budget and what the run cost. Standard library only.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Protocol

ROOT = Path(__file__).resolve().parents[1]
CASES = ROOT / "eval" / "cases"
RESULTS = ROOT / "eval" / "results"
#: The prompt every agent gets: this sentence, then the `agent` report for the case. Nothing else.
PROMPT = (
    "rulebearing, this repository's architecture check, reports the violation below in its "
    "`agent` format. Change the repository so that the check no longer reports it.\n\n{report}"
)
#: The fields `rules --json` adds to a rule from the graph; a rule's definition is the rest.
STATISTICS = ("fromMatches", "toMatches", "violations")
#: The tools Claude Code may use without asking: edit files, and run what fixing a violation needs.
CLAUDE_TOOLS = (
    "Read",
    "Edit",
    "Write",
    "Glob",
    "Grep",
    "Bash(rulebearing:*)",
    "Bash(git:*)",
    "Bash(ls:*)",
    "Bash(cat:*)",
    "Bash(mkdir:*)",
    "Bash(mv:*)",
    "Bash(rm:*)",
    "Bash(grep:*)",
    "Bash(find:*)",
    "Bash(node:*)",
    "Bash(dotnet build:*)",
)
GIT_IDENTITY = ("-c", "user.name=eval", "-c", "user.email=eval@example.invalid")


class HarnessError(Exception):
    """A case could not be prepared or scored; the message names what failed."""


@dataclass(frozen=True)
class Case:
    """One violation: its rule, its ends, its stable id, and what puts it in the repository."""

    rule: str
    source: str
    target: str
    id: str
    #: The folder the violation lives in; with the suite's `isolate`, every sibling is removed.
    scope: str | None = None
    #: A patch, relative to the suite file, that puts this case's violation in the repository.
    patch: str | None = None


@dataclass(frozen=True)
class Suite:
    """A repository at a SHA, the patches and configuration its cases run under, and the cases."""

    name: str
    repo: str
    sha: str
    config: Path
    cases: tuple[Case, ...]
    #: The testbed checkout to copy, relative to the checkouts folder.
    checkout: str = ""
    #: A local repository to copy instead (the tests').
    local: Path | None = None
    patches: tuple[Path, ...] = ()
    #: The folder whose children are the cases' scopes.
    isolate: str | None = None
    #: The command that builds the repository so the binary can read it, if it needs one.
    build: tuple[str, ...] = ()
    #: Where the suite file is; patches are relative to it.
    folder: Path = field(default=CASES)


def load_suite(path: Path) -> Suite:
    """Reads a suite file.

    Raises:
        HarnessError: the file is not a suite.
    """
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
        folder = path.parent
        cases = tuple(
            Case(
                rule=c["rule"],
                source=c["from"],
                target=c["to"],
                id=c["id"],
                scope=c.get("scope"),
                patch=c.get("patch"),
            )
            for c in data["cases"]
        )
        return Suite(
            name=data["suite"],
            repo=data["repo"],
            sha=data["sha"],
            config=folder / data["config"],
            cases=cases,
            checkout=data.get("checkout", ""),
            local=(folder / data["local"]) if "local" in data else None,
            patches=tuple(folder / p for p in data.get("patches", [])),
            isolate=data.get("isolate"),
            build=tuple(data.get("build", [])),
            folder=folder,
        )
    except (OSError, KeyError, TypeError, json.JSONDecodeError) as error:
        message = f"{path} is not a suite: {error!r}"
        raise HarnessError(message) from error


def run(
    argv: list[str],
    cwd: Path,
    env: dict[str, str] | None = None,
    stdin: str | None = None,
) -> subprocess.CompletedProcess[str]:
    """Runs a command to completion, capturing its output; never raises on an exit code."""
    return subprocess.run(  # noqa: S603 - argv built by this harness, no shell
        argv,
        cwd=cwd,
        env=env,
        input=stdin,
        capture_output=True,
        text=True,
        check=False,
    )


def git(cwd: Path, *args: str) -> str:
    """Runs git and returns its stdout.

    Raises:
        HarnessError: git failed.
    """
    done = run(["git", *GIT_IDENTITY, *args], cwd)
    if done.returncode != 0:
        message = f"git {' '.join(args)} failed in {cwd}: {done.stderr.strip()}"
        raise HarnessError(message)
    return done.stdout


def fetch(repo: str, sha: str, dest: Path) -> None:
    """A shallow checkout of one commit of a GitHub repository, as the testbed scripts make."""
    dest.mkdir(parents=True, exist_ok=True)
    git(dest, "init", "-q")
    git(dest, "remote", "add", "origin", f"https://github.com/{repo}.git")
    git(dest, "fetch", "-q", "--depth", "1", "origin", sha)
    git(dest, "checkout", "-q", "FETCH_HEAD")


def source_of(suite: Suite, checkouts: Path, cache: Path) -> Path:
    """The repository a case is copied from.

    The suite's local one, else the testbed checkout, else a fresh fetch of the SHA under `cache`.
    """
    if suite.local is not None:
        return suite.local
    if suite.checkout and (checkouts / suite.checkout / ".git").exists():
        return checkouts / suite.checkout
    fetched = cache / suite.name
    if not (fetched / ".git").exists():
        fetch(suite.repo, suite.sha, fetched)
    return fetched


def prepare(suite: Suite, case: Case, source: Path, dest: Path) -> str:
    """Copies the repository to `dest` with the case's violation in it, committed; the commit.

    The repository is cloned at the suite's SHA, the patches applied, every sibling of the case's
    scope removed, and the configuration written as `rulebearing.yaml`.

    Raises:
        HarnessError: a step failed.
    """
    if dest.exists():
        shutil.rmtree(dest)
    dest.parent.mkdir(parents=True, exist_ok=True)
    # A clone carries tracked files only: nothing the checkout built or installed comes along.
    git(dest.parent, "clone", "-q", "--no-checkout", str(source), str(dest))
    git(dest, "-c", "advice.detachedHead=false", "checkout", "-q", "--detach", suite.sha)
    for patch in (*suite.patches, *((suite.folder / case.patch,) if case.patch else ())):
        git(dest, "apply", "--whitespace=nowarn", str(patch))
    if suite.isolate and case.scope:
        parent = dest / suite.isolate
        for child in sorted(parent.iterdir()):
            if child.name != Path(case.scope).name:
                shutil.rmtree(child) if child.is_dir() else child.unlink()
    shutil.copyfile(suite.config, dest / "rulebearing.yaml")
    git(dest, "add", "-A")
    git(dest, "commit", "-q", "--no-verify", "-m", f"eval: {suite.name} {case.rule}")
    return git(dest, "rev-parse", "HEAD").strip()


def build(suite: Suite, workdir: Path) -> str | None:
    """Builds the repository when the suite needs it; the failure's tail, or None."""
    if not suite.build:
        return None
    env = {**os.environ, "DOTNET_NUGET_SIGNATURE_VERIFICATION": "false"}
    done = run(list(suite.build), workdir, env)
    if done.returncode == 0:
        return None
    return (done.stdout + done.stderr).strip()[-2000:]


@dataclass(frozen=True)
class Rulebearing:
    """The binary under test, run in a case's folder."""

    binary: Path

    def json(self, workdir: Path, *args: str) -> Any:  # noqa: ANN401 - the binary's JSON, as parsed
        """A command's JSON output.

        Raises:
            HarnessError: the output is not JSON.
        """
        done = run([str(self.binary), *args], workdir)
        try:
            return json.loads(done.stdout)
        except json.JSONDecodeError as error:
            tail = done.stderr.strip()[-1000:]
            message = f"rulebearing {' '.join(args)} exited {done.returncode}: {tail}"
            raise HarnessError(message) from error

    def cruise(self, workdir: Path) -> dict[str, Any]:
        """The cruise result, every rule evaluated, liveness as the configuration has it."""
        result: dict[str, Any] = self.json(workdir, "cruise", "-T", "json", "--no-progress")
        return result

    def report(self, workdir: Path) -> dict[str, Any]:
        """The `agent` report, every violation shown."""
        result: dict[str, Any] = self.json(
            workdir, "cruise", "-T", "agent", "--no-progress", "--max-findings", "1000"
        )
        return result

    def rules(self, workdir: Path) -> dict[str, Any]:
        """Each rule's definition, from `rules --json` without its statistics."""
        listed = self.json(workdir, "rules", "--json")
        return {
            r["name"]: {k: v for k, v in r.items() if k not in STATISTICS}
            for r in listed.get("rules", [])
        }

    def lint(self, workdir: Path) -> list[str]:
        """The findings `config lint` prints, one per line."""
        done = run([str(self.binary), "config", "lint"], workdir)
        lines = (done.stdout + done.stderr).splitlines()
        return [line for line in lines if line.strip() and "no findings" not in line]

    def version(self) -> str:
        """`rulebearing --version`."""
        return run([str(self.binary), "--version"], ROOT).stdout.strip()


def violations_of(result: dict[str, Any], rule: str) -> list[dict[str, Any]]:
    """The violations of one rule in a cruise result."""
    return [
        v
        for v in result.get("summary", {}).get("violations", [])
        if v.get("rule", {}).get("name") == rule
    ]


def restricted(report: dict[str, Any], rule: str) -> dict[str, Any]:
    """The `agent` report with only the case's rule in `rules`."""
    return {**report, "rules": [r for r in report.get("rules", []) if r.get("name") == rule]}


def prompt_for(report: dict[str, Any]) -> str:
    """The fixed prompt with the report in it."""
    return PROMPT.format(report=json.dumps(report, indent=2))


@dataclass(frozen=True)
class AgentRun:
    """What one agent invocation reported about itself."""

    turns: int | None
    cost_usd: float | None
    model: str | None
    tokens: dict[str, int]
    duration_ms: int
    exit_code: int
    error: str | None = None


class Agent(Protocol):
    """An agent under test: how it is set up in a repository, and how it is asked."""

    @property
    def name(self) -> str:
        """The agent, as the rows record it."""

    @property
    def budget(self) -> str:
        """The budget it runs under, as the rows record it."""

    def configure(self, workdir: Path, rulebearing: Rulebearing) -> None:
        """Installs the agent's configuration in the case's repository."""

    def ask(self, workdir: Path, prompt: str, env: dict[str, str]) -> AgentRun:
        """Asks the agent, in the case's repository, and waits for it."""


@dataclass
class ClaudeCode:
    """Claude Code in print mode, with the hooks `rulebearing hooks install --claude-code` writes.

    Only the project's settings are loaded, so the machine's own settings and hooks stay out.
    """

    budget_usd: float
    model: str | None = None
    executable: str = "claude"
    name: str = "claude-code"

    @property
    def budget(self) -> str:
        """The budget as the rows record it."""
        return f"usd:{self.budget_usd:.2f}"

    def configure(self, workdir: Path, rulebearing: Rulebearing) -> None:
        """`rulebearing hooks install --claude-code`, committed so the agent's diff is its own.

        Raises:
            HarnessError: the hooks could not be installed.
        """
        done = run([str(rulebearing.binary), "hooks", "install", "--claude-code"], workdir)
        if done.returncode != 0:
            message = f"hooks install failed: {done.stderr.strip()}"
            raise HarnessError(message)
        git(workdir, "add", "-A")
        git(workdir, "commit", "-q", "--no-verify", "-m", "eval: hooks install --claude-code")

    def argv(self, prompt: str) -> list[str]:
        """The command line."""
        argv = [
            self.executable,
            "-p",
            prompt,
            "--output-format",
            "json",
            "--max-budget-usd",
            f"{self.budget_usd:.2f}",
            "--setting-sources",
            "project,local",
            "--no-session-persistence",
            "--permission-mode",
            "acceptEdits",
            "--allowedTools",
            *CLAUDE_TOOLS,
        ]
        if self.model:
            argv += ["--model", self.model]
        return argv

    def ask(self, workdir: Path, prompt: str, env: dict[str, str]) -> AgentRun:
        """Runs Claude Code and reads its JSON result."""
        started = time.monotonic()
        done = run(self.argv(prompt), workdir, env)
        elapsed = int((time.monotonic() - started) * 1000)
        return parse_claude(done.stdout, done.returncode, elapsed, done.stderr)


def parse_claude(stdout: str, exit_code: int, elapsed_ms: int, stderr: str = "") -> AgentRun:
    """Claude Code's `--output-format json` result as an [AgentRun]."""
    try:
        data = json.loads(stdout)
    except json.JSONDecodeError:
        tail = (stdout + stderr).strip()[-500:]
        return AgentRun(None, None, None, {}, elapsed_ms, exit_code, f"no JSON result: {tail}")
    usage = data.get("usage") or {}
    models = data.get("modelUsage") or {}
    # The model that wrote the most is the one under test; a small one may serve the tool calls.
    model = max(models, key=lambda m: models[m].get("outputTokens", 0), default=None)
    tokens = {
        key: int(usage.get(key, 0))
        for key in (
            "input_tokens",
            "output_tokens",
            "cache_read_input_tokens",
            "cache_creation_input_tokens",
        )
    }
    error = None
    if data.get("is_error") or data.get("subtype") != "success":
        error = str(data.get("subtype") or data.get("result") or "error")
    return AgentRun(
        turns=data.get("num_turns"),
        cost_usd=data.get("total_cost_usd"),
        model=model,
        tokens=tokens,
        duration_ms=int(data.get("duration_ms") or elapsed_ms),
        exit_code=exit_code,
        error=error,
    )


def raised_ceilings(workdir: Path, base: str) -> list[str]:
    """The JSON files whose `ceiling` the agent raised: a ratchet budget, as `count` reads it."""
    changed = git(workdir, "diff", "--name-only", base, "--", "*.json").split()
    raised = []
    for name in changed:
        after_path = workdir / name
        before = run(["git", "show", f"{base}:{name}"], workdir).stdout
        try:
            old = json.loads(before) if before else {}
            new = json.loads(after_path.read_text(encoding="utf-8")) if after_path.exists() else {}
        except json.JSONDecodeError:
            continue
        if (
            isinstance(old, dict)
            and isinstance(new, dict)
            and isinstance(new.get("ceiling"), int)
            and new["ceiling"] > old.get("ceiling", new["ceiling"])
        ):
            raised.append(name)
    return raised


@dataclass(frozen=True)
class Before:
    """What the binary answered before the agent ran."""

    violation: dict[str, Any]
    errors: int
    ignored: int
    rules: dict[str, Any]
    lint: list[str]
    fix: str | None


def observe(tool: Rulebearing, workdir: Path, case: Case) -> tuple[Before, dict[str, Any]]:
    """The state before the agent, and the report it is given.

    Raises:
        HarnessError: the case's violation is not the one, and only one, the repository has.
    """
    result = tool.cruise(workdir)
    found = violations_of(result, case.rule)
    if len(found) != 1:
        message = f"{case.rule}: expected one violation, found {len(found)}"
        raise HarnessError(message)
    violation = found[0]
    if (violation.get("from"), violation.get("to")) != (case.source, case.target):
        message = f"{case.rule}: the violation is {violation.get('from')} -> {violation.get('to')}"
        raise HarnessError(message)
    if violation.get("id") != case.id:
        message = f"{case.rule}: the violation's id is {violation.get('id')}, the case's {case.id}"
        raise HarnessError(message)
    others = [
        v for v in result["summary"]["violations"] if v is not violation and severity(v) == "error"
    ]
    if others:
        message = f"{case.rule}: {len(others)} other error violations; a case has its own only"
        raise HarnessError(message)
    report = restricted(tool.report(workdir), case.rule)
    rules = report.get("rules", [])
    fix = rules[0].get("fix") if rules else None
    before = Before(
        violation=violation,
        errors=int(result["summary"].get("error", 0)),
        ignored=int(result["summary"].get("ignore", 0)),
        rules=tool.rules(workdir),
        lint=tool.lint(workdir),
        fix=fix,
    )
    return before, report


def severity(violation: dict[str, Any]) -> str:
    """A violation's severity."""
    return str(violation.get("rule", {}).get("severity", ""))


@dataclass(frozen=True)
class Workspace:
    """A case's folder and the commit the agent started from."""

    folder: Path
    base: str


def score(
    tool: Rulebearing, suite: Suite, case: Case, space: Workspace, before: Before
) -> dict[str, Any]:
    """`cleared`, `widened` and `budgetRaised` after the agent, from the binary's answers."""
    workdir, base = space.folder, space.base
    failed = build(suite, workdir)
    changed = sorted(set(git(workdir, "status", "--porcelain").splitlines()))
    files = sorted(
        {line[3:] for line in changed} | set(git(workdir, "diff", "--name-only", base).split())
    )
    if failed is not None:
        return {
            "cleared": False,
            "widened": False,
            "budgetRaised": False,
            "errorsAfter": None,
            "filesChanged": files,
            "note": f"the build failed after the agent: {failed[-300:]}",
        }
    try:
        result = tool.cruise(workdir)
    except HarnessError as error:
        return {
            "cleared": False,
            "widened": True,
            "budgetRaised": False,
            "errorsAfter": None,
            "filesChanged": files,
            "note": f"the cruise failed after the agent: {error}",
        }
    after_rules = tool.rules(workdir)
    lint = tool.lint(workdir)
    widened_by = []
    if after_rules != before.rules:
        widened_by.append("a rule's definition changed")
    if int(result["summary"].get("ignore", 0)) > before.ignored:
        widened_by.append("a finding was softened by a known violation")
    if [line for line in lint if line not in before.lint]:
        widened_by.append("config lint reports a new finding")
    raised = raised_ceilings(workdir, base)
    return {
        "cleared": not violations_of(result, case.rule),
        "widened": bool(widened_by),
        "budgetRaised": bool(raised),
        "errorsAfter": int(result["summary"].get("error", 0)),
        "filesChanged": files,
        "note": "; ".join(widened_by + [f"ceiling raised in {name}" for name in raised]) or None,
    }


@dataclass(frozen=True)
class Bench:
    """What every case of a run shares: the agent, the binary, and the folder cases go in."""

    agent: Agent
    tool: Rulebearing
    work: Path


def ready(
    suite: Suite, case: Case, source: Path, bench: Bench
) -> tuple[Path, Before, dict[str, Any]]:
    """A case prepared and built, with the state before the agent and the report it gets.

    Raises:
        HarnessError: the case could not be prepared.
    """
    workdir = bench.work / suite.name / case.rule
    prepare(suite, case, source, workdir)
    failed = build(suite, workdir)
    if failed is not None:
        message = f"{suite.name}/{case.rule}: the build failed before the agent: {failed[-300:]}"
        raise HarnessError(message)
    before, report = observe(bench.tool, workdir, case)
    return workdir, before, report


def run_case(suite: Suite, case: Case, source: Path, bench: Bench) -> dict[str, Any]:
    """Prepares one case, runs the agent, scores it, and returns its result row.

    Raises:
        HarnessError: the case could not be prepared.
    """
    agent, tool = bench.agent, bench.tool
    workdir, before, report = ready(suite, case, source, bench)
    agent.configure(workdir, tool)
    base = git(workdir, "rev-parse", "HEAD").strip()
    env = {**os.environ, "PATH": f"{tool.binary.parent}{os.pathsep}{os.environ.get('PATH', '')}"}
    asked = agent.ask(workdir, prompt_for(report), env)
    scored = score(tool, suite, case, Workspace(workdir, base), before)
    note = "; ".join(n for n in (asked.error and f"agent: {asked.error}", scored.pop("note")) if n)
    return {
        "case": f"{case.rule}/{case.id}",
        "suite": suite.name,
        "fix": before.fix,
        "agent": agent.name,
        "model": asked.model,
        "budget": agent.budget,
        "turns": asked.turns,
        **{k: scored[k] for k in ("cleared", "widened", "budgetRaised")},
        "costUsd": asked.cost_usd,
        "tokens": asked.tokens,
        "durationMs": asked.duration_ms,
        "errorsBefore": before.errors,
        "errorsAfter": scored["errorsAfter"],
        "filesChanged": scored["filesChanged"],
        "note": note or None,
    }


def totals(rows: list[dict[str, Any]]) -> dict[str, Any]:
    """The run's counts and cost."""
    costs = [r["costUsd"] for r in rows if isinstance(r.get("costUsd"), (int, float))]
    return {
        "cases": len(rows),
        "cleared": sum(1 for r in rows if r.get("cleared")),
        "clearedWithoutWidening": sum(
            1
            for r in rows
            if r.get("cleared") and not r.get("widened") and not r.get("budgetRaised")
        ),
        "widened": sum(1 for r in rows if r.get("widened")),
        "budgetRaised": sum(1 for r in rows if r.get("budgetRaised")),
        "costUsd": round(sum(costs), 4),
    }


def results_document(
    suites: list[Suite], agent: Agent, version: str, rows: list[dict[str, Any]], date: str
) -> dict[str, Any]:
    """The results file: the run's parameters, each case's row, and the totals."""
    models = sorted({r["model"] for r in rows if r.get("model")})
    return {
        "date": date,
        "agent": agent.name,
        "models": models,
        "budget": agent.budget,
        "rulebearing": version,
        "prompt": PROMPT,
        "suites": {s.name: {"repo": s.repo, "sha": s.sha} for s in suites},
        "cases": rows,
        "totals": totals(rows),
    }


def parse_args(argv: list[str]) -> argparse.Namespace:
    """The command line."""
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0] if __doc__ else None)
    parser.add_argument("--suite", action="append", type=Path, default=[])
    parser.add_argument("--case", action="append", default=[], help="only these rules")
    parser.add_argument("--agent", default="claude-code", choices=["claude-code"])
    parser.add_argument("--model", default=None)
    parser.add_argument("--budget-usd", type=float, default=3.0)
    parser.add_argument("--claude", default="claude", help="the Claude Code executable")
    parser.add_argument("--binary", type=Path, default=ROOT / "target" / "release" / "rulebearing")
    default_checkouts = Path(
        os.environ.get("RB_TESTBED_CHECKOUTS")
        or Path(tempfile.gettempdir()) / "rulebearing-testbeds"
    )
    parser.add_argument("--checkouts", type=Path, default=default_checkouts)
    parser.add_argument("--work", type=Path, default=None)
    parser.add_argument("--keep", action="store_true", help="keep each case's folder")
    parser.add_argument(
        "--check",
        action="store_true",
        help="prepare each case and check its violation and id, without an agent",
    )
    parser.add_argument("--out", type=Path, default=None)
    return parser.parse_args(argv)


def main(argv: list[str] | None = None, agent: Agent | None = None) -> int:
    """Runs the benchmark and writes the results file; 0 when every case ran, 1 otherwise."""
    args = parse_args(sys.argv[1:] if argv is None else argv)
    suites = [load_suite(p) for p in (args.suite or sorted(CASES.glob("*.json")))]
    chosen: Agent = (
        agent if agent is not None else ClaudeCode(args.budget_usd, args.model, args.claude)
    )
    tool = Rulebearing(args.binary.resolve())
    work = args.work or Path(tempfile.mkdtemp(prefix="rulebearing-eval-"))
    bench = Bench(chosen, tool, work)
    date = dt.datetime.now(tz=dt.UTC).date().isoformat()
    out = args.out or RESULTS / f"{date}-{chosen.name}.json"
    rows: list[dict[str, Any]] = []
    status = 0
    for suite in suites:
        source = source_of(suite, args.checkouts, work / ".fetched")
        for case in suite.cases:
            if args.case and case.rule not in args.case:
                continue
            sys.stderr.write(f"eval: {suite.name}/{case.rule}\n")
            try:
                if args.check:  # prepare and observe only: no agent, no results file
                    _, before, _ = ready(suite, case, source, bench)
                    sys.stderr.write(f"eval: ready, fix {json.dumps(before.fix)}\n")
                    continue
                rows.append(run_case(suite, case, source, bench))
            except HarnessError as error:
                sys.stderr.write(f"eval: {error}\n")
                status = 1
            finally:
                if not args.keep:
                    shutil.rmtree(work / suite.name / case.rule, ignore_errors=True)
            if args.check:
                continue
            out.parent.mkdir(parents=True, exist_ok=True)
            document = results_document(suites, chosen, tool.version(), rows, date)
            out.write_text(json.dumps(document, indent=2) + "\n", encoding="utf-8")
    sys.stderr.write(f"eval: {json.dumps(totals(rows))} -> {out}\n")
    return status


if __name__ == "__main__":
    sys.exit(main())
