# ADR-0076: `fix --plan` prints one verified candidate per template, costed by the imports it changes; the eval harness scores the `fix` text on a benchmark with injected violations

- **Status:** Proposed (2026-10-11)
- **Date:** 2026-10-11
- **Derives from:** [plan 0004, Step 9 and Step 10](../plans/pending/0004-wave-4-reach.md#23-steps-for-sub-wave-4c-fix---plan-and-the-eval-harness-funded-only-by-the--12-rule), its [§ 1.6](../plans/pending/0004-wave-4-reach.md#16-interfaces-and-contracts-this-wave-freezes) (the `fix --plan` output and the harness record) and [§ 1.7](../plans/pending/0004-wave-4-reach.md#17-decisions-applied-and-decisions-this-wave-must-make) (exactly three templates; the agent and its budget are parameters); [design § The agentic engineering hat](../artifacts/design.md#the-agentic-engineering-hat-turn-two); [ADR-0021](0021-agent-surface-cli-first.md); [ADR-0015](0015-stable-violation-id.md)
- **Constrains:** `crates/rb-cli/src/cmd/fix_plan.rs`, `cruise --with-plans`, `eval/`
- **Requirements:** [FR-REACH-03](../prd.md#fr-reach-03)

## Context

The design asks `fix --plan <rule>` for "the three cheapest refactors that clear it, as a machine-readable plan (move file to a legal directory, introduce a port in package X, invert the edge), each with the number of edges it touches". Plan 0004 freezes the output, fixes the templates at three, and requires every candidate to be verified with `can-import` against a graph with the change applied. It leaves open what "edges it touches" counts, which of the many legal folders a template names, which edge a finding that is not one edge is planned on, and what "legal" means beyond the one edge `can-import` answers for.

## Decision

**One edge per finding.** A dependency, cycle or instability finding is planned on its own `from -> to`; a reachability finding on the first step of its path, since breaking any step clears it. A finding on a module alone (an orphan, a `required` rule, a dependents count), a folder, a project or a code element has no edge to change: its `plans` is empty and its `reason` says so.

**`edgesTouched` counts the import statements the agent edits.** `move`: the moved module's imports plus the imports of it. `port`: two (the importer's import rewritten to the port, the target's new import of it). `invert`: the removed import, the new reversed import (none when the target already imports the importer), and one per other dependent of the target, listed as `dependentsToChange`.

**One candidate per template, the first legal one in a fixed order.** Both `move` and `port` try the folders of the module's language that `place` lists, except the repository root (a module there belongs to no package): `move` the fewest folders away from the module's own folder first, `port` the fewest away from both ends' folders, summed; then the deeper; then by name. The template's plan is the first candidate that is legal. At most three plans are printed, one per template, sorted by `edgesTouched`, then `move`, `port`, `invert`. A port is a module of one language, so a cross-language edge gets no port ([ADR-0014](0014-no-invented-cross-language-edges.md)); a target that is a package, a built-in or an unresolved name gets neither a port nor an inversion.

**Legal means two checks on the changed graph.** Every edge the change adds is allowed by `can-import`'s own decision (`legalBy: "can-import"`), and the whole rule set, evaluated as `cruise` evaluates it, no longer reports the violation and reports no error-severity violation, and no violation of the planned rule, that the unchanged graph does not. A moved module's findings are compared under its old path, so a finding it already had is not counted as new. A candidate that fails either check is never printed; with none left, `reason` names what ruled out each template.

**`cruise --output-type agent --with-plans`** adds the same `plans` (and `reason`) to each violation the `agent` report shows. With any other reporter the flag is refused (exit 3), as `--from` is.

**The eval harness's benchmark is three suites, and the .NET violations are injected.** The plan names "every violation on the mutation branch and the imported rules of two .NET oracles". The twelve mutation violations are cases as they are, each isolated to its own folder so the agent sees one. The two .NET oracles with imported rules that agree with their tests (phongnguyend/Practical.CleanArchitecture, five rules; onebeyond/monaco, six) report no violation at their pinned SHAs, since their tests pass, so each rule gets a committed patch that puts one violation in, and the repository is built before and after the agent. The `fix` text a case scores lives in the suite's configuration; for the .NET suites it is added to what `import archunit` writes and nothing else changes.

**A case is scored from the binary's own answers.** `cleared`: `cruise` reports no violation of the case's rule. `widened`: a rule's definition in `rules --json` changed, `summary.ignore` grew (a known violation softened a finding), or `config lint` reports a new finding. `budgetRaised`: a JSON file's `ceiling` went up. The agent is told only a fixed sentence and the `agent` report restricted to the case's rule. The agent and its budget are parameters (`--agent`, `--model`, `--budget-usd`); Claude Code's `--max-budget-usd` is the budget it enforces, so the rows record the budget in dollars and the tokens the agent reported alongside. Each row carries the agent, the model the agent reported, the budget, turns, cost and tokens.

## Consequences

- A printed plan is legal by construction, and a test proves it independently: each fixture plan applied to the graph as JSON, every added edge answered `yes` by `can-import --graph`, and no rule's count up in `rules --json`.
- `move` follows the rules and nothing else, so it can name a folder outside the rule's scope; that is what "move file to a legal directory" means when the rules are the only statement of intent. The `port` and `invert` plans are offered beside it, cheapest first, and the agent chooses.
- The planner evaluates the graph once, then once per candidate tried. On the mutation branch (285 modules) a rule plans in under half a second.

## Alternatives considered

| Option | Why not |
| --- | --- |
| The three cheapest candidates of any template | On a real graph the three are the same template in three folders, all the same cost, and the agent loses the choice of refactor the design lists. |
| `edgesTouched` as the edges in the changed graph that differ | A moved file's edges all "differ" while its importers' statements change too; the count the agent pays is statements edited. |
| Verify with `can-import` only | It answers for one edge: a move that closes a cycle elsewhere, or a port that empties a `required` rule's target, would pass. |
| Verify with the full evaluation only | Correct, but the plan freezes `legalBy: "can-import"`, and the per-edge answer is what the agent will ask again before it writes the import. |
| Benchmark the .NET oracles as they are | They have no violation to fix: their architecture tests pass at the pinned SHA. |
| One case with all twelve mutations present | The `Stop` hook blocks while any error remains, so every case would fix all twelve and the rows would not be per `fix`. |
| A token budget enforced by the harness | Claude Code enforces a dollar budget itself; a harness-side token cap would have to kill the agent mid-edit and score a half-made change. |
