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
| (design) | | | 5,574 | 13 s | | |
| 2026-09-23 | Apple M2 Pro, 12 threads, load average 5.6 to 6.9 | `4bb7d96` (0.1.0) | 5,838 | 10.64 s (18.2.0, mean of 10) | 3.07 s | 3.36 s |

How the 2026-09-23 row was taken, with `hyperfine --warmup 2 --runs 10` over the roots the repository's own graph script cruises:

- **Same configuration for both.** One rule uses a negative lookahead, which Rulebearing refuses with exit 3 because it has no linear-time equivalent ([ADR-0016](adr/0016-linear-time-regex-and-strict-compat.md)). That rule's pattern had two alternatives, `X(?!Y)` and `XY`, which together match exactly `X`, so both tools ran a scratch copy of the configuration with that one equivalent substitution. The repository itself was not changed.
- **`--no-liveness`.** One rule matches no module. At `4bb7d96` that made Rulebearing exit 2 ([ADR-0007](adr/0007-vacuous-rules-fail-by-default.md)); since [ADR-0032](adr/0032-liveness-follows-the-configuration-format.md) a dependency-cruiser configuration only warns about it. dependency-cruiser has no such check, so it was off, as in layer 5, and both tools did the same work.
- **Parity.** The two results were diffed with layer 5's harness: 5,838 modules and 1 violation in each, 0 differences.
- **Stage split, one run.** Configuration 9 ms, extract 2,044 ms, evaluate 686 ms, report 122 ms. The run spent 10.2 s of system time against 3.3 s of user time.

3.46 times faster than dependency-cruiser, but over the 2-second target, and the miss is in extraction: 2.0 s here against 0.18 s for the synthetic tree's 5,500 modules on the runner. The high system time points at file-system work in resolution, which the synthetic tree does not exercise. Finding and fixing it is a wave 2 performance item; the synthetic figure above meets [NFR-PERF-01](prd.md#nfr-perf-01).

## Scale repositories

The nightly scale rows ([testbeds/README.md](../testbeds/README.md), `scale.sh`) cruise a large repository with the configuration `rulebearing init` writes for it, which carries a `knownViolations` entry for every finding. home-assistant/core (the SHA pinned in `testbeds/manifest.yaml`, Python, 22,926 modules, 165,470 dependencies, 33,020 violations) exposed three quadratic scans over violations, all ported from, or modelled on, pairwise loops upstream:

| Stage | Where | What it did | Now |
| --- | --- | --- | --- |
| evaluate | `summarize_modules` | `uniqWith(isSameViolation)`: each violation against every one kept before it | candidates from an index by rule, `from` and `to`, and by the sorted cycle names |
| report | `rewrap`'s `carry_additions` | a linear search of the saved violations for each recomputed one | a first-match map by rule, `from` and `to` |
| evaluate, `init` | `KnownSet` (`softenKnownViolations`) | every baseline entry tested against every module rule, dependency rule and violation | candidates from an index by id, `from` and rule, type and rule, or cycle names |

Each candidate is still decided by the original predicate, and property tests compare each index with the scan it replaced. The cycle search was not the cost: it already runs only inside a strongly connected component ([`graph/indexed.rs`](../crates/rb-rules/src/graph/indexed.rs)).

| Date | Machine | Command | Before (`d9cb2fa`) | After | Output |
| --- | --- | --- | --- | --- | --- |
| 2026-09-24 | Apple M2 Pro, 12 threads, load average 10 to 16 | `cruise` with `extends: [rulebearing:python, rulebearing:recommended]` | 556 s (evaluate 189 s, report 342 s) | 9.7 s (extract 3.1 s, evaluate 2.9 s, report 3.0 s) | byte-identical |
| 2026-09-24 | same | `init --owner testbed --force` | about 26 CPU-minutes | 25 s | the same `rulebearing.yaml` |
| 2026-09-24 | same | `cruise` with that `rulebearing.yaml` (33,016 known violations) | 788 s | 11.8 s | byte-identical |

The synthetic tree did not move: `hyperfine -N --warmup 3 --runs 20` gave 817 ms before and 810 ms after on the same loaded machine, with identical output.

## The Stop hook on a large .NET solution

The target is [NFR-PERF-02](prd.md#nfr-perf-02): the Stop hook's p95 under 2 s on dotnet/aspnetcore in source mode, which no build precedes ([Wave 3, Step 17](plans/pending/0003-wave-3-operations-surface-inner-loop.md#24-steps-for-sub-wave-3d---mode-source-guard---watch-the-2-s-proof); [source-mode.md](source-mode.md)). [`testbeds/bench/stop-hook.sh`](../testbeds/bench/stop-hook.sh) clones aspnetcore at its pinned SHA (10,706 `.cs` files in 587 projects), writes [the benchmark's rules](../testbeds/bench/aspnetcore.yaml) into it, and runs [`stop_hook.py`](../testbeds/bench/stop_hook.py). The hook is

```sh
rulebearing cruise --from-hook --mode source --cache --affected HEAD
```

run twice untimed so the cache is warm, then once after each of 200 seeded one-line edits, each file restored after its run. The `stop-hook` job of the [nightly](../.github/workflows/nightly-testbeds.yml) runs it on the standard Linux runner and fails the night at a p95 of 2 s or more; the result is published with the others as `stop-hook.json`, and the exit criterion asks for three consecutive nights under the line.

| Date | Machine | Edits | p50 | p95 | p99 | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| 2026-09-30 | Linux 6.18 (aarch64), 2 vCPUs, 12 GB, a Docker VM on the machine below | 40 | 1.62 s | 1.78 s | 1.97 s | The tree copied into the container's own file system; the Linux figure is the one the nightly target is set for, and a hosted runner has 4 vCPUs |
| 2026-09-30 | Apple M2 Pro, 12 threads, 32 GB | 40 | 2.64 s | 2.72 s | 2.74 s | Every file open and git walk on this machine passes an endpoint-security scan; `git status` alone takes 0.5 to 2.6 s here and 40 ms in the container |
| 2026-10-02 | Apple M2 Pro, 12 threads, 32 GB | 200 | 2.71 s | 2.81 s | 2.89 s | The full 200 seeded edits at `112b75c`, nothing else of this repository's running; unchanged from the 40-edit figure, so the 3D guard speed-ups (which shorten evaluation and the report) do not move this machine's hook, whose time is the git walk under the endpoint-security scan. The Linux runner's nightly figure decides the target |
| 2026-10-02 | GitHub `ubuntu-latest`: AMD EPYC 7763, 4 vCPUs, 16 GB (Linux 6.17) | 200 | 2.24 s | 2.27 s | 2.29 s | The nightly `stop-hook` job, dispatched on the branch at `77380a3` ([run 37009293677](https://github.com/benbahrenburg/rulebearing/actions/runs/37009293677)). Over the 2 s of [NFR-PERF-02](prd.md#nfr-perf-02) by about 13%, so the job failed: the target is not met on the runner |

Midway through this work, when the container's p95 was 2.34 s, one warm run after an edit split into 47 ms configuration, 1,086 ms extraction, 765 ms evaluation and 206 ms report. Extraction became cheaper when an incremental source-mode run stopped walking the tree, reading project files and re-serialising unchanged parses; evaluation and the report became cheaper when resolution stopped linking a file to another solution's copy of a type (fewer spurious cycles) and the agent reporter stopped scanning every module per violation.

`guard --watch` ([agents.md](agents.md#the-hook-without-the-wait-guard---watch)) takes the hook out of the loop: the hook serves its answer once the guard confirms it has seen every change. A saved file is checked again in under 100 ms on the integration test's fixture, and on the 5,500-module synthetic tree above at a p50 of 74 to 81 ms and a p95 of 88 to 98 ms over three runs of 40 edits (2026-10-01, Apple M2 Pro, idle at a load average near 3.2), from about 470 ms. At a load average of 8 to 10 the same build gave a p95 of 100 to 107 ms, which is why a timing is taken on a machine running nothing else of this repository's ([ADR-0059](adr/0059-gates-run-by-tier-and-mutants-by-diff.md)). Three changes made the difference: the report no longer re-summarises an unfiltered result and builds its input in parallel; the engine evaluates in parallel without copying each module; and the guard's TypeScript extraction replays its kept walk instead of walking the tree again. Both are under the 100 ms of [NFR-PERF-03](prd.md#nfr-perf-03) on that machine, whose twelve threads the evaluation uses. On the standard Linux runner (4 vCPUs) the same check took a p50 of 229 ms and a p95 of 255 ms (19 ms extracting, 190 ms answering; [run 37009290813](https://github.com/benbahrenburg/rulebearing/actions/runs/37009290813), 2026-10-02), so the target is not met there: four slower cores do less than one of the laptop's, where one thread answers in about 130 ms.

Since then the guard gives its earlier answer again when a save leaves the graph as it was (no import changed), without evaluating. [`testbeds/synth/guard.sh`](../testbeds/synth/guard.sh) therefore saves each seeded file twice and reports both kinds from the daemon's own `latencyMs`; it exits 1 when either p95 reaches 100 ms, and the nightly `bench` workflow runs it after the synthetic benchmark.

| Date | Machine | Save | p50 | p95 | Answer (median) |
| --- | --- | --- | --- | --- | --- |
| 2026-10-02 | Apple M2 Pro, 12 threads, load average near 6 | a comment added | 44 ms | 58 ms | 8 ms |
| 2026-10-02 | Apple M2 Pro, 12 threads, load average near 6 | an import added | 93 ms | 108 ms | 55 ms |
| 2026-10-02 | Apple M2 Pro, one thread | a comment added | 46 ms | 58 ms | 7 ms |
| 2026-10-02 | Apple M2 Pro, one thread | an import added | 173 ms | 188 ms | 137 ms |

| 2026-10-02 | GitHub `ubuntu-latest`, 4 vCPUs ([run 37018546157](https://github.com/benbahrenburg/rulebearing/actions/runs/37018546157)) | a comment added | 50 ms | 59 ms | 17 ms |
| 2026-10-02 | GitHub `ubuntu-latest`, 4 vCPUs (the same run) | an import added | 254 ms | 270 ms | 223 ms |

A save that changes the graph still evaluates every rule over the whole graph, and that is what the runner cannot do in 100 ms. [ADR-0060](adr/0060-the-guards-latency-target-is-set-for-a-developer-machine.md) (Proposed) sets the 100 ms for a developer machine of eight or more threads, has the runner gate the unchanged-graph save at 100 ms and hold the graph-changing one to a ceiling of 324 ms (the recorded 270 ms plus 20%), and defers incremental evaluation until a measured need.
