# Synthetic monorepo

`gen.mjs` writes the public synthetic tree that [plan 0001 § 2 Step 20](../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-20-performance-measurement-for-nfr-perf-01-1g-started-in-1c) measures [NFR-PERF-01](../../docs/prd.md#nfr-perf-01) against: 5,500 TypeScript modules, the size of the private monorepo the design's 13-second figure comes from. It is checked in as a generator, never as files.

```sh
node testbeds/synth/gen.mjs /tmp/synth        # Node 22 or later, no dependencies
cargo run --release -p rb-extract-ts --example extract-timing -- /tmp/synth
```

## What it writes

| Part | Count | Shape |
| --- | --- | --- |
| Packages | 40 (`packages/p00` to `packages/p39`) | 118 modules under `src/feature-<n>/` plus `src/index.ts`, which re-exports them; a `package.json` named `@pkg/<name>` |
| Apps | 4 (`apps/app-0` to `apps/app-3`) | 183 modules plus `src/main.ts`, which imports them all; a `package.json` depending on every package through `workspace:*` |
| Root | 1 | `package.json` with `workspaces: ["apps/*", "packages/*"]`; `tsconfig.json` with `paths` for `@pkg/<name>` and `@pkg/<name>/*` to `packages/<name>/src` |
| Cycles | exactly 2 | `packages/p05/src/cycle/{x,y}.ts` import each other (relative); `packages/p10/src/cycle/a.ts` and `packages/p30/src/cycle/b.ts` import each other through the path aliases |

Every other import points "down": a module imports lower-numbered modules of its own `src` (relative) and lower-numbered packages (by alias), so the rest of the graph is acyclic and the two cycles are the only ones. Each module also has one `import type`, which a run without `tsPreCompilationDeps` drops, as dependency-cruiser does. The PRNG is seeded, so every run writes the same bytes.

## The first measurement (sub-wave 1C)

The extractor alone, `extract-timing` in release mode, with the tree's `tsconfig.json` as `tsConfig`:

| Date | Machine | Commit | Modules | Edges | Mean of three |
| --- | --- | --- | --- | --- | --- |
| 2026-09-22 | Apple M2 Pro, 12 threads | sub-wave 1C | 5,500 | 21,550 (26,958 with `tsPreCompilationDeps: true`) | about 350 ms |

This is extraction only. The end-to-end figure with `hyperfine`, the stage split from `--progress performance-log` and the CI runner's number follow in sub-wave 1G, once `rulebearing cruise` exists ([plan 0001 § 3, Wave 1G](../../docs/plans/pending/0001-wave-1-typescript-parity.md#wave-1g-distribution-zero-diff-upstream-offer)).
