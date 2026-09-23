//! `rb-extract-ts`: the TypeScript and JavaScript extractor over `oxc_parser` and `oxc_resolver`.
//!
//! - Architecture: [`docs/architecture.md#extractors`](../../../docs/architecture.md#extractors)
//! - Decisions: [ADR-0012](../../../docs/adr/0012-oxc-for-typescript.md),
//!   [ADR-0017](../../../docs/adr/0017-coffeescript-livescript-sidecar.md)
//! - Plans: [Wave 0, Spike A](../../../docs/plans/pending/0000-wave-0-spike.md),
//!   [Wave 1, sub-wave 1C](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#wave-1c-rb-extract-ts-completion)
//! - Requirements: [FR-EXT-TS-01](../../../docs/prd.md#fr-ext-ts-01) to [FR-EXT-TS-05](../../../docs/prd.md#fr-ext-ts-05)
//! - Specification: dependency-cruiser 18.2.0's `test/extract` suite, recorded as conformance
//!   gate 1 layer 1 ([ADR-0009](../../../docs/adr/0009-conformance-suites-as-specification.md))
//!
//! Rule of the boundary: this crate reads source files and writes `rb_model` types only. It
//! must not depend on `rb-config` or `rb-rules`.
//!
//! | Module | Does |
//! | --- | --- |
//! | [`walk`] | finds every dependency form, as dependency-cruiser's tsc, swc or acorn extractor would |
//! | [`jsdoc`] | the JSDoc `@import` and `{import('x')}` forms |
//! | [`resolve`] | resolves a specifier over `oxc_resolver` and classifies it |
//! | [`npm`] | `package.json` lookup and the `npm*` dependency types |
//! | [`core`] | runtime built-in modules |
//! | [`babel`] | `babelConfig`'s module-resolver aliases |
//! | [`collate`] | JavaScript's `localeCompare` order, which dependency-cruiser sorts with |
//! | [`pipeline`] | files to resolved, filtered, sorted dependencies, and the reachable modules |
//!
//! `checksum` is left absent on every module: dependency-cruiser computes it only in its cache
//! (`src/cache/`), never during extraction, and the cache is a later wave's surface.

pub mod babel;
pub mod collate;
pub mod core;
pub mod jsdoc;
pub mod npm;
pub mod pipeline;
pub mod resolve;
pub mod walk;

use std::path::{Path, PathBuf};

use rb_model::ExternalModuleResolutionStrategy;
use rb_model::{
    Dependency, DependencyKind, ExtractError, Extraction, Extractor, Language, Module, Receipt,
    TypeScriptOptions,
};

use babel::BabelAliases;
use pipeline::{Extracted, PipelineError, Settings};
use resolve::ResolveConfig;

/// File extensions this extractor owns, from
/// [coverage § Extraction and resolution](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#extraction-and-resolution).
pub const EXTENSIONS: &[&str] = &["js", "mjs", "cjs", "jsx", "ts", "tsx", "mts", "cts"];

/// Extensions handled by the Node sidecar rather than natively
/// ([ADR-0017](../../../docs/adr/0017-coffeescript-livescript-sidecar.md)). `.coffee.md`, literate
/// CoffeeScript under a double extension, is one too.
pub const SIDECAR_EXTENSIONS: &[&str] = &["coffee", "litcoffee", "ls", "cjsx", "csx"];

/// Whether a path is one this extractor parses natively.
pub fn owns(path: &str) -> bool {
    extension(path).is_some_and(|e| EXTENSIONS.contains(&e))
}

/// Whether a path needs the sidecar.
pub fn needs_sidecar(path: &str) -> bool {
    path.ends_with(".coffee.md") || extension(path).is_some_and(|e| SIDECAR_EXTENSIONS.contains(&e))
}

fn extension(path: &str) -> Option<&str> {
    let name = path.rsplit('/').next()?;
    let (_, ext) = name.rsplit_once('.')?;
    Some(ext)
}

/// The reason a sidecar file stops a run without `--sidecar node`.
pub const SIDECAR_REASON: &str = "CoffeeScript and LiveScript are extracted by the Node sidecar \
     (ADR-0017), which is not enabled; run with `--sidecar node`, or exclude the file";

/// The TypeScript and JavaScript extractor.
#[derive(Debug, Clone, Copy, Default)]
pub struct TypeScriptExtractor;

impl From<PipelineError> for ExtractError {
    fn from(error: PipelineError) -> Self {
        match error {
            PipelineError::Io { source, .. } => Self::Io(source),
            PipelineError::Parse { path, reason } => Self::UnsupportedFile { path, reason },
            PipelineError::Pattern { pattern, reason } => Self::UnsupportedFile {
                path: PathBuf::from(pattern),
                reason,
            },
        }
    }
}

/// The resolver settings a `TypeScriptOptions` block asks for. Pure: nothing is read from disk
/// (see [`prepare`] for the tsconfig, the Babel config and the PnP manifest).
pub fn resolve_config(options: &TypeScriptOptions) -> ResolveConfig {
    let mut config = ResolveConfig {
        symlinks: !options.preserve_symlinks.unwrap_or(false),
        tsconfig: options
            .ts_config
            .as_ref()
            .and_then(|t| t.file_name.as_ref())
            .map(PathBuf::from),
        built_in_modules: options.built_in_modules.clone(),
        combined_dependencies: options.combined_dependencies.unwrap_or(false),
        yarn_pnp: options.external_module_resolution_strategy()
            == ExternalModuleResolutionStrategy::YarnPnp,
        tsconfig_references: true,
        ..ResolveConfig::default()
    };
    if let Some(enhanced) = &options.enhanced_resolve_options {
        let set = |field: &Option<Vec<String>>, target: &mut Vec<String>| {
            if let Some(values) = field {
                target.clone_from(values);
            }
        };
        set(&enhanced.extensions, &mut config.extensions);
        set(&enhanced.main_fields, &mut config.main_fields);
        set(&enhanced.main_files, &mut config.main_files);
        set(&enhanced.exports_fields, &mut config.exports_fields);
        set(&enhanced.condition_names, &mut config.condition_names);
        set(&enhanced.alias_fields, &mut config.alias_fields);
        // `cachedInputFileSystem.cacheDuration` is accepted and ignored: each run has one
        // resolver cache that lives exactly as long as the run.
    }
    config
}

fn absolute(cwd: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
}

/// Reads the tsconfig named in `config` (with its `extends` chain) and records its `baseUrl` and
/// `paths` keys, which classify a dependency as `aliased-tsconfig-*`.
fn load_tsconfig(config: &mut ResolveConfig, cwd: &Path) -> Result<(), ExtractError> {
    let Some(file) = config.tsconfig.clone() else {
        return Ok(());
    };
    let file = absolute(cwd, &file);
    config.tsconfig = Some(file.clone());
    let tsconfig = config.resolver(None).resolve_tsconfig(&file).map_err(|e| {
        ExtractError::UnsupportedFile {
            path: file.clone(),
            reason: format!("tsConfig.fileName could not be read: {e}"),
        }
    })?;
    let options = &tsconfig.compiler_options;
    config.tsconfig_base_url = options
        .base_url
        .as_ref()
        .map(|p| p.to_string_lossy().replace('\\', "/"));
    config.tsconfig_paths = options
        .paths
        .as_ref()
        .map(|p| p.keys().cloned().collect())
        .unwrap_or_default();
    Ok(())
}

/// The `.pnp.cjs` at or above `start`, checked well-formed enough for the resolver: the manifest
/// data must parse and name the top-level package, which the PnP reader otherwise asserts.
///
/// # Errors
/// A named reason when there is no manifest or it is malformed.
pub fn pnp_manifest(start: &Path) -> Result<PathBuf, ExtractError> {
    let unsupported = |path: &Path, reason: &str| ExtractError::UnsupportedFile {
        path: path.to_path_buf(),
        reason: reason.to_owned(),
    };
    let Some(manifest) = start
        .ancestors()
        .map(|dir| dir.join(".pnp.cjs"))
        .find(|p| p.is_file())
    else {
        return Err(unsupported(
            start,
            "externalModuleResolutionStrategy is yarn-pnp but no .pnp.cjs was found here or \
             above; run `yarn install`, or use node_modules",
        ));
    };
    let text = std::fs::read_to_string(&manifest)?;
    let payload = ["RAW_RUNTIME_STATE", "hydrateRuntimeState(JSON.parse("]
        .iter()
        .find_map(|marker| {
            let after = &text[text.find(marker)? + marker.len()..];
            let start = after.find('\'')? + 1;
            let mut json = String::new();
            let mut escaped = false;
            for c in after[start..].chars() {
                match c {
                    '\'' if !escaped => return Some(json),
                    '\\' if !escaped => escaped = true,
                    _ => {
                        escaped = false;
                        json.push(c);
                    }
                }
            }
            None
        });
    let well_formed = payload
        .and_then(|json| serde_json::from_str::<serde_json::Value>(&json).ok())
        .and_then(|value| {
            let registry = value.get("packageRegistryData")?.as_array()?;
            registry.iter().find_map(|entry| {
                let entry = entry.as_array()?;
                (entry.first()?.is_null()
                    && entry.get(1)?.as_array()?.iter().any(|range| {
                        range
                            .as_array()
                            .and_then(|r| r.first())
                            .is_some_and(serde_json::Value::is_null)
                    }))
                .then_some(())
            })
        })
        .is_some();
    if well_formed {
        Ok(manifest)
    } else {
        Err(unsupported(
            &manifest,
            "the Plug'n'Play manifest has no readable top-level package; regenerate it with \
             `yarn install`",
        ))
    }
}

/// Everything a run needs from `options`: the normalised settings and the resolver
/// configuration, with the tsconfig, the Babel config and the PnP manifest read.
///
/// # Errors
/// An invalid pattern, or a tsconfig, Babel config or PnP manifest that cannot be used.
pub fn prepare(
    options: &TypeScriptOptions,
    cwd: &Path,
) -> Result<(Settings, ResolveConfig), ExtractError> {
    let mut settings = Settings::new(options, cwd)?;
    let mut config = resolve_config(options);
    load_tsconfig(&mut config, cwd)?;
    if let Some(file) = options
        .babel_config
        .as_ref()
        .and_then(|b| b.file_name.as_ref())
    {
        let path = absolute(cwd, Path::new(file));
        settings.babel =
            Some(
                BabelAliases::load(&path, cwd).map_err(|e| ExtractError::UnsupportedFile {
                    path: e.path.clone(),
                    reason: e.to_string(),
                })?,
            );
    }
    if config.yarn_pnp {
        let base = absolute(cwd, &settings.base_dir);
        pnp_manifest(&base)?;
        config.pnp_root = Some(base);
    }
    Ok((settings, config))
}

fn to_dependency(extracted: &Extracted) -> Dependency {
    Dependency {
        protocol: extracted.protocol,
        mime_type: extracted.mime_type.clone(),
        core_module: extracted.core_module,
        dependency_types: extracted.dependency_types.clone(),
        license: extracted.license.clone(),
        followable: extracted.followable,
        dynamic: extracted.dynamic,
        exotically_required: extracted.exotically_required,
        exotic_require: extracted.exotic_require.clone(),
        matches_do_not_follow: Some(extracted.matches_do_not_follow),
        could_not_resolve: extracted.could_not_resolve,
        pre_compilation_only: extracted.pre_compilation_only,
        line: Some(extracted.line),
        column: Some(extracted.column),
        dependency_kind: Some(DependencyKind::Import),
        ..Dependency::new(
            extracted.module.clone(),
            extracted.resolved.clone(),
            extracted.module_system,
        )
    }
}

/// 1-based line and column of a byte offset.
pub fn line_column(source: &str, offset: u32) -> (u32, u32) {
    pipeline::Lines::new(source).locate(offset)
}

/// Extracts with `settings` and `config` from [`prepare`]: the modules the inputs reach, sorted
/// by source.
///
/// # Errors
/// Any [`ExtractError`]: a missing input, an unreadable or unparsable file, a file only the
/// sidecar reads ([ADR-0017](../../../docs/adr/0017-coffeescript-livescript-sidecar.md)), or no
/// module at all.
pub fn extract_with(
    roots: &[PathBuf],
    settings: &Settings,
    config: &ResolveConfig,
) -> Result<Extraction, ExtractError> {
    let inputs: Vec<String> = roots
        .iter()
        .map(|r| r.to_string_lossy().replace('\\', "/"))
        .collect();
    let extracted = pipeline::extract(&inputs, settings, config)?;
    if let Some(sidecar) = extracted
        .iter()
        .find(|m| m.as_dependency.is_none() && needs_sidecar(&m.source))
    {
        return Err(ExtractError::UnsupportedFile {
            path: PathBuf::from(&sidecar.source),
            reason: SIDECAR_REASON.to_owned(),
        });
    }
    let mut modules = Vec::with_capacity(extracted.len());
    let mut files = 0u64;
    for module in extracted {
        let mut node = Module::new(module.source.clone());
        if let Some(dependency) = &module.as_dependency {
            node.followable = Some(dependency.followable);
            node.core_module = Some(dependency.core_module);
            node.could_not_resolve = Some(dependency.could_not_resolve);
            node.matches_do_not_follow = Some(dependency.matches_do_not_follow);
            node.dependency_types = Some(dependency.dependency_types.clone());
        } else {
            files += 1;
            let typescript = Path::new(&module.source)
                .extension()
                .is_some_and(|e| matches!(e.to_str(), Some("ts" | "tsx" | "mts" | "cts")));
            node.dependencies = module.dependencies.iter().map(to_dependency).collect();
            node.experimental_stats = module.experimental_stats;
            node.language = Some(if typescript {
                Language::Typescript
            } else {
                Language::Javascript
            });
        }
        modules.push(node);
    }
    if files == 0 {
        return Err(ExtractError::NoModulesFound);
    }
    // Stable, so a source that appears twice (an unfollowed dependency reached from two
    // modules) keeps upstream's relative order.
    modules.sort_by(|a, b| a.source.cmp(&b.source));
    let count = modules.len() as u64;
    Ok(Extraction {
        modules,
        code: None,
        inspected: Receipt {
            files,
            assemblies: 0,
            modules: count,
        },
        warnings: Vec::new(),
    })
}

impl Extractor for TypeScriptExtractor {
    type Options = TypeScriptOptions;

    fn extract(
        &self,
        roots: &[PathBuf],
        options: &TypeScriptOptions,
    ) -> Result<Extraction, ExtractError> {
        let cwd = std::env::current_dir()?;
        let (settings, config) = prepare(options, &cwd)?;
        extract_with(roots, &settings, &config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owns_typescript_and_javascript() {
        assert!(owns("src/a.ts"));
        assert!(owns("src/a.d.ts"));
        assert!(owns("src/a.mjs"));
        assert!(!owns("src/a.py"));
        assert!(!owns("README"));
    }

    #[test]
    fn line_and_column_are_one_based() {
        assert_eq!(line_column("a\nbc", 0), (1, 1));
        assert_eq!(line_column("a\nbc", 3), (2, 2));
        assert_eq!(line_column("ab", 99), (1, 3));
        assert_eq!(line_column("\u{e9}\u{e9}x", 3), (1, 2));
        assert_eq!(line_column("a\n\nb", 3), (3, 1));
    }

    #[test]
    fn sidecar_languages_are_separate() {
        for file in [
            "lib/x.coffee",
            "x.litcoffee",
            "x.ls",
            "x.cjsx",
            "x.csx",
            "x.coffee.md",
        ] {
            assert!(needs_sidecar(file), "{file}");
        }
        assert!(!owns("lib/x.coffee"));
        assert!(!needs_sidecar("lib/x.ts"));
        assert!(!needs_sidecar("README.md"));
        assert!(SIDECAR_REASON.contains("ADR-0017"));
    }

    #[test]
    fn resolve_config_maps_every_enhanced_resolve_key() {
        let options: TypeScriptOptions = serde_json::from_str(
            r#"{"preserveSymlinks": true, "combinedDependencies": true,
                "externalModuleResolutionStrategy": "yarn-pnp",
                "enhancedResolveOptions": {"extensions": [".ts"], "mainFields": ["module"],
                  "mainFiles": ["main"], "exportsFields": ["exports"],
                  "conditionNames": ["import"], "aliasFields": ["browser"],
                  "cachedInputFileSystem": {"cacheDuration": 10}}}"#,
        )
        .unwrap_or_default();
        let config = resolve_config(&options);
        assert!(!config.symlinks && config.combined_dependencies && config.yarn_pnp);
        assert_eq!(config.extensions, [".ts"]);
        assert_eq!(config.main_fields, ["module"]);
        assert_eq!(config.main_files, ["main"]);
        assert_eq!(config.exports_fields, ["exports"]);
        assert_eq!(config.condition_names, ["import"]);
        assert_eq!(config.alias_fields, ["browser"]);
        let default = resolve_config(&TypeScriptOptions::default());
        assert!(default.symlinks && !default.yarn_pnp);
    }

    #[test]
    fn pipeline_errors_become_extract_errors() {
        let parse = ExtractError::from(PipelineError::Parse {
            path: PathBuf::from("a.ts"),
            reason: "bad".to_owned(),
        });
        assert!(matches!(parse, ExtractError::UnsupportedFile { .. }));
        let pattern = ExtractError::from(PipelineError::Pattern {
            pattern: "(".to_owned(),
            reason: "unclosed".to_owned(),
        });
        assert_eq!(pattern.to_string(), "(: unclosed");
        let io = ExtractError::from(PipelineError::Io {
            path: PathBuf::from("x"),
            source: std::io::Error::other("gone"),
        });
        assert!(matches!(io, ExtractError::Io(_)));
    }
}
