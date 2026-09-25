//! The exit-code function every subcommand shares
//! ([ADR-0008](../../../docs/adr/0008-exit-code-contract.md)).
//!
//! - Contract: [Wave 1 plan § 1.5](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#15-interfaces-and-contracts-frozen-by-this-wave)
//! - Requirement: [FR-CORE-06](../../../docs/prd.md#fr-core-06)

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

impl RunExit {
    /// Maps the outcome to a process exit code.
    pub fn code(self) -> u8 {
        match self {
            Self::Violations(n) => u8::try_from(n).unwrap_or(u8::MAX),
            Self::Untrustworthy => 2,
            Self::InvalidConfig => 3,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_code_table_matches_adr_0008() {
        for (exit, code) in [
            (RunExit::Violations(0), 0),
            (RunExit::Violations(1), 1),
            (RunExit::Violations(7), 7),
            (RunExit::Violations(255), 255),
            (RunExit::Violations(256), 255),
            (RunExit::Violations(u64::MAX), 255),
            (RunExit::Untrustworthy, 2),
            (RunExit::InvalidConfig, 3),
        ] {
            assert_eq!(exit.code(), code, "{exit:?}");
        }
    }
}
