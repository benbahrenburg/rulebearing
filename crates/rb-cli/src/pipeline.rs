//! The five stages as one call: configuration, extraction, evaluation, the report's re-summary,
//! and the receipt. `cruise` runs it; the agent commands run it when they are not given a saved
//! graph.
//!
//! - Architecture: [The five stages](../../../docs/architecture.md#the-five-stages)
//! - Source: [design § The five stages](../../../docs/artifacts/design.md#the-five-stages)
//! - Plan: [Wave 1 § 1.4](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#14-architecture-of-what-this-wave-builds)
//!   (the path a gate takes)
//! - Requirements: [FR-CORE-02](../../../docs/prd.md#fr-core-02), [FR-CORE-04](../../../docs/prd.md#fr-core-04)

#[cfg(feature = "extract-ts")]
use std::path::PathBuf;

use rb_config::{Config, ConfigError};
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
}

/// The receipt: per language, the files and modules extracted.
pub fn receipt(document: &GraphDocument) -> Inspected {
    let mut inspected = Inspected::new();
    for module in &document.modules {
        if let Some(language) = module.language {
            let entry = inspected.entry(language).or_insert(Receipt {
                files: 0,
                assemblies: 0,
                modules: 0,
            });
            entry.files += 1;
            entry.modules += 1;
        }
    }
    inspected
}

/// Extracts the TypeScript and JavaScript graph under `paths`.
///
/// # Errors
/// [`ExtractError`] for an empty cruise or an unsupported file.
#[cfg(feature = "extract-ts")]
pub fn extract(
    ctx: &Context<'_>,
    config: &Config,
    paths: &[String],
) -> Result<GraphDocument, ExtractError> {
    let roots: Vec<PathBuf> = if paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        paths.iter().map(PathBuf::from).collect()
    };
    let (settings, resolve) = rb_extract_ts::prepare(&config.languages.typescript, &ctx.cwd)?;
    let extraction = rb_extract_ts::extract_with(&roots, &settings, &resolve)?;
    Ok(GraphDocument {
        modules: extraction.modules,
        ..GraphDocument::default()
    })
}

/// Extraction in a build without the TypeScript extractor: nothing can be read, so the run cannot
/// be trusted.
///
/// # Errors
/// Always [`ExtractError::NoModulesFound`].
#[cfg(not(feature = "extract-ts"))]
pub fn extract(
    _ctx: &Context<'_>,
    _config: &Config,
    _paths: &[String],
) -> Result<GraphDocument, ExtractError> {
    Err(ExtractError::NoModulesFound)
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
    let document = extract(ctx, config, &options.paths)?;
    progress.stage("extract");
    evaluate_document(ctx, config, document, options, progress)
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
    })
}
