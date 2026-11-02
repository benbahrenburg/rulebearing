# allow-empty-by-list

The same excuse for a rule that lives in an extended dependency-cruiser file, which cannot carry the key, because dependency-cruiser's schema refuses it on a rule.

- Plan: [plan 0005, Step 4](../../../docs/plans/pending/0005-guard-catalogue.md#step-4-the-lifecycle-recipes-and-their-negative-cases-5e)
- Source: the guard cookbook's budgets and exceptions section ([docs/artifacts/guard-cookbook.html](../../../docs/artifacts/guard-cookbook.html))
- Requirement: [FR-CFG-07](../../../docs/prd.md#fr-cfg-07)
- Decisions: [ADR-0005](../../../docs/adr/0005-native-config-superset-and-compat.md), [ADR-0007](../../../docs/adr/0007-vacuous-rules-fail-by-default.md), [ADR-0032](../../../docs/adr/0032-liveness-follows-the-configuration-format.md)

## What the graph holds

Two modules under `src/`, and no `plugins/` folder, so the rule's `from` matches nothing. The top-level `allowEmpty` names it, so `vacuousRules` is empty and the gate exits 0.

## The native entry

`rulebearing.yaml` also names `plugins-stay-apart` under `rules.dependencies.forbidden`, with only `fix` and `examples`. A rule named the same as an extended rule replaces the attributes it names, and a dependency-cruiser file cannot hold those two, so this is how such a rule gets them. Its `from`, `to`, `severity` and `comment` stay in `.dependency-cruiser.json`.

## Negative case

| Case | What differs | Exit | Why |
| --- | --- | --- | --- |
| `unknown-name` | the list names `plugins-stay-apat` | 3 | a name that is no rule is a configuration error, so an exception cannot outlive its rule ([ADR-0032](../../../docs/adr/0032-liveness-follows-the-configuration-format.md)) |
