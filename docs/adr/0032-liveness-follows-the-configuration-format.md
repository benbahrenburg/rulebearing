# ADR-0032: Liveness follows the configuration's format, with named exceptions in the native file

- **Status:** Accepted
- **Date:** 2026-09-23
- **Derives from:** [design § Why](../artifacts/design.md#why) ("superset, precisely; nothing is dropped"), [ADR-0007](0007-vacuous-rules-fail-by-default.md), [ADR-0030](0030-the-reporter-decides-the-error-count-exit.md) (a drop-in exits as dependency-cruiser does), the private monorepo measurement in [docs/perf.md](../perf.md#the-private-monorepo)
- **Constrains:** `crates/rb-config` (`allowEmpty` at the top level of a native file), `crates/rb-model` (`summary.vacuousRules[].severity`), `crates/rb-cli` (`--liveness`, `cruise`, `fmt --exit-code`, `adopt`)
- **Supersedes:** the clause of [ADR-0007](0007-vacuous-rules-fail-by-default.md) that keeps the check failing "in dependency-cruiser compatibility mode". Its default for native files, its `allowEmpty: true` on a rule, `--no-liveness`, and `summary.vacuousRules` stand.
- **Implemented by:** [Wave 1 plan](../plans/pending/0001-wave-1-typescript-parity.md), Steps 7 and 17
- **Requirements:** [FR-CORE-05](../prd.md#fr-core-05), [NFR-CONF-01](../prd.md#nfr-conf-01)

## Context

[ADR-0007](0007-vacuous-rules-fail-by-default.md) fails a run when a rule's selecting side matches no module, in every mode. On a real dependency-cruiser repository that makes Rulebearing exit 2 where dependency-cruiser exits 0: the private monorepo of [docs/perf.md](../perf.md#the-private-monorepo) has one such rule. The way out ADR-0007 gives, `allowEmpty: true` on the rule, cannot be written in a `.dependency-cruiser.*` file. dependency-cruiser's configuration schema rejects any key it does not define on a rule, so the file would stop working with dependency-cruiser. What is left is `--no-liveness`, which turns the check off for every rule, the opposite of what the check is for.

A drop-in that fails where dependency-cruiser passes is not a drop-in, and [ADR-0030](0030-the-reporter-decides-the-error-count-exit.md) already settled the same question for the error count.

## Decision

- **Three modes.** `--liveness strict` fails the run with exit 2 (ADR-0007's behaviour). `--liveness warn` reports each vacuous rule in `summary.vacuousRules[]`, with `"severity": "warn"`, and as a `warning:` on stderr, without changing the exit code. `--liveness off` does not check. `--no-liveness` stays as the spelling of `off`, and cannot be combined with `--liveness`.
- **The default follows the root file.** A `.dependency-cruiser.*` configuration runs under `warn`, so it exits as dependency-cruiser exits. A `rulebearing.*` configuration runs under `strict`, including one that `extends` a dependency-cruiser file, which is what `adopt` writes. A repository opts into the check by adopting.
- **Named exceptions.** A native file may carry `allowEmpty: [names]` at the top level: rule names, ratchet names, and `allowed[N]` for the Nth `allowed` entry. Each named rule is exempt, as if it carried `allowEmpty: true`. The list merges through `extends`. A name that is no rule or ratchet of the configuration is a configuration error (exit 3), so an exception cannot outlive its rule. In a dependency-cruiser file the key is a native addition: a warning, and an error under `--strict-compat`, like the others ([ADR-0005](0005-native-config-superset-and-compat.md)).
- **`adopt`** lists the rules that match nothing today under `allowEmpty` in the `rulebearing.yaml` it writes, and names them in its output and in the pull request, instead of refusing to adopt.
- **Saved results.** `fmt --exit-code` counts only the `vacuousRules` entries without `"severity": "warn"`, so a saved result gates as its cruise did ([ADR-0031](0031-a-saved-result-carries-what-the-exit-code-counts.md)). `severity` is additive and absent under `strict`.

## Consequences

- A dependency-cruiser configuration runs under Rulebearing with dependency-cruiser's exit code, and a stale rule is still named on every run.
- A repository that adopts gets the check on every rule, and the exceptions are a reviewed list in one file rather than a global switch.
- Someone who uses Rulebearing only as a faster dependency-cruiser is warned about a dead rule, not stopped by it, which is what dependency-cruiser gives them today. `--liveness strict` restores the failure without adopting.
- Conformance gate 1 still passes `--no-liveness`, because dependency-cruiser's specs cover rules that match nothing on purpose.

## Alternatives considered

- **Keep ADR-0007 in every mode.** Rejected: it keeps dependency-cruiser users on `--no-liveness`, which disables the check for every rule.
- **Warn in every mode.** Rejected: ADR-0007's reason stands for a repository that has chosen Rulebearing. Four rules matched nothing for months while warnings went unread.
- **Accept `allowEmpty` on rules in a dependency-cruiser file.** Rejected: the file would then fail dependency-cruiser's own validation, and a repository that runs both tools could not use it.
