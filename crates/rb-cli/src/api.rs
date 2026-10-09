//! dependency-cruiser's programmatic API in Rust: what the Node binding (`rb-node`) exports as
//! `cruise()`, `format()`, `extractDepcruiseConfig`, `extractWebpackResolveConfig`,
//! `getAvailableTranspilers` and `allExtensions`, each the command line's own code reached
//! without argv.
//!
//! - Plan: [Wave 3, Step 22](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#26-steps-for-sub-wave-3f-the-roslyn-analyzer-and-rb-node)
//! - Decisions: [ADR-0062](../../../docs/adr/0062-the-node-binding-reads-typescript-and-babel-configs-with-the-callers-packages.md),
//!   [ADR-0010](../../../docs/adr/0010-crate-layout-and-extractor-boundary.md) (the binding reaches the
//!   workspace through this crate only)
//! - Requirement: [FR-DIST-02](../../../docs/prd.md#fr-dist-02)
//! - Specification: [coverage § Programmatic API](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#programmatic-api);
//!   dependency-cruiser 18.2.0's `types/dependency-cruiser.d.mts` and `types/config-utl/`
//!
//! **`cruise(files, options, resolveOptions, transpileOptions)`.** The options object
//! (`ICruiseOptions`) becomes the configuration a `.dependency-cruiser.json` holding it would be:
//! the rule families of `ruleSet` at the top when `validate` is true (dependency-cruiser applies a
//! rule set only then), and every other key under `options`. That configuration runs through the
//! command line's `cruise` ([`crate::cmd::cruise::answer`]), so the result equals the command
//! line's for the same file. `resolveOptions` is a webpack `resolve` block, what
//! `extractWebpackResolveConfig` returns, applied as `--webpack-config-json` applies one.
//! `transpileOptions.tsConfig` and `.babelConfig` are the objects TypeScript and Babel make from a
//! file; Rulebearing reads the file itself, so each is accepted by the file it records
//! (`options.configFilePath`, `filename`) and refused when that file is missing or differs from the
//! one `options` names. With no `outputType` (dependency-cruiser's identity reporter) the output is
//! the result object and the exit code 0; with one, the reporter's text and the count the command
//! line exits with for it, uncapped.
//!
//! **`format(result, options)`.** `fmt` over the result, with `--exit-code`'s count
//! ([`crate::cmd::fmt::answer`]); the same identity rule for a missing `outputType`.

use std::fmt;
use std::path::Path;

use serde_json::{Map, Value, json};

use crate::cli::{CruiseArgs, FmtArgs};
use crate::context::Context;
use crate::exit::RunExit;
use crate::{Outcome, configure};

/// What a reporter returned: dependency-cruiser's `IReporterOutput`.
#[derive(Debug, Clone, PartialEq)]
pub struct Reported {
    /// The result object (no `outputType`), or the reporter's text.
    pub output: Output,
    /// The reporter's exit code: the count it gates on, or 0.
    pub exit_code: u64,
    /// What the command line would have printed on stderr: warnings, expiries, ratchets.
    pub warnings: String,
}

/// `IReporterOutput.output`.
#[derive(Debug, Clone, PartialEq)]
pub enum Output {
    /// dependency-cruiser's identity reporter: the cruise result itself.
    Result(Value),
    /// Any other reporter's text.
    Text(String),
}

/// Why a call has no answer: the command line's exit code for it and the reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiError {
    /// 2 (the run cannot be trusted) or 3 (the options are invalid).
    pub exit: RunExit,
    /// What the command line prints, without its `rulebearing <command>:` prefix.
    pub message: String,
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ApiError {}

impl ApiError {
    fn invalid(message: impl Into<String>) -> Self {
        Self {
            exit: RunExit::InvalidConfig,
            message: message.into(),
        }
    }

    fn of(outcome: &Outcome) -> Self {
        let exit = if outcome.code == RunExit::InvalidConfig.code() {
            RunExit::InvalidConfig
        } else {
            RunExit::Untrustworthy
        };
        let message = outcome
            .stderr
            .lines()
            .map(|l| {
                l.strip_prefix("rulebearing cruise: ")
                    .or_else(|| l.strip_prefix("rulebearing fmt: "))
                    .unwrap_or(l)
            })
            .collect::<Vec<_>>()
            .join("\n");
        Self { exit, message }
    }
}

/// The reporter dependency-cruiser's identity reporter stands for here: the result, as JSON.
const IDENTITY: &str = "json";

/// The `ICruiseOptions` keys that are not options of the configuration.
const RULE_KEYS: &[&str] = &["ruleSet", "validate"];

/// The options object as the configuration a `.dependency-cruiser.json` holding it would be.
///
/// # Errors
/// [`ApiError`] (exit 3) when `options` is not an object, or `ruleSet` is not one.
pub fn configuration(options: Option<&Value>) -> Result<Map<String, Value>, ApiError> {
    let given = match options {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(map)) => map.clone(),
        Some(_) => return Err(ApiError::invalid("cruise options are an object")),
    };
    let validate = given.get("validate").and_then(Value::as_bool) == Some(true);
    let mut canonical = Map::new();
    let mut from_rule_set = Map::new();
    match given.get("ruleSet") {
        None | Some(Value::Null) => {}
        Some(Value::Object(rule_set)) => {
            for (key, value) in rule_set {
                if key == "options" {
                    if let Value::Object(inner) = value {
                        from_rule_set.clone_from(inner);
                    }
                } else if validate {
                    canonical.insert(key.clone(), value.clone());
                }
            }
        }
        Some(_) => return Err(ApiError::invalid("cruise options: `ruleSet` is an object")),
    }
    let mut merged = from_rule_set;
    for (key, value) in given {
        if !RULE_KEYS.contains(&key.as_str()) {
            merged.insert(key, value);
        }
    }
    canonical.insert("options".into(), Value::Object(merged));
    Ok(canonical)
}

/// `transpileOptions` folded into `options`: the file each object came from becomes the file the
/// run reads.
fn transpiled(
    ctx: &Context<'_>,
    canonical: &mut Map<String, Value>,
    transpile: Option<&Value>,
) -> Result<(), ApiError> {
    let given = match transpile {
        None | Some(Value::Null) => return Ok(()),
        Some(Value::Object(map)) => map,
        Some(_) => return Err(ApiError::invalid("transpileOptions is an object")),
    };
    for key in given.keys() {
        if !matches!(key.as_str(), "tsConfig" | "babelConfig") {
            return Err(ApiError::invalid(format!(
                "transpileOptions.{key} is not a transpile option; dependency-cruiser's are tsConfig and babelConfig"
            )));
        }
    }
    let Some(Value::Object(options)) = canonical.get_mut("options") else {
        return Ok(());
    };
    for (key, recorded) in [
        ("tsConfig", given.get("tsConfig").map(ts_config_file)),
        (
            "babelConfig",
            given.get("babelConfig").map(babel_config_file),
        ),
    ] {
        let Some(recorded) = recorded else { continue };
        let Some(file) = recorded? else { continue };
        let named = options
            .get(key)
            .and_then(|r| r.get("fileName"))
            .and_then(Value::as_str);
        match named {
            Some(name) if !same_file(&ctx.resolve(name), &ctx.resolve(&file)) => {
                return Err(ApiError::invalid(format!(
                    "transpileOptions.{key} was read from {file}, but options.{key}.fileName names {name}; Rulebearing reads the file options names, so pass the object made from it"
                )));
            }
            Some(_) => {}
            None => {
                options.insert(key.into(), json!({ "fileName": file }));
            }
        }
    }
    Ok(())
}

/// The file a TypeScript `ParsedCommandLine` was made from; none for the empty object
/// `extractTSConfig` returns when TypeScript is not installed.
fn ts_config_file(value: &Value) -> Result<Option<String>, ApiError> {
    recorded_file(value, "tsConfig", |v| {
        v.get("options")
            .and_then(|o| o.get("configFilePath"))
            .and_then(Value::as_str)
    })
}

/// The file Babel's loaded options were made from; none for the empty object
/// `extractBabelConfig` returns when Babel is not installed.
fn babel_config_file(value: &Value) -> Result<Option<String>, ApiError> {
    recorded_file(value, "babelConfig", |v| {
        v.get("filename").and_then(Value::as_str)
    })
}

fn recorded_file(
    value: &Value,
    key: &str,
    file: impl Fn(&Value) -> Option<&str>,
) -> Result<Option<String>, ApiError> {
    match value {
        Value::Null => Ok(None),
        Value::Object(map) if map.is_empty() => Ok(None),
        Value::Object(_) => file(value).map(|f| Some(f.to_owned())).ok_or_else(|| {
            ApiError::invalid(format!(
                "transpileOptions.{key} records no file it was read from; Rulebearing reads the file itself, so pass the object extract{} returns, or name the file in options.{key}.fileName",
                if key == "tsConfig" { "TSConfig" } else { "BabelConfig" }
            ))
        }),
        _ => Err(ApiError::invalid(format!("transpileOptions.{key} is an object"))),
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// dependency-cruiser's `cruise(files, options, resolveOptions, transpileOptions)`.
///
/// # Errors
/// [`ApiError`] with exit 3 for invalid options and exit 2 for a run that cannot be trusted, as
/// the command line exits.
pub fn cruise(
    ctx: &mut Context<'_>,
    files: &[String],
    options: Option<&Value>,
    resolve: Option<&Value>,
    transpile: Option<&Value>,
) -> Result<Reported, ApiError> {
    let mut canonical = configuration(options)?;
    transpiled(ctx, &mut canonical, transpile)?;
    let output_type = canonical
        .get("options")
        .and_then(|o| o.get("outputType"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let text = serde_json::to_string(&canonical).map_err(|e| ApiError::invalid(e.to_string()))?;
    let load = rb_config::LoadOptions {
        root: Some(rb_config::load::repository_root(&ctx.cwd)),
        format: Some(rb_config::ConfigFormat::DependencyCruiser),
        ..rb_config::LoadOptions::default()
    };
    let mut config = rb_config::load_text(
        &text,
        rb_config::read::Syntax::Json,
        &ctx.cwd.clone(),
        &load,
    )
    .map_err(|e| ApiError::invalid(e.to_string()))?;
    configure::evaluate_webpack(ctx, &mut config, &crate::cli::ConfigArgs::default())
        .map_err(|e| ApiError::invalid(e.to_string()))?;
    if let Some(resolve) = resolve.filter(|r| !r.is_null()) {
        config.options.webpack_config_json = Some(
            rb_config::webpack::from_json(resolve.clone(), "resolveOptions")
                .map_err(|e| ApiError::invalid(e.to_string()))?,
        );
    }
    configure::check_report_patterns(&config).map_err(|e| ApiError::invalid(e.to_string()))?;
    let args = CruiseArgs {
        paths: files.to_vec(),
        output_type: Some(output_type.clone().unwrap_or_else(|| IDENTITY.to_owned())),
        no_progress: true,
        ..CruiseArgs::default()
    };
    let answer = crate::cmd::cruise::answer(ctx, &args, config).map_err(|o| ApiError::of(&o))?;
    reported(
        output_type.is_none(),
        answer.output,
        answer.violations,
        answer.stderr,
    )
}

/// The `IReporterOutput` for a rendered output: the result object for the identity reporter.
fn reported(
    identity: bool,
    output: String,
    violations: u64,
    warnings: String,
) -> Result<Reported, ApiError> {
    if identity {
        let result = serde_json::from_str(&output).map_err(|e| ApiError {
            exit: RunExit::Untrustworthy,
            message: format!("the result could not be read back: {e}"),
        })?;
        return Ok(Reported {
            output: Output::Result(result),
            exit_code: 0,
            warnings,
        });
    }
    Ok(Reported {
        output: Output::Text(output),
        exit_code: violations,
        warnings,
    })
}

/// The `IFormatOptions` keys, each `fmt`'s flag of the same name.
const FORMAT_KEYS: &[&str] = &[
    "outputType",
    "exclude",
    "includeOnly",
    "focus",
    "focusDepth",
    "reaches",
    "highlight",
    "collapse",
    "prefix",
];

/// A filter option (`exclude`, `focus` ...): a pattern, an array of patterns (any of them), or an
/// object with `path` (and, for `focus`, `depth`).
fn filter(value: &Value, key: &str) -> Result<(Option<String>, Option<u32>), ApiError> {
    let pattern = |v: &Value| -> Result<Option<String>, ApiError> {
        match v {
            Value::Null => Ok(None),
            Value::String(s) => Ok(Some(s.clone())),
            Value::Array(items) => items
                .iter()
                .map(|i| {
                    i.as_str().map(str::to_owned).ok_or_else(|| {
                        ApiError::invalid(format!("format options: `{key}` holds patterns"))
                    })
                })
                .collect::<Result<Vec<_>, _>>()
                .map(|p| Some(p.join("|"))),
            _ => Err(ApiError::invalid(format!(
                "format options: `{key}` is a pattern, a list of patterns or an object with `path`"
            ))),
        }
    };
    match value {
        Value::Object(map) => {
            let depth = match map.get("depth") {
                None | Some(Value::Null) => None,
                Some(d) => Some(depth(d, key)?),
            };
            Ok((pattern(map.get("path").unwrap_or(&Value::Null))?, depth))
        }
        other => Ok((pattern(other)?, None)),
    }
}

fn depth(value: &Value, key: &str) -> Result<u32, ApiError> {
    let number = match value {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    };
    number.and_then(|n| u32::try_from(n).ok()).ok_or_else(|| {
        ApiError::invalid(format!("format options: `{key}` depth is a whole number"))
    })
}

/// The `fmt` flags an `IFormatOptions` object stands for.
///
/// # Errors
/// [`ApiError`] (exit 3) for a key that is not a format option or a value of the wrong shape.
pub fn format_args(options: Option<&Value>) -> Result<(FmtArgs, bool), ApiError> {
    let given = match options {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(map)) => map.clone(),
        Some(_) => return Err(ApiError::invalid("format options are an object")),
    };
    if let Some(key) = given.keys().find(|k| !FORMAT_KEYS.contains(&k.as_str())) {
        return Err(ApiError::invalid(format!(
            "`{key}` is not a format option; they are {}",
            FORMAT_KEYS.join(", ")
        )));
    }
    let text = |key: &str| -> Result<Option<String>, ApiError> {
        match given.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(s)) => Ok(Some(s.clone())),
            Some(Value::Number(n)) if key == "collapse" => Ok(Some(n.to_string())),
            Some(_) => Err(ApiError::invalid(format!(
                "format options: `{key}` is a string"
            ))),
        }
    };
    let filtered = |key: &str| -> Result<(Option<String>, Option<u32>), ApiError> {
        given.get(key).map_or(Ok((None, None)), |v| filter(v, key))
    };
    let (focus, focus_depth_inline) = filtered("focus")?;
    let focus_depth = match given.get("focusDepth") {
        None | Some(Value::Null) => focus_depth_inline,
        Some(d) => Some(depth(d, "focusDepth")?),
    };
    let output_type = text("outputType")?;
    let identity = output_type.is_none();
    let args = FmtArgs {
        input: "-".into(),
        output_type: output_type.unwrap_or_else(|| IDENTITY.to_owned()),
        output_to: "-".into(),
        exclude: filtered("exclude")?.0,
        include_only: filtered("includeOnly")?.0,
        focus,
        focus_depth,
        reaches: filtered("reaches")?.0,
        highlight: filtered("highlight")?.0,
        collapse: text("collapse")?,
        prefix: text("prefix")?,
        ..FmtArgs::default()
    };
    Ok((args, identity))
}

/// dependency-cruiser's `format(result, options)`.
///
/// # Errors
/// [`ApiError`] with exit 3 for invalid options or reporter, exit 2 for a result that cannot be
/// read.
pub fn format(
    ctx: &Context<'_>,
    result: &Value,
    options: Option<&Value>,
) -> Result<Reported, ApiError> {
    let (args, identity) = format_args(options)?;
    let text = serde_json::to_string(result).map_err(|e| ApiError::invalid(e.to_string()))?;
    let answer = crate::cmd::fmt::answer(ctx, &args, &text).map_err(|o| ApiError::of(&o))?;
    reported(identity, answer.output, answer.violations, String::new())
}

/// dependency-cruiser's `extractDepcruiseConfig(fileName, alreadyVisited, baseDirectory)`: the
/// configuration `spec` names with its `extends` merged, and every configuration read, for the
/// caller's set ([`rb_config::load::merged_after`]).
///
/// # Errors
/// [`ApiError`] (exit 3) when a configuration cannot be found or read, or the chain is circular.
pub fn extract_depcruise_config(
    ctx: &Context<'_>,
    spec: &str,
    visited: &[String],
    base_dir: Option<&str>,
) -> Result<(Value, Vec<String>), ApiError> {
    let base = base_dir.map_or_else(|| ctx.cwd.clone(), |d| ctx.resolve(d));
    let load = rb_config::LoadOptions {
        root: Some(rb_config::load::repository_root(&base)),
        ..rb_config::LoadOptions::default()
    };
    rb_config::load::merged_after(spec, &base, &load, visited)
        .map(|(canonical, read)| (Value::Object(canonical), read))
        .map_err(|e| ApiError::invalid(e.to_string()))
}

/// dependency-cruiser's `extractWebpackResolveConfig(fileName, env, arguments)`: the `resolve`
/// block of the webpack configuration, `{}` when it has none, evaluated in the sandbox
/// ([`rb_config::webpack::resolve_block`]).
///
/// # Errors
/// [`ApiError`] (exit 3) when the file cannot be read or evaluated.
pub fn extract_webpack_resolve_config(
    ctx: &Context<'_>,
    file: &str,
    env: Option<&Value>,
    arguments: Option<&Value>,
) -> Result<Value, ApiError> {
    let reference = rb_model::options::WebpackConfig {
        file_name: Some(file.to_owned()),
        env: env.filter(|v| !v.is_null()).cloned(),
        arguments: arguments.filter(|v| !v.is_null()).cloned(),
    };
    let evaluation = rb_config::read::Evaluation {
        via_node: false,
        limits: rb_config::js::Limits::default(),
    };
    rb_config::webpack::resolve_block(
        &reference,
        &ctx.cwd,
        &rb_config::load::repository_root(&ctx.cwd),
        evaluation,
    )
    .map(|block| block.map_or_else(|| json!({}), |b| b.value))
    .map_err(|e| ApiError::invalid(e.to_string()))
}

/// One of `allExtensions`: an extension and whether this build reads it here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extension {
    /// The extension, with its dot.
    pub extension: &'static str,
    /// Whether a cruise from the working directory reads it.
    pub available: bool,
}

/// One of `getAvailableTranspilers()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transpiler {
    /// dependency-cruiser's name for it.
    pub name: &'static str,
    /// The versions it accepts: `*` for one this build reads natively, dependency-cruiser's range
    /// for one the sidecar runs.
    pub version: &'static str,
    /// Whether a cruise from the working directory reads its files.
    pub available: bool,
    /// What reads them: the parser (`oxc <version>`), the sidecar's dependency-cruiser, or `-`.
    pub current_version: String,
}

/// dependency-cruiser's extensions in its order, with the transpiler each needs there.
const EXTENSIONS: &[(&str, &str)] = &[
    (".js", "javascript"),
    (".cjs", "javascript"),
    (".mjs", "javascript"),
    (".jsx", "javascript"),
    (".ts", "typescript"),
    (".tsx", "typescript"),
    (".d.ts", "typescript"),
    (".cts", "typescript"),
    (".d.cts", "typescript"),
    (".mts", "typescript"),
    (".d.mts", "typescript"),
    (".vue", "@vue/compiler-sfc"),
    (".svelte", "svelte"),
    (".ls", "livescript"),
    (".coffee", "coffeescript"),
    (".litcoffee", "coffeescript"),
    (".coffee.md", "coffeescript"),
    (".csx", "coffeescript"),
    (".cjsx", "coffeescript"),
];

/// dependency-cruiser's transpilers in its order, with the range it supports for the ones the
/// sidecar hands to it (`None`: read natively).
const TRANSPILERS: &[(&str, Option<&str>)] = &[
    ("javascript", None),
    ("babel", None),
    ("coffee-script", Some(">=1.0.0 <2.0.0")),
    ("coffeescript", Some(">=1.0.0 <3.0.0")),
    ("livescript", Some(">=1.0.0 <2.0.0")),
    ("svelte", None),
    ("swc", None),
    ("typescript", None),
    ("vue-template-compiler", None),
    ("@vue/compiler-sfc", None),
];

/// What the sidecar can read from `cwd` ([ADR-0017](../../../docs/adr/0017-coffeescript-livescript-sidecar.md)):
/// dependency-cruiser's version and the extensions it cannot read, or none when it cannot run.
#[cfg(feature = "extract-ts")]
fn sidecar(cwd: &Path) -> Option<(String, Vec<String>)> {
    let sidecar = rb_extract_ts::sidecar::Sidecar::node(cwd).ok()?;
    Some((sidecar.version().to_owned(), sidecar.unavailable().to_vec()))
}

#[cfg(not(feature = "extract-ts"))]
fn sidecar(_cwd: &Path) -> Option<(String, Vec<String>)> {
    None
}

#[cfg(feature = "extract-ts")]
const PARSER: Option<&str> = Some(rb_extract_ts::PARSER);
#[cfg(not(feature = "extract-ts"))]
const PARSER: Option<&str> = None;

/// Whether a transpiler's files are read from `cwd`, and by what: natively when this build has
/// the TypeScript extractor, else through the sidecar when it can run and read them.
fn reads(name: &str, side: Option<&(String, Vec<String>)>) -> (bool, String) {
    let native = TRANSPILERS
        .iter()
        .any(|(n, range)| *n == name && range.is_none());
    if native {
        return PARSER.map_or((false, "-".to_owned()), |p| (true, p.to_owned()));
    }
    match side {
        Some((version, unavailable)) => {
            let reads = EXTENSIONS
                .iter()
                .filter(|(_, t)| *t == name || (name == "coffee-script" && *t == "coffeescript"))
                .any(|(e, _)| !unavailable.iter().any(|u| u == e));
            if reads {
                (
                    true,
                    format!("dependency-cruiser {version} (--sidecar node)"),
                )
            } else {
                (false, "-".to_owned())
            }
        }
        None => (false, "-".to_owned()),
    }
}

/// dependency-cruiser's `allExtensions` for a cruise from `cwd`: its extensions, each available
/// when this build reads it, natively or through a sidecar that can run here.
pub fn all_extensions(cwd: &Path) -> Vec<Extension> {
    let side = sidecar(cwd);
    EXTENSIONS
        .iter()
        .map(|(extension, transpiler)| {
            let native = TRANSPILERS
                .iter()
                .any(|(n, range)| n == transpiler && range.is_none());
            let available = if native {
                PARSER.is_some()
            } else {
                side.as_ref()
                    .is_some_and(|(_, unavailable)| !unavailable.iter().any(|u| u == extension))
            };
            Extension {
                extension,
                available,
            }
        })
        .collect()
}

/// dependency-cruiser's `getAvailableTranspilers()` for a cruise from `cwd`.
pub fn available_transpilers(cwd: &Path) -> Vec<Transpiler> {
    let side = sidecar(cwd);
    TRANSPILERS
        .iter()
        .map(|(name, range)| {
            let (available, current_version) = reads(name, side.as_ref());
            Transpiler {
                name,
                version: range.unwrap_or("*"),
                available,
                current_version,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rule_set_applies_only_when_validate_is_true() {
        let options = json!({
            "ruleSet": { "forbidden": [{ "from": {}, "to": { "circular": true } }], "options": { "tsPreCompilationDeps": true } },
            "outputType": "err",
            "exclude": "node_modules"
        });
        let off = configuration(Some(&options)).unwrap_or_default();
        assert!(!off.contains_key("forbidden"));
        assert_eq!(off["options"]["outputType"], "err");
        assert_eq!(off["options"]["tsPreCompilationDeps"], true);
        let mut on = options.clone();
        on["validate"] = json!(true);
        on["tsPreCompilationDeps"] = json!(false);
        let on = configuration(Some(&on)).unwrap_or_default();
        assert_eq!(on["forbidden"].as_array().map(Vec::len), Some(1));
        assert_eq!(
            on["options"]["tsPreCompilationDeps"], false,
            "the options win over ruleSet.options"
        );
        assert!(
            !on["options"]
                .as_object()
                .is_some_and(|o| o.contains_key("validate"))
        );
        assert!(configuration(Some(&json!([]))).is_err());
        assert!(configuration(Some(&json!({ "ruleSet": 1 }))).is_err());
        assert_eq!(configuration(None).map(|c| c.len()).ok(), Some(1));
    }

    #[test]
    fn format_options_are_fmt_flags() -> Result<(), ApiError> {
        let (args, identity) = format_args(Some(&json!({
            "outputType": "err",
            "exclude": ["^a", "^b"],
            "focus": { "path": "^src", "depth": 2 },
            "reaches": { "path": "^lib" },
            "collapse": 2,
            "prefix": "https://x/"
        })))?;
        assert!(!identity);
        assert_eq!(args.output_type, "err");
        assert_eq!(args.exclude.as_deref(), Some("^a|^b"));
        assert_eq!(
            (args.focus.as_deref(), args.focus_depth),
            (Some("^src"), Some(2))
        );
        assert_eq!(args.reaches.as_deref(), Some("^lib"));
        assert_eq!(args.collapse.as_deref(), Some("2"));
        assert_eq!(args.prefix.as_deref(), Some("https://x/"));
        let (args, identity) = format_args(Some(&json!({ "focus": "^x", "focusDepth": "3" })))?;
        assert!(identity);
        assert_eq!(args.output_type, IDENTITY);
        assert_eq!(args.focus_depth, Some(3));
        for bad in [
            json!({ "outputTo": "x" }),
            json!({ "exclude": 1 }),
            json!({ "exclude": [1] }),
            json!({ "focusDepth": -1 }),
            json!({ "prefix": true }),
            json!("err"),
        ] {
            assert!(format_args(Some(&bad)).is_err(), "{bad}");
        }
        Ok(())
    }

    #[test]
    fn a_transpile_object_is_read_from_the_file_it_records() {
        assert_eq!(ts_config_file(&json!({})).ok(), Some(None));
        assert_eq!(ts_config_file(&Value::Null).ok(), Some(None));
        assert_eq!(
            ts_config_file(&json!({ "options": { "configFilePath": "/r/tsconfig.json" } })).ok(),
            Some(Some("/r/tsconfig.json".to_owned()))
        );
        assert!(ts_config_file(&json!({ "options": {} })).is_err());
        assert!(ts_config_file(&json!(1)).is_err());
        assert_eq!(
            babel_config_file(&json!({ "filename": "/r/.babelrc" })).ok(),
            Some(Some("/r/.babelrc".to_owned()))
        );
        assert!(babel_config_file(&json!({ "plugins": [] })).is_err());
    }

    #[test]
    fn the_tables_follow_dependency_cruiser() {
        // dependency-cruiser 18.2.0, src/extract/transpile/meta.mjs and src/meta.cjs.
        assert_eq!(EXTENSIONS.len(), 19);
        assert_eq!(TRANSPILERS.len(), 10);
        let cwd = std::env::temp_dir();
        let extensions = all_extensions(&cwd);
        assert_eq!(extensions.len(), EXTENSIONS.len());
        let transpilers = available_transpilers(&cwd);
        let typescript = transpilers.iter().find(|t| t.name == "typescript");
        assert_eq!(typescript.map(|t| t.version), Some("*"));
        let coffee = transpilers.iter().find(|t| t.name == "coffeescript");
        assert_eq!(coffee.map(|t| t.version), Some(">=1.0.0 <3.0.0"));
    }

    #[test]
    fn an_error_keeps_the_reason_without_the_prefix() {
        let error = ApiError::of(&Outcome {
            stdout: String::new(),
            stderr: "rulebearing cruise: no modules found\nwarning: w\n".into(),
            code: 2,
        });
        assert_eq!(error.exit, RunExit::Untrustworthy);
        assert_eq!(error.to_string(), "no modules found\nwarning: w");
        assert_eq!(
            ApiError::of(&Outcome::failed(
                RunExit::InvalidConfig,
                "rulebearing fmt: bad\n"
            ))
            .exit,
            RunExit::InvalidConfig
        );
    }
}
