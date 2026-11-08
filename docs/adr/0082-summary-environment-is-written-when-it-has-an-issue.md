# ADR-0082: `summary.environment` is written when it has an issue

- **Status:** Accepted (2026-11-07, by the owner)
- **Date:** 2026-11-07
- **Derives from:** [ADR-0080](0080-gate-1-moves-to-dependency-cruiser-18-7-0.md) (18.7.0's compiler warnings in `summary.environment`), [ADR-0004](0004-graph-document-is-cruise-result-superset.md) (the module layer is upstream's schema), [ADR-0012](0012-oxc-for-typescript.md) (oxc reads TypeScript, so no compiler is loaded)
- **Constrains:** `environment` in `crates/rb-cli/src/api.rs`, `evaluate_document` in `crates/rb-cli/src/pipeline.rs`, `environment_issues` in `crates/rb-rules/src/summarize.rs`
- **Requirements:** [FR-CFG-01](../prd.md#fr-cfg-01), [FR-DIST-02](../prd.md#fr-dist-02)

## Context

dependency-cruiser writes `summary.environment` on every cruise: its version, the Node range it supports and the Node it ran on, the processor and operating system with its release, the transpilers it found, the extensions it can read, and `issues`. From 18.7.0, `src/analyze/summarize/verify-environment.mjs` writes an issue for each option that needs a compiler it did not find: `missing-tsc-transpiler` for `tsPreCompilationDeps`, `detectJSDocImports` or the `tsc` parser without TypeScript; `missing-typescript-transpiler` for a `tsConfig` with neither TypeScript nor swc as the parser; `missing-swc-transpiler` and `missing-babel-transpiler`. The `err` reporter prints the issues. Plan 0011 Step 14 ports this.

Rulebearing has never written `summary.environment`. Three facts shape how it can:

1. The binary reads TypeScript, swc's syntax and Babel's with oxc. A build with the TypeScript extractor (every released build) has all three compilers, so none of the four issues can arise. A build without it (`--no-default-features --features extract-python`) reads no TypeScript at all, so all four can, and each is true: such a build misses the TypeScript sources the issue says it may miss.
2. The block's other fields describe the machine: the Node version and the operating system's release. Written on every run, they put machine-specific values into every result, so a result committed on one machine (the cookbook fixtures, the command-line goldens) differs on the next, and two runs agree byte for byte only on one machine.
3. No Node runs the binary, and the binary does not read the operating system's release.

## Decision

`summary.environment` is written when `environment_issues` finds an issue, and only then. When written, it holds every field upstream's schema requires, with the binary's facts:

| Field | Value |
| --- | --- |
| `version` | the binary's version |
| `nodeVersionSupported`, `nodeVersionFound` | `-`, upstream's mark for something not found: no Node runs the binary |
| `osVersionFound` | `<arch> <platform>@-`, the processor and system in Node's names (`x64`, `arm64`, `darwin`, `win32`, `linux`) and `-` for the release |
| `transpilersFound` | `getAvailableTranspilers()`, as the npm API returns it |
| `extensionsFound` | `allExtensions`, as the npm API returns it |
| `issues` | upstream's issues, in its order, with its words and `os.EOL` |

The issues are checked against the transpilers read without the sidecar. The three compilers checked are native, so the sidecar cannot change the answer, and a run without an issue starts no Node.

The npm API's `cruise()` runs the command line's code, so its result carries the same block. `transpilers.ts` resolves the caller's TypeScript and Babel only to read `tsConfig` and `babelConfig` files. It decides nothing about compilers, so the check is not copied there; its ranges are 18.7.0's, which are 18.2.0's.

## Consequences

- A build without the TypeScript extractor warns, in `summary.environment.issues` and the `err` output, when a configuration needs a compiler it lacks. Every other run writes the same bytes as before, so no golden, fixture or cache entry changes.
- The block is absent on a run without an issue, where upstream always writes one. The 18.2.0 and 18.7.0 coverage tabs mark `environment` Parity. That holds for the issues, which are the part a reporter reads, and not for the block on every run; this ADR records the difference. `summary.environment` is optional in upstream's schema, so `--strict-schema` output stays valid either way.
- A result from dependency-cruiser keeps its block: `fmt` re-reports a saved result with its summary's `environment` untouched, as upstream's `report-wrap.mjs` does.

## Alternatives considered

- **Write the block on every run.** That matches upstream's shape, but it makes output machine-specific (fact 2) and fills two of six fields with values that describe nothing (fact 3). Rejected.
- **Write `issues` without the rest of the block.** Upstream's schema requires the six fields whenever `environment` is present, so the result would fail `--strict-schema`. Rejected.
- **Leave the issues out because a released build never has one.** The narrow builds are supported (ADR-0010), and in them the warning is the only sign that TypeScript sources went unread. Rejected.
