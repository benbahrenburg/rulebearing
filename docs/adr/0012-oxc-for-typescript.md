# ADR-0012: `oxc_parser` and `oxc_resolver` for TypeScript and JavaScript

- **Status:** Accepted
- **Date:** 2026-09-20
- **Derives from:** [design.md § The five stages](../artifacts/design.md#the-five-stages) (stage 2), [§ What each extractor has to get right](../artifacts/design.md#what-each-extractor-has-to-get-right) (TypeScript), [§ Language decision](../artifacts/design.md#language-decision); [dependency-cruiser coverage § Extraction and resolution](../artifacts/dependency-cruiser-18.2.0-coverage.md#extraction-and-resolution), [§ Options](../artifacts/dependency-cruiser-18.2.0-coverage.md#options) (`parser`, `enhancedResolveOptions`, `tsConfig`)
- **Constrains:** [architecture.md § Extractors](../architecture.md#extractors)
- **Implemented by:** [Wave 0 plan](../plans/pending/0000-wave-0-spike.md), [Wave 1 plan](../plans/pending/0001-wave-1-typescript-parity.md)

## Context

dependency-cruiser resolves with enhanced-resolve and parses with acorn, swc or tsc. `oxc_resolver` is the Rust port of enhanced-resolve, so `exportsFields`, `conditionNames`, `mainFields`, `aliasFields`, tsconfig `paths` / `baseUrl` / `extends` / references, symlinks and Yarn PnP carry over by construction. `oxc_parser` handles JS, TS, JSX, TSX, decorators and stage-3 syntax in well under a millisecond per file.

## Decision

- `rb-extract-ts` parses with `oxc_parser` and resolves with `oxc_resolver`. Both are MIT.
- The `parser` option (`acorn` / `swc` / `tsc`) is accepted, satisfied by `oxc`, and recorded in `optionsUsed` for the report.
- Every dependency form dependency-cruiser knows is extracted: ES imports and re-exports, `import type`, `import()`, `require`, AMD `define` and `require`, exotic require names, `import =`, triple-slash directives, JSDoc imports (read from the comment table, not by switching parser), `process.getBuiltinModule`.
- Vue and Svelte single-file components are split to their `<script>` blocks and then parsed by `oxc` (wave 2).
- Babel syntax needs no Babel: `oxc` parses it; only `babel-plugin-module-resolver` aliases are read from the Babel config. webpack configs are evaluated in the sandbox ([ADR-0006](0006-embedded-quickjs-config-evaluator.md)) and their `resolve` block read.
- `enhancedResolveOptions.cachedInputFileSystem.cacheDuration` is accepted and ignored; the resolver caches per run.
- CoffeeScript and LiveScript are not parsed natively ([ADR-0017](0017-coffeescript-livescript-sidecar.md)).

## Consequences

- Parity is measured against dependency-cruiser's 546 extraction fixtures ([ADR-0009](0009-conformance-suites-as-specification.md)); the extractor is done when they produce the same edges.
- `oxc` and `oxc_resolver` are pinned in `Cargo.toml` and bumped through a pull request that re-runs gate 1.

## Alternatives considered

- **swc.** A larger dependency, and its resolver is not enhanced-resolve's.
- **tree-sitter.** No resolver; would leave the whole of enhanced-resolve to reimplement.
