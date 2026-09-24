//! `rulebearing can-import <from> <to>`: may this file import that one, before the import exists?
//!
//! - Source: [design § Questions an agent can ask before it writes the import](../../../../docs/artifacts/design.md#questions-an-agent-can-ask-before-it-writes-the-import)
//! - Decision: [ADR-0021](../../../../docs/adr/0021-agent-surface-cli-first.md)
//! - Plan: [Wave 1, Step 14](../../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-14-rules---json-explain-explain---plain-test-can-import-1e)
//!   (from the saved graph, in milliseconds), and
//!   [Wave 2, Step 13](../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#213-step-13-worktree-aware-cache-and-the-eslint-plugin-2g)
//!   (the graph is the worktree-aware cache entry; a miss re-extracts)
//! - Requirement: [FR-CLI-02](../../../../docs/prd.md#fr-cli-02)
//!
//! Reads `--graph FILE`, else the cache entry for this worktree, commit and configuration
//! ([`crate::cache`]), extracting and writing it on a miss; adds the hypothetical edge (circular when `to` already reaches `from`),
//! and evaluates the dependency rules for that edge alone. Prints `yes`, or `no` with the deciding
//! rule, its comment and its `fix`; exits 0 for yes, 1 for no.
//!
//! Both paths are normalised the way the graph writes them (repository-relative, `/`, no `./`).
//! The edge takes the target's attributes (`dependencyTypes`, `license`, `coreModule`, ...) from
//! an edge to it already in the graph, so a rule on an npm dependency type answers as the gate
//! would. A target the graph has never seen is a local file when it exists on disk; anything else
//! exits 2, because answering `yes` for a module whose kind is unknown would be a silent false.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt::Write as _;

use clap::Args;
use rb_rules::validate::validate_dependency;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::cli::ConfigArgs;
use crate::context::Context;
use crate::pipeline::SAVED_GRAPH;
use crate::{Outcome, RunExit, cache, configure};

/// `can-import`.
#[derive(Debug, Clone, Default, Args)]
pub struct CanImportArgs {
    /// The file that would import
    pub from: String,
    /// The file it would import
    pub to: String,
    /// A graph document to answer from instead of the cache (such as .graph/cruise.json)
    #[arg(long, value_name = "FILE")]
    pub graph: Option<String>,
    /// Extract afresh, neither reading nor writing the cache
    #[arg(long)]
    pub no_cache: bool,
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
#[serde(rename_all = "camelCase")]
struct LightDependency {
    resolved: String,
    #[serde(default)]
    dependency_types: Option<Vec<String>>,
    #[serde(default)]
    core_module: Option<bool>,
    #[serde(default)]
    could_not_resolve: Option<bool>,
    #[serde(default)]
    license: Option<String>,
    #[serde(default)]
    instability: Option<f64>,
}

/// A path as the graph writes it: relative to the working folder, `/`-separated, no `./`.
fn normalise(ctx: &Context<'_>, path: &str) -> String {
    let as_path = std::path::Path::new(path);
    let relative = if as_path.is_absolute() {
        let cwd = ctx.cwd.canonicalize().unwrap_or_else(|_| ctx.cwd.clone());
        as_path
            .strip_prefix(&cwd)
            .or_else(|_| as_path.strip_prefix(&ctx.cwd))
            .unwrap_or(as_path)
            .to_string_lossy()
            .into_owned()
    } else {
        path.to_owned()
    };
    let mut text = relative.replace('\\', "/");
    while let Some(rest) = text.strip_prefix("./") {
        text = rest.to_owned();
    }
    text
}

/// The hypothetical edge's attributes: from an edge to `to` the graph already has, else a local
/// file when `to` exists on disk, else `None`.
fn target(ctx: &Context<'_>, graph: &LightGraph, to: &str) -> Option<Value> {
    let known = graph
        .modules
        .iter()
        .flat_map(|m| &m.dependencies)
        .find(|d| d.resolved == to);
    if let Some(d) = known {
        let mut edge = json!({
            "dependencyTypes": d.dependency_types.clone().unwrap_or_else(|| vec!["local".into()]),
            "coreModule": d.core_module.unwrap_or(false),
            "couldNotResolve": d.could_not_resolve.unwrap_or(false),
        });
        if let Some(license) = &d.license {
            edge["license"] = json!(license);
        }
        if let Some(instability) = d.instability {
            edge["instability"] = json!(instability);
        }
        return Some(edge);
    }
    let local = graph.modules.iter().any(|m| m.source == to)
        || (!to.starts_with("node_modules/") && ctx.resolve(to).is_file());
    local.then(
        || json!({ "dependencyTypes": ["local"], "coreModule": false, "couldNotResolve": false }),
    )
}

/// The saved graph, or exit 2 saying how to make one.
fn load(ctx: &Context<'_>, file: &str) -> Result<LightGraph, Outcome> {
    let text = std::fs::read_to_string(ctx.resolve(file)).map_err(|e| {
        Outcome::failed(
            RunExit::Untrustworthy,
            format!(
                "rulebearing can-import: cannot read {file}: {e}; run `rulebearing cruise -T json -f {SAVED_GRAPH}` first\n"
            ),
        )
    })?;
    serde_json::from_str(&text).map_err(|e| {
        Outcome::failed(
            RunExit::Untrustworthy,
            format!("rulebearing can-import: {file} is not a cruise result: {e}\n"),
        )
    })
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
    let (file, graph) = match &args.graph {
        Some(file) => match load(ctx, file) {
            Ok(g) => (file.clone(), g),
            Err(o) => return o,
        },
        None => match cache::graph(ctx, &config, None, args.no_cache)
            .and_then(|g| serde_json::from_str::<LightGraph>(&g.text).map_err(|e| e.to_string()))
        {
            Ok(g) => ("the graph of this worktree".to_owned(), g),
            Err(m) => {
                return Outcome::failed(
                    RunExit::Untrustworthy,
                    format!("rulebearing can-import: {m}\n"),
                );
            }
        },
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
    let (from_path, to_path) = (normalise(ctx, &args.from), normalise(ctx, &args.to));
    let Some(mut dependency) = target(ctx, &graph, &to_path) else {
        return Outcome::failed(
            RunExit::Untrustworthy,
            format!(
                "rulebearing can-import: {to_path} is not in {file} and is not a file here, so its kind of dependency is unknown; add it (or install it) first, or pass --graph with a graph that has it\n"
            ),
        );
    };
    let circular = from_path == to_path || reaches(&edges, &to_path, &from_path);
    let from = json!({ "source": from_path });
    for (key, value) in [
        ("module", json!(to_path)),
        ("resolved", json!(to_path)),
        ("dynamic", json!(false)),
        ("exoticallyRequired", json!(false)),
        ("followable", json!(true)),
        ("circular", json!(circular)),
        ("moduleSystem", json!("es6")),
    ] {
        dependency[key] = value;
    }
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
