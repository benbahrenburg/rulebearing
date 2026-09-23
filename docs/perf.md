# Performance

The target is [NFR-PERF-01](prd.md#nfr-perf-01): a cruise of a 5,500-module TypeScript monorepo in 2 seconds or less, against the 13 seconds dependency-cruiser takes on the private monorepo the design measured ([design § Three pipelines](artifacts/design.md#three-pipelines)). The model behind it is in [architecture § Performance model](architecture.md#performance-model), and the plan's measurement procedure is [plan 0001, Step 20](plans/pending/0001-wave-1-typescript-parity.md#step-20-performance-measurement-for-nfr-perf-01-1g-started-in-1c).

## The command

```sh
rulebearing cruise --config .dependency-cruiser.cjs --output-type json apps packages
```

`hyperfine --warmup 2 --runs 10`, the mean and the 95th percentile. `--progress performance-log` gives the stage split, so a miss can be attributed to a stage.

## The synthetic tree

[`testbeds/synth/gen.mjs`](../testbeds/synth/README.md) writes 5,500 modules across 4 apps and 40 packages, with tsconfig paths, workspace imports, type-only imports and two cycles. [`testbeds/synth/bench.sh`](../testbeds/synth/bench.sh) generates it, adds [a typical configuration](../testbeds/synth/dependency-cruiser.cjs) and times the command. The [`bench` workflow](../.github/workflows/bench.yml) runs it nightly on the standard Linux runner, the machine the target is set for, and writes the figure to its job summary.

| Date | Machine | Commit | Mean | p95 | Stage split (one run) | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| 2026-09-22 | Apple M2 Pro, 12 threads | sub-wave 1C | about 0.35 s | | extraction only | `extract-timing`, the extractor alone ([testbeds/synth](../testbeds/synth/README.md#the-first-measurement-sub-wave-1c)) |
| 2026-09-23 | Apple M2 Pro, 12 threads | sub-wave 1G | 1.97 s | 3.23 s | configuration 2 ms, extract 1,046 ms, evaluate 197 ms, report 143 ms | End to end, timed by `bench.sh` without hyperfine, on a machine with a load average of 24 (two parallel builds and an antivirus scan). Not a clean figure: the runner's is the one that counts |
| 2026-09-23 | GitHub `ubuntu-latest` (Ubuntu 24.04, image 20260920.314) | `f2e4804` (wave 1 merged) | 0.966 s | 1.005 s | configuration 2 ms, extract 176 ms, evaluate 385 ms, report 334 ms | `hyperfine --warmup 2 --runs 10`, σ 22 ms, range 0.942 to 1.005 s, in the [`bench` run](https://github.com/benbahrenburg/rulebearing/actions/runs/35894230077). Meets the 2-second target with half of it to spare |

## The private monorepo

The design's 13-second figure comes from a private monorepo of 5,574 modules. Only the maintainer can run it, with `hyperfine` in that checkout and without `--metrics`, which is wave 2. It is recorded here with the date, the machine and the commit.

| Date | Machine | Commit | Modules | dependency-cruiser | Rulebearing mean | p95 |
| --- | --- | --- | --- | --- | --- | --- |
| | | | 5,574 | 13 s | | |
