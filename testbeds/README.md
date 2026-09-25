# Test beds

The repositories in [manifest.yaml](manifest.yaml) are the nightly proof ([NFR-CONF-03](../docs/prd.md#nfr-conf-03), [design § Test beds](../docs/artifacts/design.md#test-beds-open-source-repositories-to-validate-against)). Each is cloned at a pinned commit, read-only, and never modified. Nothing from a test bed is committed except the summary the nightly publishes.

## Roles

| Role | What the nightly does in wave 0 | From wave 1 |
| --- | --- | --- |
| `oracle` | Runs the incumbent tool (dependency-cruiser, NetArchTest, ArchUnitNET or import-linter) with the repository's own configuration and records its output, wall-clock time and peak memory | Rulebearing runs beside it; the findings must agree with zero difference |
| `greenfield` | Records the row as `idle`: there is no incumbent | `rulebearing init` fixtures. From wave 2, semantic-kernel and autogen have a nightly job of their own ([`greenfield.sh`](greenfield.sh)): built, the fixture regenerated and compared, init's proposal cruised back; exit 0 is `ok`, and a failure fails the night ([init/README.md](init/README.md)) |
| `scale` | Records the row as `idle` | Rulebearing's timing; a regression over 20% fails the night. From wave 2, aspnetcore, jellyfin and home-assistant have a job each ([`scale.sh`](scale.sh)): built, configured by `init`, the median of three cruises. n8n, grafana and kibana stay `idle` ([plan 0001 § 3](../docs/plans/pending/0001-wave-1-typescript-parity.md#3-wave-based-delivery-plan) cuts them) |
| `own` | Records the row as `idle` | The maintainer's own repositories, the first users ([NFR-ADOPT-02](../docs/prd.md#nfr-adopt-02)) |

## Scripts

| Script | Does |
| --- | --- |
| `run.sh <owner/repo> [out]` | Clones one row at its SHA outside this repository (MSBuild, eslint and tsconfig search upward for configuration), runs the incumbent under `/usr/bin/time`, and writes `result.json`, `timing.json` and the tool's output. Status is `ok`, `failed` (the tool reported violations or failing tests), `error` (clone, build or tool failure, with the log named) or `idle` |
| `summarise.mjs <out> [--readme README.md]` | Builds `summary.md` and `summary.json`; with `--readme`, rewrites the table between the `testbeds` markers in the root README |
| `check-regression.sh <previous> <current> [20]` | Fails when a Rulebearing timing grew by more than the threshold against the previous summary; passes with a message while no Rulebearing timing exists |
| `pin.sh [owner/repo ...]` | Re-pins rows to their default branch's head |
| `greenfield.sh <owner/repo> [out]` | The greenfield init proof for one row: clone, the manifest's `build` command, the init fixture regenerated and diffed with the committed one (`fixture.diff`), init's full proposal written and cruised three times. `ok` when the fixture is unchanged and the cruise exits 0, else `failed`; `error` for a clone or build failure. The median cruise is the row's Rulebearing time |
| `scale.sh <owner/repo> [out]` | The scale timing for one row: clone, `build`, `init` for the configuration, three cruises with the JSON reporter; the median is the row's Rulebearing time and the detail counts the modules per language |
| `lib.sh` | What `greenfield.sh` and `scale.sh` share (manifest fields, the clone, the build, the timing, `result.json`); sourced, never run |

The workflow is [`.github/workflows/nightly-testbeds.yml`](../.github/workflows/nightly-testbeds.yml). It gives each row with an incumbent its own runner, and each greenfield row with an init fixture and each built scale row a job of its own, records the other rows, assembles the summary, runs the regression check, and publishes the summary to the `testbeds-results` branch. `main` accepts changes only through reviewed pull requests, so the README table is refreshed from that branch by pull request:

```sh
git fetch origin testbeds-results && mkdir -p /tmp/tb && git archive origin/testbeds-results | tar -x -C /tmp/tb
node testbeds/summarise.mjs /tmp/tb/rows --readme README.md
```

## Oracle harness

The .NET and Python oracles are compared rule by rule ([plan 0002, Step 11](../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#211-step-11-the-three-importers-and-oracle-agreement-2f), [§ 1.4.5](../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#145-migration-and-oracle-agreement)): the incumbent's own tests or contracts on one side, the same rules migrated with `rulebearing import` on the other. Each script writes `results/<owner>__<repo>.json`, which is committed, and keeps its intermediate files under `out/<owner>__<repo>/`. Both exit 0 when at least one rule was compared and none disagrees or errors, 1 when one disagrees or Rulebearing reports an `<error>` for it (a vacuous or expired rule, which has no verdict to compare), 2 when the row could not be compared (the result file then says why), and 3 when nothing was compared: every test or contract stays with the incumbent or is not imported. A row of that kind has the status `nothing-compared` and `agrees: false`, and the table says "nothing compared"; it is never counted as agreement.

The nightly ([nightly-testbeds.yml](../.github/workflows/nightly-testbeds.yml)) fails an oracle's job on exit 1 and on any code outside the four. Exit 2 and 3 are reported, not failing: each writes a warning annotation, and the committed result file and the table show the row as an error or as nothing compared, so neither reads as agreement. They do not fail the job because both describe the oracle repository (a test project that does not build, rules that all stay with the incumbent), not Rulebearing.

| Script | Does |
| --- | --- |
| `oracles/dotnet.sh <owner/repo> [out]` | For a `netarchtest` or `archunitnet` row: `dotnet build` the `test` project with portable PDBs, `dotnet test --logger trx` (with the row's `filter`), `rulebearing import archunit` over the row's `tests` folder (again with `--graph` over a cruise of the assemblies it names, so that types from packages resolve), `rulebearing cruise -T junit` with the imported rules, then joins each TRX result with the JUnit cases of the rules named from its method |
| `oracles/python.sh <owner/repo> [out]` | For an `import-linter` row: `rulebearing import import-linter` in the row's `dir`, import-linter (pinned in the script) through `oracles/lint_imports_json.py` in a throwaway virtual environment with the imported roots on `PYTHONPATH` (and the row's `pip` packages), `rulebearing cruise -T junit` with the imported rules and `-T json` with none, then joins the two per contract and compares grimp's import graph with Rulebearing's local edges |
| `oracles/compare.py python\|dotnet` | The join both scripts end with; its module doc lists the verdicts and the kinds of graph difference |
| `oracles/table.py [--readme README.md \| --check README.md]` | Renders the agreement table in the root README from `results/*.json`; `--check` is the CI job that fails on a stale table |

A .NET architecture test is a test whose class is declared in a file under the row's `tests` folder that uses ArchUnitNET or NetArchTest; other tests in the project are out of scope. A test or contract the importer writes commented out is `stays` (a custom predicate or contract type, which stays with the incumbent) or `not-imported` (with the importer's reason), and never counts as a disagreement. The imported rules carry the graph import-linter checks a contract over as each rule's `graph` ([ADR-0038](../docs/adr/0038-a-rule-narrows-the-graph-it-sees.md)): `ignore_imports`, `TYPE_CHECKING` imports under `exclude_type_checking_imports`, folders without `__init__.py` below a root package, and chains only through the root packages. A Python contract that still disagrees is re-checked with those imports removed from Rulebearing's graph by the harness; when the re-check keeps the contract, the row carries that `cause`, which points at the translation of a filter, and stays a disagreement.

Run one row locally with the release binary built, reusing checkouts between runs:

```sh
cargo build --release -p rb-cli
RB_TESTBED_CHECKOUTS=/tmp/rb-testbeds testbeds/oracles/python.sh kedro-org/kedro
RB_TESTBED_CHECKOUTS=/tmp/rb-testbeds testbeds/oracles/dotnet.sh onebeyond/monaco
python3 testbeds/oracles/table.py --readme README.md
```

## Adding or bumping a row

Add one line to `manifest.yaml` with `repo`, `role`, `languages` and `tool`. An oracle also needs `config` (dependency-cruiser) or `solution` and `test` (.NET). A greenfield or scale row with .NET projects needs `build`, the command that builds them at the checkout root (Rulebearing reads the built assemblies, [ADR-0011](../docs/adr/0011-read-dotnet-assemblies-not-source.md)). Then run `testbeds/pin.sh <owner/repo>` and `testbeds/run.sh <owner/repo>` locally before opening the pull request. Bump a SHA with `pin.sh` in its own pull request, so the nightly figures before and after the bump can be told apart.
