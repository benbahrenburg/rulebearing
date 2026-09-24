# ADR-0033: One crate-scoped licence exception, Apache-2.0 WITH LLVM-exception for `ar_archive_writer`, which the Python parser's build requires

- **Status:** Proposed
- **Date:** 2026-09-24
- **Derives from:** [ADR-0019](0019-mit-licence.md) (licence policy), [ADR-0013](0013-ruff-parser-for-python.md) (`ruff_python_parser` for Python), [ADR-0025](0025-ci-and-supply-chain-hardening.md) (`cargo deny` gates the supply chain), [ADR-0026](0026-bsl-exception-for-dragonbox.md) (the precedent for a crate-scoped exception)
- **Constrains:** `deny.toml`
- **Implemented by:** [Wave 2 plan](../plans/pending/0002-wave-2-dotnet-python-element-rules.md), [sub-wave 2B](../plans/pending/0002-wave-2-dotnet-python-element-rules.md#wave-2b-rb-extract-python)
- **Requirements:** [FR-EXT-PY-01](../prd.md#fr-ext-py-01)

## Context

`ruff_python_parser` 0.0.14, pinned by the Python extractor under [ADR-0013](0013-ruff-parser-for-python.md), depends unconditionally on `stacker` (it grows the stack for deeply nested source), which depends on `psm` (MIT OR Apache-2.0). `psm`'s build script uses `ar_archive_writer` 0.5, a build-time dependency published by the Rust project, whose licence is `Apache-2.0 WITH LLVM-exception`. The `deny.toml` allow-list from [ADR-0019](0019-mit-licence.md) names Apache-2.0 but not the LLVM exception, so the `deny` job refuses the build.

The LLVM exception only adds permissions to Apache-2.0: it waives the attribution requirements for object code that embeds parts of the licensed work. Apache-2.0 is already allowed, so the exception licence is at least as permissive as one the project accepts. The crate runs at build time to assemble an archive; it is not a parser and reads no untrusted input.

## Decision

- `deny.toml` gains one exception, scoped to the crate: `ar_archive_writer` may use `Apache-2.0 WITH LLVM-exception`. The global allow-list does not change.
- The exception follows the pinned `ruff_python_parser` version. When a bump drops the dependency, the exception is removed in the same pull request; `cargo deny` reports an unused exception so it cannot linger.

## Consequences

- The third-party notices list one Apache-2.0-with-LLVM-exception build-time component.
- Any further non-listed licence still fails the `deny` job and needs its own ADR.

## Alternatives considered

- **Add the expression to the global allow-list.** Rejected, as in [ADR-0026](0026-bsl-exception-for-dragonbox.md): it would admit the licence for every future dependency without review.
- **Patch `ruff_python_parser` to drop `stacker`.** Rejected: a patched or git dependency is an ADR-level change under [ADR-0025](0025-ci-and-supply-chain-hardening.md), and removing the stack guard would let deeply nested input overflow the stack, which [architecture § Security posture](../architecture.md#security-posture) forbids.
- **Another Python parser.** Rejected: [ADR-0013](0013-ruff-parser-for-python.md) chose ruff's parser; reversing it for a build-time licence with strictly more permissions than Apache-2.0 would not be proportionate.
