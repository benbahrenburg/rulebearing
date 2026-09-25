//! `rulebearing explain <rule> [--plain]`: a rule, its fix, what it matches and the first ten edges.
//!
//! - Source: [design § Questions an agent can ask](../../../../docs/artifacts/design.md#questions-an-agent-can-ask-before-it-writes-the-import),
//!   [design § The agentic engineering hat](../../../../docs/artifacts/design.md#the-agentic-engineering-hat-turn-two) (`explain --plain`)
//! - Decision: [ADR-0021](../../../../docs/adr/0021-agent-surface-cli-first.md)
//! - Plan: [Wave 1, Step 14](../../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-14-rules---json-explain-explain---plain-test-can-import-1e)
//! - Requirement: [FR-CLI-01](../../../../docs/prd.md#fr-cli-01)

use std::fmt::Write as _;

use clap::Args;

use crate::cli::{ConfigArgs, GraphArgs};
use crate::cmd::{plain, rules};
use crate::context::Context;
use crate::{Outcome, RunExit, configure};

/// `explain`.
#[derive(Debug, Clone, Default, Args)]
pub struct ExplainArgs {
    /// The rule (for an `allowed` entry, `allowed[n]`)
    pub rule: String,
    /// One English sentence per rule, no graph needed
    #[arg(long)]
    pub plain: bool,
    /// Configuration
    #[command(flatten)]
    pub config: ConfigArgs,
    /// The graph
    #[command(flatten)]
    pub graph: GraphArgs,
}

/// Runs `explain`.
pub fn run(ctx: &mut Context<'_>, args: &ExplainArgs) -> Outcome {
    let config = match configure::required(ctx, &args.config) {
        Ok(c) => c,
        Err(o) => return o,
    };
    let listed = rules::listed(&config);
    let allowed_offset = config.rules.dependencies.forbidden.len();
    let Some(index) = listed.iter().enumerate().position(|(i, (family, rule))| {
        rule.name() == args.rule
            || (*family == rb_config::Family::Allowed
                && args.rule == format!("allowed[{}]", i - allowed_offset))
    }) else {
        let names: Vec<&str> = listed.iter().map(|(_, r)| r.name()).collect();
        return Outcome::failed(
            RunExit::InvalidConfig,
            format!(
                "rulebearing explain: no rule `{}`; the rules are {}\n",
                args.rule,
                names.join(", ")
            ),
        );
    };
    let (family, rule) = listed[index];
    if args.plain {
        return Outcome::printed(format!("{}\n", plain::sentence(family, rule)));
    }
    let evaluation = match rules::statistics(ctx, &config, &args.graph) {
        Ok(e) => e,
        Err(o) => return o,
    };
    let stats = evaluation.rule_stats.get(index);
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{}  ({}, {})",
        rule.name(),
        family.as_str(),
        rule.severity()
    );
    let _ = writeln!(out, "  {}", plain::sentence(family, rule));
    if let Some(comment) = &rule.meta.comment {
        let _ = writeln!(out, "  why: {comment}");
    }
    let _ = writeln!(out, "  fix: {}", rule.meta.fix.as_deref().unwrap_or("-"));
    let selectors = rules::rule_json(family, rule, None);
    for side in ["from", "to", "module", "select"] {
        if let Some(value) = selectors.get(side).filter(|v| !v.is_null()) {
            let _ = writeln!(out, "  {side}: {value}");
        }
    }
    if let Some(stats) = stats {
        let _ = writeln!(
            out,
            "  matches: from {} modules, to {}; {} violations",
            stats.from_matches, stats.to_matches, stats.violations
        );
    }
    let edges: Vec<String> = evaluation
        .document
        .summary
        .violations
        .iter()
        .filter(|v| v.rule.name == rule.name())
        .take(10)
        .map(|v| format!("    {} -> {}", v.from, v.to))
        .collect();
    if !edges.is_empty() {
        let _ = writeln!(out, "  first edges:\n{}", edges.join("\n"));
    }
    Outcome::printed(out)
}
