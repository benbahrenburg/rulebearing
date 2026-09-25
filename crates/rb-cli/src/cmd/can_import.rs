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
//! rule, its comment and its `fix`; exits 0 for yes, 1 for no. With `--json` the same answer is
//! one JSON object: `verdict` (`yes` or `no`), `from`, `to`, `violations` (each deciding rule with
//! its `name`, `severity`, `id`, `comment` and `fix`) and `warnings` (the rules below error
//! severity that the edge would also match). The `id` is the one the gate would give the edge
//! ([ADR-0015](../../../../docs/adr/0015-stable-violation-id.md)): the edge's `dependencyKind` in the
//! graph when it is already there, else `import`. This is what `eslint-plugin-rulebearing` reads
//! ([Wave 2, Step 13](../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#213-step-13-worktree-aware-cache-and-the-eslint-plugin-2g)).
//!
//! Both paths are normalised the way the graph writes them (repository-relative, `/`, no `./`).
//! The edge takes the attributes that describe its target (`license`, `coreModule`, and of the
//! `dependencyTypes` only those that say what the target is: `npm`, `core`, `local`, the
//! `aliased` family, ... ([`describes_target`])) from an edge to it already in the graph, so a rule
//! on an npm dependency type answers as the gate would; how the other edge imports it (`require`,
//! `dynamic-import`, `type-only`) is not copied. The new edge is a static ES `import`: not dynamic,
//! not type-only, `import` among its types when the importer is TypeScript or JavaScript. A
//! target the graph has never seen is a local file when it exists on disk; anything else exits 2,
//! because answering `yes` for a module whose kind is unknown would be a silent false.
//!
//! The configuration's `options.knownViolations` apply as they do in the gate
//! ([`rb_rules::known::KnownSet`], [Wave 2, Step 10](../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#210-step-10-reporters-and-baseline-semantics-2e)):
//! a rule an entry matches (by the edge's id, or by its shape, with the cycle the gate would find)
//! is softened to `ignore`, so the verdict is `yes`, and it is listed among the `warnings` with
//! severity `ignore` and `"known": true`. An entry past its `expires` date is not honoured, so the
//! rule still blocks, as it fails the gate.
//! The cross-language keys read each module's `language`, `namespaces`, `project` and the
//! assemblies of its code-layer types from the same graph, and the hypothetical edge is an
//! `import` ([Wave 2, Step 8](../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#28-step-8-cross-language-rule-additions-per-language-dependencytypes-license-moreunstable-2d)).

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::fmt::Write as _;

use clap::Args;
use rb_model::DependencyType;
use rb_model::violation_id::violation_id;
use rb_rules::graph::indexed::IndexedGraph;
use rb_rules::known::KnownSet;
use rb_rules::matchers::{Facts, ModuleFacts};
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
    /// Print the answer as JSON: the verdict and each deciding rule with its id, comment and fix
    #[arg(long)]
    pub json: bool,
    /// Configuration
    #[command(flatten)]
    pub config: ConfigArgs,
}

/// Only what `can-import` reads from a saved graph, so a large result loads fast.
#[derive(Debug, Deserialize)]
struct LightGraph {
    modules: Vec<LightModule>,
    #[serde(default)]
    code: Option<LightCode>,
}

#[derive(Debug, Deserialize)]
struct LightModule {
    source: String,
    #[serde(default)]
    dependencies: Vec<LightDependency>,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    namespaces: Option<Vec<String>>,
    #[serde(default)]
    project: Option<String>,
}

/// The code layer's types, for the assembly each file declares.
#[derive(Debug, Deserialize)]
struct LightCode {
    #[serde(default)]
    types: Vec<LightType>,
}

#[derive(Debug, Deserialize)]
struct LightType {
    #[serde(default)]
    file: Option<String>,
    #[serde(default)]
    files: Vec<String>,
    #[serde(default)]
    assembly: Option<String>,
}

/// The facts the cross-language keys read, from the light graph.
fn facts(graph: &LightGraph) -> ModuleFacts {
    let mut assemblies: HashMap<&str, BTreeSet<&str>> = HashMap::new();
    for ty in graph.code.iter().flat_map(|c| &c.types) {
        if let Some(assembly) = ty.assembly.as_deref() {
            for file in ty.file.iter().chain(&ty.files) {
                assemblies.entry(file).or_default().insert(assembly);
            }
        }
    }
    let mut out = ModuleFacts::default();
    for m in &graph.modules {
        out.insert(
            m.source.clone(),
            Facts {
                language: m.language.clone(),
                namespaces: m.namespaces.clone(),
                project: m.project.clone(),
                assemblies: assemblies
                    .get(m.source.as_str())
                    .map(|a| a.iter().map(|s| (*s).to_owned()).collect())
                    .unwrap_or_default(),
            },
        );
    }
    out
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
    #[serde(default)]
    dependency_kind: Option<String>,
}

/// A path as the graph writes it: relative to the working folder, `/`-separated, no `./`.
pub fn normalise(ctx: &Context<'_>, path: &str) -> String {
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

/// Whether a dependency type says what the target is (a package, a built-in, a local file, how
/// it was resolved) rather than how one edge imports it (`import`, `require`, `dynamic-import`,
/// `type-only`, a triple-slash directive, ...). Only these carry over from another edge to the
/// same target.
pub fn describes_target(kind: DependencyType) -> bool {
    use DependencyType as T;
    matches!(
        kind,
        T::AliasedSubpathImport
            | T::AliasedTsconfigBaseUrl
            | T::AliasedTsconfigPaths
            | T::AliasedTsconfig
            | T::AliasedWebpack
            | T::AliasedWorkspace
            | T::Aliased
            | T::Core
            | T::Deprecated
            | T::Local
            | T::Localmodule
            | T::NpmBundled
            | T::NpmDev
            | T::NpmNoPkg
            | T::NpmOptional
            | T::NpmPeer
            | T::NpmUnknown
            | T::Npm
            | T::Undetermined
            | T::Unknown
            | T::Project
            | T::Package
            | T::Framework
            | T::Unresolved
            | T::Stdlib
            | T::Site
    )
}

/// Whether a file of this language (or, when the graph records none, this name) writes an ES
/// `import` whose edge carries the `import` dependency type.
pub fn writes_es_import(language: Option<&str>, path: &str) -> bool {
    if let Some(language) = language {
        return matches!(language, "typescript" | "javascript");
    }
    let extension = path.rsplit_once('.').map_or("", |(_, e)| e);
    matches!(
        extension,
        "ts" | "tsx" | "mts" | "cts" | "js" | "jsx" | "mjs" | "cjs" | "vue" | "svelte"
    )
}

/// The target-describing types of `types`, in order, then `import` when the importer writes an
/// ES import; `local` when no type describes the target.
fn hypothetical_types(types: Option<&[String]>, es_import: bool) -> Vec<String> {
    let mut out: Vec<String> = types
        .unwrap_or_default()
        .iter()
        .filter(|t| t.parse::<DependencyType>().is_ok_and(describes_target))
        .cloned()
        .collect();
    if out.is_empty() {
        out.push(DependencyType::Local.as_str().to_owned());
    }
    if es_import {
        out.push(DependencyType::Import.as_str().to_owned());
    }
    out
}

/// The hypothetical edge's attributes: from an edge to `to` the graph already has, else a local
/// file when `to` exists on disk, else `None`.
fn target(ctx: &Context<'_>, graph: &LightGraph, to: &str, es_import: bool) -> Option<Value> {
    let known = graph
        .modules
        .iter()
        .flat_map(|m| &m.dependencies)
        .find(|d| d.resolved == to);
    if let Some(d) = known {
        let mut edge = json!({
            "dependencyTypes": hypothetical_types(d.dependency_types.as_deref(), es_import),
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
    local.then(|| {
        json!({
            "dependencyTypes": hypothetical_types(None, es_import),
            "coreModule": false,
            "couldNotResolve": false,
        })
    })
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

/// The graph `can-import` answers from, and how to name it: `--graph FILE`, else this
/// worktree's cached graph.
fn graph_for(
    ctx: &mut Context<'_>,
    args: &CanImportArgs,
    config: &rb_config::Config,
) -> Result<(String, LightGraph), Outcome> {
    match &args.graph {
        Some(file) => load(ctx, file).map(|g| (file.clone(), g)),
        None => cache::graph(ctx, config, None, args.no_cache)
            .and_then(|g| serde_json::from_str::<LightGraph>(&g.text).map_err(|e| e.to_string()))
            .map(|g| ("the graph of this worktree".to_owned(), g))
            .map_err(|m| {
                Outcome::failed(
                    RunExit::Untrustworthy,
                    format!("rulebearing can-import: {m}\n"),
                )
            }),
    }
}

/// Runs `can-import`.
pub fn run(ctx: &mut Context<'_>, args: &CanImportArgs) -> Outcome {
    let config = match configure::required(ctx, &args.config) {
        Ok(c) => c,
        Err(o) => return o,
    };
    let (file, graph) = match graph_for(ctx, args, &config) {
        Ok(found) => found,
        Err(o) => return o,
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
    let from_language = graph
        .modules
        .iter()
        .find(|m| m.source == from_path)
        .and_then(|m| m.language.as_deref());
    let es_import = writes_es_import(from_language, &from_path);
    let Some(mut dependency) = target(ctx, &graph, &to_path, es_import) else {
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
        ("typeOnly", json!(false)),
        ("exoticallyRequired", json!(false)),
        ("followable", json!(true)),
        ("circular", json!(circular)),
        ("moduleSystem", json!("es6")),
        ("dependencyKind", json!("import")),
    ] {
        dependency[key] = value;
    }
    let verdict = validate_dependency(
        &config.rules.dependencies,
        &from,
        &dependency,
        &facts(&graph),
    );
    let rules = verdict
        .get("rules")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    // The gate keys an edge's id on the kind the graph records for it (empty when it records
    // none); an edge the graph does not have yet is the hypothetical `import`.
    let kind = graph
        .modules
        .iter()
        .find(|m| m.source == from_path)
        .and_then(|m| m.dependencies.iter().find(|d| d.resolved == to_path))
        .map_or_else(
            || "import".to_owned(),
            |d| d.dependency_kind.clone().unwrap_or_default(),
        );
    let rules = soften_known(
        &config.known_violations,
        ctx.today,
        &graph,
        Edge {
            from: &from_path,
            to: &to_path,
            kind: &kind,
            circular,
        },
        rules,
    );
    let decisions = decide(&config, &rules, &from_path, &to_path, &kind);
    let blocking = decisions.iter().filter(|d| d.blocks()).count();
    // `no` is one finding for the gate to count, whichever number of rules decide it.
    let code = RunExit::Violations(u64::from(blocking > 0)).code();
    let stdout = if args.json {
        as_json(&from_path, &to_path, &decisions)
    } else {
        as_text(&decisions)
    };
    Outcome {
        stdout,
        stderr: String::new(),
        code,
    }
}

/// Each matched rule with the id the gate would give the edge, and the rule's comment and fix.
fn decide(
    config: &rb_config::Config,
    rules: &[(Value, bool)],
    from: &str,
    to: &str,
    kind: &str,
) -> Vec<Decision> {
    rules
        .iter()
        .map(|(r, known)| {
            let name = r["name"].as_str().unwrap_or_default().to_owned();
            let rule = config
                .rules
                .all_dependency_rules()
                .map(|(_, rule)| rule)
                .find(|x| x.name() == name);
            Decision {
                id: violation_id(&name, from, to, kind),
                severity: r["severity"].as_str().unwrap_or_default().to_owned(),
                comment: rule.and_then(|x| x.meta.comment.clone()),
                fix: rule.and_then(|x| x.meta.fix.clone()),
                known: *known,
                name,
            }
        })
        .collect()
}

/// The hypothetical edge, as the known violations match it.
#[derive(Debug, Clone, Copy)]
struct Edge<'a> {
    from: &'a str,
    to: &'a str,
    kind: &'a str,
    circular: bool,
}

/// The cycle the gate would record for the edge once it exists: dependency-cruiser's
/// `getCycle` over the graph with the edge added.
fn cycle_with(graph: &LightGraph, edge: Edge<'_>, types: &Value) -> Vec<Value> {
    let mut modules: Vec<Value> = graph
        .modules
        .iter()
        .map(|m| {
            let dependencies: Vec<Value> = m
                .dependencies
                .iter()
                .map(|d| json!({ "resolved": d.resolved, "dependencyTypes": d.dependency_types }))
                .collect();
            json!({ "source": m.source, "dependencies": dependencies })
        })
        .collect();
    let hypothetical = json!({ "resolved": edge.to, "dependencyTypes": types });
    match modules.iter_mut().find(|m| m["source"] == edge.from) {
        Some(m) => {
            if let Some(Value::Array(list)) = m.get_mut("dependencies")
                && !list.iter().any(|d| d["resolved"] == edge.to)
            {
                list.push(hypothetical);
            }
        }
        None => modules.push(json!({ "source": edge.from, "dependencies": [hypothetical] })),
    }
    IndexedGraph::new(&modules, "source").cycle(edge.from, edge.to)
}

/// The rules the edge matches, each with whether a known violation softened it: the entries
/// applied through [`KnownSet::soften_modules`], as `cruise` applies them to the same edge.
fn soften_known(
    entries: &[rb_config::model::KnownViolation],
    today: chrono::NaiveDate,
    graph: &LightGraph,
    edge: Edge<'_>,
    rules: Vec<Value>,
) -> Vec<(Value, bool)> {
    if entries.is_empty() || rules.is_empty() {
        return rules.into_iter().map(|r| (r, false)).collect();
    }
    let mut dependency = json!({
        "resolved": edge.to,
        "valid": false,
        "rules": rules,
    });
    if !edge.kind.is_empty() {
        dependency["dependencyKind"] = json!(edge.kind);
    }
    if edge.circular {
        let cycle = cycle_with(graph, edge, &json!([]));
        if !cycle.is_empty() {
            dependency["cycle"] = Value::Array(cycle);
        }
    }
    let mut modules = vec![json!({
        "source": edge.from,
        "valid": true,
        "dependencies": [dependency],
    })];
    KnownSet::new(entries, today, &mut Vec::new()).soften_modules(&mut modules);
    let softened = modules[0]["dependencies"][0]["rules"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    softened
        .into_iter()
        .map(|r| {
            let known = r["severity"] == "ignore";
            (r, known)
        })
        .collect()
}

/// One rule the hypothetical edge matches, with what the gate would report for it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Decision {
    name: String,
    severity: String,
    id: String,
    comment: Option<String>,
    fix: Option<String>,
    /// Softened to `ignore` by a known violation.
    known: bool,
}

impl Decision {
    /// Only an error-severity rule says `no`, as only it fails the gate.
    fn blocks(&self) -> bool {
        self.severity == "error"
    }
}

/// `yes` with the rules that only warn, or `no` with each deciding rule, its comment and `fix`.
fn as_text(decisions: &[Decision]) -> String {
    let mut out = String::new();
    if decisions.iter().any(Decision::blocks) {
        out.push_str("no\n");
        for d in decisions.iter().filter(|d| d.blocks()) {
            let _ = writeln!(out, "  rule: {}", d.name);
            if let Some(comment) = &d.comment {
                let _ = writeln!(out, "  why: {comment}");
            }
            if let Some(fix) = &d.fix {
                let _ = writeln!(out, "  fix: {fix}");
            }
        }
    } else {
        out.push_str("yes\n");
        for d in decisions {
            if d.known {
                let _ = writeln!(out, "  (known violation: {} {}, baselined)", d.id, d.name);
            } else {
                let _ = writeln!(out, "  (warns: {} {})", d.severity, d.name);
            }
        }
    }
    out
}

/// The answer as one JSON object, fields in a fixed order, ending in a newline.
fn as_json(from: &str, to: &str, decisions: &[Decision]) -> String {
    let entry = |d: &Decision| {
        let mut value = json!({ "name": d.name, "severity": d.severity, "id": d.id });
        if let Some(comment) = &d.comment {
            value["comment"] = json!(comment);
        }
        if let Some(fix) = &d.fix {
            value["fix"] = json!(fix);
        }
        if d.known {
            value["known"] = json!(true);
        }
        value
    };
    let blocks = decisions.iter().any(Decision::blocks);
    let answer = json!({
        "verdict": if blocks { "no" } else { "yes" },
        "from": from,
        "to": to,
        "violations": decisions.iter().filter(|d| d.blocks()).map(entry).collect::<Vec<_>>(),
        "warnings": decisions.iter().filter(|d| !d.blocks()).map(entry).collect::<Vec<_>>(),
    });
    let mut out = serde_json::to_string_pretty(&answer).unwrap_or_default();
    out.push('\n');
    out
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

    fn decision(name: &str, severity: &str, fix: Option<&str>) -> Decision {
        Decision {
            name: name.into(),
            severity: severity.into(),
            id: violation_id(name, "a.ts", "b.ts", "import"),
            comment: fix.map(|_| "why".to_owned()),
            fix: fix.map(str::to_owned),
            known: false,
        }
    }

    #[test]
    fn text_says_no_with_the_deciding_rules_and_yes_with_the_warnings() {
        let no = as_text(&[
            decision("hard", "error", Some("Do this")),
            decision("soft", "warn", None),
        ]);
        assert_eq!(no, "no\n  rule: hard\n  why: why\n  fix: Do this\n");
        assert_eq!(
            as_text(&[decision("soft", "warn", None)]),
            "yes\n  (warns: warn soft)\n"
        );
        assert_eq!(as_text(&[]), "yes\n");
    }

    #[test]
    fn json_carries_the_verdict_ids_and_fix() {
        let text = as_json(
            "a.ts",
            "b.ts",
            &[
                decision("hard", "error", Some("Do this")),
                decision("soft", "info", None),
            ],
        );
        let value: Value = serde_json::from_str(&text).unwrap_or_default();
        assert_eq!(value["verdict"], "no");
        assert_eq!(value["from"], "a.ts");
        assert_eq!(value["to"], "b.ts");
        assert_eq!(value["violations"][0]["name"], "hard");
        assert_eq!(value["violations"][0]["fix"], "Do this");
        assert_eq!(value["violations"][0]["comment"], "why");
        // The fixed vector of ADR-0015, so the plugin's id is the gate's.
        assert_eq!(
            violation_id(
                "no-cross-app-imports",
                "apps/web/src/x.ts",
                "apps/worker/src/y.ts",
                "import"
            ),
            "RB-a85578a3"
        );
        assert_eq!(
            value["violations"][0]["id"],
            violation_id("hard", "a.ts", "b.ts", "import")
        );
        assert_eq!(value["warnings"][0]["name"], "soft");
        assert!(value["warnings"][0].get("fix").is_none());
        assert!(text.ends_with("}\n"));
        let yes: Value = serde_json::from_str(&as_json("a.ts", "b.ts", &[])).unwrap_or_default();
        assert_eq!(yes["verdict"], "yes");
        assert_eq!(yes["violations"], json!([]));
    }

    #[test]
    fn only_the_types_that_describe_the_target_carry_over() {
        let form = [
            DependencyType::Import,
            DependencyType::Require,
            DependencyType::DynamicImport,
            DependencyType::TypeOnly,
            DependencyType::TypeImport,
            DependencyType::Export,
            DependencyType::ExoticRequire,
            DependencyType::TripleSlashFileReference,
            DependencyType::AmdDefine,
            DependencyType::AmdRequire,
            DependencyType::AmdExoticRequire,
            DependencyType::Jsdoc,
            DependencyType::JsdocBracketImport,
            DependencyType::TripleSlashAmdDependency,
            DependencyType::TripleSlashDirective,
            DependencyType::TripleSlashTypeReference,
            DependencyType::ProcessGetBuiltinModule,
            DependencyType::ImportEquals,
            DependencyType::JsdocImportTag,
            DependencyType::PreCompilationOnly,
            DependencyType::SignatureOnly,
            DependencyType::TestOnly,
            DependencyType::Dynamic,
        ];
        for kind in DependencyType::ALL {
            assert_eq!(describes_target(*kind), !form.contains(kind), "{kind:?}");
        }
        let types = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            hypothetical_types(Some(&types(&["npm", "require"])), true),
            ["npm", "import"]
        );
        assert_eq!(
            hypothetical_types(Some(&types(&["local", "type-only", "type-import"])), false),
            ["local"]
        );
        assert_eq!(
            hypothetical_types(Some(&types(&["dynamic-import", "not-a-type"])), true),
            ["local", "import"]
        );
        assert_eq!(hypothetical_types(None, false), ["local"]);
    }

    #[test]
    fn es_imports_are_written_by_typescript_and_javascript() {
        assert!(writes_es_import(Some("typescript"), "a.py"));
        assert!(writes_es_import(Some("javascript"), "a"));
        assert!(!writes_es_import(Some("python"), "a.ts"));
        assert!(!writes_es_import(Some("dotnet"), "a.cs"));
        for path in ["a.ts", "b/c.tsx", "d.mjs", "e.vue"] {
            assert!(writes_es_import(None, path), "{path}");
        }
        for path in ["a.py", "b.cs", "Makefile"] {
            assert!(!writes_es_import(None, path), "{path}");
        }
    }

    #[test]
    fn a_known_entry_softens_the_rule_it_matches_unless_expired() -> Result<(), serde_json::Error> {
        let graph: LightGraph = serde_json::from_value(json!({
            "modules": [
                { "source": "a.ts", "dependencies": [{ "resolved": "b.ts" }] },
                { "source": "b.ts", "dependencies": [] }
            ]
        }))?;
        let id = violation_id("no-b", "b.ts", "a.ts", "import");
        let rule = || vec![json!({ "name": "no-b", "severity": "error" })];
        let edge = Edge {
            from: "b.ts",
            to: "a.ts",
            kind: "import",
            circular: true,
        };
        let today = chrono::NaiveDate::from_ymd_opt(2026, 9, 24).unwrap_or_default();
        let entries = |value: Value| -> Vec<rb_config::model::KnownViolation> {
            serde_json::from_value(value).unwrap_or_default()
        };
        let known = soften_known(&entries(json!([{ "id": id }])), today, &graph, edge, rule());
        assert_eq!(known[0].0["severity"], "ignore");
        assert!(known[0].1);
        let past = entries(json!([{ "id": id, "expires": "2026-01-01" }]));
        assert_eq!(
            soften_known(&past, today, &graph, edge, rule())[0].0["severity"],
            "error"
        );
        // A cycle entry matches on the modules of the cycle the gate would find.
        let cycle = entries(
            json!([{ "type": "cycle", "from": "x", "to": "y", "rule": { "name": "no-b" },
            "cycle": [{ "name": "a.ts" }, { "name": "b.ts" }] }]),
        );
        assert!(soften_known(&cycle, today, &graph, edge, rule())[0].1);
        assert!(!soften_known(&[], today, &graph, edge, rule())[0].1);
        let steps: Vec<String> = cycle_with(&graph, edge, &json!([]))
            .iter()
            .filter_map(|s| s["name"].as_str().map(str::to_owned))
            .collect();
        assert_eq!(steps, ["a.ts", "b.ts"]);
        let fresh = Edge {
            from: "new.ts",
            to: "new.ts",
            kind: "import",
            circular: true,
        };
        assert_eq!(
            cycle_with(&graph, fresh, &json!([])).len(),
            1,
            "a self-import"
        );
        Ok(())
    }
}
