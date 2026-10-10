//! What the playground's exports do, in plain Rust so the native tests reach every line: a
//! configuration and a cruise result in, a report out, through the same library calls the
//! command line makes for `cruise --graph`.
//!
//! - Plan: [Wave 4, Step 2](../../../docs/plans/pending/0004-wave-4-reach.md#21-steps-for-sub-wave-4a-docs-site-playground-cookbook-live-tables)
//!   (`evaluate(config, cruise_json, output_type)`: "rules over a pasted graph document")
//! - Contract: [plan 0004 § 1.6](../../../docs/plans/pending/0004-wave-4-reach.md#16-interfaces-and-contracts-this-wave-freezes)
//! - Requirement: [FR-REACH-01](../../../docs/prd.md#fr-reach-01)
//!
//! A browser has no file system, so the configuration is text: JavaScript when it assigns
//! `module.exports` or has an `export default`, JSON when it opens with `{`, YAML otherwise.
//! `extends` of a file therefore cannot be followed and is an error that says so; the bundled
//! presets (`rulebearing:recommended` and the rest) resolve as they do on the command line.

use std::path::Path;

use chrono::NaiveDate;
use rb_config::Config;
use rb_config::load::LoadOptions;
use rb_config::read::Syntax;
use rb_rules::rewrap::{FormatOptions, rewrap};
use rb_rules::{EvalOptions, evaluate as evaluate_rules, reset};

/// Why the playground could not answer. The command line's exit code for each is in its doc.
#[derive(Debug, thiserror::Error)]
pub enum PlaygroundError {
    /// The configuration did not load (the command line's exit 3).
    #[error("the configuration did not load: {0}")]
    Config(String),
    /// The pasted text is not a cruise result (exit 2).
    #[error("the graph is not a cruise result: {0}")]
    Graph(String),
    /// The engine refused the graph or the rules (exit 2).
    #[error("the rules could not be evaluated: {0}")]
    Engine(String),
    /// No reporter has this name (exit 3).
    #[error("{0}")]
    Report(String),
}

/// The moment a playground run is evaluated at: the day `expires` and lifecycle dates are
/// compared with, and the timestamp a reporter that prints one shows. The page passes the
/// browser's clock; the tests pass a fixed one.
#[derive(Debug, Clone)]
pub struct Now {
    /// Today.
    pub today: NaiveDate,
    /// An ISO 8601 timestamp.
    pub timestamp: String,
}

/// The syntax of configuration text, from its content: there is no file name to go by.
pub fn syntax_of(text: &str) -> Syntax {
    let trimmed = text.trim_start();
    if text.contains("module.exports") || text.contains("export default") {
        Syntax::JavaScript
    } else if trimmed.starts_with('{') {
        Syntax::Json
    } else {
        Syntax::Yaml
    }
}

/// The folder a pasted configuration is read from, and the JavaScript sandbox's root. It holds
/// nothing: a browser has no file system, and natively the folder does not exist, so a
/// configuration can `require` or `extends` no file, and the sandbox never reaches past it
/// ([ADR-0006](../../../docs/adr/0006-embedded-quickjs-config-evaluator.md)).
pub const ROOT: &str = "/playground";

/// Loads configuration text with no file system, rooted at [`ROOT`].
///
/// # Errors
/// [`PlaygroundError::Config`] with the loader's reason.
pub fn load_config(text: &str) -> Result<Config, PlaygroundError> {
    let root = Path::new(ROOT);
    let options = LoadOptions {
        root: Some(root.to_path_buf()),
        ..LoadOptions::default()
    };
    rb_config::load::load_text(text, syntax_of(text), root, &options)
        .map_err(|e| PlaygroundError::Config(e.to_string()))
}

/// `evaluate(config, cruise_json, output_type)`: the configuration's rules over the pasted cruise
/// result, rendered by `output_type`, as `rulebearing cruise --graph <file> -T <output_type>`
/// renders it.
///
/// # Errors
/// A [`PlaygroundError`] naming what failed.
pub fn evaluate(
    config: &str,
    cruise_json: &str,
    output_type: &str,
    now: &Now,
) -> Result<String, PlaygroundError> {
    if !rb_report::OUTPUT_TYPES
        .iter()
        .any(|(name, _)| *name == output_type)
    {
        let names: Vec<&str> = rb_report::OUTPUT_TYPES.iter().map(|(n, _)| *n).collect();
        return Err(PlaygroundError::Report(format!(
            "no output type `{output_type}`; the output types are {}",
            names.join(", ")
        )));
    }
    let config = load_config(config)?;
    let mut document = rb_ingest::dependency_cruiser::read(cruise_json)
        .map_err(|e| PlaygroundError::Graph(e.to_string()))?;
    reset(&mut document);
    let options = EvalOptions {
        liveness: true,
        validate: true,
        metrics: config.options.metrics.unwrap_or(false),
        today: now.today,
        args: Vec::new(),
        options_used: serde_json::Map::new(),
    };
    let evaluation = evaluate_rules(document, &config, &options)
        .map_err(|e| PlaygroundError::Engine(e.to_string()))?;
    let format = rb_rules::rewrap::report_format(&config);
    let mut document = if format == FormatOptions::default() {
        evaluation.document
    } else {
        let vacuous = evaluation.document.summary.vacuous_rules.clone();
        let mut rewrapped = rewrap(
            evaluation.document,
            &format,
            Some(&config.rules.dependencies),
        )
        .map_err(|e| PlaygroundError::Engine(e.to_string()))?;
        rewrapped.summary.vacuous_rules = vacuous;
        rewrapped
    };
    document.summary.inspected = None;
    let value =
        serde_json::to_value(&document).map_err(|e| PlaygroundError::Engine(e.to_string()))?;
    let report = rb_report::ReportOptions {
        timestamp: now.timestamp.clone(),
        collapse_pattern: format.collapse.clone(),
        ..rb_report::ReportOptions::default()
    };
    rb_report::render(output_type, &value, &report)
        .map(|rendered| rendered.output)
        .map_err(|e| PlaygroundError::Report(e.to_string()))
}

/// `version()`: the Rulebearing version the playground was built from.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
