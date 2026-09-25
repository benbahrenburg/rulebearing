# ADR-0010: Crate layout: extractors behind a feature-gated boundary, the engine language-agnostic

- **Status:** Accepted
- **Date:** 2026-09-20
- **Derives from:** [design.md § Crate layout](../artifacts/design.md#crate-layout), [§ Architecture](../artifacts/design.md#architecture), [§ One engine, three languages, one monorepo](../artifacts/design.md#one-engine-three-languages-one-monorepo)
- **Constrains:** [architecture.md § Crate layout](../architecture.md#crate-layout), the workspace `Cargo.toml`
- **Implemented by:** [Wave 0 plan](../plans/pending/0000-wave-0-spike.md)

## Context

The rule engine must never know which language a node came from, and the .NET extractor must be replaceable by a C# program without touching anything else ([ADR-0003](0003-dotnet-extractor-fallback.md)). A Python-only build should not link the metadata reader.

## Decision

The Cargo workspace has these crates, with the dependency direction fixed as listed:

| Crate | Owns | Depends on |
| --- | --- | --- |
| `rb-model` | graph document types, JSON schema, serde | nothing |
| `rb-config` | native and dependency-cruiser config parsing, `extends`, presets, `defines`, `$0`–`$9` captures, the QuickJS evaluator | `rb-model` |
| `rb-rules` | matchers, cycles, reachability, dependents, instability, element predicates and conditions, slices, PlantUML adherence, violation summary | `rb-model`, `rb-config` |
| `rb-extract-ts` | workspace discovery, `oxc_parser`, `oxc_resolver`, npm classification, Vue and Svelte splitting, JSDoc and triple-slash parsing | `rb-model` |
| `rb-extract-dotnet` | MSBuild discovery, ECMA-335 and portable PDB readers, IL operand scan, edge projection | `rb-model` |
| `rb-extract-python` | `ruff_python_parser`, import resolver, stdlib list, `TYPE_CHECKING` | `rb-model` |
| `rb-ingest` | dependency-cruiser and ArchUnitNET-style JSON in, graph document out | `rb-model` |
| `rb-report` | every reporter | `rb-model`, `rb-rules` |
| `rb-cli` | every subcommand, cache, `--affected` | all of the above |
| `rb-node` | napi-rs binding exposing `cruise()` and `format()` | `rb-cli` |

Rules of the boundary:

1. Extractors are the only crates that read files other than the config. Each is a Cargo feature of `rb-cli` (`extract-ts`, `extract-dotnet`, `extract-python`), on by default, so a narrower build never links the others.
2. An extractor's only output is the graph document. It may not depend on `rb-config` or `rb-rules`; per-language options reach it as a plain struct defined in `rb-model`.
3. `rb-rules` and `rb-report` compare strings and read the document; they contain no `match language`.
4. Test adapters (`Rulebearing.TestAdapter`, `pytest-rulebearing`, the vitest reporter) and the wrappers live outside the workspace under `adapters/` and `wrappers/` and read the JSON.

## Consequences

- Swapping the .NET extractor for the C# fallback is a change to `rb-ingest` and a build script, nothing else.
- A cross-language rule is a matcher over one document, which is the whole reason the boundary exists.
- The layout is enforced by this repository's own `rulebearing.yaml` from wave 1, citing this ADR.

## Alternatives considered

- **One crate.** Rejected: compile times and the fallback swap.
- **A plugin ABI for extractors.** Rejected for now: features are enough for three languages; revisit if a fourth language is proposed.
