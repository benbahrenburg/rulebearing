//! `rulebearing`: the command-line tool.
//!
//! - Architecture: [`docs/architecture.md#outputs-and-ci-contract`](../../../docs/architecture.md#outputs-and-ci-contract),
//!   [`#agent-surface`](../../../docs/architecture.md#agent-surface)
//! - Decisions: [ADR-0008](../../../docs/adr/0008-exit-code-contract.md),
//!   [ADR-0021](../../../docs/adr/0021-agent-surface-cli-first.md)
//! - Plans: [Wave 1](../../../docs/plans/pending/0001-wave-1-typescript-parity.md) onward
//! - Requirements: [FR-CLI-01](../../../docs/prd.md#fr-cli-01) to [FR-CLI-08](../../../docs/prd.md#fr-cli-08)
//!
//! This library holds the dispatch and the exit-code table so the binary, the tests and the Node
//! binding (`rb-node`, [ADR-0010](../../../docs/adr/0010-crate-layout-and-extractor-boundary.md))
//! share one implementation; `main.rs` only moves bytes to the process.
//!
//! | Module | Does |
//! | --- | --- |
//! | [`cli`] | every flag, declared once |
//! | [`cmd`] | one module per subcommand |
//! | [`pipeline`] | the five stages as one call |
//! | [`configure`] | the configuration and the flags laid over it |
//! | [`context`] | the working directory, the clock, the terminal |
//! | [`exit`] | the exit-code table |
//! | [`progress`] | `--progress` |
//! | [`protocol`] | the conformance harness's `validate` and `report` |

pub mod cli;
pub mod cmd;
pub mod configure;
pub mod context;
pub mod exit;
pub mod pipeline;
pub mod progress;
pub mod protocol;
pub mod ratchets;

use std::fmt::Write as _;

use clap::Parser;

use crate::cli::{Cli, Command};
pub use crate::context::Context;
pub use crate::exit::RunExit;

/// Subcommands later waves deliver, with the wave. Asking for one says so and exits 2, so no
/// pipeline mistakes a missing command for a passing gate.
pub const LATER: &[(&str, u8)] = &[
    ("baseline", 2),
    ("diff", 3),
    ("place", 2),
    ("docs", 2),
    ("propose", 2),
    ("decisions", 2),
    ("guard", 3),
    ("snapshot", 3),
    ("changelog", 3),
    ("serve", 3),
];

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

impl Outcome {
    /// A successful run that printed `stdout`.
    pub fn printed(stdout: impl Into<String>) -> Self {
        Self {
            stdout: stdout.into(),
            stderr: String::new(),
            code: 0,
        }
    }

    /// A failed run.
    pub fn failed(code: RunExit, stderr: impl Into<String>) -> Self {
        Self {
            stdout: String::new(),
            stderr: stderr.into(),
            code: code.code(),
        }
    }
}

/// Writes a report to `-` (stdout, collected in `stdout`) or to a file under the working
/// directory, creating its folder.
///
/// # Errors
/// A message naming the file when it cannot be written.
pub fn write_output(
    ctx: &Context<'_>,
    to: &str,
    text: &str,
    stdout: &mut String,
) -> Result<(), String> {
    if to == "-" || to.is_empty() {
        stdout.push_str(text);
        return Ok(());
    }
    let path = ctx.resolve(to);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    std::fs::write(&path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

/// Dispatches the command line in `ctx`.
pub fn run_in(ctx: &mut Context<'_>, args: &[String]) -> Outcome {
    if args.is_empty() {
        return Outcome::printed(help());
    }
    if let Some((name, wave)) = LATER
        .iter()
        .find(|(n, _)| args.first().map(String::as_str) == Some(*n))
    {
        return Outcome::failed(
            RunExit::Untrustworthy,
            format!(
                "rulebearing {name}: arrives in wave {wave}; see docs/plans/pending/ for the plan that delivers it\n"
            ),
        );
    }
    let command_line = std::iter::once("rulebearing".to_owned()).chain(args.iter().cloned());
    let cli = match Cli::try_parse_from(command_line) {
        Ok(cli) => cli,
        Err(error) => {
            let text = error.render().to_string();
            return match error.kind() {
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion => {
                    Outcome::printed(text)
                }
                _ => Outcome::failed(RunExit::InvalidConfig, text),
            };
        }
    };
    match cli.command {
        Command::Cruise(a) => cmd::cruise::run(ctx, &a),
        Command::Fmt(a) => cmd::fmt::run(ctx, &a),
        Command::Rules(a) => cmd::rules::run(ctx, &a),
        Command::Explain(a) => cmd::explain::run(ctx, &a),
        Command::Test(a) => cmd::test_rules::run(ctx, &a),
        Command::CanImport(a) => cmd::can_import::run(ctx, &a),
        Command::Count(a) => cmd::count::run(ctx, &a),
        Command::Config(c) => cmd::config::run(ctx, &c),
        Command::Hooks(c) => cmd::hooks::run(ctx, &c),
        Command::Summary(a) => cmd::summary::run(ctx, &a),
        Command::Impact(a) => cmd::impact::run(ctx, &a),
        Command::Attest(a) => cmd::attest::run(ctx, &a),
        Command::Init(a) => cmd::init::run(ctx, &a),
        Command::Adopt(a) => cmd::adopt::run(ctx, &a),
        Command::Import(c) => cmd::import::run(ctx, &c),
        Command::Validate(_) => match ctx.read_stdin() {
            Ok(text) => protocol::validate(&text),
            Err(e) => Outcome::failed(
                RunExit::Untrustworthy,
                format!("rulebearing validate: cannot read stdin: {e}\n"),
            ),
        },
        Command::Report(a) => match ctx.read_stdin() {
            Ok(text) => protocol::report(a.output_type.as_deref().unwrap_or("err"), &text),
            Err(e) => Outcome::failed(
                RunExit::Untrustworthy,
                format!("rulebearing report: cannot read stdin: {e}\n"),
            ),
        },
    }
}

/// The top-level help.
pub fn help() -> String {
    let mut command = <Cli as clap::CommandFactory>::command();
    command.render_help().to_string()
}

/// Dispatches the command line in the process's environment.
pub fn run_with_input(args: &[String], stdin: &mut dyn std::io::Read) -> Outcome {
    use std::io::IsTerminal as _;
    let (today, timestamp) = context::clock();
    let mut ctx = Context {
        cwd: std::env::current_dir().unwrap_or_default(),
        stdin,
        today,
        timestamp,
        color_terminal: std::io::stdout().is_terminal(),
    };
    run_in(&mut ctx, args)
}

/// Kept for callers that pass no stdin.
pub fn run(args: &[String]) -> Outcome {
    let mut empty: &[u8] = &[];
    run_with_input(args, &mut empty)
}

/// Every subcommand name `--help` lists, then the later-wave ones.
pub fn subcommands() -> Vec<String> {
    let command = <Cli as clap::CommandFactory>::command();
    let mut names: Vec<String> = command
        .get_subcommands()
        .filter(|c| !c.is_hide_set())
        .map(|c| c.get_name().to_owned())
        .collect();
    names.extend(LATER.iter().map(|(n, _)| (*n).to_owned()));
    let _ = names.iter().fold(String::new(), |mut s, n| {
        let _ = write!(s, "{n} ");
        s
    });
    names
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(h.stdout.contains("cruise"));
        assert!(h.stdout.contains("Exit codes"));
        assert_eq!(run(&[]).code, 0);
    }

    #[test]
    fn later_subcommands_name_their_wave() {
        let o = run(&args(&["diff", "a.json", "b.json"]));
        assert_eq!(o.code, 2);
        assert!(o.stderr.contains("wave 3"));
        assert!(subcommands().contains(&"cruise".to_owned()));
        assert!(subcommands().contains(&"serve".to_owned()));
    }

    #[test]
    fn unknown_subcommands_and_flags_are_invalid() {
        assert_eq!(run(&args(&["frobnicate"])).code, 3);
        assert_eq!(run(&args(&["cruise", "--no-such-flag"])).code, 3);
    }

    #[test]
    fn outcomes() {
        assert_eq!(Outcome::printed("x").code, 0);
        assert_eq!(Outcome::failed(RunExit::InvalidConfig, "e").code, 3);
    }
}
