# ADR-0022: The .NET extractor stays the Rust metadata reader; the C# fallback is not invoked

- **Status:** Accepted
- **Date:** 2026-09-22
- **Supersedes:** the "otherwise" branch of [ADR-0003](0003-dotnet-extractor-fallback.md)
- **Derives from:** [ADR-0003](0003-dotnet-extractor-fallback.md) (the trigger), [wave 0 plan § 1.6](../plans/pending/0000-wave-0-spike.md#16-decisions-applied-and-decisions-to-make) (the denominator rule, fixed before measuring), [design § The fallback, decided now rather than under pressure](../artifacts/design.md#the-fallback-decided-now-rather-than-under-pressure)
- **Constrains:** `rb-extract-dotnet`, [Wave 2 plan](../plans/pending/0002-wave-2-dotnet-python-element-rules.md) (its .NET sub-waves stay on the Rust reader)
- **Implemented by:** [Wave 0 plan](../plans/pending/0000-wave-0-spike.md), sub-waves 0D and 0E
- **Requirements:** [FR-EXT-DN-02](../prd.md#fr-ext-dn-02)

## Context

[ADR-0003](0003-dotnet-extractor-fallback.md) fixed the rule: if the Rust reader attributes at least 99% of the .NET oracle repositories' types to a source file within the three-week window, it stays; otherwise a C# `dotnet tool` replaces it. The wave 0 plan operationalised "attributed" before any measurement (§ 1.6 and Step 9): the trigger figure is `(pdb + inferred) / (types - excluded)`, where `excluded` is `<Module>` and every type carrying `CompilerGeneratedAttribute`, and `inferred` covers a nested type taking its enclosing type's file and the `<TypeName>.cs` naming convention.

The reader was built in sub-wave 0D and measured on 2026-09-22 over the eleven .NET oracles in `testbeds/manifest.yaml` at their pinned commits, each built with `-c Release -p:DebugType=portable` (`conformance/archunitnet/scripts/spike-b-attribution.sh`; per-repository JSON under `conformance/archunitnet/attribution/`).

| Figure | Attributed of 4,495 attributable types | Share |
| --- | --- | --- |
| PDB only (`MethodDebugInformation`, `TypeDefinitionDocuments`) | 4,196 | 0.9335 |
| PDB, plus nested types taking a PDB-attributed enclosing type's file | 4,424 | 0.9842 |
| **Trigger figure (§ 1.6): PDB plus both inferences** | **4,463** | **0.9929** |
| Raw, counting the 3,497 excluded types in the denominator | 4,463 of 7,992 | 0.5584 |

Ten of the eleven repositories were measured; TNG/ArchUnitNET's `global.json` requires a newer SDK than the measuring machine had, and the weekly `spike-b` workflow measures it with the current SDK. One project targets .NET Framework (Nager.Date); no assembly had a Windows or missing PDB. `TestAssembly` attributes 45 of 45 types through the PDB.

Of the 32 unattributed types, most are compiler-synthesised without the attribute that would exclude them (`<>y__InlineArray2`1`, a synthesised collection's nested enumerator, `<PrivateImplementationDetails>+__StaticArrayInitTypeSize=88`); the rule counts them against the reader, as it was fixed to. The rest are Razor Page models (`LoginModel` in `Login.cshtml.cs`) and one nested enum in a type with no sequence points.

## Decision

- The trigger figure, 0.9929, is at or above 0.99 inside the window, so **`rb-extract-dotnet`'s Rust reader is the .NET extractor**. `Rulebearing.Extract` is not built, and `rb-ingest` does not gain the fallback entry point.
- The margin is recorded as it is: the decision depends on 39 types attributed by naming convention, and a PDB-only reading (0.9335) would not have cleared the bar. Wave 2 therefore treats attribution quality as a live requirement, not a settled one:
  - the weekly `spike-b` figure continues through the window and afterwards; a pooled trigger figure below 0.99 on two consecutive weeks reopens this decision in a new ADR;
  - wave 2 reports the PDB-only share alongside the trigger figure in its .NET status table, so a regression in the part that does not depend on convention is visible.

## Consequences

- One toolchain and one binary for the .NET path, as [ADR-0002](0002-rust-as-implementation-language.md) intended.
- Attribution by convention is a documented state (`attribution: inferred`), so a rule author can see which findings rest on it.
- [ADR-0003](0003-dotnet-extractor-fallback.md)'s status line changes to "Superseded by ADR-0022", the only edit an accepted ADR permits ([ADR-0001](0001-record-architecture-decisions.md)).

## Alternatives considered

- **Count only PDB attribution and invoke the fallback.** Rejected: the operational definition was fixed in the plan before measuring and approved with it; changing it after seeing the number would make the pre-committed rule meaningless in both directions.
- **Extend the exclusions to every compiler-synthesised name (types whose names begin with `<`).** Rejected for the same reason; it would raise the figure to 0.9993 (29 of the 32 unattributed types have such names), and it is recorded here only so a later ADR can revisit the rule openly.
