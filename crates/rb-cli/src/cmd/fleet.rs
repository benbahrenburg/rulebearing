//! `rulebearing fleet`: one run, one report across sibling repositories, with each repository's
//! own rules and the manifest's fleet-wide rules over the union of their graphs, joined by the
//! edges `edges.yaml` declares.
//!
//! - Source: [design § The architect's hat](../../../../docs/artifacts/design.md#the-architects-hat-across-repos-and-across-time)
//! - Architecture: [The graph document](../../../../docs/architecture.md#the-graph-document)
//! - Decisions: [ADR-0077](../../../../docs/adr/0077-fleet-and-declared-edges.md) (what a fleet
//!   run does, in order), [ADR-0014](../../../../docs/adr/0014-no-invented-cross-language-edges.md)
//!   (declared edges are marked, never inferred),
//!   [ADR-0010](../../../../docs/adr/0010-crate-layout-and-extractor-boundary.md) (`rb-rules`
//!   evaluates the union unchanged),
//!   [ADR-0031](../../../../docs/adr/0031-a-saved-result-carries-what-the-exit-code-counts.md)
//!   (the exit code from the document alone)
//! - Plan: [plan 0004, Steps 11 and 12](../../../../docs/plans/pending/0004-wave-4-reach.md#24-steps-for-sub-wave-4d-fleet-and-declared-edges)
//! - Requirement: [FR-REACH-03](../../../../docs/prd.md#fr-reach-03)
//!
//! | Stage | What happens |
//! | --- | --- |
//! | manifest | `fleet.yaml` read and checked; its `rules` loaded as a native configuration (exit 3 when either is wrong) |
//! | repositories | each cruised in its own folder with its own configuration and cache, `-T json`, in parallel; a missing folder, or a repository run that exits 2 or 3, stops the fleet with that code |
//! | union | every module path prefixed with `<name>/` ([`prefix`]); the documents joined ([`union`]) |
//! | declared edges | `edges.yaml` (beside the manifest, or `--edges`) and the OpenAPI pairs it names, added as `declared` dependencies (a malformed file exits 2) |
//! | fleet rules | evaluated by `rb-rules` over the union, liveness strict |
//! | report | each repository's findings folded in ([`fold`]); `summary.fleet` and `summary.declared` written; reported as `fmt` reports a saved result |

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use clap::Args;
use rayon::prelude::*;
use rb_config::fleet::{FleetManifest, FleetRepo};
use rb_ingest::declared::{self, EdgesFile};
use rb_model::{
    DeclaredReceipt, FleetReceipt, GraphDocument, MiniDependency, RepoReceipt, ViolationType,
};
use serde_json::Value;

use crate::cli::{CacheStrategyArg, ColorChoice, ConfigArgs, CruiseArgs, EXIT_CODES, FmtArgs};
use crate::context::Context;
use crate::exit::ExitCodeMode;
use crate::pipeline::{self, RunOptions};
use crate::progress::Progress;
use crate::{Outcome, RunExit, configure};

/// The file declared edges are read from, beside the manifest, when `--edges` is not given.
pub const EDGES_FILE: &str = "edges.yaml";

/// `fleet`.
#[derive(Debug, Clone, Args)]
#[command(after_help = EXIT_CODES)]
pub struct FleetArgs {
    /// The workspace manifest: the repositories and the fleet-wide rules
    #[arg(value_name = "MANIFEST", default_value = "fleet.yaml")]
    pub manifest: String,
    /// Declared edges (default: edges.yaml beside the manifest, when there is one)
    #[arg(long, value_name = "FILE")]
    pub edges: Option<String>,
    /// Output type: any of cruise's
    #[arg(short = 'T', long, value_name = "TYPE", default_value = "err")]
    pub output_type: String,
    /// File to write output to; - for stdout
    #[arg(short = 'f', long, value_name = "FILE", default_value = "-")]
    pub output_to: String,
    /// Cruise every repository without its cache
    #[arg(long)]
    pub no_cache: bool,
    /// How each repository's cache finds what changed: metadata (the default) or content
    #[arg(long, value_enum, value_name = "STRATEGY", conflicts_with = "no_cache")]
    pub cache_strategy: Option<CacheStrategyArg>,
    /// json: strip every Rulebearing addition, the declared edges' marks and the fleet receipt
    /// included, so the output validates against cruise-result 18.2.0
    #[arg(long)]
    pub strict_schema: bool,
    /// agent: the most violations shown per rule
    #[arg(long, value_name = "N")]
    pub max_findings: Option<usize>,
    /// Colour terminal output
    #[arg(long, value_enum, default_value_t = ColorChoice::Auto)]
    pub color: ColorChoice,
    /// How the error count becomes the exit code: default (the count) or strict (10 + the count)
    #[arg(long, value_enum, value_name = "MODE", default_value_t = ExitCodeMode::Default)]
    pub exit_code_mode: ExitCodeMode,
    /// Let a repository read in source mode (--mode source) decide the exit code; never in CI
    /// (ADR-0011)
    #[arg(long)]
    pub allow_approximate_gate: bool,
}

/// One repository's own run.
#[derive(Debug, Clone)]
pub struct RepoRun {
    /// The manifest's name for it.
    pub name: String,
    /// Its JSON result, paths as the repository names them.
    pub document: GraphDocument,
    /// What `summary.fleet.perRepo` records for it.
    pub receipt: RepoReceipt,
    /// What its run printed on stderr.
    pub warnings: String,
}

fn failed(code: RunExit, message: &str) -> Outcome {
    Outcome::failed(code, format!("rulebearing fleet: {message}\n"))
}

/// What a fleet run reads before it cruises anything.
struct Prepared {
    manifest: FleetManifest,
    config: rb_config::Config,
    edges: Option<EdgesFile>,
    folders: Vec<PathBuf>,
}

/// Reads the manifest, its rules, the declared edges and the repositories' folders.
///
/// # Errors
/// The [`Outcome`]: exit 3 for the manifest and its rules, 2 for the declared edges and a
/// missing folder.
fn prepare(
    ctx: &Context<'_>,
    manifest_file: &str,
    edges_file: Option<&str>,
) -> Result<Prepared, Outcome> {
    let manifest_path = ctx.resolve(manifest_file);
    let dir = manifest_path
        .parent()
        .map_or_else(|| ctx.cwd.clone(), Path::to_path_buf);
    let text = std::fs::read_to_string(&manifest_path).map_err(|e| {
        failed(
            RunExit::InvalidConfig,
            &format!("cannot read the manifest {manifest_file}: {e}"),
        )
    })?;
    let invalid = |e: rb_config::ConfigError| {
        failed(RunExit::InvalidConfig, &format!("{manifest_file}: {e}"))
    };
    let manifest = rb_config::fleet::read_manifest(&text).map_err(invalid)?;
    let config = rb_config::fleet::rules_config(&manifest, &dir).map_err(invalid)?;
    let untrustworthy = |message: String| failed(RunExit::Untrustworthy, &message);
    let edges = read_edges(ctx, edges_file, &dir).map_err(untrustworthy)?;
    let folders = folders(&manifest, &dir).map_err(untrustworthy)?;
    Ok(Prepared {
        manifest,
        config,
        edges,
        folders,
    })
}

/// Runs `fleet`.
pub fn run(ctx: &mut Context<'_>, args: &FleetArgs) -> Outcome {
    let prepared = match prepare(ctx, &args.manifest, args.edges.as_deref()) {
        Ok(prepared) => prepared,
        Err(outcome) => return outcome,
    };
    let runs = match cruise_all(ctx, args, &prepared.manifest.repos, &prepared.folders) {
        Ok(runs) => runs,
        Err(outcome) => return outcome,
    };
    let mut stderr = String::new();
    for run in &runs {
        for line in run.warnings.lines() {
            let _ = writeln!(stderr, "{}: {line}", run.name);
        }
    }
    let document = match evaluate(ctx, &prepared, runs, &mut stderr) {
        Ok(document) => document,
        Err((code, message)) => {
            return Outcome::failed(code, format!("{stderr}rulebearing fleet: {message}\n"));
        }
    };
    let report = FmtArgs {
        output_type: args.output_type.clone(),
        output_to: args.output_to.clone(),
        exit_code: true,
        exit_code_mode: args.exit_code_mode,
        allow_approximate_gate: args.allow_approximate_gate,
        strict_schema: args.strict_schema,
        max_findings: args.max_findings,
        color: args.color,
        ..FmtArgs::default()
    };
    ctx.usage.fired(
        document
            .summary
            .violations
            .iter()
            .map(|v| v.rule.name.clone()),
    );
    let outcome = crate::cmd::fmt::report(ctx, &report, document);
    Outcome {
        stderr: format!("{stderr}{}", outcome.stderr),
        ..outcome
    }
}

/// The union of the repositories' runs with the declared edges, evaluated with the fleet rules,
/// each repository's findings folded in, and the two receipts written. Skipped declared edges
/// and vacuous fleet rules are written to `stderr`.
///
/// # Errors
/// The exit and the message when the declared edges cannot be read or the rules evaluated.
fn evaluate(
    ctx: &Context<'_>,
    prepared: &Prepared,
    runs: Vec<RepoRun>,
    stderr: &mut String,
) -> Result<GraphDocument, (RunExit, String)> {
    let prefixed: Vec<(String, GraphDocument)> = runs
        .iter()
        .map(|run| {
            let mut document = run.document.clone();
            prefix(&run.name, &mut document);
            (run.name.clone(), document)
        })
        .collect();
    let mut joined = union(&prefixed);
    rb_rules::reset(&mut joined);
    let declared = prepared
        .edges
        .as_ref()
        .map(|edges| {
            declare(
                edges,
                &prepared.manifest,
                &prepared.folders,
                &prefixed,
                &mut joined,
            )
        })
        .transpose()
        .map_err(|message| (RunExit::Untrustworthy, message))?;
    for skipped in declared.iter().flat_map(|d| d.skipped.iter().flatten()) {
        let _ = writeln!(
            stderr,
            "warning: declared edge {} -> {} not added: {}",
            skipped.from, skipped.to, skipped.reason
        );
    }
    let options = RunOptions {
        liveness: true,
        ..RunOptions::default()
    };
    let mut progress = Progress::new(None);
    let evaluation =
        pipeline::evaluate_document(ctx, &prepared.config, joined, &options, &mut progress)
            .map_err(|e| (e.exit(), format!("fleet rules: {e}")))?
            .into_evaluation();
    for vacuous in &evaluation.vacuous {
        let _ = writeln!(
            stderr,
            "{}",
            crate::cmd::cruise::vacuous_message(&vacuous.name, &vacuous.side, true)
        );
    }
    let mut document = evaluation.document;
    fold(&mut document, &prefixed);
    document.summary.fleet = Some(FleetReceipt {
        repos: runs.len() as u64,
        per_repo: runs
            .into_iter()
            .map(|run| (run.name, run.receipt))
            .collect(),
    });
    document.summary.declared = declared;
    Ok(document)
}

/// `config lint --fleet`: the manifest's rules as a configuration, and a `stale-declared-edge`
/// finding for each declared `from` or `to` that no longer exists (not a module of `graph` when
/// one is given, else not a file of its repository) and each OpenAPI server document that is not
/// a file of its repository.
///
/// # Errors
/// The [`Outcome`] when the manifest, its rules or the declared edges cannot be read, or a
/// repository's folder is missing.
pub fn lint(
    ctx: &Context<'_>,
    manifest_file: &str,
    edges_file: Option<&str>,
    graph: Option<&GraphDocument>,
) -> Result<(rb_config::Config, Vec<rb_config::lint::Finding>), Outcome> {
    let prepared = prepare(ctx, manifest_file, edges_file)?;
    let mut findings = Vec::new();
    if let Some(edges) = &prepared.edges {
        let repos = &prepared.manifest.repos;
        let file = |path: &str| {
            locate(repos, path)
                .is_some_and(|(index, inside)| prepared.folders[index].join(inside).is_file())
        };
        let modules: Option<std::collections::BTreeSet<&str>> =
            graph.map(|g| g.modules.iter().map(|m| m.source.as_str()).collect());
        let module = |path: &str| {
            modules
                .as_ref()
                .map_or_else(|| file(path), |m| m.contains(path))
        };
        for (index, side, path) in declared::stale(edges, &module, &file) {
            let (list, missing) = match (side, graph) {
                ("server", _) => ("openapi", "is not a file of its repository"),
                (_, Some(_)) => ("declared", "is not a module of the graph"),
                (_, None) => ("declared", "is not a file of its repository"),
            };
            findings.push(rb_config::lint::Finding {
                rule: format!("{list}[{index}].{side}"),
                code: "stale-declared-edge",
                message: format!("`{path}` {missing}: update the declared edge or delete it"),
            });
        }
    }
    Ok((prepared.config, findings))
}

/// `edges.yaml`: the file `--edges` names, else the one beside the manifest when there is one.
///
/// # Errors
/// A message naming the file when it cannot be read or is not the format.
fn read_edges(
    ctx: &Context<'_>,
    edges_file: Option<&str>,
    dir: &Path,
) -> Result<Option<EdgesFile>, String> {
    let path = if let Some(file) = edges_file {
        ctx.resolve(file)
    } else {
        let beside = dir.join(EDGES_FILE);
        if !beside.is_file() {
            return Ok(None);
        }
        beside
    };
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read the declared edges {}: {e}", path.display()))?;
    declared::read_edges(&text)
        .map(Some)
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// Each repository's folder, from the manifest's folder.
///
/// # Errors
/// A message naming the first repository whose folder is missing.
fn folders(manifest: &FleetManifest, dir: &Path) -> Result<Vec<PathBuf>, String> {
    manifest
        .repos
        .iter()
        .map(|repo| {
            let folder = dir.join(&repo.path);
            if folder.is_dir() {
                Ok(folder)
            } else {
                Err(format!(
                    "repository `{}`: {} is not a folder; fix its `path` in the manifest or clone it there",
                    repo.name,
                    folder.display()
                ))
            }
        })
        .collect()
}

/// Every repository's own run, in manifest order, on a pool as wide as the machine.
///
/// # Errors
/// The first failing repository's [`Outcome`], in manifest order.
fn cruise_all(
    ctx: &Context<'_>,
    args: &FleetArgs,
    repos: &[FleetRepo],
    folders: &[PathBuf],
) -> Result<Vec<RepoRun>, Outcome> {
    let width = std::thread::available_parallelism()
        .map_or(1, std::num::NonZeroUsize::get)
        .min(repos.len())
        .max(1);
    let (today, timestamp, ci) = (ctx.today, ctx.timestamp.clone(), ctx.usage.in_ci());
    let each = |(repo, folder): (&FleetRepo, &PathBuf)| {
        let mut stdin = std::io::empty();
        let mut repo_ctx = Context {
            cwd: folder.clone(),
            stdin: &mut stdin,
            today,
            timestamp: timestamp.clone(),
            color_terminal: false,
            warm: None,
            // Each repository's run counts on its own recorder; the fleet records what fired.
            usage: crate::usage::Recorder::new(ci),
        };
        cruise_one(&mut repo_ctx, args, repo)
    };
    let pairs: Vec<(&FleetRepo, &PathBuf)> = repos.iter().zip(folders).collect();
    let runs: Vec<Result<RepoRun, Outcome>> =
        match rayon::ThreadPoolBuilder::new().num_threads(width).build() {
            Ok(pool) => pool.install(|| pairs.into_par_iter().map(each).collect()),
            Err(_) => pairs.into_iter().map(each).collect(),
        };
    runs.into_iter().collect()
}

/// The cruise arguments for one repository: `-T json`, its configuration, and its cache as
/// ADR-0077 lays it out.
pub fn repo_args(args: &FleetArgs, repo: &FleetRepo, configured: Option<&str>) -> CruiseArgs {
    let mut cruise = CruiseArgs {
        config: ConfigArgs {
            config: repo.config.clone(),
            ..ConfigArgs::default()
        },
        output_type: Some("json".into()),
        no_progress: true,
        ..CruiseArgs::default()
    };
    if args.no_cache {
        cruise.no_cache = true;
    } else if configured.is_none() || args.cache_strategy.is_some() {
        // The configuration's own folder is kept; without one, the default folder.
        cruise.cache = Some(configured.unwrap_or_default().to_owned());
        cruise.cache_strategy = args.cache_strategy;
    }
    cruise
}

/// One repository's run, in its own folder.
///
/// # Errors
/// The [`Outcome`] to stop the fleet with, naming the repository.
fn cruise_one(
    ctx: &mut Context<'_>,
    args: &FleetArgs,
    repo: &FleetRepo,
) -> Result<RepoRun, Outcome> {
    let stopped = |outcome: Outcome| Outcome {
        stdout: String::new(),
        stderr: format!(
            "rulebearing fleet: repository `{}` ({}): {}",
            repo.name,
            repo.path,
            outcome.stderr.trim_start_matches("rulebearing cruise: ")
        ),
        code: outcome.code,
    };
    let config_args = ConfigArgs {
        config: repo.config.clone(),
        ..ConfigArgs::default()
    };
    let config = configure::load(ctx, &config_args)
        .map_err(|e| stopped(Outcome::failed(RunExit::InvalidConfig, format!("{e}\n"))))?;
    let configured = config.as_ref().and_then(|c| {
        c.options.cache.as_ref().map(|setting| {
            setting
                .options()
                .map(|o| o.folder.clone())
                .unwrap_or_default()
        })
    });
    let cruise = repo_args(args, repo, configured.as_deref());
    let (output, warnings) = if let Some(config) = config {
        crate::cmd::cruise::answer(ctx, &cruise, config)
            .map(|answer| (answer.output, answer.stderr))
            .map_err(stopped)?
    } else {
        let outcome = crate::cmd::cruise::run(ctx, &cruise);
        if outcome.code != 0 {
            return Err(stopped(outcome));
        }
        (outcome.stdout, outcome.stderr)
    };
    let document: GraphDocument = serde_json::from_str(&output).map_err(|e| {
        stopped(Outcome::failed(
            RunExit::Untrustworthy,
            format!("its run did not write a graph document: {e}\n"),
        ))
    })?;
    let receipt = RepoReceipt {
        path: repo.path.clone(),
        config: repo.config.clone(),
        modules: document.modules.len() as u64,
        dependencies: document
            .modules
            .iter()
            .map(|m| m.dependencies.len() as u64)
            .sum(),
        error: document.summary.error,
        warn: document.summary.warn,
        info: document.summary.info,
        inspected: document.summary.inspected.clone(),
        cache: document.summary.cache,
    };
    Ok(RepoRun {
        name: repo.name.clone(),
        document,
        receipt,
        warnings,
    })
}

/// Whether a violation's `from` and `to` name modules or folders, which a fleet prefixes; a
/// project, element or slice violation names other things.
fn names_paths(kind: Option<ViolationType>) -> bool {
    !matches!(
        kind,
        Some(ViolationType::Project | ViolationType::Element | ViolationType::Slice)
    )
}

/// Whether a violation is over an edge, so its stable id hashes the edge's `dependencyKind`, as
/// the engine's does ([ADR-0015](../../../../docs/adr/0015-stable-violation-id.md)).
fn over_an_edge(kind: Option<ViolationType>) -> bool {
    matches!(
        kind,
        Some(ViolationType::Dependency | ViolationType::Cycle | ViolationType::Instability)
    )
}

/// Prefixes every path of a repository's result with `<name>/`: module sources, resolved
/// dependencies, cycles, and the `from`, `to`, `cycle` and `via` of each violation over modules or
/// folders, whose stable ids are recomputed. A core module and an unresolved specifier are not
/// paths of the repository and keep their names. The folder layer is dropped, since the union's
/// is computed afresh, and the code layer is kept for the OpenAPI clients.
pub fn prefix(name: &str, document: &mut GraphDocument) {
    let external: std::collections::BTreeSet<String> = document
        .modules
        .iter()
        .filter(|m| m.core_module == Some(true) || m.could_not_resolve == Some(true))
        .map(|m| m.source.clone())
        .collect();
    let path = |p: &str| {
        if external.contains(p) {
            p.to_owned()
        } else {
            format!("{name}/{p}")
        }
    };
    let steps = |steps: &mut Option<Vec<MiniDependency>>| {
        for step in steps.iter_mut().flatten() {
            step.name = path(&step.name);
        }
    };
    for module in &mut document.modules {
        module.source = path(&module.source);
        if let Some(dependents) = &mut module.dependents {
            for dependent in dependents.iter_mut() {
                *dependent = path(dependent);
            }
        }
        for dependency in &mut module.dependencies {
            if !dependency.core_module && !dependency.could_not_resolve {
                dependency.resolved = path(&dependency.resolved);
            }
            steps(&mut dependency.cycle);
        }
    }
    document.folders = None;
    let mut kinds: HashMap<(String, String), String> = HashMap::new();
    for module in &document.modules {
        for dependency in &module.dependencies {
            if let Some(kind) = dependency.dependency_kind {
                kinds.insert(
                    (module.source.clone(), dependency.resolved.clone()),
                    kind.as_str().to_owned(),
                );
            }
        }
    }
    for violation in &mut document.summary.violations {
        if !names_paths(violation.violation_type) {
            continue;
        }
        violation.from = path(&violation.from);
        violation.to = path(&violation.to);
        steps(&mut violation.cycle);
        steps(&mut violation.via);
        if violation.id.is_some() {
            let kind = if over_an_edge(violation.violation_type) {
                kinds
                    .get(&(violation.from.clone(), violation.to.clone()))
                    .map_or("", String::as_str)
            } else {
                ""
            };
            violation.id = Some(rb_model::violation_id::violation_id(
                &violation.rule.name,
                &violation.from,
                &violation.to,
                kind,
            ));
        }
    }
}

/// The prefixed documents' modules as one graph, sorted by source; a module two repositories
/// both reach (a core module, an unresolved specifier) is kept once, from the first.
pub fn union(documents: &[(String, GraphDocument)]) -> GraphDocument {
    let mut modules: BTreeMap<String, rb_model::Module> = BTreeMap::new();
    for (_, document) in documents {
        for module in &document.modules {
            modules
                .entry(module.source.clone())
                .or_insert_with(|| module.clone());
        }
    }
    GraphDocument {
        modules: modules.into_values().collect(),
        ..GraphDocument::default()
    }
}

/// The repository a union path is in, by its first segment, and the path inside it.
fn locate<'p>(repos: &[FleetRepo], path: &'p str) -> Option<(usize, &'p str)> {
    let (first, rest) = path.split_once('/')?;
    repos
        .iter()
        .position(|repo| repo.name == first)
        .map(|index| (index, rest))
}

/// Adds `edges.yaml`'s edges and its OpenAPI pairs' to `joined`, and the receipt.
///
/// # Errors
/// A message naming the pair when a server document or client folder names no repository, or a
/// server document cannot be read or is not OpenAPI.
fn declare(
    edges: &EdgesFile,
    manifest: &FleetManifest,
    folders: &[PathBuf],
    prefixed: &[(String, GraphDocument)],
    joined: &mut GraphDocument,
) -> Result<DeclaredReceipt, String> {
    let mut declarations = declared::hand_declarations(edges);
    let mut sources = Vec::new();
    for (index, pair) in edges.openapi.iter().enumerate() {
        let (server_repo, inside) = locate(&manifest.repos, &pair.server).ok_or_else(|| {
            format!(
                "edges: openapi[{index}].server `{}`: its first segment names no repository of the manifest",
                pair.server
            )
        })?;
        let file = folders[server_repo].join(inside);
        let text = std::fs::read_to_string(&file).map_err(|e| {
            format!(
                "edges: openapi[{index}].server `{}`: cannot read {}: {e}",
                pair.server,
                file.display()
            )
        })?;
        let operations =
            declared::read_openapi(&pair.server, &text).map_err(|e| format!("edges: {e}"))?;
        let client = pair.client_prefix();
        let (client_repo, _) = locate(&manifest.repos, &client).ok_or_else(|| {
            format!(
                "edges: openapi[{index}].client `{}`: its first segment names no repository of the manifest",
                pair.client
            )
        })?;
        let (name, document) = &prefixed[client_repo];
        let clients = document
            .code
            .as_ref()
            .map(|code| declared::client_names(code, &format!("{name}/"), &client))
            .unwrap_or_default();
        let base = format!("{}/", manifest.repos[server_repo].name);
        let (found, receipt) = declared::openapi_declarations(pair, &operations, &base, &clients);
        declarations.extend(found);
        sources.push(receipt);
    }
    let applied = declared::apply(joined, declarations);
    Ok(DeclaredReceipt {
        edges_yaml: applied.edges_yaml,
        openapi: applied.openapi,
        skipped: (!applied.skipped.is_empty()).then_some(applied.skipped),
        openapi_sources: (!sources.is_empty()).then_some(sources),
    })
}

/// Folds each repository's own findings into the evaluated union: the rules each module and
/// dependency violated and their validity, the violations, the counts, the expired entries, the
/// ratchets, the vacuous rules, and their rules in `ruleSetUsed`, where a reporter finds each
/// finding's comment and `fix`. The violations are sorted as the engine sorts them.
pub fn fold(document: &mut GraphDocument, prefixed: &[(String, GraphDocument)]) {
    let mut theirs: HashMap<&str, &rb_model::Module> = HashMap::new();
    for (_, repo) in prefixed {
        for module in &repo.modules {
            theirs.entry(module.source.as_str()).or_insert(module);
        }
    }
    for module in &mut document.modules {
        let Some(own) = theirs.get(module.source.as_str()) else {
            continue;
        };
        module.valid &= own.valid;
        extend(&mut module.rules, own.rules.as_ref());
        for (index, dependency) in own.dependencies.iter().enumerate() {
            if dependency.valid && dependency.rules.is_none() {
                continue;
            }
            let same = |d: &&mut rb_model::Dependency| {
                d.resolved == dependency.resolved && d.module == dependency.module
            };
            let at =
                if module.dependencies.get(index).is_some_and(|d| {
                    d.resolved == dependency.resolved && d.module == dependency.module
                }) {
                    Some(index)
                } else {
                    module.dependencies.iter_mut().position(|d| same(&d))
                };
            if let Some(target) = at.and_then(|i| module.dependencies.get_mut(i)) {
                target.valid &= dependency.valid;
                extend(&mut target.rules, dependency.rules.as_ref());
            }
        }
    }
    let summary = &mut document.summary;
    for (_, repo) in prefixed {
        let own = &repo.summary;
        summary.violations.extend(own.violations.iter().cloned());
        summary.error += own.error;
        summary.warn += own.warn;
        summary.info += own.info;
        if let Some(ignored) = own.ignore {
            *summary.ignore.get_or_insert(0) += ignored;
        }
        if let Some(theirs) = &own.rule_set_used {
            let ours = summary
                .rule_set_used
                .get_or_insert_with(serde_json::Map::new);
            for (key, value) in theirs {
                match (ours.get_mut(key), value) {
                    (Some(Value::Array(list)), Value::Array(more)) => {
                        list.extend(more.iter().cloned());
                    }
                    (None, _) => {
                        ours.insert(key.clone(), value.clone());
                    }
                    // A scalar the fleet's rules already set (`allowedSeverity`) stays the fleet's.
                    (Some(_), _) => {}
                }
            }
        }
        if let Some(expired) = &own.expired {
            summary
                .expired
                .get_or_insert_with(Vec::new)
                .extend(expired.iter().cloned());
        }
        if let Some(ratchets) = &own.ratchets {
            summary
                .ratchets
                .get_or_insert_with(Vec::new)
                .extend(ratchets.iter().cloned());
        }
        if let Some(vacuous) = &own.vacuous_rules {
            summary
                .vacuous_rules
                .get_or_insert_with(Vec::new)
                .extend(vacuous.iter().cloned());
        }
    }
    let mut values: Vec<Value> = summary
        .violations
        .iter()
        .filter_map(|v| serde_json::to_value(v).ok())
        .collect();
    values.sort_by(rb_rules::compare::compare_violations);
    summary.violations = values
        .into_iter()
        .filter_map(|v| serde_json::from_value(v).ok())
        .collect();
}

fn extend(
    into: &mut Option<Vec<rb_model::RuleSummary>>,
    from: Option<&Vec<rb_model::RuleSummary>>,
) {
    if let Some(rules) = from.filter(|r| !r.is_empty()) {
        into.get_or_insert_with(Vec::new)
            .extend(rules.iter().cloned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use rb_model::{
        Dependency, DependencyKind, ExpiredEntry, Module, ModuleSystem, RatchetResult,
        RatchetStatus, RuleSummary, Severity, VacuousRule, Violation,
    };
    use serde_json::json;

    #[derive(Parser)]
    struct Line {
        #[command(flatten)]
        args: FleetArgs,
    }

    fn args(extra: &[&str]) -> FleetArgs {
        let line = std::iter::once("fleet").chain(extra.iter().copied());
        Line::try_parse_from(line).map_or_else(|_| Line::parse_from(["fleet"]).args, |l| l.args)
    }

    fn repo(name: &str, config: Option<&str>) -> FleetRepo {
        FleetRepo {
            name: name.into(),
            path: format!("../{name}"),
            config: config.map(str::to_owned),
        }
    }

    #[test]
    fn the_flags_default_to_fleet_yaml_and_err() {
        let parsed = args(&[]);
        assert_eq!(parsed.manifest, "fleet.yaml");
        assert_eq!(
            (parsed.output_type.as_str(), parsed.output_to.as_str()),
            ("err", "-")
        );
        assert!(parsed.edges.is_none() && !parsed.no_cache && !parsed.strict_schema);
        assert!(
            Line::try_parse_from(["fleet", "--no-cache", "--cache-strategy", "content"]).is_err()
        );
    }

    /// The fleet's flags, the configuration's cache folder, and the `--cache`,
    /// `--cache-strategy` and `--no-cache` the repository's cruise gets.
    type CacheCase<'a> = (
        &'a [&'a str],
        Option<&'a str>,
        Option<&'a str>,
        Option<CacheStrategyArg>,
        bool,
    );

    #[test]
    fn a_repository_keeps_its_own_cache_folder_or_gets_the_default() {
        let web = repo("web", Some("rulebearing.yaml"));
        let table: [CacheCase<'_>; 5] = [
            (&[], None, Some(""), None, false),
            (&[], Some("custom/cache"), None, None, false),
            (&[], Some(""), None, None, false),
            (
                &["--cache-strategy", "content"],
                Some("custom/cache"),
                Some("custom/cache"),
                Some(CacheStrategyArg::Content),
                false,
            ),
            (&["--no-cache"], Some("custom/cache"), None, None, true),
        ];
        for (flags, configured, cache, strategy, off) in table {
            let cruise = repo_args(&args(flags), &web, configured);
            assert_eq!(cruise.cache.as_deref(), cache, "{flags:?} {configured:?}");
            assert_eq!(cruise.cache_strategy, strategy, "{flags:?}");
            assert_eq!(cruise.no_cache, off, "{flags:?}");
            assert_eq!(cruise.output_type.as_deref(), Some("json"));
            assert_eq!(cruise.config.config.as_deref(), Some("rulebearing.yaml"));
            assert!(cruise.no_progress);
        }
        assert!(
            repo_args(&args(&[]), &repo("api", None), None)
                .config
                .config
                .is_none()
        );
    }

    #[test]
    fn which_violations_name_paths_and_which_are_over_an_edge() {
        use ViolationType as T;
        let table = [
            (None, true, false),
            (Some(T::Dependency), true, true),
            (Some(T::Module), true, false),
            (Some(T::Reachability), true, false),
            (Some(T::Cycle), true, true),
            (Some(T::Instability), true, true),
            (Some(T::Folder), true, false),
            (Some(T::Project), false, false),
            (Some(T::Element), false, false),
            (Some(T::Slice), false, false),
        ];
        for (kind, paths, edge) in table {
            assert_eq!(names_paths(kind), paths, "{kind:?}");
            assert_eq!(over_an_edge(kind), edge, "{kind:?}");
        }
    }

    fn violation(rule: &str, from: &str, to: &str, kind: Option<ViolationType>) -> Violation {
        Violation {
            from: from.into(),
            to: to.into(),
            unresolved_to: None,
            dependency_types: None,
            violation_type: kind,
            rule: RuleSummary {
                name: rule.into(),
                severity: Severity::Error,
            },
            cycle: None,
            via: None,
            metrics: None,
            comment: None,
            id: Some("RB-00000000".into()),
            fix: None,
            decision: None,
        }
    }

    fn step(name: &str) -> rb_model::MiniDependency {
        rb_model::MiniDependency {
            name: name.into(),
            dependency_types: Vec::new(),
        }
    }

    /// `src/a.ts` imports `src/b.ts` (an `import` edge in a cycle), the core module `fs` and the
    /// unresolved `left-pad`.
    fn repository() -> GraphDocument {
        let local = Dependency {
            dependency_kind: Some(DependencyKind::Import),
            cycle: Some(vec![step("src/b.ts"), step("src/a.ts")]),
            ..Dependency::new("./b", "src/b.ts", ModuleSystem::Es6)
        };
        let core = Dependency {
            core_module: true,
            ..Dependency::new("fs", "fs", ModuleSystem::Es6)
        };
        let unresolved = Dependency {
            could_not_resolve: true,
            ..Dependency::new("left-pad", "left-pad", ModuleSystem::Es6)
        };
        let fs = Module {
            core_module: Some(true),
            ..Module::new("fs")
        };
        let missing = Module {
            could_not_resolve: Some(true),
            ..Module::new("left-pad")
        };
        let mut document = GraphDocument {
            modules: vec![
                Module {
                    dependents: Some(vec!["src/b.ts".into()]),
                    dependencies: vec![local, core, unresolved],
                    ..Module::new("src/a.ts")
                },
                Module::new("src/b.ts"),
                fs,
                missing,
            ],
            folders: Some(Vec::new()),
            ..GraphDocument::default()
        };
        let mut cycle = violation(
            "no-circular",
            "src/a.ts",
            "src/b.ts",
            Some(ViolationType::Cycle),
        );
        cycle.cycle = Some(vec![step("src/b.ts"), step("src/a.ts")]);
        let mut reach = violation(
            "reach",
            "src/a.ts",
            "src/b.ts",
            Some(ViolationType::Reachability),
        );
        reach.via = Some(vec![step("src/b.ts")]);
        document.summary.violations = vec![
            cycle,
            reach,
            violation("no-core", "src/a.ts", "fs", Some(ViolationType::Dependency)),
            violation(
                "projects",
                "Shop.Web",
                "Shop.Domain",
                Some(ViolationType::Project),
            ),
        ];
        document
    }

    fn id(rule: &str, from: &str, to: &str, kind: &str) -> String {
        rb_model::violation_id::violation_id(rule, from, to, kind)
    }

    #[test]
    fn prefix_names_every_path_of_the_repository_and_nothing_else() {
        let mut document = repository();
        prefix("web", &mut document);
        let sources: Vec<&str> = document.modules.iter().map(|m| m.source.as_str()).collect();
        assert_eq!(sources, ["web/src/a.ts", "web/src/b.ts", "fs", "left-pad"]);
        let a = &document.modules[0];
        assert_eq!(
            a.dependents.as_deref(),
            Some(&["web/src/b.ts".to_owned()][..])
        );
        let resolved: Vec<&str> = a.dependencies.iter().map(|d| d.resolved.as_str()).collect();
        assert_eq!(resolved, ["web/src/b.ts", "fs", "left-pad"]);
        let cycle: Vec<&str> = a.dependencies[0]
            .cycle
            .iter()
            .flatten()
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(cycle, ["web/src/b.ts", "web/src/a.ts"]);
        assert!(document.folders.is_none());
        let found = &document.summary.violations;
        assert_eq!(
            (found[0].from.as_str(), found[0].to.as_str()),
            ("web/src/a.ts", "web/src/b.ts")
        );
        assert_eq!(
            found[0].id.as_deref(),
            Some(&*id(
                "no-circular",
                "web/src/a.ts",
                "web/src/b.ts",
                "import"
            )),
            "an edge violation hashes the edge's kind"
        );
        assert_eq!(
            found[0].cycle.as_ref().map(|c| c[0].name.as_str()),
            Some("web/src/b.ts")
        );
        assert_eq!(
            found[1].via.as_ref().map(|c| c[0].name.as_str()),
            Some("web/src/b.ts")
        );
        assert_eq!(
            found[1].id.as_deref(),
            Some(&*id("reach", "web/src/a.ts", "web/src/b.ts", "")),
            "a reachability violation hashes no kind"
        );
        assert_eq!(found[2].to, "fs", "a core module keeps its name");
        assert_eq!(
            found[2].id.as_deref(),
            Some(&*id("no-core", "web/src/a.ts", "fs", "")),
            "the core edge has no kind"
        );
        assert_eq!(
            (found[3].from.as_str(), found[3].id.as_deref()),
            ("Shop.Web", Some("RB-00000000")),
            "a project violation names projects, not paths"
        );
        let mut without_id = repository();
        without_id.summary.violations[0].id = None;
        prefix("web", &mut without_id);
        assert!(without_id.summary.violations[0].id.is_none());
    }

    #[test]
    fn union_sorts_and_keeps_a_shared_external_once() {
        let mut web = repository();
        prefix("web", &mut web);
        let mut admin = repository();
        prefix("admin", &mut admin);
        web.modules[2].dependency_types = Some(vec![rb_model::DependencyType::Core]);
        let joined = union(&[("web".into(), web), ("admin".into(), admin)]);
        let sources: Vec<&str> = joined.modules.iter().map(|m| m.source.as_str()).collect();
        assert_eq!(
            sources,
            [
                "admin/src/a.ts",
                "admin/src/b.ts",
                "fs",
                "left-pad",
                "web/src/a.ts",
                "web/src/b.ts"
            ]
        );
        let fs = joined.modules.iter().find(|m| m.source == "fs");
        assert!(
            fs.is_some_and(|m| m.dependency_types.is_some()),
            "the first repository's"
        );
        assert!(joined.summary.violations.is_empty());
    }

    #[test]
    fn locate_finds_the_repository_by_its_first_segment() {
        let repos = [repo("web", None), repo("api", None)];
        assert_eq!(
            locate(&repos, "api/openapi.json"),
            Some((1, "openapi.json"))
        );
        assert_eq!(locate(&repos, "web/src/gen/"), Some((0, "src/gen/")));
        assert_eq!(locate(&repos, "other/x"), None);
        assert_eq!(locate(&repos, "web"), None);
    }

    fn rule(name: &str) -> RuleSummary {
        RuleSummary {
            name: name.into(),
            severity: Severity::Warn,
        }
    }

    fn object(value: &serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
        value.as_object().cloned().unwrap_or_default()
    }

    /// A repository's evaluated result, prefixed: a module rule, two edge rules, every count,
    /// an expired entry, a ratchet, a vacuous rule and its rule set.
    fn evaluated_repository() -> GraphDocument {
        let mut theirs = repository();
        prefix("web", &mut theirs);
        theirs.modules[0].valid = false;
        theirs.modules[0].rules = Some(vec![rule("module-rule")]);
        theirs.modules[0].dependencies[0].valid = false;
        theirs.modules[0].dependencies[0].rules = Some(vec![rule("edge-rule")]);
        theirs.modules[0].dependencies[1].rules = Some(vec![rule("core-rule")]);
        theirs.summary.error = 4;
        theirs.summary.warn = 1;
        theirs.summary.info = 2;
        theirs.summary.ignore = Some(3);
        theirs.summary.expired = Some(vec![ExpiredEntry {
            name: "old".into(),
            expires: "2020-01-01".into(),
            kind: "rule".into(),
        }]);
        theirs.summary.ratchets = Some(vec![RatchetResult {
            name: "budget".into(),
            budget: "b.json".into(),
            count: 1,
            ceiling: Some(2),
            status: RatchetStatus::Held,
        }]);
        theirs.summary.vacuous_rules = Some(vec![VacuousRule::new("empty", "from")]);
        theirs.summary.rule_set_used = Some(object(&json!({
            "forbidden": [{ "name": "edge-rule" }], "allowedSeverity": "error", "elements": []
        })));
        theirs
    }

    #[test]
    fn fold_adds_the_findings_of_each_repository_to_the_union() {
        let theirs = evaluated_repository();
        // The union, evaluated: the fleet's own rule on the edge and its own violation; the
        // core dependency moved, so it is found by search rather than by index.
        let mut document = union(&[("web".into(), theirs.clone())]);
        rb_rules::reset(&mut document);
        let at = document
            .modules
            .iter()
            .position(|m| m.source == "web/src/a.ts")
            .unwrap_or_default();
        document.modules[at].dependencies.swap(1, 2);
        document.modules[at].dependencies[0].rules = Some(vec![rule("fleet-rule")]);
        document.summary.violations = vec![violation(
            "fleet-rule",
            "web/src/a.ts",
            "web/src/b.ts",
            Some(ViolationType::Dependency),
        )];
        document.summary.error = 1;
        document.summary.rule_set_used = Some(object(&json!({
            "forbidden": [{ "name": "fleet-rule" }], "allowedSeverity": "warn"
        })));
        fold(&mut document, &[("web".into(), theirs)]);
        let a = &document.modules[at];
        assert!(!a.valid);
        assert_eq!(a.rules.as_deref().map(<[_]>::len), Some(1));
        let names = |d: &Dependency| -> Vec<String> {
            d.rules.iter().flatten().map(|r| r.name.clone()).collect()
        };
        assert_eq!(names(&a.dependencies[0]), ["fleet-rule", "edge-rule"]);
        assert!(!a.dependencies[0].valid);
        let core = a.dependencies.iter().find(|d| d.resolved == "fs");
        assert_eq!(core.map(names), Some(vec!["core-rule".to_owned()]));
        let unresolved = a.dependencies.iter().find(|d| d.resolved == "left-pad");
        assert!(unresolved.is_some_and(|d| d.rules.is_none() && d.valid));
        let summary = &document.summary;
        assert_eq!(
            (summary.error, summary.warn, summary.info, summary.ignore),
            (5, 1, 2, Some(3))
        );
        let order: Vec<&str> = summary
            .violations
            .iter()
            .map(|v| v.rule.name.as_str())
            .collect();
        assert_eq!(
            order,
            ["fleet-rule", "no-circular", "no-core", "projects", "reach"],
            "one severity: sorted by rule name"
        );
        assert_eq!(summary.expired.as_ref().map(Vec::len), Some(1));
        assert_eq!(summary.ratchets.as_ref().map(Vec::len), Some(1));
        assert_eq!(summary.vacuous_rules.as_ref().map(Vec::len), Some(1));
        let used = summary.rule_set_used.clone().unwrap_or_default();
        assert_eq!(
            used["forbidden"],
            json!([{ "name": "fleet-rule" }, { "name": "edge-rule" }])
        );
        assert_eq!(used["allowedSeverity"], "warn", "the fleet's scalar stays");
        assert_eq!(used["elements"], json!([]));
    }

    #[test]
    fn fold_into_a_union_without_a_rule_set_takes_the_rule_set_of_the_repository() {
        let theirs = evaluated_repository();
        let mut document = union(&[("web".into(), theirs.clone())]);
        rb_rules::reset(&mut document);
        fold(&mut document, &[("web".into(), theirs)]);
        let used = document.summary.rule_set_used.unwrap_or_default();
        assert_eq!(used["allowedSeverity"], "error");
        assert_eq!(document.summary.ignore, Some(3));
    }

    fn edge(module: &str, resolved: &str, rules: Option<&str>) -> Dependency {
        Dependency {
            rules: rules.map(|r| vec![rule(r)]),
            ..Dependency::new(module, resolved, ModuleSystem::Es6)
        }
    }

    #[test]
    fn fold_finds_each_edge_by_both_its_specifier_and_its_target() {
        let theirs = GraphDocument {
            modules: vec![Module {
                dependencies: vec![
                    edge("./b", "x/b", None),
                    edge("./b.js", "x/b", Some("rb")),
                    edge("./d", "x/d", Some("rd")),
                ],
                ..Module::new("x/a")
            }],
            ..GraphDocument::default()
        };
        // At index 1 an edge with the same specifier and another target, at index 2 one with the
        // same target and another specifier: neither is the repository's edge.
        let mut document = GraphDocument {
            modules: vec![Module {
                dependencies: vec![
                    edge("./b.js", "x/b", None),
                    edge("./b.js", "x/c", None),
                    edge("./dd", "x/d", None),
                    edge("./d", "x/d", None),
                    edge("./b", "x/b", None),
                ],
                ..Module::new("x/a")
            }],
            ..GraphDocument::default()
        };
        fold(&mut document, &[("x".into(), theirs)]);
        let found: Vec<Vec<String>> = document.modules[0]
            .dependencies
            .iter()
            .map(|d| d.rules.iter().flatten().map(|r| r.name.clone()).collect())
            .collect();
        let none: Vec<String> = Vec::new();
        assert_eq!(
            found,
            [
                vec!["rb".to_owned()],
                none.clone(),
                none.clone(),
                vec!["rd".to_owned()],
                none
            ]
        );
    }

    #[test]
    fn prefix_keeps_a_core_or_unresolved_target_the_graph_does_not_list() {
        let mut document = GraphDocument {
            modules: vec![Module {
                dependencies: vec![
                    Dependency {
                        core_module: true,
                        ..Dependency::new("os", "os", ModuleSystem::Py)
                    },
                    Dependency {
                        could_not_resolve: true,
                        ..Dependency::new("requests", "requests", ModuleSystem::Py)
                    },
                ],
                ..Module::new("p/a.py")
            }],
            ..GraphDocument::default()
        };
        prefix("pipeline", &mut document);
        let resolved: Vec<&str> = document.modules[0]
            .dependencies
            .iter()
            .map(|d| d.resolved.as_str())
            .collect();
        assert_eq!(resolved, ["os", "requests"]);
    }

    #[test]
    fn extend_adds_only_a_non_empty_list() {
        let mut into = None;
        extend(&mut into, Some(&Vec::new()));
        assert!(into.is_none());
        extend(&mut into, None);
        assert!(into.is_none());
        extend(&mut into, Some(&vec![rule("a")]));
        extend(&mut into, Some(&vec![rule("b")]));
        assert_eq!(
            into.iter()
                .flatten()
                .map(|r| r.name.as_str())
                .collect::<Vec<_>>(),
            ["a", "b"]
        );
    }
}
