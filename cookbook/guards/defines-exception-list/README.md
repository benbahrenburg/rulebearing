# defines-exception-list

A named exception list kept out of the regex. The packages still migrating live in `eng/migration.json`, which has its own owner and review, and `${migratingPackages}` substitutes them.

- Plan: [plan 0005, Step 4](../../../docs/plans/pending/0005-guard-catalogue.md#step-4-the-lifecycle-recipes-and-their-negative-cases-5e)
- Source: the guard cookbook's budgets and exceptions section ([docs/artifacts/guard-cookbook.html](../../../docs/artifacts/guard-cookbook.html))
- Requirement: [FR-CFG-04](../../../docs/prd.md#fr-cfg-04)

## What the graph holds

| Package | Imports | Expected |
| --- | --- | --- |
| `billing`, `orders` | `legacy-http` | none: both are in the list |
| `search` | `legacy-http` | one violation: it is not in the list, which proves the rule still fires, so the gate exits 1 |
| `checkout` | `http` | none |

`expanded.yaml` shows the alternation `^packages/(billing|orders)/`. In a real run the edges come from `rb-extract-ts`.

## Negative case

| Case | What differs | Exit | Why |
| --- | --- | --- | --- |
| `missing-file` | `eng/migration.json` is removed; the command is `config lint` | 3 | the configuration cannot load, and the output names the file |

## Differences from the cookbook

The cookbook's `from` is `pathNot: "^packages/(${migratingPackages})/"` alone. Here `pathNot` also excludes `^packages/legacy-http/`, so the legacy package's own imports do not fire.
