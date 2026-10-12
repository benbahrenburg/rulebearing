# fleet fixture

Three sibling repositories and the manifest that joins them, for `rulebearing fleet` and declared edges ([plan 0004, Steps 11 and 12](../../../../../docs/plans/pending/0004-wave-4-reach.md#24-steps-for-sub-wave-4d-fleet-and-declared-edges); [ADR-0077](../../../../../docs/adr/0077-fleet-and-declared-edges.md)). It is this repository's own code, under its MIT licence.

| Path | What it is |
| --- | --- |
| [`fleet.yaml`](fleet.yaml) | the manifest: `web`, `api`, `pipeline`, and three fleet rules: one that fires through a declared edge, one that does not fire, and one from the .NET api to the Python pipeline that matches nothing until an edge between them is declared |
| [`edges.yaml`](edges.yaml) | two declared edges that join modules, one whose `from` is gone (skipped and reported), and an OpenAPI pair |
| [`edges-without-operation-ids.yaml`](edges-without-operation-ids.yaml) | the same client against a server document without any `operationId` |
| `web/` | TypeScript: `src/api/generated/orders.ts` declares `createOrder` and `listOrders`, one function per operation; its own rule fires once (`warn`) |
| `api/` | the .NET api's configuration and OpenAPI documents. The test copies the compiled [`init-layers`](../init-layers/PROVENANCE.md) solution here, each assembly where `dotnet build` puts it. Its portable PDBs record the sources under `crates/rb-cli/tests/fixtures/init-layers/`, so the api's module paths, and the handlers `x-rulebearing-handler` names, carry that prefix |
| `pipeline/` | Python: a worker and its jobs |
| [`expected/fleet.err-long.txt`](expected/fleet.err-long.txt) | the `err-long` report, byte for byte |

The test ([`tests/fleet.rs`](../../fleet.rs)) copies the fixture to a scratch folder and makes each repository a git repository with one commit, so each one's cache works as it would in a clone.
