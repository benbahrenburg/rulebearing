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
| `check-regression.sh <summary> [20]` | Fails when a Rulebearing timing grew by more than the threshold against the baseline, the build behind the previous committed summary. `run.sh` times both on the same runner, interleaved, when `RULEBEARING_BASELINE_BIN` is set, because hosted runners differ by more than 20% on the same binary. Passes with a message while no row has a pair |
| `pin.sh [owner/repo ...]` | Re-pins rows to their default branch's head |
| `greenfield.sh <owner/repo> [out]` | The greenfield init proof for one row: clone, the manifest's `build` command, the init fixture regenerated and diffed with the committed one (`fixture.diff`), init's full proposal written and cruised three times. `ok` when the fixture is unchanged and the cruise exits 0, else `failed`; `error` for a clone or build failure. The median cruise is the row's Rulebearing time |
| `scale.sh <owner/repo> [out]` | The scale timing for one row: clone, `build`, `init` for the configuration, three cruises with the JSON reporter; the median is the row's Rulebearing time and the detail counts the modules per language |
| `lib.sh` | What `greenfield.sh` and `scale.sh` share (manifest fields, the clone, the build, the timing, `result.json`); sourced, never run |

The workflow is [`.github/workflows/nightly-testbeds.yml`](../.github/workflows/nightly-testbeds.yml). It gives each row with an incumbent its own runner, and each greenfield row with an init fixture and each built scale row a job of its own, records the other rows, assembles the summary, runs the regression check, and publishes the summary to the `testbeds-results` branch. `main` accepts changes only through reviewed pull requests, so the README table is refreshed from that branch by pull request:

```sh
git fetch origin testbeds-results && mkdir -p /tmp/tb && git archive origin/testbeds-results | tar -x -C /tmp/tb
node testbeds/summarise.mjs /tmp/tb/rows --readme README.md
```

## Adding or bumping a row

Add one line to `manifest.yaml` with `repo`, `role`, `languages` and `tool`. An oracle also needs `config` (dependency-cruiser) or `solution` and `test` (.NET). A greenfield or scale row with .NET projects needs `build`, the command that builds them at the checkout root (Rulebearing reads the built assemblies, [ADR-0011](../docs/adr/0011-read-dotnet-assemblies-not-source.md)). Then run `testbeds/pin.sh <owner/repo>` and `testbeds/run.sh <owner/repo>` locally before opening the pull request. Bump a SHA with `pin.sh` in its own pull request, so the nightly figures before and after the bump can be told apart.
