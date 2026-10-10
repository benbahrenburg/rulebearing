# Cookbook fixtures

Each recipe in the guard catalogue is a fixture here, run by CI with the same commands a user runs. A recipe that is not run is prose, and prose rules do not hold ([plan 0005 § 1.1](../docs/plans/pending/0005-guard-catalogue.md#11-purpose-and-business-value)).

- Plan: [plan 0005](../docs/plans/pending/0005-guard-catalogue.md), [Step 1](../docs/plans/pending/0005-guard-catalogue.md#step-1-fixture-layout-the-runner-and-the-first-recipe-5a)
- Requirements: [FR-RULE-01](../docs/prd.md#fr-rule-01), [FR-CFG-07](../docs/prd.md#fr-cfg-07)
- Decisions: [ADR-0004](../docs/adr/0004-graph-document-is-cruise-result-superset.md) (the graph shape), [ADR-0005](../docs/adr/0005-native-config-superset-and-compat.md) (what conversion drops), [ADR-0007](../docs/adr/0007-vacuous-rules-fail-by-default.md) (no vacuous recipe), [ADR-0021](../docs/adr/0021-agent-surface-cli-first.md) (public commands only), [ADR-0024](../docs/adr/0024-test-quality-gates.md) (a regenerated expectation is explained)

## A fixture

`guards/<slug>/`, where the slug is kebab-case and names the guard, not the rule:

| File | Required | Holds |
| --- | --- | --- |
| `rulebearing.yaml` | yes | the recipe: each rule with `comment` (ending in a decision token), `fix`, `severity` and `examples` with at least one `forbidden` and one `allowed` edge |
| `graph.json` | yes | a small graph in dependency-cruiser's `cruise-result` shape, holding only the modules the recipe needs, with every module fact as the extractor writes it |
| `expected.json` | yes | `summary.violations` and `summary.vacuousRules` exactly as `cruise -T json` prints them |
| `README.md` | yes | what the graph holds and why, which extractor supplies each fact in a real run, and any difference from the cookbook export |
| `expanded.yaml` | when the recipe uses `defines` or a shorthand | `config expand`'s output |
| `converted/.dependency-cruiser.json`, `converted/dropped.txt` | for a dependency-layer recipe | `config convert --to dependency-cruiser`'s output and its report of what it dropped |
| `budget.json` | for a ratchet recipe | the budget file |
| `negative/<case>/rulebearing.yaml`, `exit-code`, optional `graph.json` | when the recipe has a failure to prove | a configuration that must stop `cruise` with that exit code ([ADR-0008](../docs/adr/0008-exit-code-contract.md)) |

The graph makes every rule non-vacuous, so `vacuousRules` is empty, except in a recipe whose point is `allowEmpty`.

## Running

```sh
cargo build --release
cookbook/guards/run.sh no-circular     # one fixture
cookbook/guards/run.sh --all           # every fixture, one line each
```

[`guards/run.sh`](guards/run.sh) lists the commands it runs. `RB` names another binary.

## Adding a recipe

1. Write `rulebearing.yaml`, `graph.json` and `README.md`.
2. Run `RB_UPDATE_SNAPSHOTS=1 cookbook/guards/run.sh <slug>` to write `expected.json`, `expanded.yaml` and `converted/`.
3. Read what it wrote. The violations must be the ones the README says the graph holds, and nothing else.
4. Run `cookbook/guards/run.sh <slug>` without the variable.

A recipe that does not behave as the cookbook says is a finding. Correct the recipe in the fixture and say so in its README; the dated cookbook export in `docs/artifacts/` is not edited. If the engine is wrong rather than the recipe, record the defect in plan 0005's status table against the plan that owns the engine.
