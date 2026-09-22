# ADR-0005: A native config format that is a strict superset, and dependency-cruiser's format accepted as is

- **Status:** Accepted
- **Date:** 2026-09-20
- **Derives from:** [design.md § Configuration](../artifacts/design.md#configuration-a-native-format-and-dependency-cruisers-as-it-is), [§ The native format](../artifacts/design.md#the-native-format), [§ The rule language](../artifacts/design.md#the-rule-language), [§ Where it would be ignored](../artifacts/design.md#where-it-would-be-ignored)
- **Constrains:** [architecture.md § Configuration and the rule language](../architecture.md#configuration-and-the-rule-language)
- **Implemented by:** [Wave 1 plan](../plans/pending/0001-wave-1-typescript-parity.md), [Wave 2 plan](../plans/pending/0002-wave-2-dotnet-python-element-rules.md)

## Context

Every one of the 112 `.dependency-cruiser.*` files found on GitHub must run on day one, and models trained on dependency-cruiser and ArchUnitNET will write those dialects from memory. A renamed key is an adoption tax paid on every repository and every agent turn.

## Decision

- **Both formats load into one internal model.** A file named `.dependency-cruiser.{json,yaml,yml,cjs,js,mjs}`, or passed with `--config-format dependency-cruiser`, is read with dependency-cruiser's semantics and needs no conversion.
- **The native format (`rulebearing.yaml`, also `.json`, `.jsonc`, `.toml`) is a strict superset.** Every dependency-cruiser key is legal at the same place with the same meaning. The native format adds `languages`, `defines`, `rules.dependencies` / `elements` / `slices` / `diagrams` / `ratchets`, the `layers` and `independence` shorthands, and the rule metadata `fix`, `examples`, `owner`, `expires`. It renames nothing.
- **Element-rule keys are ArchUnitNET's method names in camelCase**, one key per predicate or condition, with `all`, `any`, `not` as the combinators.
- **`config convert`** is lossless from dependency-cruiser to native and lossy the other way, and says exactly what it dropped. **`config lint`** and **`config expand`** exist so nothing is hidden.
- The native schema is published with descriptions at the `$schema` URL.

## Consequences

- `rb-config` carries two front-ends and one model, and the conformance suite runs the dependency-cruiser front-end against the original specs ([ADR-0009](0009-conformance-suites-as-specification.md)).
- A predicate a language cannot answer is a validation error, never a silent false ([ADR-0014](0014-no-invented-cross-language-edges.md) states the companion rule for edges).

## Alternatives considered

- **A new, cleaner rule language.** Rejected: the design's adoption analysis shows a new dialect is the first place agents go wrong.
- **dependency-cruiser's format only.** Rejected: it has no place for element, slice, diagram or ratchet rules, nor for `fix` and `examples`.
