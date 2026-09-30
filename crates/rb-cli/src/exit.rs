//! The exit-code function every subcommand shares
//! ([ADR-0008](../../../docs/adr/0008-exit-code-contract.md)).
//!
//! - Contract: [Wave 1 plan § 1.5](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#15-interfaces-and-contracts-frozen-by-this-wave)
//! - Strict mode: [Wave 3, Step 5](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#21-steps-for-sub-wave-3a-cache---affected-diff---exit-code-mode-strict),
//!   [Wave 3 plan § 1.5](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#15-interfaces-and-contracts-this-wave-freezes)
//! - Requirement: [FR-CORE-06](../../../docs/prd.md#fr-core-06)
//!
//! ADR-0008 leaves one ambiguity on purpose: a run with exactly two or three error violations
//! exits 2 or 3, the codes reserved for an untrustworthy run and an invalid configuration.
//! `--exit-code-mode strict` removes it for pipelines that need the distinction: a violation
//! count `n > 0` becomes `10 + n`, capped at 255, and 0, 2 and 3 keep their meaning. The default
//! mode is unchanged.
//!
//! A run whose graph was read in source mode is approximate, and an approximate run never
//! decides a gate ([ADR-0011](../../../docs/adr/0011-read-dotnet-assemblies-not-source.md),
//! [Wave 3, Step 15](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#24-steps-for-sub-wave-3d---mode-source-guard---watch-the-2-s-proof)):
//! [`gate`] turns the verdict of one that would into exit 2 with
//! [`APPROXIMATE_REASON`], unless `--allow-approximate-gate` asks for the count in a local
//! script.

use clap::ValueEnum;

/// How a violation count becomes an exit code (`--exit-code-mode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum ExitCodeMode {
    /// The error count, capped at 255, as dependency-cruiser exits.
    #[default]
    Default,
    /// 10 plus the error count when there is one, capped at 255, so 2 and 3 only ever mean an
    /// untrustworthy run and an invalid configuration.
    Strict,
}

/// The exit code for a run, from [ADR-0008](../../../docs/adr/0008-exit-code-contract.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunExit {
    /// Zero or more error-severity violations; the code is the count, capped at 255.
    Violations(u64),
    /// The run cannot be trusted: zero modules, missing assemblies, non-portable PDB, a file
    /// the sidecar could not handle, or a vacuous rule.
    Untrustworthy,
    /// The configuration is invalid, or a predicate names a concept the language lacks.
    InvalidConfig,
}

/// The offset strict mode adds to a non-zero violation count.
const STRICT_OFFSET: u64 = 10;

impl RunExit {
    /// Maps the outcome to a process exit code in the default mode.
    pub fn code(self) -> u8 {
        self.code_in(ExitCodeMode::Default)
    }

    /// Maps the outcome to a process exit code in `mode`.
    pub fn code_in(self, mode: ExitCodeMode) -> u8 {
        match (self, mode) {
            (Self::Violations(0), _) => 0,
            (Self::Violations(n), ExitCodeMode::Default) => u8::try_from(n).unwrap_or(u8::MAX),
            (Self::Violations(n), ExitCodeMode::Strict) => {
                u8::try_from(n.saturating_add(STRICT_OFFSET)).unwrap_or(u8::MAX)
            }
            (Self::Untrustworthy, _) => 2,
            (Self::InvalidConfig, _) => 3,
        }
    }
}

/// The warning an approximate run that would decide a gate prints before it exits 2.
pub const APPROXIMATE_REASON: &str = "approximate-mode-not-a-gate: the .NET graph was read from source (--mode source), whose edges are approximate, so the run neither passes nor fails a gate; run compiled mode for the exit code, or pass --allow-approximate-gate in a local script (ADR-0011)";

/// Whether a document was read, in whole or in part, by source mode: a receipt says
/// `mode: source`, or an edge is marked `approximate`.
pub fn is_approximate(document: &rb_model::GraphDocument) -> bool {
    document
        .summary
        .inspected
        .iter()
        .flatten()
        .any(|(_, receipt)| receipt.mode == Some(rb_model::DotnetMode::Source))
        || document
            .modules
            .iter()
            .flat_map(|m| &m.dependencies)
            .any(|d| d.approximate == Some(true))
}

/// The exit of a run that `decides` a gate (its count is the exit code): an approximate one is
/// refused, exit 2 with [`APPROXIMATE_REASON`], unless `allowed`; `None` when the verdict
/// stands.
pub fn gate(verdict: RunExit, decides: bool, approximate: bool, allowed: bool) -> Option<RunExit> {
    match verdict {
        RunExit::Violations(_) if decides && approximate && !allowed => {
            Some(RunExit::Untrustworthy)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn an_approximate_run_never_decides_a_gate() {
        let (pass, fail) = (RunExit::Violations(0), RunExit::Violations(4));
        for (verdict, decides, approximate, allowed, expected) in [
            // A compiled run is never touched.
            (pass, true, false, false, None),
            (fail, true, false, false, None),
            // An approximate run that decides a gate is refused, passing or failing.
            (pass, true, true, false, Some(RunExit::Untrustworthy)),
            (fail, true, true, false, Some(RunExit::Untrustworthy)),
            // ...unless a local script asks for the count.
            (pass, true, true, true, None),
            (fail, true, true, true, None),
            // A reporter that does not gate decides nothing to refuse.
            (fail, false, true, false, None),
            // An untrustworthy or invalid run keeps its own code.
            (RunExit::Untrustworthy, true, true, false, None),
            (RunExit::InvalidConfig, true, true, false, None),
        ] {
            assert_eq!(
                gate(verdict, decides, approximate, allowed),
                expected,
                "{verdict:?} decides={decides} approximate={approximate} allowed={allowed}"
            );
        }
        assert_eq!(RunExit::Untrustworthy.code_in(ExitCodeMode::Strict), 2);
        assert!(APPROXIMATE_REASON.starts_with("approximate-mode-not-a-gate: "));
    }

    #[test]
    fn a_document_is_approximate_by_its_receipt_or_an_edge() {
        let mut document = rb_model::GraphDocument::default();
        assert!(!is_approximate(&document));
        let mut receipt = rb_model::Receipt {
            mode: Some(rb_model::DotnetMode::Compiled),
            ..rb_model::Receipt::default()
        };
        document.summary.inspected = Some(
            [(rb_model::Language::Dotnet, receipt.clone())]
                .into_iter()
                .collect(),
        );
        assert!(!is_approximate(&document));
        receipt.mode = Some(rb_model::DotnetMode::Source);
        document.summary.inspected = Some(
            [(rb_model::Language::Dotnet, receipt)]
                .into_iter()
                .collect(),
        );
        assert!(is_approximate(&document));
        let mut stripped = rb_model::GraphDocument::default();
        let mut module = rb_model::Module::new("A.cs");
        let mut edge = rb_model::Dependency::new("B", "B.cs", rb_model::ModuleSystem::Clr);
        edge.approximate = Some(true);
        module.dependencies.push(edge);
        stripped.modules.push(module);
        assert!(is_approximate(&stripped));
    }

    #[test]
    fn exit_code_table_matches_adr_0008() {
        for (exit, default, strict) in [
            (RunExit::Violations(0), 0, 0),
            (RunExit::Violations(1), 1, 11),
            (RunExit::Violations(2), 2, 12),
            (RunExit::Violations(3), 3, 13),
            (RunExit::Violations(7), 7, 17),
            (RunExit::Violations(245), 245, 255),
            (RunExit::Violations(246), 246, 255),
            (RunExit::Violations(255), 255, 255),
            (RunExit::Violations(256), 255, 255),
            (RunExit::Violations(u64::MAX), 255, 255),
            (RunExit::Untrustworthy, 2, 2),
            (RunExit::InvalidConfig, 3, 3),
        ] {
            assert_eq!(exit.code(), default, "{exit:?} default");
            assert_eq!(exit.code_in(ExitCodeMode::Default), default, "{exit:?}");
            assert_eq!(
                exit.code_in(ExitCodeMode::Strict),
                strict,
                "{exit:?} strict"
            );
        }
    }

    #[test]
    fn the_modes_parse_by_their_command_line_names() {
        for (name, mode) in [
            ("default", ExitCodeMode::Default),
            ("strict", ExitCodeMode::Strict),
        ] {
            assert_eq!(ExitCodeMode::from_str(name, false), Ok(mode));
        }
        assert!(ExitCodeMode::from_str("loose", false).is_err());
        assert_eq!(ExitCodeMode::default(), ExitCodeMode::Default);
    }

    proptest! {
        #[test]
        fn strict_never_says_untrustworthy_or_invalid_for_violations(n in 1u64..) {
            let code = RunExit::Violations(n).code_in(ExitCodeMode::Strict);
            prop_assert!(code > 10, "{n} -> {code}");
            prop_assert_ne!(code, 2);
            prop_assert_ne!(code, 3);
        }

        #[test]
        fn strict_is_monotone_up_to_the_cap(n in 0u64..300) {
            let here = RunExit::Violations(n).code_in(ExitCodeMode::Strict);
            let next = RunExit::Violations(n + 1).code_in(ExitCodeMode::Strict);
            prop_assert!(next >= here);
            if n + 1 + STRICT_OFFSET <= 255 {
                prop_assert_eq!(u64::from(next), n + 1 + STRICT_OFFSET);
            } else {
                prop_assert_eq!(next, 255);
            }
        }

        #[test]
        fn default_mode_is_the_count_capped(n in 0u64..) {
            let code = RunExit::Violations(n).code();
            prop_assert_eq!(u64::from(code), n.min(255));
        }
    }
}
