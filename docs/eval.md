# The `fix`-text eval

A rule's `fix` is the sentence an agent reads when the rule fires ([agents.md](agents.md#the-agent-reporter)). This page scores those sentences: a benchmark of violations, each handed to an agent with nothing but the `agent` report, and a check of whether the agent cleared the rule without widening it ([design § The agentic engineering hat](artifacts/design.md#the-agentic-engineering-hat-turn-two); [plan 0004, Step 10](plans/pending/0004-wave-4-reach.md#23-steps-for-sub-wave-4c-fix---plan-and-the-eval-harness-funded-only-by-the--12-rule); [ADR-0076](adr/0076-fix-plan-candidates-are-verified-and-the-eval-harness-scores-fix-text.md); [FR-REACH-03](prd.md#fr-reach-03)). It measures the agent as much as the text, so a `fix` is not called bad on one configuration's result; the table is a record, not a gate.

## The benchmark

| Suite | Repository | The violations | Configuration |
| --- | --- | --- | --- |
| `mutations` | dependency-cruiser at the [manifest](../testbeds/manifest.yaml) SHA, with [the mutation branch](../conformance/dependency-cruiser/mutations/README.md) applied | the twelve, one per rule shape; each case keeps only its own folder under `src/mutation/`, so the agent sees one violation | [eval/cases/mutations/rulebearing.yaml](../eval/cases/mutations/rulebearing.yaml): the branch's rules with a `fix` each |

Each suite is a file under [eval/cases/](../eval/cases/) naming the repository, the SHA, the patches and the cases, each by rule, ends and stable id ([ADR-0015](adr/0015-stable-violation-id.md)). The harness refuses a case whose repository does not hold exactly that violation.

## How a case runs

1. The repository is cloned from the testbed checkout (or fetched at the SHA), the patches applied, and the state committed. A compiled language is built.
2. The agent's configuration is installed and committed. The one that ships is Claude Code with what `rulebearing hooks install --claude-code` writes: the session brief, `impact` before each edit and the `Stop` hook that hands back the findings ([agents.md](agents.md#the-hooks)). Only the repository's settings are loaded, and the agent may edit files and run `rulebearing`, `git`, `node`, `dotnet build` and the file commands.
3. The agent gets a fixed sentence and `rulebearing cruise --output-type agent` restricted to the case's rule, nothing else ([eval/run.py](../eval/run.py), `PROMPT`), and runs under a budget.
4. The repository is built again and scored from the binary's own answers:

| Score | Means |
| --- | --- |
| `cleared` | `cruise` reports no violation of the case's rule |
| `widened` | a rule's definition in `rules --json` changed, a finding was softened to `ignore` by a known violation, or `config lint` reports a finding it did not before |
| `budgetRaised` | a ratchet budget's `ceiling` went up |

Every row records the agent, the model the agent reported, the budget, the turns, the cost and the tokens. The results are committed under `eval/results/` as `<date>-<agent>.json`, and the tables below are written from them by `python3 eval/report.py --write docs/eval.md`; CI checks they are current.

```sh
cargo build --release -p rb-cli
python3 eval/run.py --check                                  # prepare every case, no agent
python3 eval/run.py --agent claude-code --budget-usd 3       # the benchmark; writes eval/results/
python3 eval/report.py --write docs/eval.md
```

## Results

<!-- eval-results:start -->
No run has been recorded yet.
<!-- eval-results:end -->
