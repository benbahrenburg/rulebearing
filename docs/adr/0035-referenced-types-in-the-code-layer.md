# ADR-0035: The code layer holds the types the analysed code references, as `ArchUnitNET`'s `ReferencedTypes`

- **Status:** Accepted (2026-09-24, by the maintainer)
- **Date:** 2026-09-24
- **Derives from:** [ADR-0004](0004-graph-document-is-cruise-result-superset.md) (additions are additive), [ADR-0009](0009-conformance-suites-as-specification.md), [ADR-0010](0010-crate-layout-and-extractor-boundary.md), [ADR-0011](0011-read-dotnet-assemblies-not-source.md)
- **Constrains:** `crates/rb-model/src/code.rs` (`TypeElement.referenced`), `crates/rb-extract-dotnet`, `crates/rb-rules/src/elements/`, `select.includeReferenced` in `crates/rb-config/src/elements.rs`
- **Implemented by:** [Wave 2 plan](../plans/pending/0002-wave-2-dotnet-python-element-rules.md), [Step 5](../plans/pending/0002-wave-2-dotnet-python-element-rules.md#25-step-5-the-element-rule-engine-and-the-capability-table-2c) and [Step 7](../plans/pending/0002-wave-2-dotnet-python-element-rules.md#27-step-7-gate-2-porting-to-completion-2c)
- **Requirements:** [FR-RULE-03](../prd.md#fr-rule-03), [NFR-CONF-02](../prd.md#nfr-conf-02)

## Context

`ArchUnitNET` keeps two sets of types. `Architecture.Types` are the loaded assemblies' own; `ReferencedTypes` are stubs for what they depend on, built by `DomainResolver` with the definition's kind, visibility and flags when Mono.Cecil can resolve the reference (from the loaded assembly's folder), else as an `UnavailableType` with only a name. Three things read the second set:

- `Types(true)`, `Classes(true)`, `Interfaces(true)`, `Attributes(true)` select from both;
- a name or `typeof` resolves against both (`GetITypeOfType` searches `AllTypes`);
- a relation condition with a nested predicate (`DependOnAnyTypesThat().Are(...)`, `HaveAnyAttributesThat()`) filters the actual dependency targets, which are often referenced types.

`DependenciesToOtherAssembliesTests` loads `ArchUnitNETTests` alone and asserts all three over types defined in `TestAssembly`. NetArchTest searches for dependencies on `System` namespaces, which only referenced types can answer.

## Decision

- `TypeElement` gains `referenced: true` (absent otherwise). The .NET extractor adds one referenced element for every dependency target the code layer does not define and that is not a generic parameter. It reads the assemblies beside the analysed ones that their references name, transitively, only to describe those types; when one defines the target, the element carries its kind, visibility, abstract, sealed, record, value type, nested and generic flags and its assembly names. Otherwise its kind is `unavailable`. A referenced element has no members, dependencies, file or attribution.
- A selection leaves referenced types out unless it sets `select.includeReferenced: true`. Names resolve against both sets. A selector nested in a relation condition sees both, as `ComplexCondition` sees the targets. `onlyDependOn` still judges only dependencies on the architecture's own types, and slices hold only them.
- Framework assemblies from the .NET runtime directory are not read, so a `System` type is `unavailable` where `ArchUnitNET` would resolve it. No ported case depends on the difference, and reading the runtime would make the graph depend on the machine.

## Consequences

- `DependenciesToOtherAssembliesTests` (14 cases) and three NetArchTest searches are ported.
- Graphs of .NET code are larger by one small element per external type referenced.
- TypeScript and Python extractors add no referenced elements; their external targets remain names that resolve but carry no facts.

## Alternatives considered

- **Build stubs in the engine from dependency targets.** Rejected: the engine would know names but not kinds, so `Classes(true)` could not be answered, and [ADR-0010](0010-crate-layout-and-extractor-boundary.md) keeps facts about code in the document.
- **Load the referenced assemblies as analysed ones (`includeDependencies`).** Rejected: their types would become rule objects, which `ArchUnitNET` does not do by default.
- **Read the runtime's reference assemblies.** Deferred: it would make extraction depend on the installed SDK.
