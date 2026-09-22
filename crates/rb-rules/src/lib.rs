//! `rb-rules`: the rule engine. Reads the graph document and the config model; writes
//! violations. Contains no `match language`
//! ([ADR-0010](../../../docs/adr/0010-crate-layout-and-extractor-boundary.md)).
//!
//! - Architecture: [`docs/architecture.md#the-rule-engine`](../../../docs/architecture.md#the-rule-engine)
//! - Decisions: [ADR-0007](../../../docs/adr/0007-vacuous-rules-fail-by-default.md),
//!   [ADR-0014](../../../docs/adr/0014-no-invented-cross-language-edges.md),
//!   [ADR-0016](../../../docs/adr/0016-linear-time-regex-and-strict-compat.md)
//! - Plans: [Wave 1, sub-wave 1B](../../../docs/plans/pending/0001-wave-1-typescript-parity.md)
//!   (dependency rules), [Wave 2, sub-wave 2C](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md)
//!   (element, slice, diagram rules)
//! - Requirements: [FR-RULE-01](../../../docs/prd.md#fr-rule-01) to [FR-RULE-10](../../../docs/prd.md#fr-rule-10)
//! - Source: [design § The rule language](../../../docs/artifacts/design.md#the-rule-language)

/// Severity of a rule, as dependency-cruiser defines it. Only `Error` counts toward the exit
/// code ([ADR-0008](../../../docs/adr/0008-exit-code-contract.md)).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Not reported.
    Ignore,
    /// Reported, does not affect the exit code.
    Info,
    /// Reported, does not affect the exit code.
    Warn,
    /// Reported and counted in the exit code.
    Error,
}

impl Severity {
    /// Parses dependency-cruiser's severity strings.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "ignore" => Some(Self::Ignore),
            "info" => Some(Self::Info),
            "warn" => Some(Self::Warn),
            "error" => Some(Self::Error),
            _ => None,
        }
    }
}

/// The liveness verdict for one rule
/// ([ADR-0007](../../../docs/adr/0007-vacuous-rules-fail-by-default.md)).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Liveness {
    /// The selecting side matched at least one element.
    Live,
    /// The selecting side matched nothing and `allowEmpty` was not set: the run is untrustworthy.
    Vacuous,
    /// The selecting side matched nothing and the rule opted out with `allowEmpty: true`.
    AllowedEmpty,
}

/// Decides liveness from the number of matches on the selecting side and the rule's opt-out.
pub fn liveness(from_matches: usize, allow_empty: bool) -> Liveness {
    match (from_matches, allow_empty) {
        (0, false) => Liveness::Vacuous,
        (0, true) => Liveness::AllowedEmpty,
        _ => Liveness::Live,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_parses_and_orders() {
        // Every value dependency-cruiser defines, so deleting an arm cannot pass unnoticed.
        assert_eq!(Severity::parse("ignore"), Some(Severity::Ignore));
        assert_eq!(Severity::parse("info"), Some(Severity::Info));
        assert_eq!(Severity::parse("warn"), Some(Severity::Warn));
        assert_eq!(Severity::parse("error"), Some(Severity::Error));
        assert_eq!(Severity::parse("bogus"), None);
        assert_eq!(
            Severity::parse("Error"),
            None,
            "the vocabulary is lowercase"
        );
        assert_eq!(Severity::parse(""), None);
        assert!(Severity::Error > Severity::Warn);
        assert!(Severity::Warn > Severity::Info);
        assert!(Severity::Info > Severity::Ignore);
    }

    #[test]
    fn vacuous_by_default_and_opt_out() {
        assert_eq!(liveness(0, false), Liveness::Vacuous);
        assert_eq!(liveness(0, true), Liveness::AllowedEmpty);
        assert_eq!(liveness(3, false), Liveness::Live);
    }
}
