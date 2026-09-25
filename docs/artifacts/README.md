# Artifacts

Read-only inputs, exported verbatim from the source design document ([ADR-0001](../adr/0001-record-architecture-decisions.md)). A change of intent is a new export plus an ADR, never an edit here.

| File | Source | Exported |
| --- | --- | --- |
| [design.md](design.md) | "Rulebearing: one architecture rule set for TypeScript, .NET and Python", Design tab | 2026-09-20 |
| [dependency-cruiser-18.2.0-coverage.md](dependency-cruiser-18.2.0-coverage.md) | same document, "dependency-cruiser 18.2.0 coverage" tab: every attribute, option, flag, output type and result field with its Rulebearing status and wave | 2026-09-20 |
| [archunitnet-0.13.4-coverage.md](archunitnet-0.13.4-coverage.md) | same document, "ArchUnitNET 0.13.4 coverage" tab: every selector, predicate, condition, slice, PlantUML, loader and adapter item with its declarative key | 2026-09-20 |

Source: the Claude Docs document at https://claude.ai/code/artifact/74210034-25ca-4a9c-9dd6-bbd24a62ac87 (private). The design's own internal links between tabs (`file/1704a18f-ee24`, `file/6eb75df7-6f94`) refer to the two coverage files above.

The two coverage tabs are also the ledger the conformance gates report against ([ADR-0009](../adr/0009-conformance-suites-as-specification.md)): a row may not say **Parity** until the pinned upstream suite says so.
