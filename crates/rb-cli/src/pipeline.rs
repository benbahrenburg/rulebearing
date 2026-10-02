//! The five stages as one call: configuration, extraction, evaluation, the report's re-summary,
//! and the receipt. `cruise` runs it; the agent commands run it when they are not given a saved
//! graph.
//!
//! - Architecture: [The five stages](../../../docs/architecture.md#the-five-stages)
//! - Source: [design § The five stages](../../../docs/artifacts/design.md#the-five-stages)
//! - Plan: [Wave 1 § 1.4](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#14-architecture-of-what-this-wave-builds)
//!   (the path a gate takes)
//! - Requirements: [FR-CORE-02](../../../docs/prd.md#fr-core-02), [FR-CORE-04](../../../docs/prd.md#fr-core-04),
//!   [FR-CLI-05](../../../docs/prd.md#fr-cli-05) (the extraction in parts the cache keeps:
//!   [Wave 3, Step 2](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#21-steps-for-sub-wave-3a-cache---affected-diff---exit-code-mode-strict))

#[cfg(any(feature = "extract-ts", feature = "extract-python"))]
use std::path::PathBuf;

use rb_config::{Config, ConfigError};
use rb_model::{
    ExtractError, ExtractRequest, Extraction, GraphDocument, Inspected, Language, Receipt,
};
use rb_rules::graph::filters::{Filter, Filters};
use rb_rules::rewrap::{FormatOptions, collapse_pattern, rewrap};
use rb_rules::{EngineError, EvalOptions, Evaluation, evaluate};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::context::Context;
use crate::progress::Progress;

/// Why a run stopped.
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    /// The configuration is invalid (exit 3).
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// Extraction cannot be trusted (exit 2).
    #[error(transparent)]
    Extract(#[from] ExtractError),
    /// An engine bug (exit 2).
    #[error(transparent)]
    Engine(#[from] EngineError),
    /// The report could not be made: `x-dot-webpage` without a working GraphViz `dot`
    /// ([ADR-0053](../../../docs/adr/0053-x-dot-webpage-draws-with-graphviz-dot.md)) (exit 2).
    #[error("{0}")]
    Report(String),
    /// A `plugin:<path>` reporter could not run: missing, invalid, refused by the sandbox or
    /// throwing ([`crate::plugin`]) (exit 3).
    #[error(transparent)]
    Plugin(#[from] crate::plugin::Failure),
}

impl RunError {
    /// The exit code a run that stopped for this reason gives
    /// ([ADR-0008](../../../docs/adr/0008-exit-code-contract.md)): 3 for an invalid
    /// configuration (a malformed element rule and a plugin that cannot run included), 2 when
    /// the run cannot be trusted.
    pub fn exit(&self) -> crate::RunExit {
        match self {
            Self::Config(_) | Self::Plugin(_) | Self::Engine(EngineError::Element(_)) => {
                crate::RunExit::InvalidConfig
            }
            Self::Extract(_) | Self::Engine(_) | Self::Report(_) => crate::RunExit::Untrustworthy,
        }
    }
}

/// What a run needs besides the configuration.
#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    /// Liveness on ([ADR-0007](../../../docs/adr/0007-vacuous-rules-fail-by-default.md)).
    pub liveness: bool,
    /// `optionsUsed` as the command line normalised it.
    pub options_used: Map<String, Value>,
    /// The positional arguments.
    pub paths: Vec<String>,
    /// `--affected`: the changes since the revision ([`crate::affected`]).
    pub affected: Option<crate::affected::Selection>,
}

/// A finished run. The graph is held once: the report's document is the engine's when nothing
/// filtered, collapsed or narrowed it, and only otherwise is the whole graph kept beside it, so
/// a large graph is not in memory twice.
#[derive(Debug, Clone)]
pub struct Run {
    /// Rules whose selecting side matched nothing, with liveness on and no `allowEmpty`.
    pub vacuous: Vec<rb_model::VacuousRule>,
    /// Per-rule statistics, in rule order.
    pub rule_stats: Vec<rb_rules::RuleStats>,
    /// Rules and known violations past their date; each fails the run.
    pub expired: Vec<rb_rules::Expired>,
    /// The indices into `config.known_violations` of the entries no violation matched.
    pub unmatched_known: Vec<usize>,
    /// The whole evaluated graph, when the report shows a part of it.
    whole: Option<GraphDocument>,
    /// The document as the report shows it.
    pub document: GraphDocument,
    /// Extraction problems that did not stop the run, as `path: message` lines.
    pub warnings: Vec<String>,
}

impl Run {
    /// The whole evaluated graph, whatever the report shows of it: what ratchets count over and
    /// what a query command reads.
    pub fn evaluated(&self) -> &GraphDocument {
        self.whole.as_ref().unwrap_or(&self.document)
    }

    /// The error-severity violations of the whole graph plus the expired entries: what the exit
    /// code counts ([ADR-0008](../../../docs/adr/0008-exit-code-contract.md)).
    pub fn error_count(&self) -> u64 {
        self.evaluated().summary.error + self.expired.len() as u64
    }

    /// The engine's result over the whole graph, for a caller that wants the evaluation and
    /// not a report.
    pub fn into_evaluation(self) -> Evaluation {
        Evaluation {
            document: self.whole.unwrap_or(self.document),
            vacuous: self.vacuous,
            rule_stats: self.rule_stats,
            expired: self.expired,
            unmatched_known: self.unmatched_known,
        }
    }

    /// Takes the whole graph out when it was kept beside the report's document, so a caller
    /// that is done with it can free it elsewhere.
    pub fn take_whole(&mut self) -> Option<GraphDocument> {
        self.whole.take()
    }
}

/// The receipt: per language, the files and modules extracted.
pub fn receipt(document: &GraphDocument) -> Inspected {
    // An extractor that writes its own receipt (.NET, Python) keeps it; the rest are counted.
    let written = document.summary.inspected.clone().unwrap_or_default();
    let mut inspected = written.clone();
    for module in &document.modules {
        if let Some(language) = module.language
            && !written.contains_key(&language)
        {
            let entry = inspected
                .entry(language)
                .or_insert(Receipt::counts(0, 0, 0));
            entry.files += 1;
            entry.modules += 1;
        }
    }
    inspected
}

/// Each language's extraction, kept apart so the cache can store them and a later run can reuse
/// or re-extract them one by one ([Wave 3, Step 2](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#21-steps-for-sub-wave-3a-cache---affected-diff---exit-code-mode-strict)).
/// [`merge`] turns them into the run's document; a cold run and a cached one both go through it,
/// so equal parts give byte-identical documents.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Parts {
    /// The TypeScript and JavaScript extraction, with the warnings the command line added before
    /// it; present with no modules when the walk found none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub typescript: Option<Extraction>,
    /// The .NET extraction as the extractor returned it, before the path filters; absent when
    /// .NET was not asked for or found nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dotnet: Option<Extraction>,
    /// The Python extraction, likewise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub python: Option<Extraction>,
}

/// How one language is extracted in a run.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum Plan {
    /// Every file read.
    #[default]
    Full,
    /// The changed files read, the unchanged ones taken from the earlier extraction.
    Incremental(ExtractRequest),
    /// The earlier extraction as it was: none of the language's inputs changed.
    Reuse(Option<Extraction>),
}

/// How each language is extracted, and whether the extractors keep the per-file state a later
/// incremental run reuses.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Plans {
    /// TypeScript and JavaScript.
    pub typescript: Plan,
    /// .NET: [`Plan::Full`] or [`Plan::Reuse`] in compiled mode, where the assembly is the unit
    /// of change; also [`Plan::Incremental`] in source mode, where a `.cs` file is.
    pub dotnet: Plan,
    /// Python.
    pub python: Plan,
    /// Keep each file's state in [`Extraction::files`].
    pub keep_file_states: bool,
    /// Keep the walk of the inputs in [`Extraction::walk`], for a caller that watches its
    /// folders and can then promise [`ExtractRequest::walk_unchanged`] (`guard --watch`).
    pub keep_walk: bool,
}

/// Whether a file with one of `names` sits in `root` (not below it): the signal that a language
/// is present when its `languages` block is absent.
#[cfg(any(feature = "extract-dotnet", feature = "extract-python"))]
fn has_root_file(root: &std::path::Path, test: impl Fn(&str) -> bool) -> bool {
    std::fs::read_dir(root).is_ok_and(|entries| {
        entries.flatten().any(|e| {
            e.file_type().is_ok_and(|t| t.is_file()) && test(&e.file_name().to_string_lossy())
        })
    })
}

/// Whether the .NET extractor runs: `languages.dotnet` is set or a solution sits in the working
/// directory.
#[cfg(feature = "extract-dotnet")]
pub fn dotnet_enabled(ctx: &Context<'_>, config: &Config) -> bool {
    let solution = |name: &str| {
        std::path::Path::new(name)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("sln") || e.eq_ignore_ascii_case("slnx"))
    };
    config.languages.dotnet.is_some() || has_root_file(&ctx.cwd, solution)
}

/// Whether .NET is read from source (`--mode source` or `languages.dotnet.mode: source`), whose
/// unit of change is a `.cs` file rather than an assembly.
pub fn dotnet_source_mode(config: &Config) -> bool {
    config
        .languages
        .dotnet
        .as_ref()
        .is_some_and(|d| d.mode() == rb_model::DotnetMode::Source)
}

/// Whether the Python extractor runs: `languages.python` is set or a `pyproject.toml`,
/// `setup.cfg` or `setup.py` sits in the working directory.
#[cfg(feature = "extract-python")]
pub fn python_enabled(ctx: &Context<'_>, config: &Config) -> bool {
    let project = |name: &str| matches!(name, "pyproject.toml" | "setup.cfg" | "setup.py");
    config.languages.python.is_some() || has_root_file(&ctx.cwd, project)
}

/// `options.exclude` and `includeOnly` applied to the modules of an extractor that does not apply
/// them while it walks, with the same filter code the report uses.
fn filter_paths(config: &Config, modules: Vec<rb_model::Module>) -> Vec<rb_model::Module> {
    let filter = |f: &Option<rb_model::options::PathFilter>| {
        f.as_ref().and_then(|f| f.path()).map(|p| Filter {
            path: Some(p.joined()),
            depth: None,
        })
    };
    let filters = Filters {
        exclude: filter(&config.languages.typescript.exclude),
        include_only: filter(&config.languages.typescript.include_only),
        ..Filters::default()
    };
    if filters.is_empty() {
        return modules;
    }
    let values: Vec<Value> = modules
        .into_iter()
        .filter_map(|m| serde_json::to_value(m).ok())
        .collect();
    rb_rules::graph::filters::apply(values, &filters)
        .into_iter()
        .filter_map(|v| serde_json::from_value(v).ok())
        .collect()
}

/// Extracts every language under `paths` and merges the graphs: TypeScript and JavaScript as
/// wave 1 did; .NET when `languages.dotnet` is set or a solution sits in the working directory;
/// Python when `languages.python` is set or a `pyproject.toml`, `setup.cfg` or `setup.py` does
/// ([FR-CORE-01](../../../docs/prd.md#fr-core-01), [ADR-0014](../../../docs/adr/0014-no-invented-cross-language-edges.md):
/// the graphs are joined, never linked across languages). An extractor that finds nothing is
/// skipped; the run fails only when none found anything or one found something it cannot trust.
///
/// # Errors
/// [`ExtractError`] for an empty cruise, an untrustworthy input or an unsupported file.
pub fn extract(
    ctx: &Context<'_>,
    config: &Config,
    paths: &[String],
) -> Result<GraphDocument, ExtractError> {
    extract_with_warnings(ctx, config, paths).map(|(document, _)| document)
}

/// [`extract`], with the problems the extractors met that did not stop them, each naming the
/// file and the fix (a missing PDB, an unbuilt project, a file that does not parse).
///
/// # Errors
/// As [`extract`].
pub fn extract_with_warnings(
    ctx: &Context<'_>,
    config: &Config,
    paths: &[String],
) -> Result<(GraphDocument, Vec<rb_model::Warning>), ExtractError> {
    let parts = extract_parts(ctx, config, paths, Plans::default())?;
    merge(config, &parts)
}

/// Runs each extractor as `plans` says, in the order [`merge`] joins them; the first error that
/// makes the run untrustworthy stops it. The plans are taken, so an earlier extraction they carry
/// moves into the result rather than being copied.
///
/// # Errors
/// As [`extract`].
#[cfg_attr(
    not(any(
        feature = "extract-ts",
        feature = "extract-dotnet",
        feature = "extract-python"
    )),
    expect(
        unused_variables,
        unused_mut,
        reason = "a build with no extractor reads nothing and adds nothing"
    )
)]
#[cfg_attr(
    all(
        feature = "extract-dotnet",
        not(any(feature = "extract-ts", feature = "extract-python"))
    ),
    expect(
        unused_variables,
        reason = "the .NET extractor reads the solution, not the positional paths"
    )
)]
pub fn extract_parts(
    ctx: &Context<'_>,
    config: &Config,
    paths: &[String],
    mut plans: Plans,
) -> Result<Parts, ExtractError> {
    let mut parts = Parts::default();
    #[cfg(feature = "extract-ts")]
    {
        let plan = std::mem::take(&mut plans.typescript);
        parts.typescript = typescript_part(ctx, config, paths, plan, &plans)?;
    }
    #[cfg(feature = "extract-dotnet")]
    {
        parts.dotnet = match std::mem::take(&mut plans.dotnet) {
            Plan::Reuse(part) => part,
            plan @ (Plan::Full | Plan::Incremental(_)) if dotnet_enabled(ctx, config) => {
                plans.dotnet = plan;
                let options = config.languages.dotnet.clone().unwrap_or_default();
                found(dotnet_part(ctx, &options, &plans))?
            }
            Plan::Full | Plan::Incremental(_) => None,
        };
    }
    #[cfg(feature = "extract-python")]
    {
        parts.python = python_part(ctx, config, paths, &mut plans)?;
    }
    Ok(parts)
}

/// The .NET part: compiled mode through the extractor; source mode reading only the changed
/// `.cs` files of an incremental plan and keeping each file's parse when asked
/// ([Wave 3, Step 14](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#24-steps-for-sub-wave-3d---mode-source-guard---watch-the-2-s-proof)).
#[cfg(feature = "extract-dotnet")]
fn dotnet_part(
    ctx: &Context<'_>,
    options: &rb_model::DotnetOptions,
    plans: &Plans,
) -> Result<Extraction, ExtractError> {
    #[cfg(feature = "source-mode")]
    if options.mode() == rb_model::DotnetMode::Source {
        let request = match &plans.dotnet {
            Plan::Incremental(request) => Some(request),
            Plan::Full | Plan::Reuse(_) => None,
        };
        return rb_extract_dotnet::source::extract(
            &ctx.cwd,
            options,
            request,
            plans.keep_file_states,
        );
    }
    #[cfg(not(feature = "source-mode"))]
    let _ = plans;
    rb_model::Extractor::extract(
        &rb_extract_dotnet::DotnetExtractor,
        std::slice::from_ref(&ctx.cwd),
        options,
    )
}

/// The .NET solution and project files an extraction read: the ones source mode kept in its
/// file states, else (compiled mode, or a source extraction without them) read again.
#[cfg(feature = "extract-dotnet")]
pub fn dotnet_project_files(
    cwd: &std::path::Path,
    options: &rb_model::DotnetOptions,
    part: Option<&Extraction>,
) -> Vec<std::path::PathBuf> {
    #[cfg(feature = "source-mode")]
    if let Some(files) = part.and_then(|p| rb_extract_dotnet::source::kept_project_files(cwd, p)) {
        return files;
    }
    #[cfg(not(feature = "source-mode"))]
    let _ = part;
    rb_extract_dotnet::project_files(cwd, options).unwrap_or_default()
}

/// An extraction, or nothing when the extractor found nothing to read; any other error stops
/// the run.
#[cfg(any(feature = "extract-dotnet", feature = "extract-python"))]
fn found(result: Result<Extraction, ExtractError>) -> Result<Option<Extraction>, ExtractError> {
    match result {
        Ok(extraction) => Ok(Some(extraction)),
        Err(ExtractError::NoModulesFound) => Ok(None),
        Err(error) => Err(error),
    }
}

/// The TypeScript and JavaScript part: the walk from `paths` (the working directory when none),
/// with the warnings about the webpack `resolve` keys the resolver does not read first.
#[cfg(feature = "extract-ts")]
fn typescript_part(
    ctx: &Context<'_>,
    config: &Config,
    paths: &[String],
    plan: Plan,
    plans: &Plans,
) -> Result<Option<Extraction>, ExtractError> {
    if let Plan::Reuse(part) = plan {
        return Ok(part);
    }
    let roots: Vec<PathBuf> = if paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        paths.iter().map(PathBuf::from).collect()
    };
    let (mut settings, mut resolve) =
        rb_extract_ts::prepare(&config.languages.typescript, &ctx.cwd)?;
    // Markdown fences are a native addition; a dependency-cruiser configuration never has a
    // file of `extraExtensionsToScan` read (ADR-0036).
    settings.markdown_fences = config.compat == rb_config::CompatMode::Native;
    settings.keep_file_states = plans.keep_file_states;
    settings.keep_walk = plans.keep_walk;
    let mut warnings = Vec::new();
    // The webpack configuration's `resolve` block wins over `enhancedResolveOptions`, as
    // upstream spreads it last.
    if let Some(block) = config
        .options
        .webpack_config_json
        .as_ref()
        .and_then(Value::as_object)
    {
        let unread = rb_extract_ts::resolve::apply_resolve_block(&mut resolve, block);
        if !unread.is_empty() {
            warnings.push(rb_model::Warning {
                path: None,
                message: format!(
                    "the webpack resolve keys {} are not applied; the resolver reads {}",
                    unread.join(", "),
                    rb_extract_ts::resolve::RESOLVE_BLOCK_KEYS.join(", ")
                ),
            });
        }
    }
    // Licences and deprecations are read from package.json only when a rule asks for them, as
    // upstream's ruleSetHasLicenseRule and ruleSetHasDeprecationRule decide.
    resolve.resolve_licenses = rb_rules::derive::has_license_rule(&config.rules.dependencies);
    resolve.resolve_deprecations =
        rb_rules::derive::has_deprecation_rule(&config.rules.dependencies);
    let result = match plan {
        Plan::Incremental(request) => {
            rb_extract_ts::extract_incremental_from(&roots, &settings, &resolve, request)
        }
        Plan::Full | Plan::Reuse(_) => rb_extract_ts::extract_with(&roots, &settings, &resolve),
    };
    match result {
        Ok(mut extraction) => {
            warnings.append(&mut extraction.warnings);
            extraction.warnings = warnings;
            Ok(Some(extraction))
        }
        Err(ExtractError::NoModulesFound) => Ok(Some(Extraction {
            warnings,
            ..Extraction::default()
        })),
        Err(error) => Err(error),
    }
}

/// The Python part, when Python is asked for or found.
#[cfg(feature = "extract-python")]
fn python_part(
    ctx: &Context<'_>,
    config: &Config,
    paths: &[String],
    plans: &mut Plans,
) -> Result<Option<Extraction>, ExtractError> {
    if let Plan::Reuse(part) = &mut plans.python {
        return Ok(part.take());
    }
    if !python_enabled(ctx, config) {
        return Ok(None);
    }
    let options = config.languages.python.clone().unwrap_or_default();
    let inputs: Vec<PathBuf> = if paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        paths.iter().map(PathBuf::from).collect()
    };
    let virtual_env = std::env::var_os("VIRTUAL_ENV").map(PathBuf::from);
    let result = rb_extract_python::prepare(&ctx.cwd, &options, virtual_env.as_deref()).and_then(
        |mut settings| {
            settings.keep_file_states = plans.keep_file_states;
            match &plans.python {
                Plan::Incremental(request) => {
                    rb_extract_python::extract_incremental(&inputs, &settings, request)
                }
                Plan::Full | Plan::Reuse(_) => rb_extract_python::extract_with(&inputs, &settings),
            }
        },
    );
    found(result)
}

/// Joins the parts into the run's document: the TypeScript modules in dependency-cruiser's
/// visiting order, then the .NET and the Python modules, each already sorted by source and
/// passed through the path filters; the code layers merged and normalised; the .NET and Python
/// receipts and the sidecar's (`summary.sidecar`, [ADR-0017](../../../docs/adr/0017-coffeescript-livescript-sidecar.md));
/// the warnings in the same order.
///
/// # Errors
/// [`ExtractError::NoModulesFound`] when no part has a module.
pub fn merge(
    config: &Config,
    parts: &Parts,
) -> Result<(GraphDocument, Vec<rb_model::Warning>), ExtractError> {
    let mut modules = Vec::new();
    let mut code: Option<rb_model::CodeLayer> = None;
    let mut inspected = Inspected::new();
    let mut warnings = Vec::new();
    if let Some(typescript) = &parts.typescript {
        modules.extend(typescript.modules.iter().cloned());
        warnings.extend(typescript.warnings.iter().cloned());
        if let Some(layer) = &typescript.code {
            code.get_or_insert_with(Default::default)
                .merge(layer.clone());
        }
    }
    for (language, part) in [
        (Language::Dotnet, &parts.dotnet),
        (Language::Python, &parts.python),
    ] {
        let Some(extraction) = part else {
            continue;
        };
        modules.extend(filter_paths(config, extraction.modules.clone()));
        if let Some(layer) = &extraction.code {
            code.get_or_insert_with(Default::default)
                .merge(layer.clone());
        }
        inspected.insert(language, extraction.inspected.clone());
        warnings.extend(extraction.warnings.iter().cloned());
    }
    if modules.is_empty() {
        return Err(ExtractError::NoModulesFound);
    }
    // The TypeScript modules keep dependency-cruiser's visiting order; each other extractor's
    // follow, already sorted by source.
    let mut document = GraphDocument {
        modules,
        code: code.map(|mut code| {
            code.normalise();
            code
        }),
        ..GraphDocument::default()
    };
    if !inspected.is_empty() {
        document.summary.inspected = Some(inspected);
    }
    document.summary.sidecar = parts.typescript.as_ref().and_then(|t| t.sidecar.clone());
    Ok((document, warnings))
}

/// The report filters `cruise` applies after evaluation: `reaches` and `highlight` (the rest were
/// applied while extracting and evaluating, and reapplying them changes nothing).
fn cruise_filters(config: &Config) -> Filters {
    let path_only = |option: Option<&rb_config::model::FilterOption>| {
        option.map(|o| Filter {
            path: o.path.clone(),
            depth: None,
        })
    };
    Filters {
        reaches: path_only(config.options.reaches.as_ref()),
        // `highlight` marks every module `matchesHighlight: true` or `false`, as upstream's
        // reportWrap tags it from the cruise options.
        highlight: path_only(config.options.highlight.as_ref()),
        ..Filters::default()
    }
}

/// `collapse` from the configuration or `--collapse`: a folder depth (a single digit) becomes
/// upstream's pattern, anything else is the pattern itself.
pub(crate) fn cruise_collapse(config: &Config) -> Option<String> {
    config.options.collapse.as_ref().and_then(collapse_pattern)
}

/// Runs the stages over `paths`.
///
/// # Errors
/// [`RunError`].
pub fn run(
    ctx: &Context<'_>,
    config: &Config,
    options: &RunOptions,
    progress: &mut Progress,
) -> Result<Run, RunError> {
    let (document, warnings) = extract_with_warnings(ctx, config, &options.paths)?;
    progress.stage("extract");
    run_extracted(ctx, config, (document, warnings), options, progress)
}

/// [`run`] from an extraction already made (by the cache, say): evaluation, the report's
/// re-summary, and the warnings as `path: message` lines.
///
/// # Errors
/// [`RunError::Engine`].
pub fn run_extracted(
    ctx: &Context<'_>,
    config: &Config,
    (document, warnings): (GraphDocument, Vec<rb_model::Warning>),
    options: &RunOptions,
    progress: &mut Progress,
) -> Result<Run, RunError> {
    let mut run = evaluate_document(ctx, config, document, options, progress)?;
    run.warnings = warnings
        .into_iter()
        .map(|w| match w.path {
            Some(path) => format!("{}: {}", path.display(), w.message),
            None => w.message,
        })
        .collect();
    Ok(run)
}

/// Evaluates an extracted document and prepares it for reporting.
///
/// # Errors
/// [`RunError::Engine`].
pub fn evaluate_document(
    ctx: &Context<'_>,
    config: &Config,
    document: GraphDocument,
    options: &RunOptions,
    progress: &mut Progress,
) -> Result<Run, RunError> {
    let inspected = receipt(&document);
    let sidecar = document.summary.sidecar.clone();
    let mut filters = cruise_filters(config);
    let mut options_used = options.options_used.clone();
    // `--affected` with a dependency-cruiser configuration: the other languages' changed modules
    // join upstream's expression, which is only known once the graph is (crate::affected).
    let upstream = options
        .affected
        .as_ref()
        .filter(|s| s.mode == crate::affected::Mode::Upstream)
        .map(|selection| {
            let pattern = selection.pattern(&document);
            crate::affected::Selection::record(&mut options_used, &pattern);
            filters.reaches = Some(Filter {
                path: Some(pattern.clone()),
                depth: selection.depth,
            });
            (selection, pattern)
        });
    let eval_options = EvalOptions {
        liveness: options.liveness,
        validate: true,
        metrics: config.options.metrics.unwrap_or(false),
        today: ctx.today,
        args: options.paths.clone(),
        options_used,
    };
    let evaluation = evaluate(document, config, &eval_options)?;
    progress.stage("evaluate");
    let format = FormatOptions {
        filters,
        collapse: cruise_collapse(config),
        options: Map::new(),
    };
    let Evaluation {
        document: engine,
        vacuous,
        rule_stats,
        expired,
        unmatched_known,
    } = evaluation;
    // With a native configuration, the rules were evaluated over the whole graph and the report
    // keeps what touches the closure (crate::affected::Selection::narrow).
    let native = options
        .affected
        .as_ref()
        .filter(|s| s.mode == crate::affected::Mode::Closure)
        .map(|selection| selection.narrow(&engine));
    // With nothing to filter, collapse or narrow, the re-summary finds what the engine just
    // summarised, so it is skipped and the engine's document is the report's, held once: on a
    // large graph the re-summary is most of a guard's check (NFR-PERF-03) and a second copy most
    // of a run's memory. `the_unfiltered_report_is_the_engine_s_document` holds the two equal.
    let (mut document, whole, native) = match native {
        Some((narrowed, receipt)) => (
            rewrap(narrowed, &format, Some(&config.rules.dependencies))?,
            Some(engine),
            Some(receipt),
        ),
        None if format == FormatOptions::default() => (engine, None, None),
        None => (
            rewrap(
                crate::value::copy(&engine),
                &format,
                Some(&config.rules.dependencies),
            )?,
            Some(engine),
            None,
        ),
    };
    let evaluated = whole.as_ref().unwrap_or(&document);
    let vacuous_rules = evaluated.summary.vacuous_rules.clone();
    let affected = native
        .or_else(|| upstream.map(|(selection, pattern)| selection.receipt(evaluated, &pattern)));
    document.summary.inspected = Some(inspected);
    document.summary.sidecar = sidecar;
    document.summary.vacuous_rules = vacuous_rules;
    document.summary.affected = affected;
    Ok(Run {
        vacuous,
        rule_stats,
        expired,
        unmatched_known,
        whole,
        document,
        warnings: Vec::new(),
    })
}

/// The default place a cruise result is saved, and where the agent commands look for it.
pub const SAVED_GRAPH: &str = ".graph/cruise.json";

/// Clears what an earlier evaluation wrote, so a saved graph is evaluated as if just extracted.
pub fn reset(document: &mut GraphDocument) {
    for module in &mut document.modules {
        module.valid = true;
        module.rules = None;
        module.dependents = None;
        module.orphan = None;
        module.reachable = None;
        module.reaches = None;
        module.instability = None;
        module.matches_focus = None;
        module.matches_reaches = None;
        module.matches_highlight = None;
        for dependency in &mut module.dependencies {
            dependency.valid = true;
            dependency.rules = None;
            dependency.circular = false;
            dependency.cycle = None;
            dependency.instability = None;
        }
    }
    document.folders = None;
    document.summary = rb_model::Summary::default();
}

/// Reads a saved graph (a Rulebearing or dependency-cruiser result).
///
/// # Errors
/// A message naming the file when it is missing or not a result.
pub fn load_graph(ctx: &Context<'_>, file: &str) -> Result<GraphDocument, String> {
    let path = ctx.resolve(file);
    let text = std::fs::read_to_string(&path).map_err(|e| format!("cannot read the graph {}: {e}; run `rulebearing cruise -T json -f {SAVED_GRAPH}` first, or pass --graph", path.display()))?;
    rb_ingest::dependency_cruiser::read(&text)
        .map_err(|e| format!("{} is not a cruise result: {e}", path.display()))
}

/// The graph a query command works on: `--graph FILE`, else the saved graph when there is one,
/// else a fresh extraction of `paths`. The result is un-annotated and ready to evaluate.
///
/// # Errors
/// A message when the graph cannot be read or extracted.
pub fn query_graph(
    ctx: &Context<'_>,
    config: &Config,
    graph: Option<&str>,
    paths: &[String],
) -> Result<GraphDocument, String> {
    let saved = ctx.resolve(SAVED_GRAPH);
    let mut document = match graph {
        Some(file) => load_graph(ctx, file)?,
        None if paths.is_empty() && saved.is_file() => load_graph(ctx, SAVED_GRAPH)?,
        None => extract(ctx, config, paths).map_err(|e| e.to_string())?,
    };
    reset(&mut document);
    Ok(document)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RunExit;

    /// Three modules, with one forbidden edge from `src/a.ts` to `lib/c.ts`.
    fn graph() -> GraphDocument {
        let module = |source: &str, to: &[&str]| rb_model::Module {
            dependencies: to
                .iter()
                .map(|t| rb_model::Dependency::new(*t, *t, rb_model::ModuleSystem::Es6))
                .collect(),
            ..rb_model::Module::new(source)
        };
        GraphDocument {
            modules: vec![
                module("src/a.ts", &["src/b.ts", "lib/c.ts"]),
                module("src/b.ts", &[]),
                module("lib/c.ts", &[]),
            ],
            ..GraphDocument::default()
        }
    }

    fn evaluated(collapse: Option<&str>) -> Result<Run, Box<dyn std::error::Error>> {
        let rules = serde_json::json!({ "forbidden": [{ "name": "not-to-lib", "severity": "error",
            "from": { "path": "^src/" }, "to": { "path": "^lib/" } }] });
        let mut config = rb_config::load::from_canonical(
            rules.as_object().cloned().unwrap_or_default(),
            rb_config::CompatMode::Native,
        )?;
        config.options.collapse = collapse.map(|c| serde_json::Value::String(c.to_owned()));
        let mut stdin = std::io::empty();
        let ctx = Context {
            cwd: std::env::temp_dir(),
            stdin: &mut stdin,
            today: chrono::NaiveDate::default(),
            timestamp: String::new(),
            color_terminal: false,
        };
        let options = RunOptions {
            liveness: false,
            options_used: Map::new(),
            paths: Vec::new(),
            affected: None,
        };
        Ok(evaluate_document(
            &ctx,
            &config,
            graph(),
            &options,
            &mut Progress::new(None),
        )?)
    }

    #[test]
    fn an_unfiltered_run_holds_the_graph_once_and_a_collapsed_one_keeps_the_whole_beside_it()
    -> Result<(), Box<dyn std::error::Error>> {
        // Nothing filters: the report's document is the whole graph, and there is no other.
        let mut plain = evaluated(None)?;
        assert_eq!(plain.evaluated().modules.len(), 3);
        assert_eq!(plain.document.modules.len(), 3);
        assert_eq!(plain.error_count(), 1);
        assert!(plain.take_whole().is_none());
        let sources = |document: &GraphDocument| -> Vec<String> {
            document.modules.iter().map(|m| m.source.clone()).collect()
        };
        let whole = sources(&plain.document);
        let evaluation = plain.into_evaluation();
        assert_eq!(sources(&evaluation.document), whole);
        assert_eq!(evaluation.error_count(), 1);

        // Collapsed to folders: the report shows two modules, and the whole graph stays for
        // what counts over it.
        let mut collapsed = evaluated(Some("^[^/]+"))?;
        assert_eq!(sources(&collapsed.document), ["lib", "src"]);
        assert_eq!(sources(collapsed.evaluated()), whole);
        assert_eq!(collapsed.error_count(), 1);
        let kept = collapsed.clone().into_evaluation();
        assert_eq!(sources(&kept.document), whole);
        assert_eq!(collapsed.take_whole().map(|d| sources(&d)), Some(whole));
        // Once taken, the report's document is all that is left.
        assert_eq!(sources(collapsed.evaluated()), ["lib", "src"]);
        Ok(())
    }

    #[test]
    fn a_stopped_run_exits_as_adr_0008_says() -> Result<(), Box<dyn std::error::Error>> {
        let Err(json) = serde_json::from_str::<u8>("x") else {
            return Err("`x` is not a number".into());
        };
        let element = rb_rules::elements::ElementError::Pattern {
            rule: "r".into(),
            pattern: "(".into(),
        };
        let table = [
            (
                RunError::Config(ConfigError::Invalid("bad".into())),
                RunExit::InvalidConfig,
            ),
            (
                RunError::Engine(EngineError::Element(element)),
                RunExit::InvalidConfig,
            ),
            (
                RunError::Plugin(crate::plugin::Failure::ExitCode {
                    name: "p".into(),
                    code: 0.5,
                }),
                RunExit::InvalidConfig,
            ),
            (
                RunError::Extract(ExtractError::NoModulesFound),
                RunExit::Untrustworthy,
            ),
            (
                RunError::Engine(EngineError::Document(json)),
                RunExit::Untrustworthy,
            ),
            (RunError::Report("no dot".into()), RunExit::Untrustworthy),
        ];
        for (error, exit) in table {
            assert_eq!(error.exit(), exit, "{error}");
        }
        Ok(())
    }
}
