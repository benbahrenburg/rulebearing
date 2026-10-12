# ADR-0075: The coverage tabs' rows are kept in a tracked ledger with their evidence, and each row's status is computed from the gates' results

- **Status:** Proposed (2026-10-11)
- **Date:** 2026-10-11
- **Derives from:** [plan 0004, Step 4](../plans/pending/0004-wave-4-reach.md#21-steps-for-sub-wave-4a-docs-site-playground-cookbook-live-tables) ("`testbeds/scripts/coverage_status.py` reads the conformance results (`target/conformance/*.json` from gate 1 and gate 2) and the two coverage tabs, and writes `site/data/coverage.json` with a status and an evidence link per row ... a row whose evidence is missing renders as unproven regardless of the tab's text. The nightly publishes the file and the scale table alongside"), its [§ 1.5](../plans/pending/0004-wave-4-reach.md#15-architecture-of-what-this-wave-builds) ("Live coverage tables means the Parity status column is written by the nightly from the conformance results rather than by hand"); [ADR-0009](0009-conformance-suites-as-specification.md); [ADR-0039](0039-plans-and-design-kept-local.md); [ADR-0072](0072-the-docs-site-builds-from-docs-and-serves-the-released-schemas.md)
- **Amends:** [ADR-0039](0039-plans-and-design-kept-local.md), narrowly: the rows of the two coverage tabs (their cells, as exported) are tracked and published, in `conformance/ledger/`. The design, the plans and the tabs' files stay local, as ADR-0039 decides.
- **Constrains:** `conformance/ledger/`, `testbeds/scripts/coverage_status.py`, `site/data/coverage.json`, `site/src/tables.ts`, the results writers of both gates (`crates/rb-extract-ts/tests/extract_fixtures.rs`, `conformance/dependency-cruiser/harness/run-layer-2.mjs`, `run-layer-3.mjs`, `results.mjs`, `scripts/run-layer-5.sh`, `crates/rb-rules/tests/gate2_common/mod.rs`), `.github/workflows/nightly-testbeds.yml`, `.github/workflows/docs.yml`
- **Requirements:** [NFR-CONF-03](../prd.md#nfr-conf-03), [FR-REACH-01](../prd.md#fr-reach-01)

## Context

ADR-0009 makes the two coverage tabs the ledger the conformance gates report against: a row may not say Parity until the pinned suite says so. Until now the proof of each row was recorded by hand in the plans, row by row with the test that proves it (plan 0002's ledger of 91 rows, plan 0003's of 13). Plan 0004 asks for that to be live: the nightly writes each row's status from the gates' results, and the docs site renders the tables, so a row whose evidence is missing shows as unproven whatever the tab says.

Four facts shape the design:

- **The tabs are not in CI.** `docs/artifacts/` is git-ignored (ADR-0039), so neither the nightly nor the docs build can read them. The rows must come from somewhere tracked.
- **The tabs are dated exports, never edited.** A row's text is the tab's; only its proof changes. A copy of the rows must be derived from the tab, not retyped, and must notice when the tab is re-exported.
- **The gates said pass or fail, not which row.** Gate 1 printed counts and wrote `layer1.md`; layers 2 and 3 printed a tally; gate 2 asserted one test per suite. Nothing machine-readable said which spec, which case file or which test passed.
- **Not every row has an upstream case.** A Parity+ row's .NET and Python halves, Rulebearing's additions (`sarif`, the `code` section, solution loading), and options upstream tests only through a transpiler are proven by Rulebearing's own tests, as plans 0002 and 0003 recorded. The table has to say which proof is upstream's and which is ours.

## Decision

**A tracked ledger per tab.** `conformance/ledger/dependency-cruiser-18.2.0.json` and `conformance/ledger/archunitnet-0.13.4.json` hold, per `##` section of the tab, its column headers and each row's cells exactly as exported, the row's claim (the cell under the tab's status column: `Rulebearing` in the dependency-cruiser tab, `Status` in the ArchUnitNET tab; ArchUnitNET's selector table has no status column, and its lead defines every listed method as Parity, so a method row's claim is `Parity` and the `(none)` row's is `Addition`), the sha256 of the tab's text, and the row's evidence. The ledger has no status field.

| Command | What it does |
| --- | --- |
| `coverage_status.py rows` | Derives every row from the local tabs and keeps each row's evidence, keyed by section and first cell. `--check` exits 1 when a ledger differs from its tab. Run on the maintainer's machine, where the tabs are |
| `coverage_status.py check` | Every evidence reference names something the tree holds: a known results name, a ported case file, a crate, a test target and a function of that name. The pytest suite runs it over the committed ledgers on every pull request |
| `coverage_status.py tests` | Runs each Rust test target the ledger names, with every feature, and records each test's verdict from libtest's own lines (`test <path> ... ok`) |
| `coverage_status.py jobs` | Records a CI run's job conclusions from `gh run view --json jobs` |
| `coverage_status.py write` | Computes each row's status and writes `site/data/coverage.json` |

**One results shape.** Every producer writes `target/conformance/<name>.json` as `{ results, ok, passed, total, items: { <item>: "pass" | "fail" } }`, keys sorted:

| Results | Written by | Items |
| --- | --- | --- |
| `layer1` | `extract_fixtures.rs` | each replayed `test/extract` case |
| `layer2` | `run-layer-2.mjs` (a full run in gate mode) | each `test/validate` and `test/graph-utl` spec |
| `layer3` | `run-layer-3.mjs` (a full run) | each `test/report` spec, and `oracle:<output type>` for each reporter compared with upstream's (compared at least once, never differing) |
| `layer5` | read from `run-layer-5.sh`'s folders | each oracle repository whose `zeroDiff` is `yes`; `mutations:mutations` and `mutations:affected`, written only when both comparisons held |
| `gate2-archunitnet`, `gate2-netarchtest` | `gate2_common::report` | each `ported/<file>.yaml` with at least one case and none differing |
| `tests` | `coverage_status.py tests` | `<crate>/<target>::<test path>`, and `<crate>/<target>` when the whole target passed |
| `jobs` | `coverage_status.py jobs` | each job of the run, with the run's id |
| `unported-archunitnet`, `unported-netarchtest` | read from the tracked `unported.json` | each reason with an entry (the proof of a row that stays upstream) |

**The status rule.** A row is `proven` when it names evidence and every piece holds: the results file exists and records the item as `pass`, or, for a whole results file, records every item `pass` and at least one. Anything else is `unproven`, a missing file included. The site shows the tab's claim only for a proven row; an unproven row shows "unproven", with the tab's text beside it for reference. Each piece of evidence links to its source (the upstream spec at the pinned tag, the case file, the test file) and, when it holds, to the run that proved it.

**The nightly writes it, by pull request.** A `conformance` job in `nightly-testbeds.yml` runs gate 1's layers 1 to 4 (`run.sh`), gate 2, and `coverage_status.py tests`, and uploads `target/conformance/*.json`. The summary job adds layer 5's folders (the existing `layer5` artifact) and the job conclusions of the latest `ci.yml` run on `main`, writes `site/data/coverage.json` with the night's run id, and opens or updates a pull request from `testbeds/coverage-tables`, as the scale table's step does from `testbeds/scale-table`. `main` takes the file only through review, so a status can change only through a reviewed diff of a file the script wrote.

**The site renders the data.** `site/src/tables.ts` renders `coverage/index.html`, `coverage/dependency-cruiser.html`, `coverage/archunitnet.html` and `coverage/scale.html` from `site/data/coverage.json` and `testbeds/results/scale.json` at build time; `docs.yml` rebuilds when either changes.

**Evidence beyond the upstream suites is labelled.** A piece of evidence from gate 1 or gate 2 is shown as the upstream spec or case file; a Rust test is shown as `<crate>/<target>: <test>`. A row proven only by Rulebearing's tests is still proven, as plans 0002 and 0003 accepted, and the reader can see that it is.

## Consequences

- A row's status can no longer be written by hand anywhere: the ledger has no status, and `coverage.json` is the script's output. Choosing a row's evidence is still a judgement, made in the ledger and reviewed in its diff, and `check` rejects evidence that names nothing.
- The rows of both coverage tabs become public, in the ledger and on the site. That is the purpose of plan 0004's live tables. The design document, the plans and the tabs' own files stay local.
- A re-export of a tab is noticed: `rows --check` fails on the maintainer's machine, and `rows` rewrites the cells while keeping each row's evidence. A row whose first cell changed loses its evidence and shows as unproven until evidence is given again.
- `site/data/coverage.json` is only as fresh as the last merged nightly pull request. The page names the run it came from.
- The nightly runs gate 1 and gate 2 once more each night, and builds the test targets the ledger names, about 30 minutes of runner time.
- A row proven by a CI job (the `rb-node` and `.NET` adapter rows) depends on the latest `ci.yml` run on `main`; a failed job there makes the row unproven until it passes.
- The PlantUML generation row of the ArchUnitNET tab lost its status cell in the export (a code span with pipes split the cell); its claim reads as the fragment the export kept. It is shown as exported, never corrected here (ADR-0001: a change of intent is a new export).

## Alternatives considered

| Option | Why not |
| --- | --- |
| Read the tabs in CI | They are git-ignored (ADR-0039), so CI and the docs build have no copy. |
| Keep the row list inside the plans | Also git-ignored, and the plans record a row's proof once, at a date; they cannot be recomputed. |
| A status column in the ledger, set by the nightly | It would be a second place a status can be typed. Computing it at write time leaves nothing to edit. |
| Parse the gates' console output | Brittle against wording changes, and the gates did not print per-spec verdicts. A results file per gate costs a few lines each. |
| libtest's JSON output (`--format json`) | Unstable: it needs `-Z unstable-options`, which stable Rust rejects. libtest's pretty lines are stable and enough. |
| Download the gates' results from the last `ci.yml` run on `main` | The night would depend on the last push having passed every job, and CI's `test` job records no per-test verdict. Rerunning the gates gives one run id for everything they prove. CI-job evidence is used only for the two rows a nightly runner cannot build (the Node addon on three platforms, the .NET adapters). |
| Accept only upstream evidence | About a third of the rows have no upstream case (additions, the .NET and Python halves, options upstream tests only through a transpiler). They would show as unproven forever, which says nothing true about them. |
