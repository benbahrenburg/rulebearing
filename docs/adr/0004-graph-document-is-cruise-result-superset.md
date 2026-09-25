# ADR-0004: The graph document is an additive superset of dependency-cruiser's `cruise-result` schema

- **Status:** Accepted
- **Date:** 2026-09-20
- **Derives from:** [design.md § Architecture](../artifacts/design.md#architecture), [§ The five stages](../artifacts/design.md#the-five-stages) (stage 3), [§ The run and its consumers](../artifacts/design.md#the-run-and-its-consumers); [dependency-cruiser coverage § Result document](../artifacts/dependency-cruiser-18.2.0-coverage.md#result-document-cruise-result-schema)
- **Constrains:** [architecture.md § The graph document](../architecture.md#the-graph-document)
- **Implemented by:** [Wave 0 plan](../plans/pending/0000-wave-0-spike.md), [Wave 1 plan](../plans/pending/0001-wave-1-typescript-parity.md)

## Context

In the reference monorepo the cruise JSON is read by a gate, a ratchet guard, an importer finder and a graph script. Those scripts read `modules[].source`, `modules[].dependencies[].resolved`, `modules.length` and `summary.violations` by name. A tool that only prints violations covers a third of the use; a tool that changes those names breaks every consumer.

## Decision

The graph document has two layers in one JSON:

- **The module layer is dependency-cruiser 18.2.0's `cruise-result` schema, unchanged:** `modules[]`, `folders[]`, `summary`, `revisionData`, with every field the coverage tab lists.
- **Additions are additive only:** `language`, `project`, `namespaces`, `attribution` on a module; `line`, `column`, `dependencyKind`, `member` on a dependency; `inspected` and `vacuousRules[]` on the summary; and a new `code` section with `types[]`, `members[]`, `attributes[]`, `calls[]`, each carrying `language`, `file`, `line`, `column`.
- **`--strict-schema` strips the additions**, and the result must validate against the pinned 18.2.0 `cruise-result` schema. This is conformance gate 1 layer 4 ([ADR-0009](0009-conformance-suites-as-specification.md)).
- A published JSON schema for the superset lives in `rb-model` and is served at the `$schema` URL the config names.

## Consequences

- A script written against dependency-cruiser today reads Rulebearing's output unchanged.
- `rb-model` has no dependencies and every other crate depends on it; changing a field name is a breaking change subject to a new ADR.
- The `code` layer is the contract element, slice and diagram rules read; extractors that cannot fill part of it leave it empty rather than guessing.

## Alternatives considered

- **A new schema with a converter.** Rejected: the converter would be one more step in every pipeline and every consumer.
- **Two documents.** Rejected: one extraction feeding one file is the whole point of `fmt`.
