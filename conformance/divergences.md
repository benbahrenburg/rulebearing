# Documented divergences

Conformance gate 1, layer 5 compares dependency-cruiser's cruise result with Rulebearing's for the same repository, tree, configuration and roots ([scripts/run-layer-5.sh](dependency-cruiser/scripts/run-layer-5.sh), [harness/zero-diff.mjs](dependency-cruiser/harness/zero-diff.mjs)). The design allows a difference only as "a documented divergence" with a reason ([design § Test beds](../docs/artifacts/design.md#test-beds-open-source-repositories-to-validate-against), item 1; [plan 0001, Step 18](../docs/plans/pending/0001-wave-1-typescript-parity.md#step-18-gate-1-layer-5-the-mutation-branch-oracle-zero-diff-1g)). This file is that record, and the gate reads it: a difference whose key matches a row's `Match` expression, for that row's repository (or `*`), is reported as documented; any other difference fails the gate.

The keys `zero-diff.mjs` prints, one per difference:

| Key | Means |
| --- | --- |
| `module-missing <source>`, `module-extra <source>` | a module only dependency-cruiser, or only Rulebearing, reports |
| `module-field <source> <field>` | a module-level field differs |
| `dependency-missing <source> -> <resolved>`, `dependency-extra ...` | an edge only one tool reports |
| `dependency-field <source> -> <resolved> <field>` | a field of an edge differs |
| `violation-missing <rule> <from> -> <to>`, `violation-extra ...` | a `summary.violations` entry only one tool reports |

Rulebearing's additions are never compared, because dependency-cruiser has nothing to compare them with ([ADR-0004](../docs/adr/0004-graph-document-is-cruise-result-superset.md)): `line`, `column`, `dependencyKind` and `language` on a module or dependency, `id`, `fix` and `decision` on a violation, and `summary.inspected` and `summary.vacuousRules`.

A row is either **permanent** (an upstream behaviour Rulebearing deliberately does not reproduce, with the upstream issue or the test that contradicts it) or **open** (a known Rulebearing gap with its owner and the change that closes it; the row is deleted in the change that closes it). The table may only shrink as open rows close.

## Divergences

| Repository | Match | What differs | Reason | Link |
| --- | --- | --- | --- | --- |
| `sverweij/dependency-cruiser` | `^(module\|dependency)-field .+ instability$` | Rulebearing writes `instability` on every module and dependency; dependency-cruiser writes none | **Open, owner rb-cli.** The repository's configuration sets `options.metrics: true`, but `depcruise`'s `--metrics` flag defaults to `false` and command-line options override the configuration's, so dependency-cruiser computes no metrics. `rb-cli` (`configure.rs`) only ever sets `metrics` to `true` from the flag and keeps the configuration's `true` otherwise. The fix: `options.metrics = Some(args.metrics)`, as upstream's commander default does. Verified locally: with it this row's differences are zero | [upstream bin/dependency-cruise.mjs](https://github.com/sverweij/dependency-cruiser/blob/v18.2.0/bin/dependency-cruise.mjs), [plan 0001 Step 18](../docs/plans/pending/0001-wave-1-typescript-parity.md#step-18-gate-1-layer-5-the-mutation-branch-oracle-zero-diff-1g) |
| `sverweij/dependency-cruiser` | `^dependency-field .+ license$` | dependency-cruiser writes `license` on npm dependencies; Rulebearing writes none | **Open, owner rb-cli.** Upstream reads licences when a rule restricts `to.license` or `to.licenseNot` (`ruleSetHasLicenseRule`); this configuration's `no-non-vetted-license` does. The extractor reads them when `ResolveConfig::resolve_licenses` is set, and `rb_rules::derive::has_license_rule` and `has_deprecation_rule` answer the question, but `rb-cli` (`pipeline.rs`, `extract`) does not yet set `resolve_licenses` and `resolve_deprecations` from them after `rb_extract_ts::prepare`. Verified locally: with those two lines this row's differences are zero | [upstream src/graph-utl/rule-set.mjs](https://github.com/sverweij/dependency-cruiser/blob/v18.2.0/src/graph-utl/rule-set.mjs), [plan 0001 Step 18](../docs/plans/pending/0001-wave-1-typescript-parity.md#step-18-gate-1-layer-5-the-mutation-branch-oracle-zero-diff-1g) |
