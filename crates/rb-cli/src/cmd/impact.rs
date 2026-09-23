//! `rulebearing impact <file>`: what a file is subject to, before an edit.
//!
//! - Source: [design § Questions an agent can ask before it writes the import](../../../../docs/artifacts/design.md#questions-an-agent-can-ask-before-it-writes-the-import)
//! - Plan: [Wave 1 § 1.6](../../../../docs/plans/pending/0001-wave-1-typescript-parity.md#16-decisions-applied-and-decisions-to-make)
//!   (`impact` lands in wave 1 for the `PreToolUse` hook),
//!   [Step 15](../../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-15-hooks-install---claude-code-summary---format-agent-impact-attest---require-comment-token-1e)
//! - Requirement: [FR-CLI-03](../../../../docs/prd.md#fr-cli-03)
//!
//! Prints the rules whose `from` or `to` matches the file, its dependents to `--depth`, whether it
//! sits on a cycle, and the ratchets its edges count toward. `--from-hook` reads the file from a
//! Claude Code hook's JSON on stdin (`tool_input.file_path`), relative to the working directory.

use std::collections::{BTreeSet, VecDeque};

use clap::Args;
use rb_config::Rule;
use rb_rules::matchers::pattern;
use serde_json::{Value, json};

use crate::cli::{ConfigArgs, GraphArgs};
use crate::cmd::{rules, summary};
use crate::context::Context;
use crate::{Outcome, RunExit, configure};

/// `impact`.
#[derive(Debug, Clone, Default, Args)]
pub struct ImpactArgs {
    /// The file (repository-relative)
    #[arg(required_unless_present = "from_hook")]
    pub file: Option<String>,
    /// Read the file from a Claude Code hook's JSON on stdin
    #[arg(long)]
    pub from_hook: bool,
    /// How many steps of dependents to list
    #[arg(long, default_value_t = 1)]
    pub depth: usize,
    /// Configuration
    #[command(flatten)]
    pub config: ConfigArgs,
    /// A saved graph (default .graph/cruise.json)
    #[arg(long, value_name = "FILE")]
    pub graph: Option<String>,
}

fn matches(p: Option<String>, text: &str) -> bool {
    p.is_some_and(|p| rb_rules::patterns::test(&p, text))
}

/// The rules that mention the file on either side.
pub fn rules_mentioning<'a>(
    config: &'a rb_config::Config,
    file: &str,
) -> Vec<(&'a Rule, &'static str)> {
    config
        .rules
        .all_dependency_rules()
        .filter_map(|(_, rule)| {
            let from = matches(pattern(rule.from.path.as_ref()), file)
                || rule
                    .module
                    .as_ref()
                    .is_some_and(|m| matches(pattern(m.path.as_ref()), file))
                || (rule.from.path.is_none() && rule.module.is_none());
            let to = matches(pattern(rule.to.path.as_ref()), file);
            match (from, to) {
                (true, true) => Some((rule, "from and to")),
                (true, false) => Some((rule, "from")),
                (false, true) => Some((rule, "to")),
                (false, false) => None,
            }
        })
        .collect()
}

fn hook_file(ctx: &mut Context<'_>) -> Result<String, String> {
    let text = ctx.read_stdin().map_err(|e| e.to_string())?;
    let value: Value =
        serde_json::from_str(&text).map_err(|e| format!("the hook input is not JSON: {e}"))?;
    let path = value
        .pointer("/tool_input/file_path")
        .and_then(Value::as_str)
        .ok_or("the hook input has no tool_input.file_path")?;
    let path = std::path::Path::new(path);
    let cwd = ctx.cwd.canonicalize().unwrap_or_else(|_| ctx.cwd.clone());
    let relative = path
        .strip_prefix(&cwd)
        .or_else(|_| path.strip_prefix(&ctx.cwd))
        .unwrap_or(path);
    Ok(relative.to_string_lossy().replace('\\', "/"))
}

/// Runs `impact`.
pub fn run(ctx: &mut Context<'_>, args: &ImpactArgs) -> Outcome {
    let file = if args.from_hook {
        match hook_file(ctx) {
            Ok(f) => f,
            Err(m) => {
                return Outcome::failed(
                    RunExit::InvalidConfig,
                    format!("rulebearing impact: {m}\n"),
                );
            }
        }
    } else {
        args.file.clone().unwrap_or_default()
    };
    let config = match configure::required(ctx, &args.config) {
        Ok(c) => c,
        Err(o) => return o,
    };
    let graph_args = GraphArgs {
        graph: args.graph.clone(),
        paths: Vec::new(),
    };
    let evaluation = match rules::statistics(ctx, &config, &graph_args) {
        Ok(e) => e,
        Err(o) => return o,
    };
    let document = &evaluation.document;
    let mut dependents = BTreeSet::new();
    let mut queue = VecDeque::from([(file.clone(), 0usize)]);
    while let Some((name, depth)) = queue.pop_front() {
        if depth >= args.depth {
            continue;
        }
        for module in document
            .modules
            .iter()
            .filter(|m| m.dependencies.iter().any(|d| d.resolved == name))
        {
            if dependents.insert(module.source.clone()) {
                queue.push_back((module.source.clone(), depth + 1));
            }
        }
    }
    let module = document.modules.iter().find(|m| m.source == file);
    let on_cycle = module.is_some_and(|m| m.dependencies.iter().any(|d| d.circular));
    let mentioned: Vec<Value> = rules_mentioning(&config, &file)
        .into_iter()
        .map(|(r, side)| json!({ "name": r.name(), "side": side, "severity": r.severity(), "fix": r.meta.fix }))
        .collect();
    let ratchets: Vec<Value> = config
        .rules
        .ratchets
        .iter()
        .filter(|r| {
            matches(pattern(r.from.path.as_ref()), &file)
                || matches(pattern(r.to.path.as_ref()), &file)
        })
        .map(|r| summary::ratchet_state(ctx, r, document))
        .collect();
    let report = json!({
        "file": file,
        "known": module.is_some(),
        "rules": mentioned,
        "dependents": dependents,
        "depth": args.depth,
        "onCycle": on_cycle,
        "ratchets": ratchets,
    });
    let mut text = serde_json::to_string_pretty(&report).unwrap_or_default();
    text.push('\n');
    Outcome::printed(text)
}
