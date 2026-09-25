# ADR-0019: MIT licence

- **Status:** Accepted
- **Date:** 2026-09-20
- **Derives from:** [design.md § Why](../artifacts/design.md#why), [§ Open questions](../artifacts/design.md#open-questions) (Licence)
- **Constrains:** `LICENSE`, `conformance/`
- **Implemented by:** [Wave 0 plan](../plans/pending/0000-wave-0-spike.md)

## Decision

Rulebearing is MIT-licensed, matching dependency-cruiser and NetArchTest, so the conformance harness can vendor their fixtures without a notice problem. ArchUnitNET's Apache 2.0 fixtures carry their `NOTICE` file under `conformance/archunitnet/`. Every dependency is checked by `cargo deny` against an allow-list of MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, Zlib and Unicode-3.0; a GPL crate (such as `dotnetdll`) may not be added.

## Consequences

- `cargo deny check licenses` is a CI check from wave 0.
- The C# fallback extractor, if invoked, is MIT too.
