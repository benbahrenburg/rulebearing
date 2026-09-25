//! `rulebearing rules [--json]`: every rule with what it matched, for an ADR guard or an agent.
//!
//! - Source: [design § The subcommands a guard reaches for](../../../../docs/artifacts/design.md#the-subcommands-a-guard-reaches-for)
//!   (the field list)
//! - Decision: [ADR-0021](../../../../docs/adr/0021-agent-surface-cli-first.md)
//! - Plan: [Wave 1, Step 14](../../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-14-rules---json-explain-explain---plain-test-can-import-1e)
//! - Requirement: [FR-CLI-01](../../../../docs/prd.md#fr-cli-01)
//!
//! `fromMatches`, `toMatches` and `violations` need a graph: `--graph`, the saved
//! `.graph/cruise.json`, or a fresh extraction.

use std::fmt::Write as _;

use clap::Args;
use rb_config::{Config, Family, Rule};
use rb_rules::RuleStats;
use serde_json::{Value, json};

use crate::cli::{ConfigArgs, GraphArgs};
use crate::context::Context;
use crate::pipeline::{self, RunOptions};
use crate::progress::Progress;
use crate::{Outcome, RunExit, configure};

/// `rules`.
#[derive(Debug, Clone, Default, Args)]
pub struct RulesArgs {
    /// Configuration
    #[command(flatten)]
    pub config: ConfigArgs,
    /// The graph
    #[command(flatten)]
    pub graph: GraphArgs,
    /// Print JSON
    #[arg(long)]
    pub json: bool,
}

/// Every dependency rule with its list, in evaluation order.
pub fn listed(config: &Config) -> Vec<(Family, &Rule)> {
    config.rules.all_dependency_rules().collect()
}

/// A rule as `rules --json` prints it.
pub fn rule_json(family: Family, rule: &Rule, stats: Option<&RuleStats>) -> Value {
    let severity = if family == Family::Allowed {
        Value::Null
    } else {
        json!(rule.severity())
    };
    json!({
        "name": stats.map_or_else(|| rule.name().to_owned(), |s| s.name.clone()),
        "family": family.as_str(),
        "severity": severity,
        "comment": rule.meta.comment,
        "fix": rule.meta.fix,
        "from": serde_json::to_value(&rule.from).unwrap_or(Value::Null),
        "to": serde_json::to_value(&rule.to).unwrap_or(Value::Null),
        "module": rule.module.as_ref().and_then(|m| serde_json::to_value(m).ok()),
        "select": Value::Null,
        "fromMatches": stats.map(|s| s.from_matches),
        "toMatches": stats.map(|s| s.to_matches),
        "violations": stats.map(|s| s.violations),
    })
}

/// Evaluates the configuration over the query graph, for its per-rule statistics.
///
/// # Errors
/// An [`Outcome`] to return when the graph cannot be read or evaluated.
pub fn statistics(
    ctx: &Context<'_>,
    config: &Config,
    graph: &GraphArgs,
) -> Result<rb_rules::Evaluation, Outcome> {
    let document = pipeline::query_graph(ctx, config, graph.graph.as_deref(), &graph.paths)
        .map_err(|m| Outcome::failed(RunExit::Untrustworthy, format!("rulebearing: {m}\n")))?;
    let options = RunOptions {
        liveness: false,
        options_used: serde_json::Map::new(),
        paths: graph.paths.clone(),
    };
    pipeline::evaluate_document(ctx, config, document, &options, &mut Progress::new(None))
        .map(|run| run.evaluation)
        .map_err(|e| Outcome::failed(RunExit::Untrustworthy, format!("rulebearing: {e}\n")))
}

/// Runs `rules`.
pub fn run(ctx: &mut Context<'_>, args: &RulesArgs) -> Outcome {
    let config = match configure::required(ctx, &args.config) {
        Ok(c) => c,
        Err(o) => return o,
    };
    let evaluation = match statistics(ctx, &config, &args.graph) {
        Ok(e) => e,
        Err(o) => return o,
    };
    let rules: Vec<Value> = listed(&config)
        .into_iter()
        .zip(evaluation.rule_stats.iter())
        .map(|((family, rule), stats)| rule_json(family, rule, Some(stats)))
        .collect();
    if args.json {
        let mut text = serde_json::to_string_pretty(&json!({ "rules": rules })).unwrap_or_default();
        text.push('\n');
        return Outcome::printed(text);
    }
    let mut out = format!(
        "{:<40} {:<10} {:<9} {:>5} {:>5} {:>10}\n",
        "rule", "family", "severity", "from", "to", "violations"
    );
    for r in &rules {
        let _ = writeln!(
            out,
            "{:<40} {:<10} {:<9} {:>5} {:>5} {:>10}",
            r["name"].as_str().unwrap_or_default(),
            r["family"].as_str().unwrap_or_default(),
            r["severity"].as_str().unwrap_or("-"),
            r["fromMatches"].to_string(),
            r["toMatches"].to_string(),
            r["violations"].to_string(),
        );
    }
    Outcome::printed(out)
}
