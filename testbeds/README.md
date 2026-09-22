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

## Adding or bumping a row

Add one line to `manifest.yaml` with `repo`, `role`, `languages` and `tool`. An oracle also needs `config` (dependency-cruiser) or `solution` and `test` (.NET). Then run `testbeds/pin.sh <owner/repo>` and `testbeds/run.sh <owner/repo>` locally before opening the pull request. Bump a SHA with `pin.sh` in its own pull request, so the nightly figures before and after the bump can be told apart.
