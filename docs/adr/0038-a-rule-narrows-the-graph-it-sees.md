# ADR-0038: A rule narrows the graph it sees: `graph.ignore`, `graph.dependencyTypesNot`, `graph.modulesNot`, `graph.chainsThrough`

- **Status:** Accepted (2026-09-24, by the maintainer)
- **Date:** 2026-09-24
- **Derives from:** [ADR-0005](0005-native-config-superset-and-compat.md) (native additions are additive; a dependency-cruiser file never sees them), [ADR-0007](0007-vacuous-rules-fail-by-default.md) (liveness), [ADR-0010](0010-crate-layout-and-extractor-boundary.md) (the engine compares strings the document carries), [ADR-0013](0013-ruff-parser-for-python.md) (the import-linter mapping), [ADR-0037](0037-baseline-modes.md) (`shrink-only`)
- **Constrains:** `crates/rb-config/src/model.rs` (`Rule`, the shorthands), `crates/rb-config/src/normalize.rs`, `crates/rb-config/src/elements.rs` (`SliceRule`), `crates/rb-rules/src/derive.rs`, `crates/rb-rules/src/validate.rs`, `crates/rb-rules/src/slices.rs`, `crates/rb-rules/src/graph/`, `crates/rb-cli/src/cmd/import/import_linter.rs`, `schema/config-v1.json`
- **Implemented by:** [Wave 2 plan](../plans/pending/0002-wave-2-dotnet-python-element-rules.md), [Step 11](../plans/pending/0002-wave-2-dotnet-python-element-rules.md#211-step-11-the-three-importers-and-oracle-agreement-2f)
- **Requirements:** [FR-RULE-07](../prd.md#fr-rule-07), [FR-RULE-08](../prd.md#fr-rule-08), [NFR-CONF-03](../prd.md#nfr-conf-03)

## Context

The Python oracle harness ([testbeds/README.md § Oracle harness](../../testbeds/README.md#oracle-harness)) compares import-linter's verdict on each contract with the rules `rulebearing import import-linter` writes from it. On 2026-09-24, 12 of 86 contracts on 8 repositories disagreed, each one reported broken by Rulebearing and kept by import-linter. The harness re-checked every one over Rulebearing's own graph with import-linter's filters applied, and every one was then kept. import-linter narrows the graph a contract sees before it follows any chain, and the imported rules could not:

1. **`ignore_imports` removes imports.** `remove_ignored_imports` deletes each matching import from the contract's copy of the graph, so a chain through it is cut. [Design § import-linter contracts](../artifacts/design.md#import-linter-contracts-for-the-python-teams-who-know-them) maps `ignore_imports` to `knownViolations`; a known violation excuses one reported violation, keyed by importer and rule, and cuts no chain. kedro, napari, kopf, river and sqlfluff disagree on this alone.
2. **`exclude_type_checking_imports = True`** builds the graph without imports under `if TYPE_CHECKING:`. The Python extractor already marks such an edge `type-only` ([design § One engine](../artifacts/design.md#one-engine-three-languages-one-monorepo)), but `to.dependencyTypesNot` is dependency-cruiser's and, on a reachability rule, restricts nothing: upstream matches a reachability rule by path alone.
3. **A folder without `__init__.py` below a root package is not walked.** grimp's graph has no module there and no import of or by one.
4. **A module outside the root packages is a leaf.** grimp builds the graph of the root packages; any other module is at most a squashed external with no imports of its own, so no chain passes through it.

checkov, dify and openedx-platform disagree on combinations of the four. The disagreements are in the imported rules, not in the extractor: the graph comparison in each result file finds no unexplained edge.

## Decision

A dependency rule may carry `graph`, a Rulebearing addition for native configurations: what to take out of the graph before the rule is evaluated. The rule then sees the document's graph less what `graph` removes, for its direct edges and for every chain it follows.

```yaml
rules:
  dependencies:
    forbidden:
      - name: io-not-to-cli
        from: { path: '^kedro/io/' }
        to: { path: '^kedro/framework/cli/', reachable: true }
        graph:
          ignore:
            - { from: '^kedro/io/core\.py$', to: '^kedro/framework/cli/utils\.py$' }
          dependencyTypesNot: [type-only]
          modulesNot: '^kedro/templates(/|$)'
          chainsThrough: '^kedro/'
```

| Key | Removes |
| --- | --- |
| `ignore` | A list of `{ from, to }`, each a pattern or a list of patterns; at least one of the two. An edge is removed when its importer's `source` matches `from` (when given) and its `resolved` matches `to` (when given). `from` alone removes every import by those modules, `to` alone every import of them |
| `dependencyTypesNot` | Every edge carrying any of the listed dependency types |
| `modulesNot` | Patterns over `source`: every edge from or to a matching module, so the module is in the rule's graph as a vertex with no edges |
| `chainsThrough` | Patterns over `source`: a chain continues only from a matching module. The start and the end of a chain need not match. Needs `to.reachable` |

1. **Where it applies.** On a `forbidden` rule, direct or reachability (`to.reachable` true or false), and so on the `layers` and `independence` shorthands, which copy it to each rule they expand into; and on a slice rule (`rules.slices`), whose slice edges are then built from the remaining imports. A direct rule reads `ignore`, `dependencyTypesNot` and `modulesNot`; a reachability rule reads all four, and its `via` path is a path through the narrowed graph. On a slice rule, `graph` narrows module imports only; a slice rule with `graph` over .NET types is refused, since a type dependency is not an edge the patterns name.
2. **Where it is refused (exit 3).** In a `.dependency-cruiser.*` file, as the other native keys are ([ADR-0005](0005-native-config-superset-and-compat.md)); `config convert` to dependency-cruiser leaves the whole rule out, because without `graph` it would report more. On an `allowed` rule (an allow-list is widened by another `allowed` rule, not by removing edges), a `required` rule, a folder-scoped rule, and an orphan or dependents rule, whose derivations read the whole graph. `chainsThrough` on a rule without `to.reachable` and on a slice rule, where there is no chain. An `ignore` entry with neither `from` nor `to`, and an empty `graph`.
3. **Liveness of `ignore`.** An `ignore` entry that matches no edge of the document is an exception that no longer excuses anything: the rule is vacuous with side `graph.ignore[<n>]`, in `summary.vacuousRules[]`, exactly as a `from` that selects nothing ([ADR-0007](0007-vacuous-rules-fail-by-default.md)). `allowEmpty: true` on the rule opts out. This is import-linter's `unmatched_ignore_imports_alerting`, whose default is `error`. `dependencyTypesNot`, `modulesNot` and `chainsThrough` describe what the graph is rather than exceptions to it, so they have no liveness.
4. **`to.dependencyTypesNot` is unchanged.** It stays dependency-cruiser's direct-edge restriction, reachability rules included; `graph.dependencyTypesNot` is the form that cuts chains. Changing the upstream key would break gate 1.
5. **`rulebearing import import-linter` writes it.** On every rule a contract produces:
   - `ignore_imports` becomes `graph.ignore`, one entry per expression, each side the module's exact path (`pkg/a.py` or `pkg/a/__init__.py`, wildcards translated) or, for a module outside the root packages, its dotted name with its submodules, as grimp squashes externals. Every rule of the contract carries every entry, because import-linter removes the imports for the whole contract and an entry can cut a chain between two modules no rule names. A contract whose `unmatched_ignore_imports_alerting` is `none` or `warn` writes `allowEmpty: true` on its rules.
   - `exclude_type_checking_imports = True` becomes `graph.dependencyTypesNot: [type-only]`.
   - The root packages become `graph.chainsThrough` on every reachability rule: the Python files under each root package's folder.
   - A folder without `__init__.py` below a root package becomes a `graph.modulesNot` prefix, the topmost such folder only. The importer reads it from the tree beside the settings, as it already reads the packages `acyclic_siblings` slices and the modules `exhaustive` checks; the extractor records no new fact. `rulebearing import import-linter` run again refreshes the list.
   - A `protected` contract is an allow-list, so the same narrowing is written as `allowed` rules: one per `ignore_imports` expression, one for `type-only` imports of the protected modules under `exclude_type_checking_imports`, one for imports by modules outside the root packages, and one for imports by modules in the folders grimp does not walk. `allowed` rules are one list for the whole file ([design § import-linter contracts](../artifacts/design.md#import-linter-contracts-for-the-python-teams-who-know-them)), so an ignored import of one protected contract is allowed for every protected contract.
   - `knownViolations` are no longer written for `ignore_imports`: the edges they named are gone from the rule's graph, so no violation is left to excuse, and `baseline --baseline-mode shrink-only` would remove every entry. Unmatched-ignore alerting is decision 3.

## Consequences

- The four filters the harness re-applied in `compare.py` are expressible in the imported rules, so the harness compares the imported rules as written. The re-check stays as a diagnosis for a disagreement that remains.
- The design's table row for `ignore_imports` (`knownViolations` with `shrink-only`) is superseded for imported contracts by this ADR, which the plan's Step 11 cites; `docs/artifacts/` is not edited. `knownViolations` and `shrink-only` remain the baseline mechanism for everything else.
- The violation-id contract ([ADR-0015](0015-stable-violation-id.md)) is unchanged: a removed edge yields no violation.
- A reachability rule with `graph` walks a graph of its own. Rules with the same `graph` share one; the others share the document's, so a configuration without `graph` costs nothing more.
- The importer's `modulesNot` list is a snapshot of the tree. A folder that later gains an `__init__.py` stays excluded until the import is run again; the header of the imported file says so.
- `schema/config-v1.json` documents `graph` on the rules, the shorthands and the slice rules.

## Alternatives considered

- **Keep `knownViolations` and widen what an entry excuses to every chain through the edge.** Rejected: a known violation is a reported finding with a stable id ([ADR-0015](0015-stable-violation-id.md)); making it rewrite the graph would change what `baseline` writes and what dependency-cruiser's format means, and the entry would still not express the other three filters.
- **Make `to.dependencyTypesNot` cut chains on a reachability rule.** Rejected: it is an upstream key with upstream semantics, which gate 1 holds; a native file that means something else by the same key would break "a native file is a superset".
- **A module fact from the Python extractor for folders without `__init__.py`.** Rejected for now: it would add a Python-only field to the module layer and a rule predicate that reads it, for one importer's translation, where the importer already reads the tree for `acyclic_siblings` and `exhaustive`. A second consumer of the fact would reopen this.
- **Filters as configuration-wide options (`options.exclude` style).** Rejected: import-linter narrows per contract, and one contract's `ignore_imports` must not excuse an import for another.
- **Separate top-level rule keys (`ignoreEdges`, `chainsThrough`).** Rejected: one `graph` object says in one place that the rule sees a narrowed graph, and keeps the rule's `from` and `to` meaning what they mean in dependency-cruiser.
