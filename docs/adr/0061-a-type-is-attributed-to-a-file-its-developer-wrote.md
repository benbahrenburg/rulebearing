# ADR-0061: A type is attributed to a file its developer wrote; a source generator's output under `obj/` only when the type is in no other

- **Status:** Accepted (2026-10-08, by the owner)
- **Date:** 2026-10-08
- **Derives from:** [ADR-0011](0011-read-dotnet-assemblies-not-source.md) (a type's fields and attributes are attributed to the document of its first constructor or first method), [design § One engine](../artifacts/design.md#one-engine-three-languages-one-monorepo) (row Module identity)
- **Supersedes:** nothing. It refines ADR-0011's attribution sentence for a type whose code is in more than one document.
- **Constrains:** `crates/rb-extract-dotnet/src/attribute.rs`, `crates/rb-extract-dotnet/src/discover/mod.rs`, [docs/source-mode.md](../source-mode.md)
- **Implemented by:** [PR #72](https://github.com/benbahrenburg/rulebearing/pull/72)
- **Requirements:** [FR-EXT-DN-04](../prd.md#fr-ext-dn-04)

## Context

Compiled mode gives every type one file, and the type's own dependencies (its base class, the interfaces it implements, its fields' and attributes' types) become that file's edges. ADR-0011 takes the file from the portable PDB: Roslyn's `TypeDefinitionDocuments` record when there is one, else the document of the type's first constructor, else the first sequence point of any of its methods.

A partial type can be split between a file its developer wrote and a file a source generator wrote. Roslyn records a generator's output under the project's intermediate folder, for example `Src/Melville.Pdf.LowLevel/obj/Release/net10.0/Melville.Generators.INPC/<generator>/ComputeOwnerPasswordV3.4E021623.g.cs`. A singleton generator writes the type's constructor there, so the constructor rule picks the generated file. In that case `ComputeOwnerPasswordV3 : ComputeOwnerPasswordV2`, written in `ComputeOwnerPasswordV3.cs`, has no edge to its base class from that file. The edge comes from the generated file instead, which no path rule over the source tree names.

The nightly found it. Source mode's precision against compiled mode on DrJohnMelville/Pdf was 70.75% on every night from 2026-10-03 to 2026-10-08, against the 90% target of [plan 0003, Step 14](../plans/pending/0003-wave-3-operations-surface-inner-loop.md#24-steps-for-sub-wave-3d---mode-source-guard---watch-the-2-s-proof). In that graph 208 of 777 modules were files under `obj/`. Source mode never reads `obj/`, since discovery skips it as build output, so every edge compiled mode credited there was one source mode could not agree with.

## Decision

**A type is attributed to a file someone wrote whenever the PDB names one.** The candidates keep ADR-0011's order: the documents of `TypeDefinitionDocuments`, then the constructor's document, then each method's first point. The first candidate outside build output is taken. A document is build output when its path passes through a folder discovery never searches (`bin`, `obj`, `artifacts`, `node_modules`, `.git`, `.vs`). One list decides both what discovery reads and what attribution avoids.

**A type only a generator declares keeps the generated file.** When every candidate is build output, the first one is taken, as before. The type is still attributed by the PDB (`attribution: pdb`), and its edges are still in the graph.

Method bodies are not moved. An edge that comes from code a generator wrote stays in the generated file's module, because that is where the code is.

## Consequences

- On DrJohnMelville/Pdf at its pinned SHA, built as the nightly builds it (SDK 10.0.401, Release), precision rises from 70.75% to 93.73% and recall moves from 89.85% to 88.74%. The oracle comparison is unchanged: 28 tests, 16 agree, 0 disagree.
- A path rule over a project's source folders now sees a partial type's base class and attributes in the file that declares them.
- A repository with no source generator is unchanged. No committed graph under `conformance/` or `crates/rb-extract-dotnet/tests/fixtures/` changed.
- The fixture `crates/rb-extract-dotnet/tests/fixtures/partial-generated` reproduces the case with a real compiler. Its build script places one file under `src/obj/Generator/` for the build only, where a generator's output lies. `a_partial_type_is_the_file_its_developer_wrote` asserts both halves: the written file keeps the base class, and a type only the generated file declares keeps that file.

## Alternatives considered

- **Credit a partial type's dependencies to every file it is in.** This would put the base-class edge on the written file. It would also put it on every generated file, where nothing names the base class, and inflate the edges a rule sees.
- **Drop generated files from the graph.** That would lose edges that do come from generated code (a generated factory's `new`), and drop types only a generator declares. Compiled mode would then report less than the assembly holds.
- **Recognise generated documents by Roslyn's naming (`<generator assembly>/<generator type>/<hint>.g.cs`) or by embedded source.** Both depend on compiler details that have changed between SDKs. The build-output folders are already the boundary discovery uses, so one list decides both.
- **Measure precision against a different compiled graph.** The nightly compares with the graph the oracle's own tests run against, which is the graph users get. The defect was in that graph.
