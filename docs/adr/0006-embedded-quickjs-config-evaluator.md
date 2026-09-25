# ADR-0006: JavaScript configs are evaluated in a sandboxed embedded QuickJS engine

- **Status:** Accepted
- **Date:** 2026-09-20
- **Derives from:** [design.md § The dependency-cruiser format](../artifacts/design.md#the-dependency-cruiser-format), [§ Rules an agent writes, held to the same bar](../artifacts/design.md#rules-an-agent-writes-held-to-the-same-bar) (hermetic runs)
- **Constrains:** [architecture.md § Configuration and the rule language](../architecture.md#configuration-and-the-rule-language), [§ Security posture](../architecture.md#security-posture)
- **Implemented by:** [Wave 1 plan](../plans/pending/0001-wave-1-typescript-parity.md)

## Context

Most real dependency-cruiser configs are `.cjs` or `.js` files, and the reference monorepo's config `require`s an exceptions JSON and splices it into a regex. A binary that cannot read those files is not a drop-in. Spawning Node for every run reintroduces the toolchain the single binary avoids and makes runs non-hermetic.

## Decision

- `rb-config` embeds QuickJS through the `rquickjs` crate (MIT).
- The engine exposes a CommonJS and ESM shim whose `require` and `import` resolve **only** JSON files, other config modules on disk under the repository, and the bundled `dependency-cruiser/configs/*` presets. There is no filesystem access beyond the repository, no network, no `process`, no timers.
- A config that needs more runs through `--config-via-node`, which asks a local Node to print the evaluated object as JSON, and `--webpack-config-json` accepts a pre-evaluated webpack `resolve` block.
- `plugin:<path>` reporters (wave 3) run in the same engine and receive the cruise result object.

## Consequences

- Runs are hermetic: no network and no code execution outside the sandbox, so a local run and CI agree byte for byte.
- The sandbox is a security boundary and is tested as one: a config that tries to read outside the repository or reach the network is a test case that must fail.

## Alternatives considered

- **Always spawn Node.** Rejected: non-hermetic and requires Node in .NET and Python repositories.
- **Refuse JavaScript configs.** Rejected: excludes most of the oracle repositories.
