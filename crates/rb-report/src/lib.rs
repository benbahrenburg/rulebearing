//! `rb-report`: every dependency-cruiser reporter plus `sarif`, `github-annotations`, `junit`,
//! `trx`, `agent` and `plantuml`.
//!
//! - Architecture: [`docs/architecture.md#outputs-and-ci-contract`](../../../docs/architecture.md#outputs-and-ci-contract)
//! - Decisions: [ADR-0015](../../../docs/adr/0015-stable-violation-id.md),
//!   [ADR-0021](../../../docs/adr/0021-agent-surface-cli-first.md)
//! - Plans: [Wave 1, sub-wave 1D](../../../docs/plans/pending/0001-wave-1-typescript-parity.md),
//!   [Wave 2, sub-wave 2E](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md),
//!   [Wave 3, sub-wave 3B](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md)
//! - Requirements: [FR-OUT-01](../../../docs/prd.md#fr-out-01) to [FR-OUT-03](../../../docs/prd.md#fr-out-03)
//! - Specification: dependency-cruiser's `test/report/<reporter>` fixtures, byte-compared
//!   ([ADR-0009](../../../docs/adr/0009-conformance-suites-as-specification.md))

/// Every output type, with the wave it lands in, from
/// [coverage § Output types](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#output-types)
/// and [design § Reporters](../../../docs/artifacts/design.md#reporters).
pub const OUTPUT_TYPES: &[(&str, u8)] = &[
    ("err", 1),
    ("err-long", 1),
    ("err-html", 2),
    ("json", 1),
    ("text", 1),
    ("csv", 1),
    ("teamcity", 1),
    ("azure-devops", 1),
    ("github-annotations", 1),
    ("agent", 1),
    ("null", 1),
    ("dot", 2),
    ("ddot", 2),
    ("archi", 2),
    ("cdot", 2),
    ("flat", 2),
    ("fdot", 2),
    ("mermaid", 2),
    ("d2", 2),
    ("baseline", 2),
    ("metrics", 2),
    ("sarif", 2),
    ("junit", 2),
    ("trx", 2),
    ("x-dot-webpage", 3),
    ("html", 3),
    ("markdown", 3),
    ("anon", 3),
    ("plantuml", 3),
];

/// Whether `name` is a known output type. `plugin:<path>` is always accepted syntactically and
/// resolved at run time ([coverage § Output types](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#output-types)).
pub fn is_output_type(name: &str) -> bool {
    name.starts_with("plugin:") || OUTPUT_TYPES.iter().any(|(n, _)| *n == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knows_every_dependency_cruiser_output_type() {
        // The twenty-one from the coverage tab, with archi/cdot and flat/fdot counted as aliases.
        for name in [
            "err",
            "err-long",
            "err-html",
            "json",
            "text",
            "csv",
            "teamcity",
            "azure-devops",
            "dot",
            "ddot",
            "cdot",
            "archi",
            "fdot",
            "flat",
            "x-dot-webpage",
            "mermaid",
            "d2",
            "html",
            "markdown",
            "anon",
            "baseline",
            "metrics",
            "null",
        ] {
            assert!(is_output_type(name), "{name}");
        }
        assert!(is_output_type("plugin:./my-reporter.cjs"));
        assert!(!is_output_type("pdf"));
    }
}
