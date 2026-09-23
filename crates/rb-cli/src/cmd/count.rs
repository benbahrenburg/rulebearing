//! `rulebearing count --from <regex> --to <regex> [--budget FILE] [--write]`: a ratchet in one line.
//!
//! - Source: [design § The subcommands a guard reaches for](../../../../docs/artifacts/design.md#the-subcommands-a-guard-reaches-for),
//!   [design § Three pipelines](../../../../docs/artifacts/design.md#three-pipelines) (the reference line)
//! - Plan: [Wave 1, Step 7](../../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-7-liveness-severity-ids-receipts-expires-ratchets-1b)
//! - Requirement: [FR-RULE-06](../../../../docs/prd.md#fr-rule-06)
//!
//! Counts the direct edges from modules matching `--from` to targets matching `--to` (`$1` takes
//! the capture from `--from`). With `--budget`, a count above the ceiling fails (exit 1) and a
//! missing budget file fails hard (exit 2); `--write` lowers the ceiling to the count and refuses
//! to raise it.

use clap::Args;
use rb_config::model::{FromRestriction, ToRestriction};
use rb_model::options::Patterns;
use rb_rules::ratchet::{self, Budget, Verdict};

use crate::cli::GraphArgs;
use crate::context::Context;
use crate::{Outcome, RunExit, pipeline};

/// `count`.
#[derive(Debug, Clone, Default, Args)]
pub struct CountArgs {
    /// Sources to count from
    #[arg(long, value_name = "REGEX")]
    pub from: String,
    /// Targets to count to; $1 takes the capture from --from
    #[arg(long, value_name = "REGEX")]
    pub to: String,
    /// The budget file, { "ceiling": n }
    #[arg(long, value_name = "FILE")]
    pub budget: Option<String>,
    /// Lower the budget's ceiling to the count (never raises it)
    #[arg(long, requires = "budget")]
    pub write: bool,
    /// The graph
    #[command(flatten)]
    pub graph: GraphArgs,
}

/// Reads a budget file.
///
/// # Errors
/// A message when it is missing or malformed.
pub fn read_budget(ctx: &Context<'_>, file: &str) -> Result<Budget, String> {
    let path = ctx.resolve(file);
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("the budget {} cannot be read: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| {
        format!(
            "the budget {} is not {{ \"ceiling\": n }}: {e}",
            path.display()
        )
    })
}

/// Runs `count`.
pub fn run(ctx: &mut Context<'_>, args: &CountArgs) -> Outcome {
    let config = rb_config::Config::default();
    let document =
        match pipeline::query_graph(ctx, &config, args.graph.graph.as_deref(), &args.graph.paths) {
            Ok(d) => d,
            Err(m) => {
                return Outcome::failed(
                    RunExit::Untrustworthy,
                    format!("rulebearing count: {m}\n"),
                );
            }
        };
    let from = FromRestriction {
        path: Some(Patterns::One(args.from.clone())),
        ..FromRestriction::default()
    };
    let to = ToRestriction {
        path: Some(Patterns::One(args.to.clone())),
        ..ToRestriction::default()
    };
    let count = ratchet::edges(&document, &from, &to).len() as u64;
    let Some(file) = &args.budget else {
        return Outcome::printed(format!("{count}\n"));
    };
    let budget = match read_budget(ctx, file) {
        Ok(b) => Some(b),
        Err(_) if args.write && !ctx.resolve(file).exists() => None,
        Err(m) => {
            return Outcome::failed(RunExit::Untrustworthy, format!("rulebearing count: {m}\n"));
        }
    };
    if args.write {
        return match ratchet::lowered(count, budget) {
            Ok(new) => {
                let text = format!("{}\n", serde_json::json!({ "ceiling": new.ceiling }));
                let mut ignored = String::new();
                match crate::write_output(ctx, file, &text, &mut ignored) {
                    Ok(()) => Outcome::printed(format!("{count} (ceiling now {})\n", new.ceiling)),
                    Err(m) => {
                        Outcome::failed(RunExit::Untrustworthy, format!("rulebearing count: {m}\n"))
                    }
                }
            }
            Err(e) => Outcome::failed(RunExit::Violations(1), format!("rulebearing count: {e}\n")),
        };
    }
    match ratchet::verdict(count, budget) {
        Verdict::Over { excess } => Outcome {
            stdout: format!("{count}\n"),
            stderr: format!(
                "rulebearing count: {count} edges, {excess} over the ceiling in {file}; remove edges, a ratchet only falls\n"
            ),
            code: RunExit::Violations(1).code(),
        },
        Verdict::Within { headroom } => {
            Outcome::printed(format!("{count} ({headroom} under the ceiling)\n"))
        }
        Verdict::Unbudgeted => Outcome::printed(format!("{count}\n")),
    }
}
