//! `rulebearing summary --format agent`: the `SessionStart` brief.
//!
//! - Source: [design § The agentic engineering hat](../../../../docs/artifacts/design.md#the-agentic-engineering-hat-turn-two)
//! - Plan: [Wave 1 § 1.6](../../../../docs/plans/pending/0001-wave-1-typescript-parity.md#16-decisions-applied-and-decisions-to-make)
//!   (limited to open violations, ratchet headroom and vacuous rules),
//!   [Step 15](../../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-15-hooks-install---claude-code-summary---format-agent-impact-attest---require-comment-token-1e)
//! - Requirement: [FR-CLI-03](../../../../docs/prd.md#fr-cli-03)
//!
//! Token-budgeted like the `agent` reporter: counts by rule, the ten most violated first, each with
//! its `fix`; the ratchets with their count and headroom; the vacuous rules. Tiers and hot
//! boundaries wait for wave 2's metrics.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use clap::{Args, ValueEnum};
use rb_config::model::Ratchet;
use rb_rules::ratchet;
use serde_json::{Value, json};

use crate::cli::{ConfigArgs, GraphArgs};
use crate::cmd::count::read_budget;
use crate::cmd::rules;
use crate::context::Context;
use crate::{Outcome, configure};

/// The output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum SummaryFormat {
    /// JSON for an agent.
    #[default]
    Agent,
    /// Lines for a person.
    Text,
}

/// `summary`.
#[derive(Debug, Clone, Default, Args)]
pub struct SummaryArgs {
    /// Output format
    #[arg(long, value_enum, default_value_t = SummaryFormat::Agent)]
    pub format: SummaryFormat,
    /// Rules listed at most
    #[arg(long, default_value_t = 10)]
    pub max_rules: usize,
    /// Configuration
    #[command(flatten)]
    pub config: ConfigArgs,
    /// The graph
    #[command(flatten)]
    pub graph: GraphArgs,
}

/// One ratchet's state.
pub fn ratchet_state(
    ctx: &Context<'_>,
    ratchet: &Ratchet,
    document: &rb_model::GraphDocument,
) -> Value {
    let count = ratchet::edges(document, &ratchet.from, &ratchet.to).len() as u64;
    match read_budget(ctx, &ratchet.budget) {
        Ok(budget) => {
            json!({ "name": ratchet.name, "count": count, "ceiling": budget.ceiling, "headroom": i128::from(budget.ceiling) - i128::from(count) })
        }
        Err(message) => {
            json!({ "name": ratchet.name, "count": count, "ceiling": Value::Null, "error": message })
        }
    }
}

/// Runs `summary`.
pub fn run(ctx: &mut Context<'_>, args: &SummaryArgs) -> Outcome {
    let config = match configure::required(ctx, &args.config) {
        Ok(c) => c,
        Err(o) => return o,
    };
    let evaluation = match rules::statistics(ctx, &config, &args.graph) {
        Ok(e) => e,
        Err(o) => return o,
    };
    let mut by_rule: BTreeMap<String, (String, usize)> = BTreeMap::new();
    for v in evaluation
        .violations()
        .iter()
        .filter(|v| v.rule.severity != rb_model::Severity::Ignore)
    {
        let entry = by_rule
            .entry(v.rule.name.clone())
            .or_insert_with(|| (v.rule.severity.to_string(), 0));
        entry.1 += 1;
    }
    let mut open: Vec<(String, String, usize)> =
        by_rule.into_iter().map(|(n, (s, c))| (n, s, c)).collect();
    open.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.cmp(&b.0)));
    let total_rules = open.len();
    let listed: Vec<Value> = open
        .iter()
        .take(args.max_rules)
        .map(|(name, severity, count)| {
            let fix = config
                .rules
                .all_dependency_rules()
                .find(|(_, r)| r.name() == name)
                .and_then(|(_, r)| r.meta.fix.clone());
            json!({ "name": name, "severity": severity, "count": count, "fix": fix })
        })
        .collect();
    let ratchets: Vec<Value> = config
        .rules
        .ratchets
        .iter()
        .map(|r| ratchet_state(ctx, r, &evaluation.document))
        .collect();
    let inspected = crate::pipeline::receipt(&evaluation.document);
    let brief = json!({
        "inspected": inspected,
        "rules": config.rules.all_dependency_rules().count(),
        "errors": evaluation.document.summary.error,
        "openViolations": listed,
        "truncated": total_rules > args.max_rules,
        "ratchets": ratchets,
        "vacuousRules": evaluation.vacuous,
    });
    if args.format == SummaryFormat::Agent {
        let mut text = serde_json::to_string_pretty(&brief).unwrap_or_default();
        text.push('\n');
        return Outcome::printed(text);
    }
    let mut out = format!(
        "{} rules, {} error violations\n",
        brief["rules"], brief["errors"]
    );
    for v in &listed {
        let _ = writeln!(
            out,
            "  {} {} x{}",
            v["severity"].as_str().unwrap_or_default(),
            v["name"].as_str().unwrap_or_default(),
            v["count"]
        );
    }
    for r in &ratchets {
        let _ = writeln!(
            out,
            "  ratchet {}: {} of {}",
            r["name"].as_str().unwrap_or_default(),
            r["count"],
            r["ceiling"]
        );
    }
    Outcome::printed(out)
}
