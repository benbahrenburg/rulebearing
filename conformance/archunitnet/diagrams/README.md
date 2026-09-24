# ArchUnitNET PlantUML diagrams

The `.puml` files the hand-ported PlantUML cases of conformance gate 2 read, through `adhereTo` in `family: diagram` cases and `file:` in `family: plantuml` cases ([plan 0002, Step 6](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#26-step-6-slice-and-diagram-rules-2c), [ADR-0009](../../../docs/adr/0009-conformance-suites-as-specification.md)). The engine that reads them is `crates/rb-rules/src/plantuml.rs`; the cases are `../ported/PlantUml*.yaml`.

| File | Provenance |
| --- | --- |
| `zzz_test_version_with_errors.puml` | Copied byte for byte from [TNG/ArchUnitNET](https://github.com/TNG/ArchUnitNET) tag `0.13.4` (commit `1ab5943d761d48f86b42f45ef047130dc9aff1c6`), `ArchUnitNETTests/Domain/PlantUml/zzz_test_version_with_errors.puml`, read by `PlantUmlErrorMessagesCheck` |
| `PlantUmlDependenciesTest.*.puml`, `TypeSyntaxElementsTests.*.puml` | What upstream's `ArchUnitNETTests/Domain/PlantUml/TestDiagram.cs` writes for the named test at the same tag: `@startuml`, one `[Name] <<Stereotype>>` line per component, one `Origin --> Target` line per dependency, `@enduml` |

ArchUnitNET is Apache-2.0; its licence and notice are in [`../fixtures/LICENSE`](../fixtures/LICENSE) and [`../fixtures/NOTICE`](../fixtures/NOTICE), which cover these files too. Diagrams the parser cases write inline are in the case files, not here.
