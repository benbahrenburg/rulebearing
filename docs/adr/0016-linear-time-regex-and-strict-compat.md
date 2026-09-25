# ADR-0016: Linear-time regex engine, with `--strict-compat` for portability

- **Status:** Accepted
- **Date:** 2026-09-20
- **Derives from:** [design.md § The rule file](../artifacts/design.md#the-rule-file) (safe-regex), [§ The dependency-cruiser format](../artifacts/design.md#the-dependency-cruiser-format), [§ Language decision](../artifacts/design.md#language-decision) (regex safety)
- **Constrains:** [architecture.md § The rule engine](../architecture.md#the-rule-engine)
- **Implemented by:** [Wave 1 plan](../plans/pending/0001-wave-1-typescript-parity.md)

## Context

dependency-cruiser rejects nested quantifiers through safe-regex, which is why one naming regex in the reference monorepo is looser in the cruiser than in the guard beside it. The Rust `regex` crate is linear-time by construction and has no such restriction.

## Decision

- All pattern matching uses the `regex` crate. A pattern safe-regex would reject is accepted.
- In dependency-cruiser compatibility mode, such a pattern is accepted **with a warning** naming the rule, so a config stays knowingly non-portable.
- `--strict-compat` refuses it (exit 3), for a repository that wants its config to run unchanged back on dependency-cruiser.
- `$0` to `$9` captures from `from.path` are substituted into `to` and `module` patterns **escaped**, so a captured segment can never become a wildcard.
- Regex syntax differences between JavaScript and Rust (`\d` semantics, lookaround, backreferences) are enumerated in `rb-rules` as a compatibility table: lookaround and backreferences are unsupported by the linear engine and are reported as exit 3 with the offending rule, in both modes.

## Consequences

- No catastrophic backtracking is possible, which matters for a tool that runs on every agent turn.
- The compatibility table is a test fixture and is part of the published documentation.

## Alternatives considered

- **`fancy-regex` for lookaround.** Rejected: it reintroduces backtracking. Revisit if an oracle config needs it; none of the found configs does.
