//! Loading the configuration for a command, and laying the command line's flags over it.
//!
//! - Coverage: [coverage § Command line](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#command-line)
//!   (`--config`, `--validate`, `--no-config`, the option flags that override the config)
//! - Decisions: [ADR-0005](../../../docs/adr/0005-native-config-superset-and-compat.md),
//!   [ADR-0006](../../../docs/adr/0006-embedded-quickjs-config-evaluator.md)
//! - Plan: [Wave 1, Step 13](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-13-rb-cli-cruise-fmt-exit-codes-flags-1d)
//! - Requirement: [FR-CFG-01](../../../docs/prd.md#fr-cfg-01)

use std::path::PathBuf;

use rb_config::load::repository_root;
use rb_config::model::FilterOption;
use rb_config::read::Syntax;
use rb_config::{Config, ConfigError, ConfigFormat, DEFAULT_NAMES, LoadOptions};
use rb_model::ModuleSystem;
use rb_model::options::{
    FileReference, PathFilter, Patterns, SpecifyLiteral, TsPreCompilationDeps,
};
use serde_json::{Map, Value, json};

use crate::cli::{ConfigArgs, CruiseArgs, KnownArgs};
use crate::context::Context;

/// Where the configuration came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// A file.
    File(PathBuf),
    /// Standard input.
    Stdin,
    /// No configuration: `--no-config`, or nothing found.
    None,
}

/// Finds the configuration `--config` names, or the first default name in the working directory.
pub fn source(ctx: &Context<'_>, args: &ConfigArgs) -> Source {
    if args.no_config {
        return Source::None;
    }
    match args.config.as_deref().or(args.validate.as_deref()) {
        Some("-") => Source::Stdin,
        Some(name) if !name.is_empty() => Source::File(ctx.resolve(name)),
        _ => DEFAULT_NAMES
            .iter()
            .map(|n| ctx.cwd.join(n))
            .find(|p| p.is_file())
            .map_or(Source::None, Source::File),
    }
}

/// Loads the configuration, or `None` when there is none.
///
/// # Errors
/// [`ConfigError`] for anything wrong with it; the caller exits 3.
pub fn load(ctx: &mut Context<'_>, args: &ConfigArgs) -> Result<Option<Config>, ConfigError> {
    let format = match args.config_format.as_deref() {
        None => None,
        Some(name) => Some(ConfigFormat::parse(name).ok_or_else(|| {
            ConfigError::Invalid(format!(
                "--config-format `{name}`: use native or dependency-cruiser"
            ))
        })?),
    };
    let options = LoadOptions {
        root: Some(repository_root(&ctx.cwd)),
        format,
        strict_compat: args.strict_compat,
        via_node: args.config_via_node,
        limits: rb_config::js::Limits::default(),
    };
    let mut config = match source(ctx, args) {
        Source::None => return Ok(None),
        Source::File(path) => rb_config::load(&path, &options)?,
        Source::Stdin => {
            let text = ctx.read_stdin().map_err(|e| ConfigError::Read {
                file: PathBuf::from("-"),
                reason: e.to_string(),
            })?;
            let syntax = match format {
                Some(ConfigFormat::Native) | None => Syntax::Yaml,
                Some(ConfigFormat::DependencyCruiser) => Syntax::JavaScript,
            };
            rb_config::load_text(&text, syntax, &ctx.cwd.clone(), &options)?
        }
    };
    if args.require_comment_token {
        rb_config::require_comment_tokens(&config)?;
    }
    evaluate_webpack(ctx, &mut config, args)?;
    check_report_patterns(&config)?;
    Ok(Some(config))
}

/// Refuses a `highlight` or `collapse` pattern that does not compile, as upstream's
/// `assertCruiseOptionsValid` does, rather than letting it match nothing.
///
/// # Errors
/// [`ConfigError::Invalid`] naming the option and the pattern.
pub fn check_report_patterns(config: &Config) -> Result<(), ConfigError> {
    let highlight = config
        .options
        .highlight
        .as_ref()
        .and_then(|h| h.path.clone());
    let collapse = config
        .options
        .collapse
        .as_ref()
        .and_then(rb_rules::rewrap::collapse_pattern);
    for (option, pattern) in [("highlight", highlight), ("collapse", collapse)] {
        if let Some(pattern) = pattern {
            rb_config::pattern::compile(&pattern).map_err(|e| {
                ConfigError::Invalid(format!("{option} `{pattern}` is not a usable pattern: {e}"))
            })?;
        }
    }
    Ok(())
}

/// Evaluates `webpackConfig.fileName`, when set, into the `resolve` block the extractor applies
/// ([`rb_config::webpack`]), in the sandbox or under `--config-via-node` as the configuration
/// itself was, and records the files it read.
///
/// # Errors
/// [`ConfigError`] when the webpack configuration cannot be evaluated.
pub fn evaluate_webpack(
    ctx: &Context<'_>,
    config: &mut Config,
    args: &ConfigArgs,
) -> Result<(), ConfigError> {
    let Some(reference) = config.languages.typescript.webpack_config.clone() else {
        return Ok(());
    };
    let evaluation = rb_config::read::Evaluation {
        via_node: args.config_via_node,
        limits: rb_config::js::Limits::default(),
    };
    if let Some(block) = rb_config::webpack::resolve_block(
        &reference,
        &ctx.cwd,
        &repository_root(&ctx.cwd),
        evaluation,
    )? {
        config.files.extend(block.files);
        config.files.sort();
        config.files.dedup();
        config.options.webpack_config_json = Some(block.value);
    }
    Ok(())
}

/// Loads the configuration a command cannot run without.
///
/// # Errors
/// An [`crate::Outcome`] to return: exit 3 with the reason, or when there is no configuration.
pub fn required(ctx: &mut Context<'_>, args: &ConfigArgs) -> Result<Config, crate::Outcome> {
    match load(ctx, args) {
        Ok(Some(config)) => Ok(config),
        Ok(None) => Err(crate::Outcome::failed(
            crate::RunExit::InvalidConfig,
            "rulebearing: no configuration found; pass --config, or create rulebearing.yaml (`rulebearing init`)\n",
        )),
        Err(e) => Err(crate::Outcome::failed(
            crate::RunExit::InvalidConfig,
            format!("rulebearing: {e}\n"),
        )),
    }
}

fn filter(pattern: &str) -> PathFilter {
    PathFilter::Patterns(Patterns::One(pattern.to_owned()))
}

/// Parses `--module-systems cjs,es6`.
///
/// # Errors
/// [`ConfigError::Invalid`] naming the value that is not a module system.
pub fn module_systems(list: &str) -> Result<Vec<ModuleSystem>, ConfigError> {
    list.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            s.parse::<ModuleSystem>().map_err(|_| {
                ConfigError::Invalid(format!(
                    "--module-systems: `{s}` is not cjs, es6, amd or tsd"
                ))
            })
        })
        .collect()
}

/// Lays `cruise`'s option flags over the configuration, as dependency-cruiser's command line does.
///
/// # Errors
/// [`ConfigError::Invalid`] for a flag value that does not parse.
pub fn apply_flags(
    config: &mut Config,
    args: &CruiseArgs,
    ctx: &Context<'_>,
) -> Result<(), ConfigError> {
    let ts = &mut config.languages.typescript;
    if let Some(p) = &args.include_only {
        ts.include_only = Some(filter(p));
    }
    if let Some(p) = &args.exclude {
        ts.exclude = Some(filter(p));
    }
    if let Some(p) = &args.do_not_follow {
        ts.do_not_follow = Some(filter(p));
    }
    if let Some(depth) = args.max_depth {
        ts.max_depth = Some(depth);
    }
    if let Some(list) = &args.module_systems {
        ts.module_systems = Some(module_systems(list)?);
    }
    if let Some(value) = &args.ts_pre_compilation_deps {
        ts.ts_pre_compilation_deps = Some(match value.as_str() {
            "true" => TsPreCompilationDeps::Enabled(true),
            "false" => TsPreCompilationDeps::Enabled(false),
            "specify" => TsPreCompilationDeps::Specify(SpecifyLiteral::Specify),
            other => {
                return Err(ConfigError::Invalid(format!(
                    "--ts-pre-compilation-deps `{other}`: use true, false or specify"
                )));
            }
        });
    }
    if let Some(file) = &args.ts_config {
        ts.ts_config = Some(FileReference {
            file_name: Some(file.clone()),
        });
    }
    if args.preserve_symlinks {
        ts.preserve_symlinks = Some(true);
    }
    let options = &mut config.options;
    if let Some(p) = &args.focus {
        options.focus = Some(FilterOption {
            path: Some(p.clone()),
            depth: args.focus_depth,
        });
    } else if let (Some(depth), Some(focus)) = (args.focus_depth, options.focus.as_mut()) {
        focus.depth = Some(depth);
    }
    if let Some(p) = &args.reaches {
        options.reaches = Some(FilterOption {
            path: Some(p.clone()),
            depth: None,
        });
    }
    if let Some(p) = &args.highlight {
        options.highlight = Some(FilterOption {
            path: Some(p.clone()),
            depth: None,
        });
    }
    if let Some(collapse) = &args.collapse {
        options.collapse = Some(Value::String(collapse.clone()));
    }
    if let Some(prefix) = &args.prefix {
        options.prefix = Some(prefix.clone());
    }
    if let Some(suffix) = &args.suffix {
        options.suffix = Some(suffix.clone());
    }
    // dependency-cruiser's `--metrics` defaults to false and its command-line options are spread
    // over the configuration's, so `options.metrics: true` alone computes nothing.
    options.metrics = Some(args.metrics);
    if let Some(file) = &args.webpack_config {
        // As upstream's `--webpack-config`: the named file replaces the configuration's.
        let reference = config
            .languages
            .typescript
            .webpack_config
            .get_or_insert_with(Default::default);
        reference.file_name = Some(file.clone());
        if args.webpack_config_json.is_none() {
            evaluate_webpack(ctx, config, &args.config)?;
        }
    }
    if let Some(file) = &args.webpack_config_json {
        let path = ctx.resolve(file);
        let text = std::fs::read_to_string(&path).map_err(|e| ConfigError::Read {
            file: path.clone(),
            reason: e.to_string(),
        })?;
        let value: Value = serde_json::from_str(&text).map_err(|e| ConfigError::Parse {
            file: path.clone(),
            reason: e.to_string(),
        })?;
        config.options.webpack_config_json = Some(rb_config::webpack::from_json(
            value,
            &path.display().to_string(),
        )?);
    }
    known_violations(config, &args.known, ctx)?;
    check_report_patterns(config)
}

/// `--ignore-known [file]` replaces `options.knownViolations` with the file's entries, as
/// dependency-cruiser sets `ruleSet.options.knownViolations` from it; `--no-ignore-known` drops
/// every entry, the configuration's included, so each finding is reported at its rule's severity
/// ([Wave 2, Step 10](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#210-step-10-reporters-and-baseline-semantics-2e)).
///
/// # Errors
/// [`ConfigError`] naming the file when it cannot be read or is not a known-violations file.
pub fn known_violations(
    config: &mut Config,
    known: &KnownArgs,
    ctx: &Context<'_>,
) -> Result<(), ConfigError> {
    if known.no_ignore_known {
        config.known_violations.clear();
    } else if let Some(file) = &known.ignore_known {
        config.known_violations = rb_config::load::known_violations_file(&ctx.resolve(file))?;
    }
    Ok(())
}

/// dependency-cruiser's option defaults, which `optionsUsed` carries.
fn defaults(cwd: &str) -> Map<String, Value> {
    let mut map = Map::new();
    map.insert("baseDir".into(), json!(cwd));
    map.insert("maxDepth".into(), json!(0));
    map.insert("moduleSystems".into(), json!(["es6", "cjs", "tsd", "amd"]));
    map.insert("detectJSDocImports".into(), json!(false));
    map.insert("detectProcessBuiltinModuleCalls".into(), json!(false));
    map.insert("skipAnalysisNotInRules".into(), json!(false));
    map.insert("tsPreCompilationDeps".into(), json!(false));
    map.insert("preserveSymlinks".into(), json!(false));
    map.insert("combinedDependencies".into(), json!(false));
    map.insert(
        "externalModuleResolutionStrategy".into(),
        json!("node_modules"),
    );
    map.insert("exoticRequireStrings".into(), json!([]));
    map
}

/// The options `summary.optionsUsed` reports: dependency-cruiser's defaults, the configuration's
/// options and the command line's, in that order of precedence.
pub fn options_used(
    config: Option<&Config>,
    ctx: &Context<'_>,
    output_type: &str,
    output_to: &str,
) -> Map<String, Value> {
    let mut map = defaults(&ctx.cwd.to_string_lossy());
    if let Some(config) = config {
        if let Ok(Value::Object(ts)) = serde_json::to_value(&config.languages.typescript) {
            map.extend(ts);
        }
        if let Ok(Value::Object(options)) = serde_json::to_value(&config.options) {
            map.extend(options);
        }
        if !config.known_violations.is_empty() {
            map.insert(
                "knownViolations".into(),
                serde_json::to_value(&config.known_violations).unwrap_or(Value::Null),
            );
        }
        if let Some(origin) = &config.origin {
            let cwd = ctx.cwd.canonicalize().unwrap_or_else(|_| ctx.cwd.clone());
            map.insert(
                "rulesFile".into(),
                json!(
                    origin
                        .strip_prefix(&cwd)
                        .unwrap_or(origin)
                        .to_string_lossy()
                ),
            );
        }
        if config.via_node {
            map.insert("viaNode".into(), json!(true));
        }
    }
    for key in ["doNotFollow", "exclude"] {
        map.entry(key).or_insert_with(|| json!({}));
    }
    map.insert("outputType".into(), json!(output_type));
    map.insert("outputTo".into(), json!(output_to));
    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn ctx<'a>(dir: &std::path::Path, stdin: &'a mut dyn std::io::Read) -> Context<'a> {
        Context {
            cwd: dir.to_path_buf(),
            stdin,
            today: NaiveDate::default(),
            timestamp: String::new(),
            color_terminal: false,
        }
    }

    #[test]
    fn module_systems_parse() -> Result<(), ConfigError> {
        assert_eq!(
            module_systems("cjs, es6")?,
            [ModuleSystem::Cjs, ModuleSystem::Es6]
        );
        assert!(module_systems("cjs,nope").is_err());
        Ok(())
    }

    #[test]
    fn sources_and_flags() -> Result<(), Box<dyn std::error::Error>> {
        let dir = std::env::temp_dir().join(format!("rb-cli-configure-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        std::fs::write(
            dir.join(".dependency-cruiser.json"),
            r#"{"forbidden":[{"name":"r","comment":"x","from":{},"to":{}}]}"#,
        )?;
        let mut empty: &[u8] = b"forbidden: []";
        let mut c = ctx(&dir, &mut empty);
        assert_eq!(
            source(&c, &ConfigArgs::default()),
            Source::File(dir.join(".dependency-cruiser.json"))
        );
        assert_eq!(
            source(
                &c,
                &ConfigArgs {
                    no_config: true,
                    ..ConfigArgs::default()
                }
            ),
            Source::None
        );
        assert_eq!(
            source(
                &c,
                &ConfigArgs {
                    config: Some("-".into()),
                    ..ConfigArgs::default()
                }
            ),
            Source::Stdin
        );
        assert_eq!(
            source(
                &c,
                &ConfigArgs {
                    validate: Some("x.json".into()),
                    ..ConfigArgs::default()
                }
            ),
            Source::File(dir.join("x.json"))
        );
        let token = load(
            &mut c,
            &ConfigArgs {
                require_comment_token: true,
                ..ConfigArgs::default()
            },
        );
        assert!(matches!(token, Err(ConfigError::MissingToken { .. })));
        let bad_format = load(
            &mut c,
            &ConfigArgs {
                config_format: Some("xml".into()),
                ..ConfigArgs::default()
            },
        );
        assert!(bad_format.is_err());
        let stdin = load(
            &mut c,
            &ConfigArgs {
                config: Some("-".into()),
                ..ConfigArgs::default()
            },
        )?;
        assert!(stdin.is_some());
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn flags_apply() -> Result<(), Box<dyn std::error::Error>> {
        let dir = std::env::temp_dir().join(format!("rb-cli-flags-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        std::fs::write(
            dir.join(".dependency-cruiser.json"),
            r#"{"forbidden":[{"name":"r","comment":"x","from":{},"to":{}}]}"#,
        )?;
        let mut empty: &[u8] = b"";
        let mut c = ctx(&dir, &mut empty);
        let mut config = load(&mut c, &ConfigArgs::default())?.unwrap_or_default();
        std::fs::write(dir.join("webpack.json"), r#"{"alias":{}}"#)?;
        let args = CruiseArgs {
            include_only: Some("^src".into()),
            exclude: Some("dist".into()),
            do_not_follow: Some("node_modules".into()),
            max_depth: Some(2),
            module_systems: Some("es6".into()),
            ts_pre_compilation_deps: Some("specify".into()),
            ts_config: Some("tsconfig.json".into()),
            preserve_symlinks: true,
            focus: Some("^src/a".into()),
            focus_depth: Some(2),
            reaches: Some("^lib".into()),
            prefix: Some("p".into()),
            suffix: Some("s".into()),
            metrics: true,
            webpack_config_json: Some("webpack.json".into()),
            ..CruiseArgs::default()
        };
        apply_flags(&mut config, &args, &c)?;
        assert_eq!(config.languages.typescript.max_depth(), 2);
        assert!(config.languages.typescript.keeps_pre_compilation_deps());
        assert_eq!(config.options.focus.as_ref().and_then(|f| f.depth), Some(2));
        assert!(config.options.webpack_config_json.is_some());
        assert_eq!(config.options.metrics, Some(true));
        // Without the flag, metrics are off whatever the configuration says, as upstream's
        // commander default overrides `options.metrics`.
        let mut plain = config.clone();
        apply_flags(&mut plain, &CruiseArgs::default(), &c)?;
        assert_eq!(plain.options.metrics, Some(false));
        let used = options_used(Some(&config), &c, "json", "-");
        assert_eq!(used["outputType"], "json");
        assert_eq!(used["rulesFile"], ".dependency-cruiser.json");
        assert_eq!(used["moduleSystems"], json!(["es6"]));
        let wrong = CruiseArgs {
            ts_pre_compilation_deps: Some("maybe".into()),
            ..CruiseArgs::default()
        };
        assert!(apply_flags(&mut config, &wrong, &c).is_err());
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }
}
