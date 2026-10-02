//! `xtask-gate`: the documentation link check as a compile-time gate.
//!
//! - Decision: [ADR-0058](../../../docs/adr/0058-the-edit-compile-cycle-rebuilds-only-what-changed.md), which places the gate of [ADR-0023](../../../docs/adr/0023-documentation-link-and-lint-gates.md) here
//! - Architecture: [`docs/architecture.md#verification-strategy`](../../../docs/architecture.md#verification-strategy)
//! - Plan: [Wave 0, sub-wave 0A](../../../docs/plans/pending/0000-wave-0-spike.md)
//! - Requirements: [NFR-DOC-01](../../../docs/prd.md#nfr-doc-01)
//!
//! The crate's behaviour is its build script, `build.rs`, which runs
//! [`xtask::doclinks::check`](../../src/doclinks.rs) over the repository and fails the compile on
//! a link that does not resolve. The library exports nothing: cargo runs a build script only for
//! a package with a target, and no crate depends on this one, so re-running the check after an
//! edit rebuilds this crate and nothing else.
