//! `rb-extract-ts`: the TypeScript and JavaScript extractor over `oxc_parser` and `oxc_resolver`.
//!
//! - Architecture: [`docs/architecture.md#extractors`](../../../docs/architecture.md#extractors)
//! - Decisions: [ADR-0012](../../../docs/adr/0012-oxc-for-typescript.md),
//!   [ADR-0017](../../../docs/adr/0017-coffeescript-livescript-sidecar.md)
//! - Plans: [Wave 0, Spike A](../../../docs/plans/pending/0000-wave-0-spike.md),
//!   [Wave 1, sub-wave 1C](../../../docs/plans/pending/0001-wave-1-typescript-parity.md)
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
//! | [`collate`] | JavaScript's `localeCompare` order, which dependency-cruiser sorts with |
//! | [`pipeline`] | files to resolved, filtered, sorted dependencies, and the reachable modules |

pub mod collate;
pub mod core;
pub mod jsdoc;
pub mod npm;
pub mod pipeline;
pub mod resolve;
pub mod walk;

use std::path::PathBuf;

use rb_model::{
    Dependency, DependencyKind, ExtractError, Extraction, Extractor, Language, Module, Receipt,
    TypeScriptOptions,
};

use pipeline::{Extracted, PipelineError, Settings};
use resolve::ResolveConfig;

/// File extensions this extractor owns, from
/// [coverage § Extraction and resolution](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#extraction-and-resolution).
pub const EXTENSIONS: &[&str] = &["js", "mjs", "cjs", "jsx", "ts", "tsx", "mts", "cts"];

/// Extensions handled by the Node sidecar rather than natively
/// ([ADR-0017](../../../docs/adr/0017-coffeescript-livescript-sidecar.md)).
pub const SIDECAR_EXTENSIONS: &[&str] = &["coffee", "litcoffee", "ls", "cjsx", "csx"];

/// Whether a path is one this extractor parses natively.
pub fn owns(path: &str) -> bool {
    extension(path).is_some_and(|e| EXTENSIONS.contains(&e))
}

/// Whether a path needs the sidecar.
pub fn needs_sidecar(path: &str) -> bool {
    extension(path).is_some_and(|e| SIDECAR_EXTENSIONS.contains(&e))
}

fn extension(path: &str) -> Option<&str> {
    let name = path.rsplit('/').next()?;
    let (_, ext) = name.rsplit_once('.')?;
    Some(ext)
}

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

/// The resolver settings a `TypeScriptOptions` block asks for.
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
    }
    config
}

fn to_dependency(extracted: &Extracted, source: &str) -> Dependency {
    let (line, column) = line_column(source, extracted.span.start);
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
        line: Some(line),
        column: Some(column),
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
    let offset = (offset as usize).min(source.len());
    let before = &source[..offset];
    let line = before.matches('\n').count() + 1;
    let column = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
    (
        u32::try_from(line).unwrap_or(u32::MAX),
        u32::try_from(column).unwrap_or(u32::MAX),
    )
}

impl Extractor for TypeScriptExtractor {
    type Options = TypeScriptOptions;

    fn extract(
        &self,
        roots: &[PathBuf],
        options: &TypeScriptOptions,
    ) -> Result<Extraction, ExtractError> {
        let cwd = std::env::current_dir()?;
        let settings = Settings::new(options, &cwd)?;
        let config = resolve_config(options);
        let inputs: Vec<String> = roots
            .iter()
            .map(|r| r.to_string_lossy().replace('\\', "/"))
            .collect();
        let extracted = pipeline::extract(&inputs, &settings, &config)?;
        let mut modules = Vec::with_capacity(extracted.len());
        let mut files = 0u64;
        for module in extracted {
            let typescript = std::path::Path::new(&module.source)
                .extension()
                .is_some_and(|e| matches!(e.to_str(), Some("ts" | "tsx" | "mts" | "cts")));
            let language = if typescript {
                Language::Typescript
            } else {
                Language::Javascript
            };
            let mut node = Module::new(module.source.clone());
            if let Some(dependency) = &module.as_dependency {
                node.followable = Some(dependency.followable);
                node.core_module = Some(dependency.core_module);
                node.could_not_resolve = Some(dependency.could_not_resolve);
                node.matches_do_not_follow = Some(dependency.matches_do_not_follow);
                node.dependency_types = Some(dependency.dependency_types.clone());
            } else {
                files += 1;
                let source = std::fs::read_to_string(settings.base_dir.join(&module.source))
                    .unwrap_or_default();
                node.dependencies = module
                    .dependencies
                    .iter()
                    .map(|d| to_dependency(d, &source))
                    .collect();
                node.experimental_stats = module.experimental_stats;
                node.language = Some(language);
            }
            modules.push(node);
        }
        if files == 0 {
            return Err(ExtractError::NoModulesFound);
        }
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
    }

    #[test]
    fn sidecar_languages_are_separate() {
        assert!(needs_sidecar("lib/x.coffee"));
        assert!(!owns("lib/x.coffee"));
        assert!(!needs_sidecar("lib/x.ts"));
    }
}
