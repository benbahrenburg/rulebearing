# ADR-0051: A rule redirects the imports it sees: `graph.redirect`, grimp's import of an unwalked module

- **Status:** Proposed
- **Date:** 2026-09-27
- **Derives from:** [ADR-0038](0038-a-rule-narrows-the-graph-it-sees.md) (a rule narrows the graph it sees), [ADR-0005](0005-native-config-superset-and-compat.md) (native additions are additive), [ADR-0007](0007-vacuous-rules-fail-by-default.md) (liveness), [ADR-0010](0010-crate-layout-and-extractor-boundary.md) (the engine compares strings the document carries), [ADR-0013](0013-ruff-parser-for-python.md) (the import-linter mapping)
- **Constrains:** `crates/rb-config/src/model.rs` (`GraphFilter`, `RedirectedImports`), `crates/rb-config/src/normalize.rs`, `crates/rb-rules/src/graph/view.rs`, `crates/rb-rules/src/graph/indexed.rs`, `crates/rb-rules/src/validate.rs`, `crates/rb-rules/src/slices.rs`, `crates/rb-cli/src/cmd/import/import_linter.rs`, `schema/config-v1.json`
- **Implemented by:** the same files; `crates/rb-rules/tests/narrowed_graph.rs` and the `narrowed` import-linter fixture under `crates/rb-cli/tests/fixtures/import/`
- **Requirements:** [FR-RULE-07](../prd.md#fr-rule-07), [FR-RULE-08](../prd.md#fr-rule-08), [NFR-CONF-03](../prd.md#nfr-conf-03)

## Context

[ADR-0038](0038-a-rule-narrows-the-graph-it-sees.md) writes a folder without `__init__.py` below a root package as `graph.modulesNot`: every edge from or to a module in it leaves the rule's graph. That is half of what grimp does. grimp walks a root package's regular packages only, so it reads none of the imports of a module in such a folder, which `modulesNot` reproduces. But when a module grimp does walk imports one inside such a folder, grimp does not drop the import. When neither the imported name nor its parent is a module it found, it gives the import to the root package that holds the name. `from services.tools.mcp_tools_manage_service import X` in `core/mcp/auth_client.py`, where `services/tools/` has no `__init__.py`, is the import `core.mcp.auth_client -> services` in grimp's graph (grimp 3.17, the one import-linter 2.15 installs). It is the root package even when a walked package sits in between: `from controllers.console.auth.error import X`, with `controllers/console/` a package and `controllers/console/auth/` not, is `-> controllers`, and every import into `core/app/layers/` or `core/rag/extractor/` in dify is `-> core`.

The nightly Python oracle run of 2026-09-27 found the consequence on langgenius/dify. Its `backend-layers` contract lists `core.mcp.auth_client -> services` and eight more imports of that shape in `ignore_imports`, with `unmatched_ignore_imports_alerting = error`. import-linter matches each against the import it attributed to the package and keeps the contract. In the imported rules the same edge had been removed by `modulesNot`, so each entry matched nothing and every rule of the contract was vacuous (`graph.ignore[<n>]`). The harness recorded the contract as an error. The committed result of 2026-09-24 had the same six vacuous rules, and the harness then scored a vacuous rule as agreement.

The ignore entries are only the visible half. Without an entry, grimp's `core -> services` import breaks a `layers` contract that puts `services` above `core`, while the imported rule, having dropped the edge, keeps it. That would be a silent disagreement on a contract that import-linter breaks. A chain differs as well: grimp continues it from the root package (`services/__init__.py`'s imports), not from the unwalked module, whose imports it never read.

## Decision

A dependency rule's `graph` may carry `redirect`, a list of `{ to, into }` entries: an edge whose `resolved` matches `to` leads to the module `into` in the rule's graph.

```yaml
graph:
  modulesNot: '^services/tools(/|$)'
  redirect:
    - { to: '^services/tools(/|$)', into: 'services/__init__.py' }
```

1. **Order.** `redirect` applies first. The first entry whose `to` matches decides, and `into` is a module path as the document writes `source`, not a pattern. `ignore`, `modulesNot` and `dependencyTypesNot` then see the edge where it leads, so in the example `modulesNot` removes the edges *from* the unwalked folder and keeps the redirected edges *to* it. An edge that a redirect leads to its own importer is removed, since grimp records no import of a module by itself. An import the document already has of a module by itself is not touched by that clause.
2. **Where it reads.** The direct-edge matcher matches `to` against the dependency with `resolved` replaced by the redirect's `into`, so `to.path` and the cross-language keys read the module the edge leads to. The reachability derivation walks the redirected edges, so a chain continues from `into`. Slice edges are joined from the redirected edges. The liveness of `graph.ignore` ([ADR-0038](0038-a-rule-narrows-the-graph-it-sees.md) decision 3) reads the redirected edges, so an entry naming the package matches the import grimp attributes to it.
3. **What a violation names.** A violation of a direct rule names the edge as the document resolved it (the file under the unwalked folder), because that is the line to change. A reachability violation's `via` path is a path through the rule's graph, as it already is under ADR-0038.
4. **Where it is legal.** Wherever `graph` is ([ADR-0038](0038-a-rule-narrows-the-graph-it-sees.md) decision 1 and 2): a `forbidden` rule, direct or reachability, the `layers` and `independence` shorthands, and a slice rule over modules. It is refused in a `.dependency-cruiser.*` file with the rest of `graph`. An entry needs both `to` and a non-empty `into`. `redirect` describes what the graph is, like `modulesNot`, so it has no liveness of its own.
5. **`rulebearing import import-linter` writes it.** For each topmost folder without `__init__.py` below a root package, beside the `modulesNot` prefix ADR-0038 writes, one `redirect` entry from that prefix into the `__init__.py` of the root package holding the folder (the deepest, when root packages nest). A `protected` contract is unchanged: its narrowing is written as `allowed` rules, which carry no `graph`.

## Consequences

- The imported dify contract keeps its `ignore_imports` exactly, and a contract that grimp breaks through an unwalked folder is broken by the imported rules too.
- grimp gives an import of a missing module in a walked package (`from core.app.missing import X`) to the root package the same way. The Python extractor reports that import unresolved, so it is not a local edge and no `redirect` reaches it; the harness's graph comparison counts it as `ancestorOfMissing`, as before.
- The graph comparison in `testbeds/oracles/compare.py` still counts grimp's attributed edge as `ancestorOfMissing`, because the document keeps the file the extractor resolved. The document is unchanged: the redirect is a view one rule takes, as the rest of `graph` is.
- A rule with `redirect` builds one more string per redirected edge. A rule without it costs nothing.
- `schema/config-v1.json` documents `redirect` and `RedirectedImports`.

## Alternatives considered

- **Resolve the import to the package in the Python extractor.** Rejected: the extractor's `resolved` is the file Python loads, which dependency-cruiser's shape promises and every other rule reads. grimp's attribution is a property of one import-linter contract's view of the graph, which is what `graph` is for ([ADR-0010](0010-crate-layout-and-extractor-boundary.md): no language fact invented for the engine).
- **Widen each ignore entry to the unwalked folders below the package it names.** Rejected: it keeps the entries live but still drops the unignored imports grimp attributes to the package, so a contract import-linter breaks would still be kept.
- **Change `modulesNot` to drop only the edges from a matching module.** Rejected: `modulesNot` is accepted with its meaning in ADR-0038, and dropping an edge is not the same as leading it to another module. A chain would still end at the unwalked module instead of continuing from the package.
- **A general edge rewrite (`from` and `to` on both sides).** Rejected for now: grimp never re-attributes the importer, because it never reads an unwalked module's imports, and `modulesNot` already removes them. One side is what the known translation needs.
