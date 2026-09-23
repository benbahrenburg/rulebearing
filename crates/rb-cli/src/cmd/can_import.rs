//! `rulebearing can-import <from> <to>`: may this file import that one, before the import exists?
//!
//! - Source: [design § Questions an agent can ask before it writes the import](../../../../docs/artifacts/design.md#questions-an-agent-can-ask-before-it-writes-the-import)
//! - Decision: [ADR-0021](../../../../docs/adr/0021-agent-surface-cli-first.md)
//! - Plan: [Wave 1, Step 14](../../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-14-rules---json-explain-explain---plain-test-can-import-1e)
//!   (from the saved graph, in milliseconds; § 1.6: `.graph/cruise.json` or `--graph`)
//! - Requirement: [FR-CLI-02](../../../../docs/prd.md#fr-cli-02)
//!
//! Reads the saved graph, adds the hypothetical edge (circular when `to` already reaches `from`),
//! and evaluates the dependency rules for that edge alone. Prints `yes`, or `no` with the deciding
//! rule, its comment and its `fix`; exits 0 for yes, 1 for no.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt::Write as _;

use clap::Args;
use rb_rules::validate::validate_dependency;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::cli::ConfigArgs;
use crate::context::Context;
use crate::pipeline::SAVED_GRAPH;
use crate::{Outcome, RunExit, configure};

/// `can-import`.
#[derive(Debug, Clone, Default, Args)]
pub struct CanImportArgs {
    /// The file that would import
    pub from: String,
    /// The file it would import
    pub to: String,
    /// The saved graph (default .graph/cruise.json)
    #[arg(long, value_name = "FILE")]
    pub graph: Option<String>,
    /// Configuration
    #[command(flatten)]
    pub config: ConfigArgs,
}

/// Only what `can-import` reads from a saved graph, so a large result loads fast.
#[derive(Debug, Deserialize)]
struct LightGraph {
    modules: Vec<LightModule>,
}

#[derive(Debug, Deserialize)]
struct LightModule {
    source: String,
    #[serde(default)]
    dependencies: Vec<LightDependency>,
}

#[derive(Debug, Deserialize)]
struct LightDependency {
    resolved: String,
}

/// Whether `start` reaches `goal` over the saved edges.
fn reaches(edges: &HashMap<&str, Vec<&str>>, start: &str, goal: &str) -> bool {
    let mut seen = HashSet::new();
    let mut queue = VecDeque::from([start]);
    while let Some(node) = queue.pop_front() {
        if node == goal {
            return true;
        }
        if seen.insert(node) {
            queue.extend(edges.get(node).into_iter().flatten().copied());
        }
    }
    false
}

/// Runs `can-import`.
pub fn run(ctx: &mut Context<'_>, args: &CanImportArgs) -> Outcome {
    let config = match configure::required(ctx, &args.config) {
        Ok(c) => c,
        Err(o) => return o,
    };
    let file = args.graph.clone().unwrap_or_else(|| SAVED_GRAPH.to_owned());
    let text = match std::fs::read_to_string(ctx.resolve(&file)) {
        Ok(t) => t,
        Err(e) => {
            return Outcome::failed(
                RunExit::Untrustworthy,
                format!(
                    "rulebearing can-import: cannot read {file}: {e}; run `rulebearing cruise -T json -f {SAVED_GRAPH}` first\n"
                ),
            );
        }
    };
    let graph: LightGraph = match serde_json::from_str(&text) {
        Ok(g) => g,
        Err(e) => {
            return Outcome::failed(
                RunExit::Untrustworthy,
                format!("rulebearing can-import: {file} is not a cruise result: {e}\n"),
            );
        }
    };
    let edges: HashMap<&str, Vec<&str>> = graph
        .modules
        .iter()
        .map(|m| {
            (
                m.source.as_str(),
                m.dependencies.iter().map(|d| d.resolved.as_str()).collect(),
            )
        })
        .collect();
    let circular = args.from == args.to || reaches(&edges, &args.to, &args.from);
    let from = json!({ "source": args.from });
    let dependency = json!({
        "module": args.to, "resolved": args.to, "coreModule": false, "couldNotResolve": false,
        "dependencyTypes": ["local"], "dynamic": false, "exoticallyRequired": false,
        "followable": true, "circular": circular, "moduleSystem": "es6",
    });
    let verdict = validate_dependency(&config.rules.dependencies, &from, &dependency);
    let rules = verdict
        .get("rules")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let blocking: Vec<&Value> = rules
        .iter()
        .filter(|r| r.get("severity").and_then(Value::as_str) == Some("error"))
        .collect();
    let mut out = String::new();
    if blocking.is_empty() {
        out.push_str("yes\n");
        for r in &rules {
            let _ = writeln!(
                out,
                "  (warns: {} {})",
                r["severity"].as_str().unwrap_or_default(),
                r["name"].as_str().unwrap_or_default()
            );
        }
        return Outcome::printed(out);
    }
    out.push_str("no\n");
    for r in blocking {
        let name = r["name"].as_str().unwrap_or_default();
        let rule = config
            .rules
            .all_dependency_rules()
            .map(|(_, rule)| rule)
            .find(|x| x.name() == name);
        let _ = writeln!(out, "  rule: {name}");
        if let Some(comment) = rule.and_then(|x| x.meta.comment.as_deref()) {
            let _ = writeln!(out, "  why: {comment}");
        }
        if let Some(fix) = rule.and_then(|x| x.meta.fix.as_deref()) {
            let _ = writeln!(out, "  fix: {fix}");
        }
    }
    Outcome {
        stdout: out,
        stderr: String::new(),
        code: RunExit::Violations(1).code(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reachability_over_saved_edges() {
        let edges: HashMap<&str, Vec<&str>> = HashMap::from([("a", vec!["b"]), ("b", vec!["c"])]);
        assert!(reaches(&edges, "a", "c"));
        assert!(!reaches(&edges, "c", "a"));
    }
}
