# allow-empty-by-rule

A rule ahead of its first match. `allowEmpty: true` is the excuse, and without it the rule fails the run.

- Plan: [plan 0005, Step 4](../../../docs/plans/pending/0005-guard-catalogue.md#step-4-the-lifecycle-recipes-and-their-negative-cases-5e)
- Source: the guard cookbook's budgets and exceptions section ([docs/artifacts/guard-cookbook.html](../../../docs/artifacts/guard-cookbook.html))
- Requirement: [FR-CFG-07](../../../docs/prd.md#fr-cfg-07)
- Decisions: [ADR-0007](../../../docs/adr/0007-vacuous-rules-fail-by-default.md), [ADR-0032](../../../docs/adr/0032-liveness-follows-the-configuration-format.md)

## What the graph holds

Two modules under `src/`, and no `plugins/` folder, so the rule's `from` matches nothing. This is the one family of recipe whose `vacuousRules` could be non-empty; `allowEmpty` keeps it empty and the gate exits 0. The graph's shape does not depend on any extractor fact beyond `source`.

## Negative case

| Case | What differs | Exit | Why |
| --- | --- | --- | --- |
| `without-excuse` | the same rule without `allowEmpty` | 2 | a vacuous rule; `expected.json` holds it in `vacuousRules` ([ADR-0007](../../../docs/adr/0007-vacuous-rules-fail-by-default.md)) |

## A defect this fixture found

`config lint --graph` reported `never-matches` for a rule with `allowEmpty`, and told the author to fix or delete a rule the configuration had just excused. `never-matches` now honours `allowEmpty`, on the rule and in the top-level list, with a test in `crates/rb-config/tests/lint.rs`.
