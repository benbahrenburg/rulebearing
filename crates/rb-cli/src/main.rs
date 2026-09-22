//! `rulebearing`: the command-line tool.
//!
//! - Architecture: [`docs/architecture.md#outputs-and-ci-contract`](../../../docs/architecture.md#outputs-and-ci-contract),
//!   [`#agent-surface`](../../../docs/architecture.md#agent-surface)
//! - Decisions: [ADR-0008](../../../docs/adr/0008-exit-code-contract.md),
//!   [ADR-0021](../../../docs/adr/0021-agent-surface-cli-first.md)
//! - Plans: [Wave 1](../../../docs/plans/pending/0001-wave-1-typescript-parity.md) onward
//! - Requirements: [FR-CLI-01](../../../docs/prd.md#fr-cli-01) to [FR-CLI-08](../../../docs/prd.md#fr-cli-08)
//!
//! Wave 0 ships `--version`, `--help` and the exit-code function. Subcommands arrive with the
//! plans that specify them; until then every subcommand reports that it is not implemented and
//! exits with the "untrustworthy run" code so no pipeline mistakes a stub for a passing gate.

use std::process::ExitCode;

/// The exit code for a run, from [ADR-0008](../../../docs/adr/0008-exit-code-contract.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunExit {
    /// Zero or more error-severity violations; the code is the count, capped at 255.
    Violations(u32),
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

const SUBCOMMANDS: &[&str] = &[
    "cruise",
    "fmt",
    "baseline",
    "rules",
    "count",
    "diff",
    "explain",
    "can-import",
    "place",
    "impact",
    "test",
    "docs",
    "config",
    "init",
    "adopt",
    "hooks",
    "attest",
    "import",
    "propose",
    "decisions",
    "guard",
    "snapshot",
    "changelog",
    "serve",
];

fn usage() -> String {
    let mut s = String::from(
        "rulebearing: one architecture rule set for TypeScript, .NET and Python\n\nUsage: rulebearing <subcommand> [options]\n\nSubcommands:\n",
    );
    for c in SUBCOMMANDS {
        s.push_str("  ");
        s.push_str(c);
        s.push('\n');
    }
    s.push_str("\nExit codes: 0 no error violations; 1-255 error count; 2 untrustworthy run; 3 invalid config.\nSee docs/architecture.md and docs/plans/README.md.\n");
    s
}

/// What a run printed and how it exited; separated from `main` so the dispatch is unit-tested.
#[derive(Debug, PartialEq, Eq)]
pub struct Outcome {
    /// Text for stdout.
    pub stdout: String,
    /// Text for stderr.
    pub stderr: String,
    /// Process exit code.
    pub code: u8,
}

/// Dispatches the command line. Wave 0 knows `--version`, `--help` and the subcommand names.
pub fn run(args: &[String]) -> Outcome {
    match args.first().map(String::as_str) {
        Some("--version" | "-V") => Outcome {
            stdout: format!("rulebearing {}\n", env!("CARGO_PKG_VERSION")),
            stderr: String::new(),
            code: 0,
        },
        None | Some("--help" | "-h") => Outcome {
            stdout: usage(),
            stderr: String::new(),
            code: 0,
        },
        Some(cmd) if SUBCOMMANDS.contains(&cmd) => Outcome {
            stdout: String::new(),
            stderr: format!(
                "rulebearing {cmd}: not implemented yet; see docs/plans/pending/ for the plan that delivers it\n"
            ),
            code: RunExit::Untrustworthy.code(),
        },
        Some(other) => Outcome {
            stdout: String::new(),
            stderr: format!("rulebearing: unknown subcommand `{other}`\n\n{}", usage()),
            code: RunExit::InvalidConfig.code(),
        },
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let outcome = run(&args);
    print!("{}", outcome.stdout);
    eprint!("{}", outcome.stderr);
    ExitCode::from(outcome.code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_code_table_matches_adr_0008() {
        assert_eq!(RunExit::Violations(0).code(), 0);
        assert_eq!(RunExit::Violations(7).code(), 7);
        assert_eq!(RunExit::Violations(1_000).code(), 255);
        assert_eq!(RunExit::Untrustworthy.code(), 2);
        assert_eq!(RunExit::InvalidConfig.code(), 3);
    }

    #[test]
    fn usage_lists_every_subcommand() {
        let u = usage();
        for c in SUBCOMMANDS {
            assert!(u.contains(c));
        }
    }

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn version_and_help_exit_zero() {
        let v = run(&args(&["--version"]));
        assert_eq!(v.code, 0);
        assert!(v.stdout.starts_with("rulebearing "));
        let h = run(&args(&["--help"]));
        assert_eq!(h.code, 0);
        assert!(h.stdout.contains("Subcommands"));
        assert_eq!(run(&[]).code, 0);
    }

    #[test]
    fn known_subcommand_is_untrustworthy_until_implemented() {
        let o = run(&args(&["cruise", "--config", "rulebearing.yaml"]));
        assert_eq!(o.code, 2);
        assert!(o.stderr.contains("not implemented"));
        assert!(o.stdout.is_empty());
    }

    #[test]
    fn unknown_subcommand_is_invalid_config() {
        let o = run(&args(&["frobnicate"]));
        assert_eq!(o.code, 3);
        assert!(o.stderr.contains("unknown subcommand"));
    }
}
