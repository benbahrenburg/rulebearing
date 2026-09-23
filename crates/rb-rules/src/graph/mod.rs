//! Graph utilities: dependency-cruiser 18.2.0's `src/graph-utl`, ported.
//!
//! - Plan: [Wave 1, Step 6](../../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-6-graph-analysis-1b)
//! - Specification: `test/graph-utl/*.spec.mjs`, run by conformance gate 1 layer 2
//!   ([ADR-0009](../../../../docs/adr/0009-conformance-suites-as-specification.md))

pub mod consolidate;
pub mod filters;
pub mod indexed;
