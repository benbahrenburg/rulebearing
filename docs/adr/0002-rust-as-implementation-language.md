# ADR-0002: Rust as the implementation language, one static binary

- **Status:** Accepted
- **Date:** 2026-09-20
- **Derives from:** [design.md § Language decision](../artifacts/design.md#language-decision), [§ Why Rust wins](../artifacts/design.md#why-rust-wins), [§ What Rust does not solve](../artifacts/design.md#what-rust-does-not-solve)
- **Constrains:** [architecture.md § Technology choices](../architecture.md#technology-choices), [§ Distribution](../architecture.md#distribution)
- **Implemented by:** [Wave 0 plan](../plans/pending/0000-wave-0-spike.md)

## Context

The tool must run inside a TypeScript pipeline, a .NET pipeline and a Python pipeline, and inside an agent's Stop hook in under two seconds. The three candidate languages were Rust, C# with NativeAOT, and TypeScript by extending dependency-cruiser. The decisive facts from the design's comparison table:

- dependency-cruiser's resolver, enhanced-resolve, has a maintained Rust port (`oxc_resolver`) and no .NET port. A C# implementation would spawn Node for TypeScript, which is the two-toolchain outcome the single binary exists to avoid.
- Python parsing has a maintained Rust crate (`ruff_python_parser`) and nothing equivalent in .NET.
- The only Rust gap is reading ECMA-335 metadata and portable PDBs; the usable crates are GPL-3 or decode no IL, so a reader of about 3,000 lines against two stable specifications is required.
- Only Rust yields one artefact with no runtime dependency on every host; npm, `dotnet tool` and pip wrappers are thin shells over it.
- The `regex` crate is linear-time by construction, which removes the safe-regex class of complaint heavy users hit.

## Decision

Rulebearing is written in Rust as a Cargo workspace producing one static binary, `rulebearing`, with Node (napi-rs), NuGet and pip wrappers over that binary. The .NET metadata reader is written in Rust behind the extractor boundary, subject to the fallback in [ADR-0003](0003-dotnet-extractor-fallback.md).

## Consequences

- The maintainer's Rust familiarity is the day-to-day cost; the conformance suites ([ADR-0009](0009-conformance-suites-as-specification.md)) are the reviewer of record so that contributors in any language can change an extractor safely.
- Every language-specific dependency is MIT-compatible (oxc, ruff, rquickjs), which [ADR-0019](0019-mit-licence.md) relies on.
- A WebAssembly playground (wave 4) is nearly free because the crates compile to `wasm32`.

## Alternatives considered

- **C# with NativeAOT.** `System.Reflection.Metadata` makes the .NET side trivial, but TypeScript and Python would shell out. Kept as the extractor-only fallback.
- **TypeScript, forking dependency-cruiser.** Native for TypeScript, but requires Node in .NET and Python repositories and keeps the 13-second cruise.
