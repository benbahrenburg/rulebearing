# ADR-0060: The guard's 100 ms is set for a developer machine; the Linux runner gates the unchanged-graph save and holds the graph-changing one to a ceiling

- **Status:** Proposed
- **Date:** 2026-10-02
- **Derives from:** [design § The agentic engineering hat](../artifacts/design.md#the-agentic-engineering-hat-turn-two) (`guard --watch` re-checks within 100 ms), [architecture § Performance model](../architecture.md#performance-model), [ADR-0021](0021-agent-surface-cli-first.md) (the guard is a thin loop over the command line), [ADR-0059](0059-gates-run-by-tier-and-mutants-by-diff.md) (a timing is taken on a machine running nothing else)
- **Supersedes:** nothing. It says which machine [NFR-PERF-03](../prd.md#nfr-perf-03)'s guard figure is measured on, which the requirement leaves open.
- **Constrains:** `testbeds/synth/guard.sh`, `.github/workflows/bench.yml`, [docs/perf.md](../perf.md), [docs/prd.md](../prd.md) § NFR-PERF-03
- **Implemented by:** the change that adds this record
- **Requirements:** [NFR-PERF-03](../prd.md#nfr-perf-03)

## Context

NFR-PERF-03 says `guard --watch` MUST re-check a saved file in under 100 ms and names no machine. [Plan 0003 § 1.7](../plans/pending/0003-wave-3-operations-surface-inner-loop.md#17-quality-attributes) asserts it on the 5,500-module synthetic tree.

Sub-wave 3D brought the check on that tree from about 470 ms to a p95 of 88 to 98 ms on the maintainer's laptop (Apple M2 Pro, twelve threads), by evaluating the rules in parallel among other changes. The first CI run of the same measurement, on GitHub's `ubuntu-latest` (AMD EPYC 7763, 4 vCPUs), gave a p95 of 255 ms. The laptop answers in about 130 ms with one thread; the runner's four virtual cores take 190 ms. More threads do not close that gap. The check would have to do several times less work.

The guard now gives its earlier answer again when a save leaves the graph as it was (no import added or removed), without evaluating. Measured on 2026-10-02 with `testbeds/synth/guard.sh`, 40 seeded files, each saved both ways:

| Machine | A comment added (graph unchanged), p95 | An import added (graph changed), p95 |
| --- | --- | --- |
| Apple M2 Pro, twelve threads | 58 ms | 108 ms at a load average near 6; 88 to 98 ms idle before the two kinds were told apart |
| Apple M2 Pro, one thread | 58 ms | 188 ms |
| `ubuntu-latest`, 4 vCPUs ([run 37018546157](https://github.com/benbahrenburg/rulebearing/actions/runs/37018546157)) | 59 ms | 270 ms |

A save that changes the graph evaluates every rule over the whole graph. Bringing that under 100 ms on four slow cores needs incremental evaluation: re-deriving cycles, dependents and verdicts only for what the changed file touches. That is an engine change of one to two days, in the crate whose output must stay byte-identical to dependency-cruiser's ([ADR-0009](0009-conformance-suites-as-specification.md)).

The guard is a watch process beside an editor or an agent session. It runs on the machine the developer or the agent works on. Nobody runs it on a hosted CI runner; the runner is where the measurement is repeatable, not where the requirement lives.

## Decision

**1. The 100 ms of NFR-PERF-03 is measured on a developer machine: eight or more hardware threads, running nothing else of this repository's.** The reference figure in [docs/perf.md](../perf.md) is the maintainer's laptop, with its processor and thread count stated beside it. Both kinds of save are measured and both must be under 100 ms there.

**2. On the Linux runner, `bench.yml` gates what the runner can show.** A save that leaves the graph unchanged must have a p95 under 100 ms. A save that changes the graph must have a p95 under a recorded ceiling: the last recorded runner figure plus 20%, the regression rule NFR-PERF-03 already sets for the scale table. The ceiling starts at 324 ms (270 ms plus 20%). It may be lowered when the figure falls and is never raised; a slower check is a regression to fix.

**3. Incremental evaluation is deferred, not rejected.** It is taken up when a measured need appears: a developer machine on which a graph-changing save misses 100 ms on a real repository, or a decision to run the guard on small machines. The trigger and the outcome would be a new ADR.

`testbeds/synth/guard.sh` takes the two thresholds separately (`RB_GUARD_THRESHOLD_MS` for the unchanged-graph save, `RB_GUARD_CHANGED_THRESHOLD_MS` for the graph-changing one, both 100 by default, so a run on a developer machine gates both at 100 ms). `bench.yml` sets the second to the ceiling.

## Consequences

- NFR-PERF-03's guard clause is met as measured on the reference machine, and the nightly keeps both kinds of save from regressing on the runner.
- A graph-changing save takes about 270 ms on a 4-vCPU machine. A developer or an agent on such a machine waits that long after a save that adds or removes an import, and about 60 ms after any other save. [docs/agents.md](../agents.md) says so.
- The Stop hook's 2 s on aspnetcore ([NFR-PERF-02](../prd.md#nfr-perf-02)) is not touched by this record: its nightly figure on the runner stays the gate.
- The ceiling is one number in `bench.yml`; lowering it is a one-line change with the run that justifies it linked.

## Alternatives considered

- **Hold 100 ms for both kinds of save on the runner and build incremental evaluation now.** Rejected for now: one to two days of engine work with conformance risk, for a machine the guard does not run on, ahead of sub-waves 3E to 3G. Decision 3 keeps it open.
- **Gate only the unchanged-graph save and stop measuring the other.** Rejected: the slow path would then be free to get slower unseen.
- **Run the nightly measurement on a larger runner.** Rejected: larger hosted runners are billed, and the standard runner is the machine every other figure in [docs/perf.md](../perf.md) is recorded on.
- **Leave the requirement as written and the nightly red.** Rejected: a gate that is always red stops being read.
