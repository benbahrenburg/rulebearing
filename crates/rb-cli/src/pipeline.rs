//! The five stages as one call: configuration, extraction, evaluation, the report's re-summary,
//! and the receipt. `cruise` runs it; the agent commands run it when they are not given a saved
//! graph.
//!
//! - Architecture: [The five stages](../../../docs/architecture.md#the-five-stages)
//! - Source: [design § The five stages](../../../docs/artifacts/design.md#the-five-stages)
//! - Plan: [Wave 1 § 1.4](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#14-architecture-of-what-this-wave-builds)
//!   (the path a gate takes)
//! - Requirements: [FR-CORE-02](../../../docs/prd.md#fr-core-02), [FR-CORE-04](../../../docs/prd.md#fr-core-04)

#[cfg(any(feature = "extract-ts", feature = "extract-python"))]
use std::path::PathBuf;

use rb_config::{Config, ConfigError};
#[cfg(any(feature = "extract-dotnet", feature = "extract-python"))]
use rb_model::Language;
use rb_model::{ExtractError, GraphDocument, Inspected, Receipt};
use rb_rules::graph::filters::{Filter, Filters};
use rb_rules::rewrap::{FormatOptions, rewrap};
use rb_rules::{EngineError, EvalOptions, Evaluation, evaluate};
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
}

/// A finished run.
#[derive(Debug, Clone)]
pub struct Run {
    /// The engine's result (vacuous rules, statistics, expired entries).
    pub evaluation: Evaluation,
    /// The document as the report shows it.
    pub document: GraphDocument,
    /// Extraction problems that did not stop the run, as `path: message` lines.
    pub warnings: Vec<String>,
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

/// One extractor's result folded into the run's document.
#[derive(Debug, Default)]
struct Merged {
    modules: Vec<rb_model::Module>,
    code: Option<rb_model::CodeLayer>,
    inspected: Inspected,
    warnings: Vec<rb_model::Warning>,
}

#[cfg(any(feature = "extract-dotnet", feature = "extract-python"))]
impl Merged {
    /// Adds one extraction, its receipt keyed by `language`.
    fn add(&mut self, language: Language, extraction: rb_model::Extraction) {
        self.modules.extend(extraction.modules);
        if let Some(code) = extraction.code {
            self.code.get_or_insert_with(Default::default).merge(code);
        }
        self.inspected.insert(language, extraction.inspected);
        self.warnings.extend(extraction.warnings);
    }

    /// Adds `result` unless the extractor found nothing to read; any other error stops the run.
    fn take(
        &mut self,
        language: Language,
        result: Result<rb_model::Extraction, ExtractError>,
    ) -> Result<(), ExtractError> {
        match result {
            Ok(extraction) => {
                self.add(language, extraction);
                Ok(())
            }
            Err(ExtractError::NoModulesFound) => Ok(()),
            Err(error) => Err(error),
        }
    }
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

/// `options.exclude` and `includeOnly` applied to the modules of an extractor that does not apply
/// them while it walks, with the same filter code the report uses.
#[cfg(any(feature = "extract-dotnet", feature = "extract-python"))]
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
#[cfg_attr(
    not(any(
        feature = "extract-ts",
        feature = "extract-dotnet",
        feature = "extract-python"
    )),
    expect(unused_variables, reason = "a build with no extractor reads nothing")
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
pub fn extract_with_warnings(
    ctx: &Context<'_>,
    config: &Config,
    paths: &[String],
) -> Result<(GraphDocument, Vec<rb_model::Warning>), ExtractError> {
    let mut merged = Merged::default();
    #[cfg(feature = "extract-ts")]
    {
        let roots: Vec<PathBuf> = if paths.is_empty() {
            vec![PathBuf::from(".")]
        } else {
            paths.iter().map(PathBuf::from).collect()
        };
        let (settings, mut resolve) =
            rb_extract_ts::prepare(&config.languages.typescript, &ctx.cwd)?;
        // Licences and deprecations are read from package.json only when a rule asks for them, as
        // upstream's ruleSetHasLicenseRule and ruleSetHasDeprecationRule decide.
        resolve.resolve_licenses = rb_rules::derive::has_license_rule(&config.rules.dependencies);
        resolve.resolve_deprecations =
            rb_rules::derive::has_deprecation_rule(&config.rules.dependencies);
        match rb_extract_ts::extract_with(&roots, &settings, &resolve) {
            // The TypeScript receipt keeps its wave 1 shape: counted per language from the modules.
            Ok(extraction) => {
                merged.modules.extend(extraction.modules);
                merged.warnings.extend(extraction.warnings);
            }
            Err(ExtractError::NoModulesFound) => {}
            Err(error) => return Err(error),
        }
    }
    #[cfg(feature = "extract-dotnet")]
    {
        use rb_model::Extractor as _;
        let solution = |name: &str| {
            std::path::Path::new(name)
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("sln") || e.eq_ignore_ascii_case("slnx"))
        };
        if config.languages.dotnet.is_some() || has_root_file(&ctx.cwd, solution) {
            let options = config.languages.dotnet.clone().unwrap_or_default();
            let result = rb_extract_dotnet::DotnetExtractor
                .extract(std::slice::from_ref(&ctx.cwd), &options);
            let mut before = std::mem::take(&mut merged.modules);
            merged.take(Language::Dotnet, result)?;
            let added = filter_paths(config, std::mem::take(&mut merged.modules));
            before.extend(added);
            merged.modules = before;
        }
    }
    #[cfg(feature = "extract-python")]
    {
        let project = |name: &str| matches!(name, "pyproject.toml" | "setup.cfg" | "setup.py");
        if config.languages.python.is_some() || has_root_file(&ctx.cwd, project) {
            let options = config.languages.python.clone().unwrap_or_default();
            let inputs: Vec<PathBuf> = if paths.is_empty() {
                vec![PathBuf::from(".")]
            } else {
                paths.iter().map(PathBuf::from).collect()
            };
            let virtual_env = std::env::var_os("VIRTUAL_ENV").map(PathBuf::from);
            let result =
                rb_extract_python::extract_at(&ctx.cwd, &inputs, &options, virtual_env.as_deref());
            let mut before = std::mem::take(&mut merged.modules);
            merged.take(Language::Python, result)?;
            let added = filter_paths(config, std::mem::take(&mut merged.modules));
            before.extend(added);
            merged.modules = before;
        }
    }
    if merged.modules.is_empty() {
        return Err(ExtractError::NoModulesFound);
    }
    // The TypeScript modules keep dependency-cruiser's visiting order; each other extractor's
    // follow, already sorted by source.
    let mut document = GraphDocument {
        modules: merged.modules,
        code: merged.code.map(|mut code| {
            code.normalise();
            code
        }),
        ..GraphDocument::default()
    };
    if !merged.inspected.is_empty() {
        document.summary.inspected = Some(merged.inspected);
    }
    Ok((document, merged.warnings))
}

/// The report filters `cruise` applies after evaluation: `reaches` (the rest were applied while
/// extracting and evaluating, and reapplying them changes nothing).
fn cruise_filters(config: &Config) -> Filters {
    Filters {
        reaches: config.options.reaches.as_ref().map(|r| Filter {
            path: r.path.clone(),
            depth: None,
        }),
        ..Filters::default()
    }
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
    let eval_options = EvalOptions {
        liveness: options.liveness,
        validate: true,
        metrics: config.options.metrics.unwrap_or(false),
        today: ctx.today,
        args: options.paths.clone(),
        options_used: options.options_used.clone(),
    };
    let evaluation = evaluate(document, config, &eval_options)?;
    progress.stage("evaluate");
    let format = FormatOptions {
        filters: cruise_filters(config),
        collapse: None,
        options: Map::new(),
    };
    let mut document = rewrap(
        evaluation.document.clone(),
        &format,
        Some(&config.rules.dependencies),
    )?;
    document.summary.inspected = Some(inspected);
    document
        .summary
        .vacuous_rules
        .clone_from(&evaluation.document.summary.vacuous_rules);
    Ok(Run {
        evaluation,
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
