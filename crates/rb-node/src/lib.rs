//! `rb-node`: the napi-rs binding exposing `cruise()` and `format()` with dependency-cruiser's
//! signatures, so scripts that call dependency-cruiser as a library today keep working.
//!
//! - Architecture: [`docs/architecture.md#distribution`](../../../docs/architecture.md#distribution)
//! - Decision: [ADR-0002](../../../docs/adr/0002-rust-as-implementation-language.md)
//! - Plan: [Wave 3, sub-wave 3F](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md)
//! - Requirement: [FR-DIST-02](../../../docs/prd.md#fr-dist-02)
//! - Specification: [coverage § Programmatic API](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#programmatic-api)
//!
//! Wave 0 reserves the crate. The `napi` dependency and the `rb-cli` library target it calls
//! into are added by the wave 3 plan; until then it depends on `rb-model` only.

/// The exported function names, in the order dependency-cruiser documents them.
pub const EXPORTS: &[&str] = &[
    "cruise",
    "format",
    "extractDepcruiseConfig",
    "extractTSConfig",
    "extractWebpackResolveConfig",
    "extractBabelConfig",
    "getAvailableTranspilers",
    "allExtensions",
];

#[cfg(test)]
mod tests {
    #[test]
    fn exports_match_the_coverage_tab() {
        assert_eq!(super::EXPORTS.len(), 8);
        assert_eq!(super::EXPORTS[0], "cruise");
    }
}
