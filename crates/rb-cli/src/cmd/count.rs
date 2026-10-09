//! `rulebearing count --from <regex> --to <regex> [--budget FILE] [--write]`: a ratchet in one line.
//!
//! - Source: [design § The subcommands a guard reaches for](../../../../docs/artifacts/design.md#the-subcommands-a-guard-reaches-for),
//!   [design § Three pipelines](../../../../docs/artifacts/design.md#three-pipelines) (the reference line)
//! - Plan: [Wave 1, Step 7](../../../../docs/plans/implemented/0001-wave-1-typescript-parity.md#step-7-liveness-severity-ids-receipts-expires-ratchets-1b)
//! - Requirement: [FR-RULE-06](../../../../docs/prd.md#fr-rule-06)
//!
//! Counts the direct edges from modules matching `--from` to targets matching `--to` (`$1` takes
//! the capture from `--from`). With `--budget`, a count above the ceiling fails (exit 1) and a
//! missing budget file fails hard (exit 2); `--write` lowers the ceiling to the count and refuses
//! to raise it. `--json` prints the same answer as one object: `count`, and with `--budget` the
//! `ceiling` with the `headroom` under it or the `excess` over it; the exit code is the same.

use clap::Args;
use rb_config::model::{FromRestriction, ToRestriction};
use rb_model::options::Patterns;
use rb_rules::ratchet::{self, Budget, Edge, Verdict};
use serde_json::{Value, json};

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
    /// Print JSON
    #[arg(long)]
    pub json: bool,
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

/// The direct edges from modules matching `from` to targets matching `to` (`$1` takes the
/// capture from `from`) in the query graph, in document order: what `count` counts and `query`
/// lists.
///
/// # Errors
/// A message when the graph cannot be read.
pub fn matching_edges(
    ctx: &Context<'_>,
    graph: &GraphArgs,
    from: &str,
    to: &str,
) -> Result<Vec<Edge>, String> {
    let config = rb_config::Config::default();
    let document = pipeline::query_graph(ctx, &config, graph.graph.as_deref(), &graph.paths)?;
    let from = FromRestriction {
        path: Some(Patterns::One(from.to_owned())),
        ..FromRestriction::default()
    };
    let to = ToRestriction {
        path: Some(Patterns::One(to.to_owned())),
        ..ToRestriction::default()
    };
    Ok(ratchet::edges(&document, &from, &to))
}

/// `text` as printed, or `fields` as one JSON object with `--json`.
fn answer(args: &CountArgs, text: String, fields: &Value) -> String {
    if args.json {
        let mut json = serde_json::to_string_pretty(fields).unwrap_or_default();
        json.push('\n');
        json
    } else {
        text
    }
}

/// Runs `count`.
pub fn run(ctx: &mut Context<'_>, args: &CountArgs) -> Outcome {
    let count = match matching_edges(ctx, &args.graph, &args.from, &args.to) {
        Ok(edges) => edges.len() as u64,
        Err(m) => {
            return Outcome::failed(RunExit::Untrustworthy, format!("rulebearing count: {m}\n"));
        }
    };
    let Some(file) = &args.budget else {
        return Outcome::printed(answer(
            args,
            format!("{count}\n"),
            &json!({ "count": count }),
        ));
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
                let text = format!("{}\n", json!({ "ceiling": new.ceiling }));
                let mut ignored = String::new();
                match crate::write_output(ctx, file, &text, &mut ignored) {
                    Ok(()) => Outcome::printed(answer(
                        args,
                        format!("{count} (ceiling now {})\n", new.ceiling),
                        &json!({ "count": count, "ceiling": new.ceiling }),
                    )),
                    Err(m) => {
                        Outcome::failed(RunExit::Untrustworthy, format!("rulebearing count: {m}\n"))
                    }
                }
            }
            Err(e) => Outcome::failed(RunExit::Violations(1), format!("rulebearing count: {e}\n")),
        };
    }
    let ceiling = budget.map(|b| b.ceiling);
    match ratchet::verdict(count, budget) {
        Verdict::Over { excess } => Outcome {
            stdout: answer(
                args,
                format!("{count}\n"),
                &json!({ "count": count, "ceiling": ceiling, "excess": excess }),
            ),
            stderr: format!(
                "rulebearing count: {count} edges, {excess} over the ceiling in {file}; remove edges, a ratchet only falls\n"
            ),
            code: RunExit::Violations(1).code(),
        },
        Verdict::Within { headroom } => Outcome::printed(answer(
            args,
            format!("{count} ({headroom} under the ceiling)\n"),
            &json!({ "count": count, "ceiling": ceiling, "headroom": headroom }),
        )),
        Verdict::Unbudgeted => Outcome::printed(answer(
            args,
            format!("{count}\n"),
            &json!({ "count": count }),
        )),
    }
}
