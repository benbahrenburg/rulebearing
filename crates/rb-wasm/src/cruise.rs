//! What `cruise_ts` and `cruise_py` do, in plain Rust so the native tests reach every line: a
//! dropped small repository (paths and source text) and a configuration in, the report
//! `rulebearing cruise -T <output_type>` prints for the same files out.
//!
//! - Plan: [Wave 4, Step 2](../../../docs/plans/pending/0004-wave-4-reach.md#21-steps-for-sub-wave-4a-docs-site-playground-cookbook-live-tables)
//!   ("`cruise_ts(files: Map, config: &str)` and `cruise_py(files: Map, config: &str)` (extract and
//!   evaluate a dropped small repository)")
//! - Contract: [plan 0004 § 1.6](../../../docs/plans/pending/0004-wave-4-reach.md#16-interfaces-and-contracts-this-wave-freezes)
//! - Decisions: [ADR-0073](../../../docs/adr/0073-extractors-read-files-through-rb-model.md) (the
//!   extractors read the files from memory), [ADR-0067](../../../docs/adr/0067-the-playground-facade-and-an-optional-javascript-engine.md)
//!   (the facade and the crates it links), [ADR-0014](../../../docs/adr/0014-no-invented-cross-language-edges.md)
//!   (one language per call: nothing links the two)
//! - Requirement: [FR-REACH-01](../../../docs/prd.md#fr-reach-01)
//!
//! The files sit under [`ROOT`], the working directory of the run, so a module's `source` is the
//! path the page gave it. The steps are `cruise`'s with no flags and no cache: the
//! configuration's liveness (strict for a native configuration, a warning for a
//! dependency-cruiser one), metrics when the reporter or a rule wants them, the extraction, the
//! evaluation, the report's filters and collapse, and the reporter. What the command line prints
//! on standard error (the extractors' warnings) has no place in the one string returned. What
//! needs the disk or a process is refused by name: the Node sidecar, Yarn Plug'n'Play, and a
//! ratchet's budget file.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rb_config::{CompatMode, Config};
use rb_model::files::lexical;
use rb_model::{
    ExtractError, Extraction, Files, GraphDocument, Inspected, Language, MemoryTree, Receipt,
};
use rb_rules::graph::filters::{Filter, Filters};
use rb_rules::rewrap::{report_format, rewrap};
use rb_rules::{EvalOptions, evaluate};
use serde_json::Value;

use crate::playground::{Now, PlaygroundError, ROOT, check_output_type, load_config};

/// The extractor a playground cruise runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Extractor {
    /// TypeScript and JavaScript (`rb-extract-ts`).
    TypeScript,
    /// Python (`rb-extract-python`).
    Python,
}

/// The tree of `files` (path to source text) under [`ROOT`]. A path may start with `./` or `/`;
/// either way it is taken from the root, and one that climbs out of it is refused.
///
/// # Errors
/// [`PlaygroundError::Files`] naming the path.
pub fn file_tree(files: &BTreeMap<String, String>) -> Result<MemoryTree, PlaygroundError> {
    let root = Path::new(ROOT);
    let mut tree = MemoryTree::new();
    for (name, text) in files {
        let relative = name.replace('\\', "/");
        let path = lexical(&root.join(relative.trim_start_matches('/')));
        if path == root || !path.starts_with(root) {
            return Err(PlaygroundError::Files(format!(
                "`{name}` is not a file inside the dropped folder; give paths relative to it"
            )));
        }
        tree.insert(&path, text.as_bytes())
            .map_err(|e| PlaygroundError::Files(e.to_string()))?;
    }
    Ok(tree)
}

/// `cruise_ts` and `cruise_py`: `files` extracted by `extractor`, the configuration's rules over
/// the graph, rendered by `output_type`.
///
/// # Errors
/// A [`PlaygroundError`] naming what failed.
pub fn cruise(
    extractor: Extractor,
    files: &BTreeMap<String, String>,
    config: &str,
    output_type: &str,
    now: &Now,
) -> Result<String, PlaygroundError> {
    check_output_type(output_type)?;
    let mut config = load_config(config)?;
    if let Some(ratchet) = config.rules.ratchets.first() {
        return Err(PlaygroundError::Config(format!(
            "ratchet `{}` counts against its budget file {}, which the playground does not read; \
             run the command line",
            ratchet.name, ratchet.budget
        )));
    }
    // `cruise` without `--metrics`, as the command line lays its flags over the configuration.
    config.options.metrics = Some(rb_rules::evaluate::wants_metrics(
        &config,
        false,
        output_type,
    ));
    let options_used =
        rb_config::options_used::options_used(Some(&config), Path::new(ROOT), output_type, "-");
    let files = Files::memory(file_tree(files)?);
    let extraction = match extractor {
        Extractor::TypeScript => typescript(&config, &files),
        Extractor::Python => python(&config, &files),
    }
    .map_err(|e| PlaygroundError::Extract(e.to_string()))?;
    let document = graph(&config, extractor, extraction)
        .map_err(|e| PlaygroundError::Extract(e.to_string()))?;
    // A native configuration's vacuous rule fails the run; a dependency-cruiser one's is a
    // warning, marked in the summary (ADR-0007, ADR-0032).
    let warn = config.compat == CompatMode::DependencyCruiser;
    let mut document = evaluated(&config, document, options_used, now)?;
    if warn {
        for entry in document.summary.vacuous_rules.iter_mut().flatten() {
            entry.severity = Some("warn".into());
        }
    }
    crate::playground::render(&document, output_type, &config, now)
}

/// The TypeScript and JavaScript extraction, as `cruise` makes it from the working directory.
fn typescript(config: &Config, files: &Files) -> Result<Extraction, ExtractError> {
    let (mut settings, mut resolve) =
        rb_extract_ts::prepare_in(&config.languages.typescript, Path::new(ROOT), files)?;
    // Markdown fences are a native addition (ADR-0036).
    settings.markdown_fences = config.compat == CompatMode::Native;
    if let Some(block) = config
        .options
        .webpack_config_json
        .as_ref()
        .and_then(Value::as_object)
    {
        rb_extract_ts::resolve::apply_resolve_block(&mut resolve, block);
    }
    resolve.resolve_licenses = rb_rules::derive::has_license_rule(&config.rules.dependencies);
    resolve.resolve_deprecations =
        rb_rules::derive::has_deprecation_rule(&config.rules.dependencies);
    rb_extract_ts::extract_with(&[PathBuf::from(".")], &settings, &resolve)
}

/// The Python extraction, with no virtual environment but one the files hold.
fn python(config: &Config, files: &Files) -> Result<Extraction, ExtractError> {
    let options = config.languages.python.clone().unwrap_or_default();
    let settings = rb_extract_python::prepare_in(files, Path::new(ROOT), &options, None)?;
    rb_extract_python::extract_with(&[PathBuf::from(".")], &settings)
}

/// The document of one extraction, as the command line merges it: a Python graph through the
/// configuration's `exclude` and `includeOnly`, which the TypeScript walk applied itself; the
/// code layer normalised; the receipt.
fn graph(
    config: &Config,
    extractor: Extractor,
    extraction: Extraction,
) -> Result<GraphDocument, ExtractError> {
    let Extraction {
        modules,
        code,
        inspected,
        sidecar,
        ..
    } = extraction;
    let mut written = Inspected::new();
    let modules = match extractor {
        Extractor::TypeScript => modules,
        Extractor::Python => {
            written.insert(Language::Python, inspected);
            filter_paths(config, modules)
        }
    };
    if modules.is_empty() {
        return Err(ExtractError::NoModulesFound);
    }
    let mut document = GraphDocument {
        modules,
        code: code.map(|mut code| {
            code.normalise();
            code
        }),
        ..GraphDocument::default()
    };
    if !written.is_empty() {
        document.summary.inspected = Some(written);
    }
    document.summary.sidecar = sidecar;
    Ok(document)
}

/// `options.exclude` and `includeOnly`, with the filter code the report uses.
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

/// The receipt: per language, the files and modules extracted; an extractor's own receipt kept.
pub fn receipt(document: &GraphDocument) -> Inspected {
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

/// The engine over an extracted document, and the report's re-summary, as `cruise` makes them.
fn evaluated(
    config: &Config,
    document: GraphDocument,
    options_used: serde_json::Map<String, Value>,
    now: &Now,
) -> Result<GraphDocument, PlaygroundError> {
    let inspected = receipt(&document);
    let sidecar = document.summary.sidecar.clone();
    let options = EvalOptions {
        liveness: true,
        validate: true,
        metrics: config.options.metrics.unwrap_or(false),
        today: now.today,
        args: Vec::new(),
        options_used,
    };
    let engine = evaluate(document, config, &options)
        .map_err(|e| PlaygroundError::Engine(e.to_string()))?
        .document;
    let format = report_format(config);
    let mut document = if format == rb_rules::rewrap::FormatOptions::default() {
        engine
    } else {
        let vacuous = engine.summary.vacuous_rules.clone();
        let mut shown = rewrap(engine, &format, Some(&config.rules.dependencies))
            .map_err(|e| PlaygroundError::Engine(e.to_string()))?;
        shown.summary.vacuous_rules = vacuous;
        shown
    };
    document.summary.inspected = Some(inspected);
    document.summary.sidecar = sidecar;
    Ok(document)
}
