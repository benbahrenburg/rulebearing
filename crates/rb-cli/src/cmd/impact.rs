//! `rulebearing impact <file>`: what a file is subject to, before an edit.
//!
//! - Source: [design § Questions an agent can ask before it writes the import](../../../../docs/artifacts/design.md#questions-an-agent-can-ask-before-it-writes-the-import)
//! - Plan: [Wave 1 § 1.6](../../../../docs/plans/pending/0001-wave-1-typescript-parity.md#16-decisions-applied-and-decisions-to-make)
//!   (`impact` lands in wave 1 for the `PreToolUse` hook),
//!   [Step 15](../../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-15-hooks-install---claude-code-summary---format-agent-impact-attest---require-comment-token-1e)
//!   and [Wave 2, Step 12](../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#212-step-12-agent-subcommands-2g)
//!   (text or `--json`; the worktree-aware cache of Step 13)
//! - Requirement: [FR-CLI-03](../../../../docs/prd.md#fr-cli-03)
//!
//! Prints the rules whose `from` or `to` matches the file, its dependents to `--depth`, whether it
//! sits on a cycle, and the ratchets its edges count toward: as text, or as JSON with `--json`.
//! The graph is `--graph FILE`, else the cache entry for this worktree, commit and configuration
//! ([`crate::cache`]), extracted and written on a miss. `--from-hook` reads the file from a
//! Claude Code hook's JSON on stdin (`tool_input.file_path`), relative to the working directory,
//! and answers as a `PreToolUse` hook must: the report as `hookSpecificOutput.additionalContext`,
//! which reaches the agent, and exit 0 always, because exit 2 would block the edit and plain
//! stdout never reaches the agent ([docs/agents.md](../../../../docs/agents.md#the-hooks)).

use std::collections::{BTreeSet, VecDeque};

use clap::Args;
use rb_config::Rule;
use rb_model::options::Patterns;
use rb_rules::matchers::pattern;
use serde_json::{Value, json};

use std::fmt::Write as _;

use crate::cli::ConfigArgs;
use crate::cmd::{can_import, summary};
use crate::context::Context;
use crate::pipeline::{self, RunOptions};
use crate::progress::Progress;
use crate::{Outcome, RunExit, cache, configure};

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
    /// A graph document to answer from instead of the cache
    #[arg(long, value_name = "FILE")]
    pub graph: Option<String>,
    /// Extract afresh, neither reading nor writing the cache
    #[arg(long)]
    pub no_cache: bool,
    /// Print JSON
    #[arg(long)]
    pub json: bool,
}

fn matches(p: Option<String>, text: &str) -> bool {
    p.is_some_and(|p| rb_rules::patterns::test(&p, text))
}

/// Whether a side with `path` and `path_not` takes `file`: `path` matches, or is absent when
/// `open` (a side with no `path` covers every file), and `path_not` does not match.
fn side(path: Option<&Patterns>, path_not: Option<&Patterns>, file: &str, open: bool) -> bool {
    let included = match pattern(path) {
        Some(p) => rb_rules::patterns::test(&p, file),
        None => open,
    };
    included && !matches(pattern(path_not), file)
}

/// The rules that mention the file on either side. A `from` (or `module`) with no `path` covers
/// every file its `pathNot` leaves; a `to` mentions the file only through a `path` it matches,
/// and never when its `pathNot` excludes it.
pub fn rules_mentioning<'a>(
    config: &'a rb_config::Config,
    file: &str,
) -> Vec<(&'a Rule, &'static str)> {
    config
        .rules
        .all_dependency_rules()
        .filter_map(|(_, rule)| {
            let from = match &rule.module {
                Some(m) => side(m.path.as_ref(), m.path_not.as_ref(), file, true),
                None => side(
                    rule.from.path.as_ref(),
                    rule.from.path_not.as_ref(),
                    file,
                    true,
                ),
            };
            let to = side(
                rule.to.path.as_ref(),
                rule.to.path_not.as_ref(),
                file,
                false,
            );
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
    Ok(can_import::normalise(ctx, &relative.to_string_lossy()))
}

/// Runs `impact`.
pub fn run(ctx: &mut Context<'_>, args: &ImpactArgs) -> Outcome {
    if args.from_hook {
        return from_hook(ctx, args);
    }
    let file = can_import::normalise(ctx, args.file.as_deref().unwrap_or_default());
    match report(ctx, args, &file) {
        Ok(report) if args.json => {
            let mut text = serde_json::to_string_pretty(&report).unwrap_or_default();
            text.push('\n');
            Outcome::printed(text)
        }
        Ok(report) => Outcome::printed(text(&report)),
        Err(outcome) => outcome,
    }
}

fn names(value: &Value) -> Vec<&str> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect()
}

/// The report as text: one heading per part, one line per item.
pub fn text(report: &Value) -> String {
    let field = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or("-").to_owned();
    let mut out = field(report, "file");
    if report.get("known") != Some(&Value::Bool(true)) {
        out.push_str(" (not in the graph yet)");
    }
    out.push('\n');
    let rules = report.get("rules").and_then(Value::as_array);
    match rules {
        Some(list) if !list.is_empty() => {
            out.push_str("  rules that mention it:\n");
            for r in list {
                let _ = write!(
                    out,
                    "    {} ({}, {})",
                    field(r, "name"),
                    field(r, "side"),
                    field(r, "severity")
                );
                if let Some(fix) = r.get("fix").and_then(Value::as_str) {
                    let _ = write!(out, ": {fix}");
                }
                out.push('\n');
            }
        }
        _ => out.push_str("  rules that mention it: none\n"),
    }
    let dependents = names(&report["dependents"]);
    let depth = report.get("depth").and_then(Value::as_u64).unwrap_or(1);
    if dependents.is_empty() {
        let _ = writeln!(out, "  dependents (depth {depth}): none");
    } else {
        let _ = writeln!(out, "  dependents (depth {depth}):");
        for d in dependents {
            let _ = writeln!(out, "    {d}");
        }
    }
    let cycle = report.get("onCycle") == Some(&Value::Bool(true));
    let _ = writeln!(out, "  on a cycle: {}", if cycle { "yes" } else { "no" });
    let ratchets = report.get("ratchets").and_then(Value::as_array);
    match ratchets {
        Some(list) if !list.is_empty() => {
            out.push_str("  ratchets its edges count toward:\n");
            for r in list {
                let count = r.get("count").and_then(Value::as_u64).unwrap_or(0);
                let _ = match r.get("ceiling").and_then(Value::as_u64) {
                    Some(ceiling) => {
                        writeln!(out, "    {}: {count} of {ceiling}", field(r, "name"))
                    }
                    None => writeln!(
                        out,
                        "    {}: {count}, no ceiling ({})",
                        field(r, "name"),
                        field(r, "error")
                    ),
                };
            }
        }
        _ => out.push_str("  ratchets its edges count toward: none\n"),
    }
    out
}

/// The `PreToolUse` answer: never blocks. A report that cannot be made (no configuration, nothing
/// extracted) is left out, and the reason goes to stderr, which Claude Code logs.
fn from_hook(ctx: &mut Context<'_>, args: &ImpactArgs) -> Outcome {
    let result = hook_file(ctx)
        .map_err(|m| Outcome::failed(RunExit::InvalidConfig, format!("rulebearing impact: {m}\n")))
        .and_then(|file| report(ctx, args, &file));
    match result {
        Ok(report) => {
            let context = serde_json::to_string_pretty(&report).unwrap_or_default();
            let answer = json!({
                "hookSpecificOutput": {
                    "hookEventName": "PreToolUse",
                    "additionalContext": format!("rulebearing impact:\n{context}"),
                }
            });
            let mut text = answer.to_string();
            text.push('\n');
            Outcome::printed(text)
        }
        Err(outcome) => Outcome {
            stdout: String::new(),
            stderr: outcome.stderr,
            // A hook that exits non-zero blocks the edit; the report is advice, never a gate.
            code: RunExit::Violations(0).code(),
        },
    }
}

/// What `file` is subject to.
fn report(ctx: &mut Context<'_>, args: &ImpactArgs, file: &str) -> Result<Value, Outcome> {
    let file = file.to_owned();
    let config = configure::required(ctx, &args.config)?;
    let untrusted =
        |m: String| Outcome::failed(RunExit::Untrustworthy, format!("rulebearing impact: {m}\n"));
    let graph =
        cache::document(ctx, &config, args.graph.as_deref(), args.no_cache).map_err(untrusted)?;
    let options = RunOptions {
        liveness: false,
        options_used: serde_json::Map::new(),
        paths: Vec::new(),
    };
    let evaluation =
        pipeline::evaluate_document(ctx, &config, graph, &options, &mut Progress::new(None))
            .map_err(|e| untrusted(e.to_string()))?
            .evaluation;
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
    Ok(json!({
        "file": file,
        "known": module.is_some(),
        "rules": mentioned,
        "dependents": dependents,
        "depth": args.depth,
        "onCycle": on_cycle,
        "ratchets": ratchets,
    }))
}
