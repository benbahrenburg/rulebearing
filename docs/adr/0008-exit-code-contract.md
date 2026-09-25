# ADR-0008: Exit codes: error count, 2 for an untrustworthy run, 3 for an invalid config

- **Status:** Accepted
- **Date:** 2026-09-20
- **Derives from:** [design.md § Exit codes](../artifacts/design.md#exit-codes), [§ The run and its consumers](../artifacts/design.md#the-run-and-its-consumers) (toolchain guard), [dependency-cruiser coverage § Extraction and resolution](../artifacts/dependency-cruiser-18.2.0-coverage.md#extraction-and-resolution) (unsupported-transpiler failure mode)
- **Constrains:** [architecture.md § Outputs and CI contract](../architecture.md#outputs-and-ci-contract)
- **Implemented by:** [Wave 1 plan](../plans/pending/0001-wave-1-typescript-parity.md)

## Context

dependency-cruiser exits with the number of error-severity violations, which every gate in the wild depends on. Its one failure mode heavy users guard against separately is the silent empty cruise: zero modules, exit 0, a warning on stderr.

## Decision

| Code | Meaning |
| --- | --- |
| 0 | no error-severity violation |
| 1 to 255 | the number of error-severity violations, capped at 255 |
| 2 | the run cannot be trusted: zero modules found, a solution with no built assemblies, a PDB that is not portable, an unsupported file the sidecar could not handle, or a vacuous rule under the default liveness setting ([ADR-0007](0007-vacuous-rules-fail-by-default.md)) |
| 3 | the config is invalid against the schema, or a predicate names a concept the language lacks |

Codes 2 and 3 are reserved: a run with exactly two or three error violations reports them on stderr and in the report, and the exit code is still the count. The ambiguity is documented; `--exit-code-mode strict` (wave 3) shifts the count to `10 + n` for pipelines that need the distinction.

## Consequences

- The separate toolchain-version guard a heavy user bolts on becomes unnecessary.
- Every reporter and the `fmt` subcommand share one exit-code function in `rb-cli`, tested against a table.

## Alternatives considered

- **Exit 0 with a warning on an empty cruise, as dependency-cruiser does.** Rejected: it is the one failure mode the design sets out to remove.
