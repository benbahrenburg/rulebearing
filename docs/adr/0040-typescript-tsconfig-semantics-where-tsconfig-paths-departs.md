# ADR-0040: A TypeScript 7 tsconfig resolves as TypeScript resolves it where dependency-cruiser's `tsconfig-paths` departs; the tsconfig's `module` decides what is compiled for acorn

- **Status:** Proposed
- **Date:** 2026-09-26
- **Derives from:** the project's promise, "superset, precisely; nothing is dropped"; [ADR-0009](0009-conformance-suites-as-specification.md) (the upstream suites are the specification), [ADR-0012](0012-oxc-for-typescript.md) (oxc parses and `oxc_resolver` resolves), [ADR-0036](0036-markdown-fences-follow-the-configuration-format.md) (the precedent for a behaviour that departs from upstream)
- **Constrains:** `crates/rb-extract-ts` (`lib.rs` `load_tsconfig` and `check_module`, `pipeline.rs` `TsCompilerOptions` and `commonjs_output`), [conformance/divergences.md](../../conformance/divergences.md)
- **Implemented by:** `crates/rb-extract-ts`, with the fixtures `tests/options/ts7-config` and `tests/options/ts-config-module`
- **Requirements:** [FR-EXT-TS-02](../prd.md#fr-ext-ts-02), [FR-EXT-TS-03](../prd.md#fr-ext-ts-03)

## Context

TypeScript 7.0, the native port of the 6.0 compiler, is the `latest` tag on npm. Its source syntax is 6.0's; what changes for an extractor is the tsconfig. `baseUrl` is gone, so `paths` resolve against the folder of the config that declares them, `${configDir}` names the folder of the config that is being compiled, and `moduleResolution` is `bundler` or one of the `node16` to `nodenext` family. The `amd`, `umd`, `system` and `none` module kinds, deprecated in 6.0, are removed.

Rulebearing never loads the `typescript` package. Two things decide whether a TypeScript 7 repository extracts correctly: how `oxc_resolver` reads the tsconfig, and how the extractor models what dependency-cruiser's `transpileModule` step hands acorn when `tsPreCompilationDeps` is off. Both were measured against dependency-cruiser 18.2.0 with TypeScript 6.0.3 (the pin gate 1 records), with the fixtures `tests/options/ts7-config` and `tests/options/ts-config-module`. dependency-cruiser cannot load TypeScript 7, whose package does not carry the JavaScript API its `transpileModule` and tsc parser call; its manifest declares `typescript >=2.0.0 <7.0.0`.

**The compiled module kind.** dependency-cruiser spreads the tsconfig's own `compilerOptions` over its defaults (`src/extract/transpile/typescript-wrap.mjs`). So `module: commonjs`, `node16`, `node18`, `node20` and `nodenext`, and an unset `module` with an ES3 or ES5 `target`, turn every static import and re-export into a `require` for acorn. `commonjs` turns `import()` into one too. `export * as ns` is lowered to an import only under ES2015 modules, and a tsconfig `module` overrides the `nodenext` default of the `.mts` flavour. The extractor read none of this: it lowered `.mts` only, whatever the tsconfig said. This was a parity gap, not a decision, and it is closed. TypeScript 7 makes it common, because `nodenext` is the setting its documentation gives Node projects.

**Two resolutions where TypeScript and dependency-cruiser disagree.** dependency-cruiser resolves `paths` through `tsconfig-paths-webpack-plugin` 4.2.0 and `tsconfig-paths` 4.2.0:

| Case | TypeScript 7 (and 6) | dependency-cruiser 18.2.0 | Cause upstream |
| --- | --- | --- | --- |
| `paths` inherited through `extends`, no `baseUrl` (the shape of a TypeScript 7 monorepo's shared base config) | against the declaring config's folder; `${configDir}` expanded | unresolved | `tsconfig-loader.js` rebases an inherited `baseUrl` but never inherited `paths`, and does not expand `${configDir}` |
| a bare specifier (`src/util.js`) that no `paths` key matches, no `baseUrl` | unresolved (a package lookup) | resolved against the tsconfig's folder, type `undetermined` | the plugin calls `createMatchPathAsync` without `addMatchAll`, which defaults to `true` and adds a match-all `*` whether or not `baseUrl` is set |

`oxc_resolver` gives TypeScript's answer in both cases. Before TypeScript 7 a repository that used `paths` almost always set `baseUrl`, which `tsconfig-paths` handles correctly, so neither difference was reachable from gate 1's suites or from the layer 5 oracles.

## Decision

1. **The tsconfig's `module` and `target` decide what acorn reads**, as `transpileModule` decides it upstream: `TsCompilerOptions::emit` maps them to `Es2015`, `Es2020`, `Node` or `CommonJs`, and the forms are lowered to match. This is parity and needs no divergence row.
2. **`amd`, `umd`, `system` and `none` stop the run** (exit 2, the tsconfig named, the fix given) when a TypeScript file will be compiled for acorn, that is unless `tsPreCompilationDeps` is `true` or the parser is `tsc` or `swc`. Their output wraps imports in a loader that the lowering does not model, and upstream's treatment of `none` differs between `.ts` and `.mts`. Answering them as ES modules would be the silent wrong answer the working agreement forbids. TypeScript 7 does not accept them, so no TypeScript 7 repository reaches this error.
3. **Where `tsconfig-paths` departs from TypeScript, the extractor follows TypeScript**, under both configuration formats. The two cases in the table are recorded as permanent rows in [conformance/divergences.md](../../conformance/divergences.md) for any layer 5 oracle that reaches them, each citing its upstream cause, and the fixtures assert TypeScript's answer with the upstream answer named in their doc comments.

## Consequences

- A TypeScript 7 repository extracts correctly with `tsPreCompilationDeps` on or off. A repository with `module: nodenext` and `tsPreCompilationDeps` off gets the same `require` edges dependency-cruiser reports, where it previously got `import` edges.
- A dependency-cruiser configuration over a TypeScript 7 monorepo resolves inherited `paths` where dependency-cruiser leaves them unresolved, so a `not-to-unresolvable` rule that dependency-cruiser reports there is not reported by Rulebearing. This departs from the drop-in promise in the direction of the compiler, which is the one the repository's own build already agrees with.
- A bare specifier that resolved in dependency-cruiser only through the implicit match-all stays unresolved. Code that compiles under TypeScript cannot contain one, except where the same name also exists as a package, in which case both TypeScript and Rulebearing resolve the package.
- A repository still on `amd`, `umd`, `system` or `none` that ran before this change now exits 2 unless it sets `tsPreCompilationDeps: true`. Its edges were wrong before (`import` where dependency-cruiser reports `amd-define` or `require`).

## Alternatives considered

- **Reproduce `tsconfig-paths` for a dependency-cruiser configuration and follow TypeScript for a native one**, as [ADR-0036](0036-markdown-fences-follow-the-configuration-format.md) splits Markdown fences. Rejected: ADR-0036 kept a documented, tested upstream behaviour, but these two are resolver defects with no upstream test that asserts them. Reproducing them would mean reimplementing a second resolver beside `oxc_resolver` to produce unresolved edges for code that compiles, and only for a TypeScript that dependency-cruiser itself cannot load.
- **Treat the removed module kinds as ES modules.** Rejected: a silent wrong answer.
- **Model `amd` and `umd` output** (`define` dependencies plus the `require` and `exports` pseudo-modules that dependency-cruiser then reports as unresolved). Rejected for now: TypeScript has removed them, and no oracle uses them. A later ADR can add them if a user needs them.
