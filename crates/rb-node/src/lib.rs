//! `rb-node`: the napi-rs binding behind the `rulebearing` npm package's main export, so a script
//! that calls dependency-cruiser as a library switches by changing its `import`.
//!
//! - Architecture: [`docs/architecture.md#distribution`](../../../docs/architecture.md#distribution)
//! - Decisions: [ADR-0002](../../../docs/adr/0002-rust-as-implementation-language.md),
//!   [ADR-0020](../../../docs/adr/0020-single-name-across-registries.md),
//!   [ADR-0062](../../../docs/adr/0062-the-node-binding-reads-typescript-and-babel-configs-with-the-callers-packages.md)
//! - Plan: [Wave 3, Step 22](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#26-steps-for-sub-wave-3f-the-roslyn-analyzer-and-rb-node)
//! - Requirement: [FR-DIST-02](../../../docs/prd.md#fr-dist-02)
//! - Specification: [coverage § Programmatic API](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#programmatic-api)
//!
//! Each export is a thin call into [`rb_cli::api`], the command line's own code reached without
//! argv; this crate holds no behaviour of its own, only the conversion between JavaScript values
//! and Rust ones, and runs the calls that read the disk on libuv's thread pool, so each returns a
//! promise as dependency-cruiser's does. `index.js` beside this crate is the package's entry: it
//! loads the addon, adds `extractTSConfig` and `extractBabelConfig`, which call the caller's own
//! `typescript` and `@babel/core` as dependency-cruiser does (ADR-0062), and makes
//! `allExtensions` the array dependency-cruiser exports.
//!
//! `unsafe_code` is `deny` here rather than the workspace's `forbid` (`Cargo.toml`): `#[napi]`
//! expands to N-API glue under its own `#[allow(unsafe_code)]`. Nothing in this file is `unsafe`.

use std::io::Read;

use napi::bindgen_prelude::{AsyncTask, Env, Result, Status, Task};
use napi_derive::napi;
use rb_cli::Context;
use rb_cli::api::{self, ApiError, Output, Reported};
use serde_json::Value;

/// A call's context: the process's working directory, no standard input, no colour.
fn with_context<T>(call: impl FnOnce(&mut Context<'_>) -> T) -> Result<T> {
    let cwd = std::env::current_dir().map_err(|e| failure(&e.to_string()))?;
    let (today, timestamp) = rb_cli::context::clock();
    let mut stdin = std::io::empty();
    let mut ctx = Context {
        cwd,
        stdin: &mut stdin as &mut dyn Read,
        today,
        timestamp,
        color_terminal: false,
        warm: None,
    };
    Ok(call(&mut ctx))
}

fn failure(message: &str) -> napi::Error {
    napi::Error::new(Status::GenericFailure, message.to_owned())
}

fn rejected(error: &ApiError) -> napi::Error {
    failure(&error.message)
}

/// dependency-cruiser's `IReporterOutput`.
#[napi(object)]
pub struct ReporterOutput {
    /// The cruise result (no `outputType`), or the reporter's text.
    pub output: Value,
    /// The reporter's exit code.
    pub exit_code: f64,
}

#[allow(
    clippy::cast_precision_loss,
    reason = "a count of violations is far below 2^53"
)]
fn reporter_output(reported: Reported) -> ReporterOutput {
    // What the command line prints on stderr goes where it would.
    if !reported.warnings.is_empty() {
        eprint!("{}", reported.warnings);
    }
    ReporterOutput {
        output: match reported.output {
            Output::Result(result) => result,
            Output::Text(text) => Value::String(text),
        },
        exit_code: reported.exit_code as f64,
    }
}

/// `cruise()` off the main thread.
pub struct Cruise {
    files: Vec<String>,
    options: Option<Value>,
    resolve: Option<Value>,
    transpile: Option<Value>,
}

impl Task for Cruise {
    type Output = Reported;
    type JsValue = ReporterOutput;

    fn compute(&mut self) -> Result<Reported> {
        with_context(|ctx| {
            api::cruise(
                ctx,
                &self.files,
                self.options.as_ref(),
                self.resolve.as_ref(),
                self.transpile.as_ref(),
            )
        })?
        .map_err(|e| rejected(&e))
    }

    fn resolve(&mut self, _env: Env, output: Reported) -> Result<ReporterOutput> {
        Ok(reporter_output(output))
    }
}

/// Cruises `files` (files, folders or globs) with dependency-cruiser's cruise options, resolve
/// options and transpile options.
#[napi(ts_return_type = "Promise<IReporterOutput>")]
pub fn cruise(
    files: Vec<String>,
    options: Option<Value>,
    resolve_options: Option<Value>,
    transpile_options: Option<Value>,
) -> AsyncTask<Cruise> {
    AsyncTask::new(Cruise {
        files,
        options,
        resolve: resolve_options,
        transpile: transpile_options,
    })
}

/// `format()` off the main thread.
pub struct Format {
    result: Value,
    options: Option<Value>,
}

impl Task for Format {
    type Output = Reported;
    type JsValue = ReporterOutput;

    fn compute(&mut self) -> Result<Reported> {
        with_context(|ctx| api::format(ctx, &self.result, self.options.as_ref()))?
            .map_err(|e| rejected(&e))
    }

    fn resolve(&mut self, _env: Env, output: Reported) -> Result<ReporterOutput> {
        Ok(reporter_output(output))
    }
}

/// Formats a cruise result with dependency-cruiser's format options.
#[napi(ts_return_type = "Promise<IReporterOutput>")]
pub fn format(result: Value, options: Option<Value>) -> AsyncTask<Format> {
    AsyncTask::new(Format { result, options })
}

/// What `extractDepcruiseConfig` read: the configuration and, for the caller's `alreadyVisited`
/// set, every configuration file it read.
#[napi(object)]
pub struct Extracted {
    /// The configuration with its `extends` merged.
    pub config: Value,
    /// The configurations read, the named one first.
    pub read: Vec<String>,
}

/// `extractDepcruiseConfig()` off the main thread.
pub struct ExtractDepcruiseConfig {
    spec: String,
    visited: Vec<String>,
    base_dir: Option<String>,
}

impl Task for ExtractDepcruiseConfig {
    type Output = (Value, Vec<String>);
    type JsValue = Extracted;

    fn compute(&mut self) -> Result<(Value, Vec<String>)> {
        with_context(|ctx| {
            api::extract_depcruise_config(ctx, &self.spec, &self.visited, self.base_dir.as_deref())
        })?
        .map_err(|e| rejected(&e))
    }

    fn resolve(&mut self, _env: Env, (config, read): (Value, Vec<String>)) -> Result<Extracted> {
        Ok(Extracted { config, read })
    }
}

/// The dependency-cruiser configuration `fileName` names, with its `extends` merged; `index.js`
/// keeps dependency-cruiser's signature around it.
#[napi(js_name = "extractDepcruiseConfigRead")]
pub fn extract_depcruise_config(
    file_name: String,
    already_visited: Option<Vec<String>>,
    base_directory: Option<String>,
) -> AsyncTask<ExtractDepcruiseConfig> {
    AsyncTask::new(ExtractDepcruiseConfig {
        spec: file_name,
        visited: already_visited.unwrap_or_default(),
        base_dir: base_directory,
    })
}

/// A value a task resolves to; `index.js` hands the caller `value` itself.
#[napi(object)]
pub struct Resolved {
    /// The value.
    pub value: Value,
}

/// `extractWebpackResolveConfig()` off the main thread.
pub struct ExtractWebpackResolveConfig {
    file: String,
    env: Option<Value>,
    arguments: Option<Value>,
}

impl Task for ExtractWebpackResolveConfig {
    type Output = Value;
    type JsValue = Resolved;

    fn compute(&mut self) -> Result<Value> {
        with_context(|ctx| {
            api::extract_webpack_resolve_config(
                ctx,
                &self.file,
                self.env.as_ref(),
                self.arguments.as_ref(),
            )
        })?
        .map_err(|e| rejected(&e))
    }

    fn resolve(&mut self, _env: Env, value: Value) -> Result<Resolved> {
        Ok(Resolved { value })
    }
}

/// The `resolve` block of the webpack configuration `fileName`, evaluated with `env` and
/// `arguments` when it is a function; `index.js` keeps dependency-cruiser's signature around it.
#[napi(js_name = "extractWebpackResolveConfigRead")]
pub fn extract_webpack_resolve_config(
    file_name: String,
    env: Option<Value>,
    arguments: Option<Value>,
) -> AsyncTask<ExtractWebpackResolveConfig> {
    AsyncTask::new(ExtractWebpackResolveConfig {
        file: file_name,
        env,
        arguments,
    })
}

/// dependency-cruiser's `IAvailableTranspiler`.
#[napi(object)]
pub struct AvailableTranspiler {
    /// dependency-cruiser's name for it.
    pub name: String,
    /// The versions it accepts.
    pub version: String,
    /// Whether a cruise from the working directory reads its files.
    pub available: bool,
    /// What reads them, or `-`.
    pub current_version: String,
}

/// The transpilers dependency-cruiser lists, each with whether a cruise from the working
/// directory reads its files, and what reads them.
///
/// # Errors
/// When the working directory cannot be read.
#[napi]
pub fn get_available_transpilers() -> Result<Vec<AvailableTranspiler>> {
    let cwd = std::env::current_dir().map_err(|e| failure(&e.to_string()))?;
    Ok(api::available_transpilers(&cwd)
        .into_iter()
        .map(|t| AvailableTranspiler {
            name: t.name.to_owned(),
            version: t.version.to_owned(),
            available: t.available,
            current_version: t.current_version,
        })
        .collect())
}

/// dependency-cruiser's `IAvailableExtension`.
#[napi(object)]
pub struct AvailableExtension {
    /// The extension, with its dot.
    pub extension: String,
    /// Whether a cruise from the working directory reads it.
    pub available: bool,
}

/// The extensions dependency-cruiser lists, each with whether a cruise from the working directory
/// reads it; `index.js` exports the result as the `allExtensions` array.
///
/// # Errors
/// When the working directory cannot be read.
#[napi(js_name = "listExtensions")]
pub fn all_extensions() -> Result<Vec<AvailableExtension>> {
    let cwd = std::env::current_dir().map_err(|e| failure(&e.to_string()))?;
    Ok(api::all_extensions(&cwd)
        .into_iter()
        .map(|e| AvailableExtension {
            extension: e.extension.to_owned(),
            available: e.available,
        })
        .collect())
}
