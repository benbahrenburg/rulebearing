# ADR-0064: A `.ts` file is parsed as TypeScript when the tsconfig sets `jsx`

- **Status:** Proposed (2026-10-09)
- **Date:** 2026-10-09
- **Derives from:** [plan 0002, 2D](../plans/pending/0002-wave-2-dotnet-python-element-rules.md#wave-2d-cross-language-rule-additions-presets-vue-svelte-markdown-and-the-remaining-wave-2-option-rows) (the row carried from [plan 0001, Wave 1C](../plans/implemented/0001-wave-1-typescript-parity.md#wave-1c-rb-extract-ts-completion)); [design § Test beds](../artifacts/design.md#test-beds-open-source-repositories-to-validate-against), item 1 (a difference is allowed only as a documented divergence)
- **Constrains:** `crates/rb-extract-ts` (a file's syntax comes from its extension), [conformance/divergences.md](../../conformance/divergences.md)
- **Requirements:** [FR-EXT-TS-02](../prd.md#fr-ext-ts-02), [NFR-CONF-01](../prd.md#nfr-conf-01)

## Context

When `tsPreCompilationDeps` is not `true` and the parser is neither `tsc` nor `swc`, dependency-cruiser compiles each TypeScript file with `typescript.transpileModule` and reads the JavaScript with acorn. It passes the tsconfig's compiler options but no file name (`src/extract/transpile/typescript-wrap.mjs`). TypeScript then names the input `module.tsx` when `jsx` is set, and `module.ts` otherwise. So a tsconfig that sets `jsx` makes every `.ts` file parse as TSX.

TSX syntax is not a superset of TypeScript syntax. A generic arrow function (`<T>(x: T): T => x`) and an angle-bracket type assertion (`<Foo>bar`) are both valid in a `.ts` file and both open a JSX element in a `.tsx` file. tsc reports the parse error and recovers. Whatever the recovery takes as JSX text is no longer code, so an import whose only use falls inside it is elided as unused. The edge then disappears from the cruise result.

Which imports survive depends on where tsc's recovery resynchronises. The fixture `crates/rb-extract-ts/tests/options/ts-jsx` imports `./a`, declares a generic arrow, then imports `./b` and uses both. dependency-cruiser 18.2.0 with TypeScript 6.0.3 keeps `src/b.ts` and drops `src/a.ts`; without `jsx` it keeps both. Wave 1 found the case on langfuse with `tsPreCompilationDeps: false`: 14 edges in 6 files. That is not the oracle's configuration, so gate 1's layer 5 has never compared it.

Rulebearing parses with oxc ([ADR-0012](0012-oxc-for-typescript.md)) and takes a file's syntax from its extension. It reads the fixture's `.ts` file as TypeScript in both configurations and reports both edges.

## Decision

**Rulebearing parses a `.ts`, `.mts` or `.cts` file as TypeScript whatever the tsconfig's `jsx` says.** It does not reproduce upstream's TSX misparse. The difference is a permanent divergence in [conformance/divergences.md](../../conformance/divergences.md), in a section layer 5 does not read, because no oracle runs the configuration.

`crates/rb-extract-ts/tests/options.rs` (`a_ts_file_is_typescript_whatever_the_tsconfig_jsx`) pins Rulebearing's answer. Its doc comment records upstream's answer on the same fixture.

## Consequences

- Rulebearing reports, under this configuration, every edge a `.ts` file declares. dependency-cruiser loses the edges that tsc's recovery swallows. A rule can therefore report a violation that dependency-cruiser misses, never the reverse. The graph stays a superset ([ADR-0004](0004-graph-document-is-cruise-result-superset.md)).
- A repository moving from dependency-cruiser with this configuration can see new violations on its first run. Each one is a real import in its source.
- If an oracle ever runs the configuration, layer 5 reports `dependency-extra` keys on the affected files. They need a gate row naming those files. A broad pattern would also accept real extractor bugs.
- If dependency-cruiser passes the file name to `transpileModule` in a later release, the divergence closes without a change here.

## Alternatives considered

| Option | Why not |
| --- | --- |
| Parse a `.ts` file as TSX when `jsx` is set, as upstream does | oxc's recovery is not tsc's. The two would drop different imports, so the output would still differ from upstream, and now it would also be wrong about the source. |
| Reproduce tsc's error recovery | It means re-implementing tsc's parser failure modes, version by version, to drop edges that exist. |
| Warn when a `.ts` file holds syntax that TSX reads differently | It would need a second parse of every `.ts` file as TSX on every run, to report an upstream defect that the user can avoid by setting `tsPreCompilationDeps: true`. It can be added later without changing this decision. |
| Exit 3 under the configuration | The configuration is valid and Rulebearing answers it correctly. Refusing it would block every repository that sets `jsx` for its `.tsx` files. |
