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

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

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
