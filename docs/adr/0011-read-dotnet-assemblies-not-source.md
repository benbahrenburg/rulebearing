# ADR-0011: .NET edges come from built assemblies and portable PDBs, not C# source

- **Status:** Accepted
- **Date:** 2026-09-20
- **Derives from:** [design.md § Prior art](../artifacts/design.md#prior-art), [§ What each extractor has to get right](../artifacts/design.md#what-each-extractor-has-to-get-right) (.NET), [§ Where it would be ignored](../artifacts/design.md#where-it-would-be-ignored) (the build requirement), [§ Open questions](../artifacts/design.md#open-questions) (.NET Framework)
- **Constrains:** [architecture.md § Extractors](../architecture.md#extractors)
- **Implemented by:** [Wave 0 plan](../plans/pending/0000-wave-0-spike.md), [Wave 2 plan](../plans/pending/0002-wave-2-dotnet-python-element-rules.md), [Wave 3 plan](../plans/pending/0003-wave-3-operations-surface-inner-loop.md) (`--mode source`)

## Context

All three .NET architecture tools read compiled assemblies, because the compiler has already resolved every reference; there is no path-alias problem to reimplement. The reference set is wider than `using`: a body call, a generic instantiation, an attribute, a base type, an interface, a parameter type and a `typeof` are all edges.

## Decision

- The .NET extractor reads the built assemblies' ECMA-335 metadata tables and IL operands for the edge set, and the portable PDB's `Document` and `MethodDebugInformation` tables to map every type and method to a source file and line.
- **A type is not a file.** A type's fields and attributes are attributed to the document of its first constructor or first method. A type with no methods and no PDB row is attributed by naming convention and flagged `attribution: inferred`; a type with no attribution at all reports `attribution: none`, and path-based rules skip it with a warning.
- Every edge carries a `dependencyKind` from `inherits`, `implements`, `field`, `signature`, `body`, `attribute`, `generic-argument`, `typeof`.
- A solution with no built assemblies, or a PDB that is not portable, makes the run untrustworthy (exit 2, [ADR-0008](0008-exit-code-contract.md)). `DebugType=portable` is the SDK default; the wave 0 spike measures how many .NET Framework projects in the test beds need it set.
- **Compiled mode is the gate.** A source mode (`--mode source`, `tree-sitter-c-sharp`, namespace-level, marked `approximate`) is the inner-loop form and lands in wave 3; it never replaces compiled mode in CI.

## Consequences

- A .NET pipeline adds a build step it already has.
- Path matchers work on .NET because the PDB gives every type a file, which is what makes one rule language span three languages.

## Alternatives considered

- **Roslyn over source.** Rejected as the primary: it requires the .NET runtime, and the IL view is what ArchUnitNET's vocabulary (calls, body dependencies) needs. Kept as the wave 3 Roslyn analyzer front-end, which reports the same rules where agents already look.
