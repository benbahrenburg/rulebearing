# ADR-0034: A slice groups .NET types or TypeScript and Python modules; `(*)` names a slice as `ArchUnitNET` does; `segments` keeps the first segments

- **Status:** Proposed
- **Date:** 2026-09-24
- **Derives from:** [ADR-0009](0009-conformance-suites-as-specification.md) (the upstream suite is the specification), [ADR-0010](0010-crate-layout-and-extractor-boundary.md) (no `match language` in the engine), [ADR-0005](0005-native-config-superset-and-compat.md) (native additions are additive)
- **Constrains:** `crates/rb-rules/src/slices.rs`, `crates/rb-config/src/capability.rs`, `crates/rb-config/src/elements.rs` (`SliceRule`), the `rb-extract-python` module layer
- **Implemented by:** [Wave 2 plan](../plans/pending/0002-wave-2-dotnet-python-element-rules.md), [Step 6](../plans/pending/0002-wave-2-dotnet-python-element-rules.md#26-step-6-slice-and-diagram-rules-2c)
- **Requirements:** [FR-RULE-04](../prd.md#fr-rule-04)

## Context

Three facts surfaced while porting `ArchUnitNET` 0.13.4's `SlicesTests` and reproducing import-linter's own contracts.

1. **`(*)` is not one segment.** Plan 0002 § 2.6 says "`(*)` captures one segment, `(**)` captures the remainder". `ArchUnitNET`'s `SliceRuleInitializer.Parse` rewrites `Ns.(*)` to `Ns.(**).` and keeps the count of `(*)` only for `PlantUmlFileBuilder`; the slice identifier is the whole remainder. Its own `MatchingTest` asserts seven slices for both `TestAssembly.Slices.(*)` and `(**)`, and three for `(**)..`.
2. **What a slice holds depends on the language.** [Design § Slice rules](../artifacts/design.md#slice-rules) makes a slice pattern "a namespace pattern for .NET, a dotted module pattern for Python, and a path pattern for TypeScript". A .NET dependency is between types, as `ArchUnitNET` slices; a Python or TypeScript dependency is an import between modules. Slicing Python classes by their module sees inheritance and decorators, not imports, so an import cycle between two packages is invisible.
3. **import-linter's `acyclic_siblings` squashes.** Under each package, a child and everything below it is one sibling. No `ArchUnitNET` pattern says "the first segment, the package itself included": `(*)` and `(*)..` keep the whole remainder, `(**)..` drops a child with no segment after it.

## Decision

- `matching` follows `ArchUnitNET` exactly: a single-asterisk pattern is rewritten as upstream rewrites it, and a slice is named by everything between prefix and postfix. `Ns.(**)..` (or `src/(**)//` for a path) names a slice by its first segment. The plan's sentence is superseded by this ADR, not edited.
- The capability data in `rb-config` gains a per-language slice unit: `Types` for .NET, `Modules` for TypeScript, JavaScript and Python. The engine reads the unit; it does not match on the language. A module member is matched by its path when the pattern's separator is `/`, and by its dotted name otherwise. The Python extractor records each local module's dotted name in the module's existing `namespaces` field, as a .NET file records its namespaces.
- A slice rule may carry `segments: n`, a Rulebearing addition: after the pattern names a slice, only its first `n` segments are kept. `acyclic_siblings` with ancestor `P` is one rule per package `Q` at or below `P`: `matching: "Q.(*)"`, `segments: 1`, `should: beFreeOfCycles`.

## Consequences

- Gate 2's `SlicesTests` pass as upstream asserts them.
- A Python or TypeScript slice rule reads the import graph, and the import-linter oracle compares both of its repository's contracts. A mutation that closes a cycle under `importlinter.application` breaks the contract in import-linter and fires exactly the matching slice rule.
- `rulebearing import import-linter` (2F) writes `acyclic_siblings` as `segments` slice rules, one per package.
- `schema/config-v1.json` documents `segments`; `ArchUnitNET` has nothing it maps to, so the coverage tab gains no row.

## Alternatives considered

- **Make `(*)` one segment, as the plan says.** Rejected: it contradicts upstream's own tests, which [ADR-0009](0009-conformance-suites-as-specification.md) makes the specification.
- **Slice Python and TypeScript types.** Rejected: their dependencies are imports, which the code layer does not hold; a slice rule would pass over an import cycle.
- **`circular: true` with `via`.** The design names it as a mapping for `acyclic_siblings`, but a dependency rule finds cycles between modules, not between squashed packages, so a cycle that runs through two children of a package without a module cycle would pass.
- **A second pattern syntax.** Rejected: `segments` is one integer beside the pattern `ArchUnitNET` users already know, and leaves `matching` identical to upstream.
