//! `rb-rules`: the rule engine. Reads the graph document and the config model; writes
//! violations. Contains no `match language`
//! ([ADR-0010](../../../docs/adr/0010-crate-layout-and-extractor-boundary.md)).
//!
//! - Architecture: [`docs/architecture.md#the-rule-engine`](../../../docs/architecture.md#the-rule-engine)
//! - Decisions: [ADR-0007](../../../docs/adr/0007-vacuous-rules-fail-by-default.md),
//!   [ADR-0014](../../../docs/adr/0014-no-invented-cross-language-edges.md),
//!   [ADR-0015](../../../docs/adr/0015-stable-violation-id.md),
//!   [ADR-0016](../../../docs/adr/0016-linear-time-regex-and-strict-compat.md)
//! - Plans: [Wave 1, sub-wave 1B](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#wave-1b-rb-rules)
//!   (dependency rules), [Wave 2, sub-wave 2C](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md)
//!   (element, slice, diagram rules)
//! - Requirements: [FR-RULE-01](../../../docs/prd.md#fr-rule-01) to [FR-RULE-10](../../../docs/prd.md#fr-rule-10)
//! - Source: [design § The rule language](../../../docs/artifacts/design.md#the-rule-language)
//! - Specification: dependency-cruiser 18.2.0's `test/validate` and `test/graph-utl`, run
//!   unmodified against this crate by conformance gate 1 layer 2 through [`conformance`]
//!
//! | Module | Does |
//! | --- | --- |
//! | [`mod@evaluate`] | the entry point: derivations, validation, summary, liveness, ids |
//! | [`validate`] | one module, dependency or folder against the rules |
//! | [`matchers`] | the restriction matchers |
//! | [`mod@derive`] | cycles, dependents, orphans, reachability, instability |
//! | [`folders`] | the folder layer |
//! | [`summarize`] | violations, counts, `ruleSetUsed`, `optionsUsed` |
//! | [`compare`] | the orderings output is sorted by |
//! | [`graph`] | the indexed graph, consolidation and filters |
//! | [`rewrap`] | `fmt`'s re-summary of a saved result |
//! | [`ratchet`] | ratchet counts |
//! | [`conformance`] | the `rulebearing validate` protocol layer 2 speaks |
//! | [`js`], [`patterns`] | JavaScript's value and pattern semantics |

pub mod compare;
pub mod conformance;
pub mod derive;
pub mod elements;
pub mod evaluate;
pub mod families;
pub mod folders;
pub mod graph;
pub mod js;
pub mod matchers;
pub mod patterns;
pub mod plantuml;
pub mod ratchet;
pub mod rewrap;
pub mod slices;
pub mod summarize;
pub mod validate;

pub use evaluate::{EngineError, EvalOptions, Evaluation, Expired, RuleStats, evaluate};
/// The JavaScript-to-Rust pattern compatibility table
/// ([ADR-0016](../../../docs/adr/0016-linear-time-regex-and-strict-compat.md)).
pub use rb_config::pattern::COMPATIBILITY;

/// A rule's severity. The vocabulary lives in `rb-model` so the document, the config and the
/// engine share one declaration ([ADR-0004](../../../docs/adr/0004-graph-document-is-cruise-result-superset.md)).
pub use rb_model::Severity;

/// Whether a finding of this severity counts toward the exit code. Only `error` does
/// ([ADR-0008](../../../docs/adr/0008-exit-code-contract.md)).
pub fn counts_toward_exit(severity: Severity) -> bool {
    severity == Severity::Error
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
    fn severity_parses_and_only_error_counts() {
        assert_eq!("ignore".parse(), Ok(Severity::Ignore));
        assert_eq!("error".parse(), Ok(Severity::Error));
        assert!(
            "Error".parse::<Severity>().is_err(),
            "the vocabulary is lowercase"
        );
        let counting: Vec<Severity> = Severity::ALL
            .iter()
            .copied()
            .filter(|s| counts_toward_exit(*s))
            .collect();
        assert_eq!(counting, [Severity::Error]);
    }

    #[test]
    fn vacuous_by_default_and_opt_out() {
        assert_eq!(liveness(0, false), Liveness::Vacuous);
        assert_eq!(liveness(0, true), Liveness::AllowedEmpty);
        assert_eq!(liveness(3, false), Liveness::Live);
        assert!(!COMPATIBILITY.is_empty());
    }
}
