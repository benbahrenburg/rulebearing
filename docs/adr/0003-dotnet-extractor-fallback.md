# ADR-0003: The .NET extractor falls back to a C# `dotnet tool` on a measured trigger

- **Status:** Superseded by [ADR-0022](0022-dotnet-reader-in-rust-confirmed.md)
- **Date:** 2026-09-20
- **Derives from:** [design.md § The fallback, decided now rather than under pressure](../artifacts/design.md#the-fallback-decided-now-rather-than-under-pressure); [§ Waves](../artifacts/design.md#waves) (wave 0 exit criterion)
- **Constrains:** [architecture.md § Extractors](../architecture.md#extractors), [§ Risks and their mitigations](../architecture.md#risks-and-their-mitigations)
- **Implemented by:** [Wave 0 plan](../plans/pending/0000-wave-0-spike.md) § Spike B

## Context

Writing an ECMA-335 metadata and portable PDB reader in Rust is a bounded but real cost, and it is the one place the language choice in [ADR-0002](0002-rust-as-implementation-language.md) could stall the project. The design fixes the decision rule now so it is not made under pressure.

## Decision

The wave 0 spike measures, on the .NET oracle repositories at pinned commits, the share of types the Rust reader attributes to a source file through the portable PDB.

- **If at least 99% of types are attributed within three weeks of part-time work**, the Rust reader in `rb-extract-dotnet` is the .NET extractor.
- **Otherwise** the .NET extractor becomes `Rulebearing.Extract`, a C# `dotnet tool` over `System.Reflection.Metadata` that writes the same graph document, and the Rust binary consumes it through `rb-ingest`. The rule engine, reporters, config, TypeScript and Python sides are unchanged.

The trigger, the measurement script and the result are recorded in the wave 0 plan's status table, and the outcome is recorded as a new ADR that supersedes the open branch of this one.

## Consequences

- The extractor boundary in [ADR-0010](0010-crate-layout-and-extractor-boundary.md) must be strict enough that the swap touches nothing outside `rb-extract-dotnet` and `rb-ingest`.
- The fallback costs two toolchains to build and a .NET runtime on the host, which every .NET repository already has.

## Alternatives considered

- **Decide after wave 1.** Rejected: the .NET side is wave 2's critical path, so the decision must precede it.
- **C# from the start.** Rejected: it forfeits the single binary for the .NET path when the Rust reader may well succeed.
