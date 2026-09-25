# ADR-0028: Backreferences are matched by instantiation on the linear-time engine

- **Status:** Accepted
- **Date:** 2026-09-22
- **Derives from:** [ADR-0016](0016-linear-time-regex-and-strict-compat.md) ("Revisit if an oracle config needs it"), [design § Test beds](../artifacts/design.md#test-beds-open-source-repositories-to-validate-against), [design § Waves](../artifacts/design.md#waves) (wave 1 exit: zero-diff on langfuse)
- **Constrains:** `crates/rb-config/src/pattern.rs`, the compatibility table
- **Supersedes:** the backreference clause of [ADR-0016](0016-linear-time-regex-and-strict-compat.md); its lookaround clause stands
- **Implemented by:** [Wave 1 plan](../plans/pending/0001-wave-1-typescript-parity.md), Step 3
- **Requirements:** [FR-RULE-10](../prd.md#fr-rule-10), [NFR-CONF-01](../prd.md#nfr-conf-01)

## Context

[ADR-0016](0016-linear-time-regex-and-strict-compat.md) refused lookaround and backreferences, because the `regex` crate has neither, and said to revisit if an oracle configuration needed one. Loading the ten dependency-cruiser configurations of the test-bed manifest at their pinned commits found exactly one such pattern, in `langfuse/langfuse` (`web/.dependency-cruiser.js`, rule `rfc07-no-component-internals`, `to.pathNot`):

```text
^$1/|(^|/)([A-Z][A-Za-z0-9]*)/(\2|index)\.(ts|tsx)$
```

It says "a component folder's own file, or its index". No configuration uses lookaround. langfuse is one of the three repositories the wave 1 exit criterion names for zero difference, so refusing the pattern would fail the wave.

## Decision

- A pattern with backreferences compiles to two parts, both on the `regex` crate:
  1. an **over-approximation**, with each `\k` replaced by a non-capturing copy of group k's own pattern. It is linear-time; when it does not match, the pattern does not match.
  2. a **verifier**, used only when the over-approximation matches: for each referenced group, every substring of the input that the group's pattern matches whole is a candidate; the pattern is instantiated with the candidate as a literal in the group and at each reference, and the instantiation is matched. The pattern matches when some instantiation does.
- No backtracking engine is added and `fancy-regex` stays out. The work per input is bounded by the number of candidate substrings (at most n(n+1)/2 for an input of n characters, and in practice the handful the group's pattern accepts), and at most 4,096 candidate combinations are tried; each instantiation is compiled once and cached.
- A reference to a group that does not exist, or one inside the group it names, is refused (exit 3) as before. Groups nested inside a referenced group keep their numbers but capture the empty string in an instantiation.
- Lookaround remains refused, as [ADR-0016](0016-linear-time-regex-and-strict-compat.md) decided.

## Consequences

- langfuse's configuration loads unchanged, and the zero-diff run can compare it.
- The compatibility table in `pattern.rs` lists backreferences as supported, with this ADR as the reason.
- A pathological pattern with many referenced groups over a long input is bounded by the candidate limit rather than by time; past the limit the verifier reports no match, which is deterministic.

## Alternatives considered

- **`fancy-regex`.** Rejected, as in ADR-0016: it reintroduces backtracking for every pattern it compiles, and CLAUDE.md forbids it.
- **A hand-written backtracking matcher for backreference patterns only.** Rejected: it needs its own JavaScript regex parser and a step budget, and a budget overrun would make a match depend on input length in a way that is hard to explain in a finding.
- **Keep refusing, and list langfuse in `conformance/divergences.md`.** Rejected: the design permits a documented divergence for a behaviour difference, not for a configuration the tool cannot load.
