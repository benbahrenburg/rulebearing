//! `rulebearing explain <rule> [--plain]`: a rule, its fix, what it matches and the first ten edges.
//!
//! - Source: [design § Questions an agent can ask](../../../../docs/artifacts/design.md#questions-an-agent-can-ask-before-it-writes-the-import),
//!   [design § The agentic engineering hat](../../../../docs/artifacts/design.md#the-agentic-engineering-hat-turn-two) (`explain --plain`)
//! - Decision: [ADR-0021](../../../../docs/adr/0021-agent-surface-cli-first.md)
//! - Plan: [Wave 1, Step 14](../../../../docs/plans/implemented/0001-wave-1-typescript-parity.md#step-14-rules---json-explain-explain---plain-test-can-import-1e)
//! - Plan: [Wave 3, Step 19](../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#25-steps-for-sub-wave-3e-serve---mcp-and-serve---lsp)
//!   (`--json`, the MCP tool's answer)
//! - Requirement: [FR-CLI-01](../../../../docs/prd.md#fr-cli-01), [FR-CLI-06](../../../../docs/prd.md#fr-cli-06)
//!
//! `--json` prints the rule as `rules --json` lists it (with its statistics, or without them under
//! `--plain`), plus its `sentence` and, unless `--plain`, the `firstEdges` the text lists.

use std::fmt::Write as _;

use clap::Args;
use serde_json::{Value, json};

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
    /// Print JSON
    #[arg(long)]
    pub json: bool,
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
    let sentence = plain::sentence(family, rule);
    if args.plain {
        if args.json {
            return Outcome::printed(json_text(
                rules::rule_json(family, rule, None),
                &sentence,
                None,
            ));
        }
        return Outcome::printed(format!("{sentence}\n"));
    }
    let evaluation = match rules::statistics(ctx, &config, &args.graph) {
        Ok(e) => e,
        Err(o) => return o,
    };
    let stats = evaluation.rule_stats.get(index);
    let first: Vec<(&str, &str)> = evaluation
        .document
        .summary
        .violations
        .iter()
        .filter(|v| v.rule.name == rule.name())
        .take(10)
        .map(|v| (v.from.as_str(), v.to.as_str()))
        .collect();
    if args.json {
        return Outcome::printed(json_text(
            rules::rule_json(family, rule, stats),
            &sentence,
            Some(&first),
        ));
    }
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{}  ({}, {})",
        rule.name(),
        family.as_str(),
        rule.severity()
    );
    let _ = writeln!(out, "  {sentence}");
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
    let edges: Vec<String> = first
        .iter()
        .map(|(from, to)| format!("    {from} -> {to}"))
        .collect();
    if !edges.is_empty() {
        let _ = writeln!(out, "  first edges:\n{}", edges.join("\n"));
    }
    Outcome::printed(out)
}

/// `explain --json`: the rule as `rules --json` lists it, its sentence and, when the graph was
/// read, the first edges it flagged.
fn json_text(mut value: Value, sentence: &str, first: Option<&[(&str, &str)]>) -> String {
    if let Some(object) = value.as_object_mut() {
        object.insert("sentence".into(), json!(sentence));
        if let Some(first) = first {
            let edges: Vec<Value> = first
                .iter()
                .map(|(from, to)| json!({ "from": from, "to": to }))
                .collect();
            object.insert("firstEdges".into(), Value::Array(edges));
        }
    }
    let mut text = serde_json::to_string_pretty(&value).unwrap_or_default();
    text.push('\n');
    text
}
