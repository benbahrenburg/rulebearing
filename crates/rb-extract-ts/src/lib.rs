//! `rb-extract-ts`: the TypeScript and JavaScript extractor over `oxc_parser` and `oxc_resolver`.
//!
//! - Architecture: [`docs/architecture.md#extractors`](../../../docs/architecture.md#extractors)
//! - Decisions: [ADR-0012](../../../docs/adr/0012-oxc-for-typescript.md),
//!   [ADR-0017](../../../docs/adr/0017-coffeescript-livescript-sidecar.md)
//! - Plans: [Wave 0, Spike A](../../../docs/plans/pending/0000-wave-0-spike.md),
//!   [Wave 1, sub-wave 1C](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#wave-1c-rb-extract-ts-completion),
//!   [Wave 2C](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#wave-2c-element-slice-and-diagram-rules-the-capability-table-gate-2-to-zero)
//!   (the code layer)
//! - Requirements: [FR-EXT-TS-01](../../../docs/prd.md#fr-ext-ts-01) to [FR-EXT-TS-05](../../../docs/prd.md#fr-ext-ts-05),
//!   [FR-CORE-01](../../../docs/prd.md#fr-core-01) (the code layer for every language)
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
//! | [`sfc`] | the `<script>` blocks of `.vue` and `.svelte` components |
//! | [`md`] | the JavaScript and TypeScript fences of Markdown, when `extraExtensionsToScan` lists `.md` |
//! | [`codelayer`] | classes, interfaces, enums, type aliases, functions, members, decorators and calls |
//! | [`collate`] | JavaScript's `localeCompare` order, which dependency-cruiser sorts with |
//! | [`pipeline`] | files to resolved, filtered, sorted dependencies, and the reachable modules |
//! | [`incremental`] | an incremental run that keeps the earlier extraction's unchanged modules |
//! | [`sidecar`] | `--sidecar node`: CoffeeScript and LiveScript through the repository's own dependency-cruiser |
//!
//! `checksum` is left absent on every module: dependency-cruiser computes it only in its cache
//! (`src/cache/`), never during extraction, and the cache is a later wave's surface.

pub mod babel;
pub mod codelayer;
/// JavaScript's `localeCompare` order, shared with the engine through `rb-model`.
pub use rb_model::collate;
pub mod core;
pub mod incremental;
pub mod jsdoc;
pub mod md;
pub mod npm;
pub mod pipeline;
pub mod resolve;
pub mod sfc;
pub mod sidecar;
pub mod walk;

use std::path::{Path, PathBuf};

use rb_model::ExternalModuleResolutionStrategy;
use rb_model::{
    Dependency, DependencyKind, ExtractError, Extraction, Extractor, Module, Receipt,
    TypeScriptOptions,
};

use babel::BabelAliases;
use pipeline::{Extracted, PipelineError, PreCompilation, Settings, TsCompilerOptions};
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

/// Whether a walk with `options` gathers a file named `name` it meets in a folder, before the
/// path filters ([`pipeline::scanned`]). A caller that watches the folders of a kept walk
/// ([`rb_model::Walk`]) compares these names and the subfolders: no other entry can change the
/// walk's sources.
pub fn gathers(options: &TypeScriptOptions, name: &str) -> bool {
    pipeline::scanned(
        options
            .extra_extensions_to_scan
            .as_deref()
            .unwrap_or_default(),
        name,
    )
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

/// The reason a sidecar file stops a run without `--sidecar node`, opening with the reason's name
/// ([Wave 3 § 1.5](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#15-interfaces-and-contracts-this-wave-freezes)).
pub const SIDECAR_REASON: &str = "unsupported-file-needs-sidecar: CoffeeScript and LiveScript are \
     extracted by the Node sidecar (ADR-0017), which is not enabled; run with `--sidecar node`, or \
     exclude the file";

/// What a `.csx` file adds to a sidecar reason: `.csx` is also the extension of C# scripts
/// (dotnet-script), which are not CoffeeScript, so the fix may be to exclude the file rather than
/// to run the sidecar. Empty for every other file.
pub fn csx_note(file: &str) -> &'static str {
    if extension(file) == Some("csx") {
        "; a .csx file may be a C# script (dotnet-script) rather than CoffeeScript JSX: if it is, \
         exclude it with options.exclude (for example `exclude: {path: \"\\\\.csx$\"}`) or \
         --exclude \"\\.csx$\""
    } else {
        ""
    }
}

/// [`SIDECAR_REASON`] for `file`, with [`csx_note`] for a `.csx` file.
pub fn sidecar_reason(file: &Path) -> String {
    format!(
        "{SIDECAR_REASON}{}",
        csx_note(&file.to_string_lossy().replace('\\', "/"))
    )
}

/// The TypeScript and JavaScript extractor.
#[derive(Debug, Clone, Copy, Default)]
pub struct TypeScriptExtractor;

impl From<PipelineError> for ExtractError {
    fn from(error: PipelineError) -> Self {
        match error {
            // The error names the file, which a bare I/O error would not.
            PipelineError::Io { path, source } => Self::UnsupportedFile {
                path,
                reason: format!("cannot be read: {source}"),
            },
            PipelineError::Parse { path, reason } => Self::UnsupportedFile { path, reason },
            PipelineError::NeedsSidecar { path } => Self::UnsupportedFile {
                reason: sidecar_reason(&path),
                path,
            },
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

/// Every configuration file an extraction with `options` from `cwd` reads besides the sources and
/// `package.json` files: the tsconfig with the files its `extends` chains and project references
/// name (each read as the resolver reads it), the Babel configuration, and the Plug'n'Play
/// manifest. A cache keys a reused extraction on them
/// ([Wave 3, Step 2](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#21-steps-for-sub-wave-3a-cache---affected-diff---exit-code-mode-strict)).
/// A file named but absent is listed too, so its appearance is seen.
pub fn configuration_files(options: &TypeScriptOptions, cwd: &Path) -> Vec<PathBuf> {
    let config = resolve_config(options);
    let mut found = std::collections::BTreeSet::new();
    if let Some(file) = &config.tsconfig {
        let resolver = config.resolver(None);
        let mut stack = vec![absolute(cwd, file)];
        while let Some(file) = stack.pop() {
            if !found.insert(file.clone()) {
                continue;
            }
            let Ok(tsconfig) = resolver.resolve_tsconfig(&file) else {
                continue;
            };
            let directory = file.parent().unwrap_or(Path::new("")).to_path_buf();
            let specifiers: Vec<String> = match &tsconfig.extends {
                Some(oxc_resolver::ExtendsField::Single(one)) => vec![one.clone()],
                Some(oxc_resolver::ExtendsField::Multiple(many)) => many.clone(),
                None => Vec::new(),
            };
            for specifier in specifiers {
                stack.extend(extended_tsconfig(&resolver, &directory, &specifier));
            }
            stack.extend(tsconfig.references_resolved.iter().map(|r| r.path.clone()));
        }
    }
    if let Some(file) = options
        .babel_config
        .as_ref()
        .and_then(|b| b.file_name.as_ref())
    {
        found.insert(absolute(cwd, Path::new(file)));
    }
    if config.yarn_pnp
        && let Ok(manifest) = pnp_manifest(cwd)
    {
        found.insert(manifest);
    }
    found.into_iter().collect()
}

/// The file a tsconfig `extends` specifier names, as TypeScript and the resolver find it: a path
/// relative to the tsconfig's folder (with `.json` added when the name has none and the bare
/// name is not a file), an absolute path, or a package's file.
fn extended_tsconfig(
    resolver: &oxc_resolver::Resolver,
    directory: &Path,
    specifier: &str,
) -> Option<PathBuf> {
    let path = match specifier.as_bytes().first()? {
        b'/' => PathBuf::from(specifier),
        b'.' => directory.join(specifier),
        _ => {
            return resolver
                .resolve(directory, specifier)
                .or_else(|_| resolver.resolve(directory, &format!("{specifier}/tsconfig.json")))
                .ok()
                .map(|r| r.full_path());
        }
    };
    if path.is_file() || path.extension().is_some_and(|e| e == "json") {
        Some(path)
    } else {
        Some(PathBuf::from(format!("{}.json", path.display())))
    }
}

fn absolute(cwd: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
}

/// Reads the tsconfig named in `config` (with its `extends` chain), records its `baseUrl` and
/// `paths` keys, which classify a dependency as `aliased-tsconfig-*`, and returns its `module`
/// and `target`, which decide what `transpileModule` hands acorn.
fn load_tsconfig(
    config: &mut ResolveConfig,
    cwd: &Path,
) -> Result<TsCompilerOptions, ExtractError> {
    let Some(file) = config.tsconfig.clone() else {
        return Ok(TsCompilerOptions::default());
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
    Ok(TsCompilerOptions {
        module: options.module.as_deref().map(str::to_ascii_lowercase),
        target: options.target.as_deref().map(str::to_ascii_lowercase),
    })
}

/// Stops a run whose tsconfig sets a `module` in [`pipeline::UNSUPPORTED_MODULES`] when some
/// TypeScript file will be compiled for acorn, which is when `tsPreCompilationDeps` is not
/// `true` and neither `tsc` nor `swc` is the parser (`"specify"` compiles too, to compare).
fn check_module(settings: &Settings, tsconfig: Option<&Path>) -> Result<(), ExtractError> {
    let Some(module) = settings.compiler_options.module.as_deref() else {
        return Ok(());
    };
    let compiles = pipeline::flavour_for(settings, "module.ts") == walk::Flavour::Acorn
        || settings.pre_compilation == PreCompilation::Specify;
    if !compiles || !pipeline::UNSUPPORTED_MODULES.contains(&module) {
        return Ok(());
    }
    Err(ExtractError::UnsupportedFile {
        path: tsconfig.map(Path::to_path_buf).unwrap_or_default(),
        reason: format!(
            "compilerOptions.module \"{module}\" is not supported when TypeScript is compiled \
             before extraction (TypeScript 6 deprecates it and TypeScript 7 removes it); set \
             module to commonjs, nodenext, esnext or preserve, or set tsPreCompilationDeps to \
             true"
        ),
    })
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
    settings.compiler_options = load_tsconfig(&mut config, cwd)?;
    check_module(&settings, config.tsconfig.as_deref())?;
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
        // In the spelling the resolver gives importing files: a verbatim `\\?\` root would never
        // contain them, and the manifest would answer for none of them.
        let base = resolve::simplified(&absolute(cwd, &settings.base_dir));
        pnp_manifest(&base)?;
        config.pnp_root = Some(base);
    }
    Ok((settings, config))
}

/// A dependency the walk kept, as the document writes it. One of a file the sidecar extracted
/// carries `sidecar: true` and no position, which dependency-cruiser does not record.
pub(crate) fn to_dependency(extracted: &Extracted, by_sidecar: bool) -> Dependency {
    let position = |at: u32| (!by_sidecar).then_some(at);
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
        line: position(extracted.line),
        column: position(extracted.column),
        dependency_kind: Some(DependencyKind::Import),
        sidecar: by_sidecar.then_some(true),
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

/// Extracts with `settings` and `config` from [`prepare`]: the modules the inputs reach, in
/// dependency-cruiser's visiting order (depth first from the sorted initial sources, each
/// module's unfollowed dependencies right after it).
///
/// With [`Settings::sidecar`] set, the CoffeeScript and LiveScript files the walk reaches are
/// extracted by the sidecar ([`sidecar`]).
///
/// # Errors
/// Any [`ExtractError`]: a missing input, an unreadable or unparsable file, a file only the
/// sidecar reads when it is off ([ADR-0017](../../../docs/adr/0017-coffeescript-livescript-sidecar.md))
/// or a sidecar that cannot run, or no module at all.
pub fn extract_with(
    roots: &[PathBuf],
    settings: &Settings,
    config: &ResolveConfig,
) -> Result<Extraction, ExtractError> {
    let walk = pipeline::gather_walk(&inputs(roots), settings)?;
    let walked = sidecar::extract_modules(
        &walk.sources,
        settings,
        config,
        &std::collections::BTreeMap::new(),
        None,
    )?;
    let mut extraction = finish(walked, settings)?;
    extraction.walk = settings.keep_walk.then_some(walk);
    Ok(extraction)
}

/// The walk as an [`Extraction`], with the sidecar's receipt and, off the pinned version, its
/// warning.
fn finish(walked: sidecar::Walked, settings: &Settings) -> Result<Extraction, ExtractError> {
    let mut extraction = to_extraction(walked.modules, settings)?;
    extraction.sidecar = sidecar::receipt(&extraction.modules, walked.version.as_deref());
    if let Some(warning) = extraction
        .sidecar
        .as_ref()
        .and_then(sidecar::version_warning)
    {
        extraction.warnings.push(warning);
    }
    Ok(extraction)
}

fn inputs(roots: &[PathBuf]) -> Vec<String> {
    roots
        .iter()
        .map(|r| r.to_string_lossy().replace('\\', "/"))
        .collect()
}

/// A dependency of an earlier extraction back in the pipeline's form: the inverse of
/// `to_dependency` for every field the pipeline reads after a file is extracted. The span is
/// not kept in the document; nothing after the file's own extraction reads it.
pub fn from_dependency(dependency: &Dependency) -> Extracted {
    Extracted {
        module: dependency.module.clone(),
        module_system: dependency.module_system,
        dynamic: dependency.dynamic,
        exotically_required: dependency.exotically_required,
        exotic_require: dependency.exotic_require.clone(),
        dependency_types: dependency.dependency_types.clone(),
        protocol: dependency.protocol,
        mime_type: dependency.mime_type.clone(),
        pre_compilation_only: dependency.pre_compilation_only,
        resolved: dependency.resolved.clone(),
        core_module: dependency.core_module,
        followable: dependency.followable,
        could_not_resolve: dependency.could_not_resolve,
        matches_do_not_follow: dependency.matches_do_not_follow.unwrap_or(false),
        license: dependency.license.clone(),
        span: oxc_span::Span::default(),
        line: dependency.line.unwrap_or(0),
        column: dependency.column.unwrap_or(0),
    }
}

/// Why an incremental extraction reads every file instead of reusing the unchanged ones, or
/// `None` when reuse is exact: `exclude.dynamic` removes, after the walk, dependencies the walk
/// followed, so the modules no longer carry each file's result; and under `maxDepth` a file the
/// earlier walk met at the limit was not read, so its module (no dependencies) is not the file's
/// result once an edit brings it nearer. (A file whose code layer before linking was not kept,
/// [`Settings::keep_file_states`], is read on its own account.)
pub fn reuse_refused(settings: &Settings) -> Option<&'static str> {
    if settings.exclude.as_ref().and_then(|f| f.dynamic).is_some() {
        return Some(
            "exclude.dynamic removed dependencies the walk followed, so the kept ones are not the file's result",
        );
    }
    if settings.max_depth != 0 {
        return Some(
            "maxDepth left the files met at the limit unread, so their modules are not the files' results",
        );
    }
    None
}

/// [`extract_with`] after some files changed: each file `request.unchanged` names is taken from
/// `request.previous` instead of being read, and the walk replays over them
/// ([Wave 3, Step 2](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#21-steps-for-sub-wave-3a-cache---affected-diff---exit-code-mode-strict)).
/// The result equals [`extract_with`]'s, module order included, when the caller's precondition
/// holds: no file was added, deleted or renamed and no manifest changed since `previous`, so
/// every unchanged file resolves as it did. When [`reuse_refused`] gives a reason, every file is
/// read. This borrows the request and copies `previous`; [`extract_incremental_from`] takes it.
///
/// # Errors
/// As [`extract_with`].
pub fn extract_incremental(
    roots: &[PathBuf],
    settings: &Settings,
    config: &ResolveConfig,
    request: &rb_model::ExtractRequest,
) -> Result<Extraction, ExtractError> {
    extract_incremental_from(roots, settings, config, request.clone())
}

/// [`extract_incremental`], taking the request, so the unchanged files' modules, file states and
/// code layer move from `request.previous` into the result instead of being copied
/// ([Wave 3, Step 16](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#24-steps-for-sub-wave-3d---mode-source-guard---watch-the-2-s-proof)).
/// With `request.walk_unchanged` and a walk kept in `previous`, the initial sources are the
/// walk's and no folder is listed again; otherwise the inputs are walked. Where
/// [`incremental::applies`], the walk replays over the earlier modules as they stand
/// ([`incremental::replay`]); elsewhere each unchanged file's result is rebuilt from its module
/// for the pipeline's walk. Both give [`extract_with`]'s result.
///
/// # Errors
/// As [`extract_with`].
pub fn extract_incremental_from(
    roots: &[PathBuf],
    settings: &Settings,
    config: &ResolveConfig,
    mut request: rb_model::ExtractRequest,
) -> Result<Extraction, ExtractError> {
    let kept = if request.walk_unchanged {
        request.previous.walk.take()
    } else {
        None
    };
    let walk = match kept {
        Some(walk) => walk,
        None => pipeline::gather_walk(&inputs(roots), settings)?,
    };
    let mut extraction = if incremental::applies(settings) {
        // Asked only, never iterated.
        let unchanged: std::collections::HashSet<std::borrow::Cow<'_, str>> =
            request.unchanged.iter().map(|p| as_source(p)).collect();
        let changed: std::collections::HashSet<std::borrow::Cow<'_, str>> =
            request.changed.iter().map(|p| as_source(p)).collect();
        // As the pipeline's reuse below: an unchanged file, not also named changed, and only the
        // sidecar stands behind a CoffeeScript or LiveScript result.
        let reusable = |module: &rb_model::Module| {
            unchanged.contains(module.source.as_str())
                && !changed.contains(module.source.as_str())
                && !needs_sidecar(&module.source)
        };
        let previous = std::mem::take(&mut request.previous);
        incremental::replay(&walk.sources, settings, config, reusable, previous)?
    } else {
        rebuilt(&walk.sources, settings, config, &request)?
    };
    extraction.walk = settings.keep_walk.then_some(walk);
    Ok(extraction)
}

/// A path as a module source, as [`rb_model::source_name`] spells it, borrowed when it is
/// spelled so already.
fn as_source(path: &Path) -> std::borrow::Cow<'_, str> {
    match path.to_str() {
        Some(text) if !text.contains('\\') && !text.starts_with("./") => {
            std::borrow::Cow::Borrowed(text)
        }
        _ => std::borrow::Cow::Owned(rb_model::source_name(path)),
    }
}

/// The incremental run of the pipeline's own walk: each reusable file's result rebuilt from its
/// earlier module, the rest read, from the initial sources `initial`.
fn rebuilt(
    initial: &[String],
    settings: &Settings,
    config: &ResolveConfig,
    request: &rb_model::ExtractRequest,
) -> Result<Extraction, ExtractError> {
    let mut reused = std::collections::BTreeMap::new();
    if reuse_refused(settings).is_none() {
        let unchanged = request.unchanged_sources();
        let changed: std::collections::BTreeSet<String> = request
            .changed
            .iter()
            .map(|p| rb_model::source_name(p))
            .collect();
        for module in &request.previous.modules {
            // A module with a language is a file the walk read; the rest stand for dependencies
            // it did not follow and are rebuilt from those.
            if module.language.is_none()
                || !unchanged.contains(&module.source)
                || changed.contains(&module.source)
                // Only the sidecar may stand behind a CoffeeScript or LiveScript result.
                || (settings.sidecar.is_none() && needs_sidecar(&module.source))
            {
                continue;
            }
            // With the code layer on, a file is reused only with the code layer its earlier run
            // kept; without one it is read.
            let code = if settings.code_layer {
                match request
                    .previous
                    .files
                    .get(&module.source)
                    .map(|state| state.code.clone().map(serde_json::from_value).transpose())
                {
                    Some(Ok(code)) => code,
                    Some(Err(_)) | None => continue,
                }
            } else {
                None
            };
            reused.insert(
                module.source.clone(),
                pipeline::Reused {
                    dependencies: module.dependencies.iter().map(from_dependency).collect(),
                    experimental_stats: module.experimental_stats,
                    code,
                },
            );
        }
    }
    let earlier = request
        .previous
        .sidecar
        .as_ref()
        .map(|r| r.version.as_str());
    let walked = sidecar::extract_modules(initial, settings, config, &reused, earlier)?;
    finish(walked, settings)
}

/// The modules of a walk as an [`Extraction`]: the sidecar check, the linked code layer and the
/// document's modules in the walk's order, the dependencies of each file the sidecar extracted
/// marked `sidecar: true`. The sidecar's receipt is [`extract_with`]'s to add.
///
/// # Errors
/// A file only the sidecar reads when [`Settings::sidecar`] is off, or no file at all.
pub fn to_extraction(
    mut extracted: Vec<pipeline::ExtractedModule>,
    settings: &Settings,
) -> Result<Extraction, ExtractError> {
    if let Some(sidecar) = extracted.iter().find(|m| {
        settings.sidecar.is_none() && m.as_dependency.is_none() && needs_sidecar(&m.source)
    }) {
        return Err(ExtractError::UnsupportedFile {
            path: PathBuf::from(&sidecar.source),
            reason: sidecar_reason(Path::new(&sidecar.source)),
        });
    }
    let states = if settings.keep_file_states {
        extracted
            .iter()
            .filter(|m| m.as_dependency.is_none())
            .filter_map(|m| {
                let code = m.code.as_ref().map(serde_json::to_value).transpose().ok()?;
                Some((
                    m.source.clone(),
                    rb_model::FileState {
                        code,
                        warnings: Vec::new(),
                    },
                ))
            })
            .collect()
    } else {
        std::collections::BTreeMap::new()
    };
    let code = settings.code_layer.then(|| {
        codelayer::link(
            extracted
                .iter_mut()
                .filter_map(|module| module.code.take())
                .collect(),
        )
    });
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
            let by_sidecar = needs_sidecar(&module.source);
            node.dependencies = module
                .dependencies
                .iter()
                .map(|d| to_dependency(d, by_sidecar))
                .collect();
            node.experimental_stats = module.experimental_stats;
            node.language = Some(codelayer::language_of(&module.source));
        }
        modules.push(node);
    }
    if files == 0 {
        return Err(ExtractError::NoModulesFound);
    }
    // Upstream's order, not sorted: the cruise visits the initial sources depth first and
    // appends each module's unfollowed dependencies after it. The engine derives `dependents[]`
    // and enumerates cycles in module order, so sorting here would reorder both. The order is
    // a function of the inputs alone, so two runs still serialise byte for byte.
    let count = modules.len() as u64;
    Ok(Extraction {
        modules,
        code,
        inspected: Receipt::counts(files, 0, count),
        warnings: Vec::new(),
        files: states,
        sidecar: None,
        walk: None,
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
        assert_eq!(sidecar_reason(Path::new("a.coffee")), SIDECAR_REASON);
        let csx = sidecar_reason(Path::new("scripts/build.csx"));
        assert!(csx.starts_with(SIDECAR_REASON), "{csx}");
        assert!(
            csx.contains("C# script")
                && csx.contains("options.exclude")
                && csx.contains("--exclude"),
            "{csx}"
        );
        for file in ["a.cjsx", "a.coffee", "csx", "a.csx.js"] {
            assert_eq!(csx_note(file), "", "{file}");
        }
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
    fn the_walk_gathers_scannable_names_and_the_extra_extensions() {
        let plain = TypeScriptOptions::default();
        for (name, gathered) in [
            ("a.ts", true),
            ("a.d.ts", true),
            ("a.vue", true),
            ("a.coffee", true),
            ("a.md", false),
            ("a.json", false),
            (".a.ts.swp", false),
            ("4913", false),
        ] {
            assert_eq!(gathers(&plain, name), gathered, "{name}");
        }
        let extra = TypeScriptOptions {
            extra_extensions_to_scan: Some(vec![".md".to_owned()]),
            ..TypeScriptOptions::default()
        };
        assert!(gathers(&extra, "a.md"));
    }

    #[test]
    fn a_path_is_named_as_a_source_as_the_model_names_it() {
        for path in [
            "src/a.ts",
            "./a.ts",
            "././d.ts",
            "src\\c.ts",
            "",
            ".x/y.ts",
            "a/./b.ts",
        ] {
            let path = Path::new(path);
            assert_eq!(
                as_source(path),
                rb_model::source_name(path),
                "{}",
                path.display()
            );
        }
        assert!(matches!(
            as_source(Path::new("src/a.ts")),
            std::borrow::Cow::Borrowed("src/a.ts")
        ));
    }

    proptest::proptest! {
        #[test]
        fn as_source_agrees_with_source_name(
            parts in proptest::collection::vec("[a-z.\\\\/]{0,4}", 0..5)
        ) {
            let text = parts.concat();
            let path = Path::new(&text);
            proptest::prop_assert_eq!(as_source(path), rb_model::source_name(path));
        }
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
        assert_eq!(io.to_string(), "x: cannot be read: gone");
    }
}
