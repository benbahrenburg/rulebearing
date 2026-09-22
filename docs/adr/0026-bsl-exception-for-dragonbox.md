# ADR-0026: One crate-scoped licence exception, BSL-1.0 for `dragonbox_ecma`, which oxc requires

- **Status:** Accepted
- **Date:** 2026-09-22
- **Derives from:** [ADR-0019](0019-mit-licence.md) (licence policy), [ADR-0012](0012-oxc-for-typescript.md) (oxc for TypeScript), [ADR-0025](0025-ci-and-supply-chain-hardening.md) (`cargo deny` gates the supply chain)
- **Constrains:** `deny.toml`
- **Implemented by:** [Wave 0 plan](../plans/pending/0000-wave-0-spike.md), sub-wave 0C
- **Requirements:** [NFR-COMPAT-01](../prd.md#nfr-compat-01)

## Context

`oxc_parser` 0.151.0, pinned by [ADR-0012](0012-oxc-for-typescript.md), depends through `oxc_ecmascript` on `dragonbox_ecma`, which formats floating-point numbers the way ECMAScript does. Its licence is `Apache-2.0 WITH LLVM-exception OR BSL-1.0`. The `deny.toml` allow-list from [ADR-0019](0019-mit-licence.md) names neither the LLVM exception nor BSL-1.0, so the `deny` job refuses the build.

BSL-1.0 (the Boost Software License) is a short permissive licence: it permits use, modification and redistribution in binary form without attribution in the binary, and it is compatible with shipping an MIT-licensed binary. Choosing it from the `OR` satisfies the crate's terms.

## Decision

- `deny.toml` gains one exception, scoped to the crate: `dragonbox_ecma` may use `BSL-1.0`. The global allow-list does not change, so no other crate can bring BSL-1.0 in without a new decision.
- The exception follows the pinned oxc version. When an oxc bump drops the crate, the exception is removed in the same pull request; `cargo deny` reports an unused exception so it cannot linger unnoticed.

## Consequences

- The binary's third-party notices include one BSL-1.0 component.
- Any further non-listed licence still fails the `deny` job and needs its own ADR.

## Alternatives considered

- **Add BSL-1.0 to the global allow-list.** Rejected: it would admit the licence for every future dependency without review.
- **Pin an older oxc without the dependency.** Rejected: oxc 0.75, the newest release before this dependency that also ran on the previous toolchain, predates TypeScript syntax the fixtures exercise, and [ADR-0012](0012-oxc-for-typescript.md) chose oxc to track the language.
- **Vendor a patched oxc.** Rejected: a git or patched dependency is itself an ADR-level change under [ADR-0025](0025-ci-and-supply-chain-hardening.md), and it would put the project on the hook for a parser fork.
