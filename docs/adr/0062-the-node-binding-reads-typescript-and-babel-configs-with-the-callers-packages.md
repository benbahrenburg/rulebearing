# ADR-0062: The Node binding answers `extractTSConfig` and `extractBabelConfig` with the caller's own TypeScript and Babel; every other export is the command line's code through `rb-node`

- **Status:** Accepted (2026-10-09, by the owner, choosing this option over a Rust reimplementation and over leaving the two out)
- **Date:** 2026-10-09
- **Derives from:** [ADR-0002](0002-rust-as-implementation-language.md) (Rust, with napi-rs for the Node binding), [ADR-0010](0010-crate-layout-and-extractor-boundary.md) (`rb-node` depends on `rb-cli` only), [ADR-0020](0020-single-name-across-registries.md) (one `rulebearing` package), [coverage § Programmatic API](../artifacts/dependency-cruiser-18.2.0-coverage.md#programmatic-api)
- **Supersedes:** nothing. It narrows one sentence of [plan 0003, Step 22](../plans/pending/0003-wave-3-operations-surface-inner-loop.md#26-steps-for-sub-wave-3f-the-roslyn-analyzer-and-rb-node): "each delegating to `rb-cli`'s library entry points" holds for six of the eight exports.
- **Constrains:** `crates/rb-node`, `crates/rb-cli/src/api.rs`, `wrappers/npm/src/api/`, the platform packages' contents ([docs/release.md](../release.md))
- **Implemented by:** plan 0003, Step 22
- **Requirements:** [FR-DIST-02](../prd.md#fr-dist-02)

## Context

The coverage tab promises dependency-cruiser's programmatic API under the same names in the Node binding: `cruise`, `format`, `allExtensions`, `getAvailableTranspilers`, and the four configuration extractors at `config-utl/extract-*`. Step 22 says each export delegates to `rb-cli`.

Six of the eight are dependency-cruiser logic that Rulebearing already has in Rust. `cruise` and `format` are the command line's `cruise` and `fmt`. `extractDepcruiseConfig` is `rb-config`'s loader with `extends` merged. `extractWebpackResolveConfig` is `rb-config`'s sandboxed webpack evaluation. `allExtensions` and `getAvailableTranspilers` describe what the extractor reads.

The other two are not dependency-cruiser logic. Upstream's `extractTSConfig` is TypeScript's own `readConfigFile` and `parseJsonConfigFileContent` over the file. It returns TypeScript's `ParsedCommandLine`, with option enums as numbers, absolute paths, `lib` as file names, `fileNames` from `files`, `include` and `exclude` globbing, `wildcardDirectories` and `raw`. Upstream's `extractBabelConfig` is Babel's own `loadOptionsSync`. It returns Babel's resolved options, with plugins and presets loaded as `ConfigItem`s from the caller's `node_modules`. Both return `{}` when the package is not installed. Rulebearing's extractor never needs either object: oxc parses every syntax, and the extractor reads `tsconfig.json` and Babel's JSON configurations itself.

A Rust version would reimplement TypeScript's and Babel's configuration parsers. Its output could only approximate theirs, and Babel's plugin resolution would need the plugin packages anyway.

## Decision

**Six exports are Rust, through `rb-node`.** `rb-node` is a thin napi-rs layer over `rb_cli::api`, which reaches the command line's own code without argv:

- `cruise` turns the options object into the configuration a `.dependency-cruiser.json` holding it would be. The rule families of `ruleSet` apply only when `validate` is true, as upstream applies them. The configuration then runs through `cmd::cruise::answer`, which is the command line's run and report with the output returned instead of written.
- `format` is `fmt` with `--exit-code`'s count.
- With no `outputType`, both return the result object with exit code 0, as upstream's identity reporter does.
- `extractDepcruiseConfig` honours `alreadyVisited` and `baseDirectory`.
- `allExtensions` and `getAvailableTranspilers` list upstream's names in upstream's order, each marked available when this build reads it from the working directory:
  - Natively, through oxc and the SFC reader. Here `currentVersion` is `oxc <version>`.
  - Or through the `--sidecar node` path, when that can run.

**`extractTSConfig` and `extractBabelConfig` are upstream's code, ported line for line, run with the caller's `typescript` and `@babel/core`.**
- They live in `wrappers/npm/src/api/config-utl/` and return exactly what dependency-cruiser returns with the same packages installed, `{}` included.
- They are proven by upstream's own specs, ported to vitest.
- JSON5 is read with the `json5` Babel depends on, so the package adds no dependency.

**`cruise` accepts their results by the file they record.** A TypeScript object records `options.configFilePath` and a Babel object records `filename`. `cruise` reads that file itself, so `options.tsConfig.fileName` or `babelConfig.fileName` is set from it. An object that records no file is refused, because Rulebearing cannot apply an object it does not read. So is an object whose file differs from the one the options name. Nothing passed in `transpileOptions` is accepted and then ignored.

**The addon ships in each platform package beside the binary**, as `rulebearing.node`. `RULEBEARING_ADDON` names one to load instead, as `RULEBEARING_BINARY` does for the launcher. The package's main export becomes the API. The launcher moves to `rulebearing/launcher`.

**`unsafe_code` is `deny` in `rb-node`, not `forbid`.** `#[napi]` puts its own `#[allow(unsafe_code)]` on the glue it generates, which `forbid` refuses. Hand-written `unsafe` remains an error, and no other crate changes.

## Consequences

- A script that imports `cruise`, `format`, `allExtensions` or `getAvailableTranspilers` from `dependency-cruiser`, or an extractor from `dependency-cruiser/config-utl/...`, switches by changing the package name. The types are dependency-cruiser 18.2.0's own declarations, vendored, and each export is checked against them when the package compiles.
- `cruise()` returns the command line's result for the same options. A test compares them on a fixture, apart from `rulesFile`, which only a run from a file has. `exitCode` is uncapped, where a process exit code stops at 255.
- With TypeScript or Babel absent, `extractTSConfig` and `extractBabelConfig` return `{}`, as upstream's do. `cruise` treats `{}` as no object.
- Upstream's circular-`extends` message reads "config is circular - a -> b -> a"; Rulebearing's says "the chain is circular" and names the same files. Upstream's `alreadyVisited` accumulates, so a diamond of `extends` (two bases sharing one) is circular there and not here.
- `rb-node` has no Rust test binary, because only a Node process has the N-API symbols. Its tests are JavaScript (`crates/rb-node/__tests__`). Its Rust coverage is measured by building it instrumented and running them (`crates/rb-node/coverage.sh`), inside the per-crate check.
- The musl addon links the C runtime dynamically, as a shared library must.

## Alternatives considered

| Option | Why not |
| --- | --- |
| Reimplement TypeScript's and Babel's configuration parsing in Rust | Three to four more days for output that could only approximate TypeScript's `ParsedCommandLine` and Babel's resolved options. Babel's plugin and preset resolution needs the plugin packages regardless. Rulebearing's own extraction never uses either object. |
| Ship six exports and leave the two out | Breaks the coverage tab's "same names" promise for two names that cost a few lines each when they call the caller's own packages, as upstream does. |
| Apply `transpileOptions` objects directly | Rulebearing's extractor reads files, not TypeScript's or Babel's objects. Applying an object would mean translating TypeScript's and Babel's option models into Rulebearing's, which is the reimplementation above. Ignoring the object silently is forbidden. |
| A separate `@rulebearing/node` package | `@rulebearing` is not verified on npm (ADR-0020), and a second package would split one install into two. |
