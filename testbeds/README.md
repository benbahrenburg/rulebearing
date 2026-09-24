# Test beds

The repositories in [manifest.yaml](manifest.yaml) are the nightly proof ([NFR-CONF-03](../docs/prd.md#nfr-conf-03), [design § Test beds](../docs/artifacts/design.md#test-beds-open-source-repositories-to-validate-against)). Each is cloned at a pinned commit, read-only, and never modified. Nothing from a test bed is committed except the summary the nightly publishes.

## Roles

| Role | What the nightly does in wave 0 | From wave 1 |
| --- | --- | --- |
| `oracle` | Runs the incumbent tool (dependency-cruiser, NetArchTest, ArchUnitNET or import-linter) with the repository's own configuration and records its output, wall-clock time and peak memory | Rulebearing runs beside it; the findings must agree with zero difference |
| `greenfield` | Records the row as `idle`: there is no incumbent | `rulebearing init` and `propose` fixtures |
| `scale` | Records the row as `idle` | Rulebearing's timing; a regression over 20% fails the night |
| `own` | Records the row as `idle` | The maintainer's own repositories, the first users ([NFR-ADOPT-02](../docs/prd.md#nfr-adopt-02)) |

## Scripts

| Script | Does |
| --- | --- |
| `run.sh <owner/repo> [out]` | Clones one row at its SHA outside this repository (MSBuild, eslint and tsconfig search upward for configuration), runs the incumbent under `/usr/bin/time`, and writes `result.json`, `timing.json` and the tool's output. Status is `ok`, `failed` (the tool reported violations or failing tests), `error` (clone, build or tool failure, with the log named) or `idle` |
| `summarise.mjs <out> [--readme README.md]` | Builds `summary.md` and `summary.json`; with `--readme`, rewrites the table between the `testbeds` markers in the root README |
| `check-regression.sh <previous> <current> [20]` | Fails when a Rulebearing timing grew by more than the threshold against the previous summary; passes with a message while no Rulebearing timing exists |
| `pin.sh [owner/repo ...]` | Re-pins rows to their default branch's head |

The workflow is [`.github/workflows/nightly-testbeds.yml`](../.github/workflows/nightly-testbeds.yml). It gives each row with an incumbent its own runner, records the other rows, assembles the summary, runs the regression check, and publishes the summary to the `testbeds-results` branch. `main` accepts changes only through reviewed pull requests, so the README table is refreshed from that branch by pull request:

```sh
git fetch origin testbeds-results && mkdir -p /tmp/tb && git archive origin/testbeds-results | tar -x -C /tmp/tb
node testbeds/summarise.mjs /tmp/tb/rows --readme README.md
```

## Oracle harness

The .NET and Python oracles are compared rule by rule ([plan 0002, Step 11](../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#211-step-11-the-three-importers-and-oracle-agreement-2f), [§ 1.4.5](../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#145-migration-and-oracle-agreement)): the incumbent's own tests or contracts on one side, the same rules migrated with `rulebearing import` on the other. Each script writes `results/<owner>__<repo>.json`, which is committed, and keeps its intermediate files under `out/<owner>__<repo>/`. Both exit 0 when every compared rule agrees, 1 when one disagrees, and 2 when the row could not be compared (the result file then says why).

| Script | Does |
| --- | --- |
| `oracles/dotnet.sh <owner/repo> [out]` | For a `netarchtest` or `archunitnet` row: `dotnet build` the `test` project with portable PDBs, `dotnet test --logger trx` (with the row's `filter`), `rulebearing import archunit` over the row's `tests` folder (again with `--graph` over a cruise of the assemblies it names, so that types from packages resolve), `rulebearing cruise -T junit` with the imported rules, then joins each TRX result with the JUnit cases of the rules named from its method |
| `oracles/python.sh <owner/repo> [out]` | For an `import-linter` row: `rulebearing import import-linter` in the row's `dir`, import-linter (pinned in the script) through `oracles/lint_imports_json.py` in a throwaway virtual environment with the imported roots on `PYTHONPATH` (and the row's `pip` packages), `rulebearing cruise -T junit` with the imported rules and `-T json` with none, then joins the two per contract and compares grimp's import graph with Rulebearing's local edges |
| `oracles/compare.py python\|dotnet` | The join both scripts end with; its module doc lists the verdicts and the kinds of graph difference |
| `oracles/table.py [--readme README.md \| --check README.md]` | Renders the agreement table in the root README from `results/*.json`; `--check` is the CI job that fails on a stale table |

A .NET architecture test is a test whose class is declared in a file under the row's `tests` folder that uses ArchUnitNET or NetArchTest; other tests in the project are out of scope. A test or contract the importer writes commented out is `stays` (a custom predicate or contract type, which stays with the incumbent) or `not-imported` (with the importer's reason), and never counts as a disagreement. A disagreeing Python contract is re-checked with the imports import-linter removes before following chains (`ignore_imports`, and `TYPE_CHECKING` imports under `exclude_type_checking_imports`) removed from Rulebearing's graph too; when the re-check keeps the contract, the row carries that `cause`, and stays a disagreement, because the imported rules cannot yet express either filter.

Run one row locally with the release binary built, reusing checkouts between runs:

```sh
cargo build --release -p rb-cli
RB_TESTBED_CHECKOUTS=/tmp/rb-testbeds testbeds/oracles/python.sh kedro-org/kedro
RB_TESTBED_CHECKOUTS=/tmp/rb-testbeds testbeds/oracles/dotnet.sh onebeyond/monaco
python3 testbeds/oracles/table.py --readme README.md
```

## Adding or bumping a row

Add one line to `manifest.yaml` with `repo`, `role`, `languages` and `tool`. An oracle also needs `config` (dependency-cruiser) or `solution` and `test` (.NET). Then run `testbeds/pin.sh <owner/repo>` and `testbeds/run.sh <owner/repo>` locally before opening the pull request. Bump a SHA with `pin.sh` in its own pull request, so the nightly figures before and after the bump can be told apart.
