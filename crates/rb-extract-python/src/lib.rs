//! `rb-extract-python`: the Python extractor over `ruff_python_parser`.
//!
//! - Architecture: [`docs/architecture.md#extractors`](../../../docs/architecture.md#extractors)
//! - Decisions: [ADR-0013](../../../docs/adr/0013-ruff-parser-for-python.md),
//!   [ADR-0010](../../../docs/adr/0010-crate-layout-and-extractor-boundary.md),
//!   [ADR-0014](../../../docs/adr/0014-no-invented-cross-language-edges.md)
//! - Plan: [Wave 2, sub-wave 2B](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#wave-2b-rb-extract-python),
//!   [Step 4](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#24-step-4-the-python-extractor-2b)
//! - Requirements: [FR-EXT-PY-01](../../../docs/prd.md#fr-ext-py-01), [FR-EXT-PY-02](../../../docs/prd.md#fr-ext-py-02)
//! - Specification: import-linter's contracts and tests on the Python oracle repositories
//!   ([ADR-0009](../../../docs/adr/0009-conformance-suites-as-specification.md))
//!
//! Rule of the boundary: this crate reads `.py` files and a virtual environment's metadata and
//! writes `rb_model` types only. It never executes an interpreter and never opens a network
//! connection.
//!
//! | Module | Does |
//! | --- | --- |
//! | [`discover`] | the import roots, the files under them, dotted module names |
//! | [`parse`] | every import form, `TYPE_CHECKING` and dynamic imports, `__all__` |
//! | [`resolve`] | the resolution order of plan § 1.4.4 |
//! | [`stdlib`] | the bundled `sys.stdlib_module_names` snapshots, 3.8 to 3.14 |
//! | [`site`] | the installed-distributions index and licences |
//! | [`codelayer`] | classes, functions, methods, properties and decorators |
//!
//! Output: one module per `.py` file, `source` its repository-relative path; one module per
//! target that is not such a file (a standard-library or installed module, an unresolved name,
//! a namespace package's folder) with `followable: false`, as dependency-cruiser lists the
//! modules it does not follow, so every `resolved` names a module. Modules are sorted by
//! `source`, each module's dependencies by `resolved`, then line and column. The receipt carries
//! the Python observability fields of
//! [plan § 1.8](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#18-quality-attributes):
//! `roots`, `stdlibVersion` and `site` (`none` when no environment was found).

pub mod codelayer;
pub mod discover;
pub mod parse;
pub mod resolve;
pub mod site;
pub mod stdlib;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use rayon::prelude::*;
use rb_model::{
    CodeLayer, Dependency, DependencyKind, DependencyType, ExtractError, Extraction, Extractor,
    Language, Module, ModuleSystem, PythonOptions, Receipt, Warning,
};

use discover::Layout;
use parse::{ImportSpec, Lines, Origin};
pub use resolve::resolve_relative;
use resolve::{Identity, ModuleIndex, Resolution, Resolved};
use site::SiteIndex;
use stdlib::StdlibSet;

/// The `dependencyTypes` vocabulary for Python, from
/// [design § One engine, three languages](../../../docs/artifacts/design.md#one-engine-three-languages-one-monorepo).
pub const DEPENDENCY_TYPES: &[&str] = &[
    "local",
    "stdlib",
    "site",
    "type-only",
    "dynamic",
    "unresolved",
];

/// The Python extractor.
#[derive(Debug, Clone, Copy, Default)]
pub struct PythonExtractor;

/// Everything a run needs besides the files: the layout, the snapshot and the site index.
#[derive(Debug, Clone)]
pub struct Settings {
    /// The working directory every path is relative to.
    pub base: PathBuf,
    /// The discovered roots, entry points and `requires-python`.
    pub layout: Layout,
    /// The standard-library snapshot.
    pub stdlib: StdlibSet,
    /// Whether `.pyi` stubs are modules.
    pub stubs: bool,
    /// The installed-distributions index, when an environment was found.
    pub site: Option<SiteIndex>,
    /// Problems found while preparing, reported with the run.
    pub warnings: Vec<Warning>,
}

fn version_tuple(version: &str) -> Option<(u32, u32)> {
    let text = stdlib::major_minor(version)?;
    let (major, minor) = text.split_once('.')?;
    Some((major.parse().ok()?, minor.parse().ok()?))
}

/// The snapshot for the configured version, else the `requires-python` lower bound (brought
/// into the supported range with a warning), else the default.
fn choose_stdlib(
    configured: Option<&str>,
    requires_python: Option<&str>,
    warnings: &mut Vec<Warning>,
) -> Result<StdlibSet, ExtractError> {
    let supported = stdlib::supported();
    let range = format!(
        "{} to {}",
        supported.first().copied().unwrap_or_default(),
        supported.last().copied().unwrap_or_default()
    );
    if let Some(version) = configured {
        return StdlibSet::for_version(version).ok_or_else(|| ExtractError::UnsupportedFile {
            path: PathBuf::from("languages.python.version"),
            reason: format!(
                "no standard-library snapshot for Python {version}; set a version from {range}"
            ),
        });
    }
    let fallback = || StdlibSet::for_version(stdlib::DEFAULT_VERSION);
    let chosen = match requires_python {
        Some(bound) => StdlibSet::for_version(bound).or_else(|| {
            let wanted = version_tuple(bound)?;
            let lowest = supported.first().copied().and_then(version_tuple)?;
            let nearest = if wanted < lowest {
                supported.first()
            } else {
                supported.last()
            };
            let set = nearest.and_then(|v| StdlibSet::for_version(v))?;
            warnings.push(Warning::about(
                "pyproject.toml",
                format!(
                    "requires-python's lower bound {bound} has no standard-library snapshot; using {}. Set languages.python.version to choose one from {range}",
                    set.version()
                ),
            ));
            Some(set)
        }),
        None => fallback(),
    };
    chosen
        .or_else(fallback)
        .ok_or_else(|| ExtractError::UnsupportedFile {
            path: PathBuf::from("languages.python.version"),
            reason: format!("no standard-library snapshot is bundled; supported {range}"),
        })
}

/// Reads the layout, chooses the snapshot and indexes the environment for a run at `base`.
/// `virtual_env` is `$VIRTUAL_ENV`, passed in so the caller decides where it comes from.
///
/// # Errors
/// An unreadable `pyproject.toml` or `setup.cfg`, or a configured version with no snapshot.
pub fn prepare(
    base: &Path,
    options: &PythonOptions,
    virtual_env: Option<&Path>,
) -> Result<Settings, ExtractError> {
    let layout = discover::discover(base, options.roots.as_deref()).map_err(|e| match e {
        discover::DiscoverError::Io(io) => ExtractError::Io(io),
        discover::DiscoverError::Toml { path, reason } => ExtractError::UnsupportedFile {
            path,
            reason: format!(
                "not valid TOML ({reason}); fix the file or set languages.python.roots"
            ),
        },
    })?;
    let mut warnings = Vec::new();
    let stdlib = choose_stdlib(
        options.version.as_deref(),
        layout.requires_python.as_deref(),
        &mut warnings,
    )?;
    let site = site::find(base, &layout.roots, virtual_env, stdlib.version()).and_then(|path| {
        let shown = if path.starts_with(base) {
            discover::relative(base, &path)
        } else {
            path.to_string_lossy().replace('\\', "/")
        };
        match site::index(&path, shown) {
            Ok(index) => Some(index),
            Err(error) => {
                warnings.push(Warning::about(
                    &path,
                    format!("site-packages could not be read ({error}); third-party imports are unresolved"),
                ));
                None
            }
        }
    });
    Ok(Settings {
        base: base.to_path_buf(),
        layout,
        stdlib,
        stubs: options.stubs.unwrap_or(false),
        site,
        warnings,
    })
}

/// The `dependencyTypes` of an edge: its resolution, then `type-only` and `dynamic`.
pub fn dependency_types(
    resolution: &Resolution,
    type_only: bool,
    dynamic: bool,
) -> Vec<DependencyType> {
    let mut types = vec![base_type(resolution)];
    if type_only {
        types.push(DependencyType::TypeOnly);
    }
    if dynamic {
        types.push(DependencyType::Dynamic);
    }
    types
}

fn base_type(resolution: &Resolution) -> DependencyType {
    match resolution {
        Resolution::Local(_) => DependencyType::Local,
        Resolution::Stdlib(_) => DependencyType::Stdlib,
        Resolution::Site { .. } => DependencyType::Site,
        Resolution::Unresolved(_) => DependencyType::Unresolved,
    }
}

/// What reading and parsing one file produced.
struct Parsed {
    identity: Identity,
    specs: Vec<ImportSpec>,
    code: CodeLayer,
    warning: Option<Warning>,
}

fn fallback_identity(file: &str, unrooted: bool) -> Identity {
    let stem = file
        .strip_suffix(".pyi")
        .or_else(|| file.strip_suffix(".py"))
        .unwrap_or(file);
    let dotted = stem.replace('/', ".");
    let package = dotted
        .rsplit_once('.')
        .map(|(p, _)| p.to_owned())
        .unwrap_or_default();
    Identity {
        dotted,
        package,
        init: false,
        unrooted,
    }
}

fn read_and_parse(settings: &Settings, index: &ModuleIndex, file: &str) -> Parsed {
    let identity = index
        .identity(file)
        .unwrap_or_else(|| fallback_identity(file, index.is_unrooted(file)));
    let failed = |message: String| Parsed {
        identity: identity.clone(),
        specs: Vec::new(),
        code: CodeLayer::default(),
        warning: Some(Warning::about(file, message)),
    };
    let source = match std::fs::read_to_string(settings.base.join(file)) {
        Ok(source) => source,
        Err(error) => {
            return failed(format!(
                "could not be read ({error}); the module has no dependencies in this run"
            ));
        }
    };
    let module = match parse::parse(&source) {
        Ok(module) => module,
        Err(error) => {
            return failed(format!(
                "{error}; the module has no dependencies in this run. {}",
                error.fix()
            ));
        }
    };
    let lines = Lines::new(&source);
    let specs = parse::imports(&module, &lines, identity.init);
    let project = identity.dotted.split('.').next().unwrap_or_default();
    let code = codelayer::elements(
        &module,
        &lines,
        codelayer::Context {
            module: &identity.dotted,
            package: &identity.package,
            file,
            project,
        },
    );
    Parsed {
        identity,
        specs,
        code,
        warning: None,
    }
}

fn to_dependency(spec: &ImportSpec, resolved: Resolved) -> Dependency {
    let dynamic = spec.origin == Origin::Dynamic;
    let resolution = resolved.resolution;
    let mut dependency = Dependency::new(resolved.module, resolved.resolved, ModuleSystem::Py);
    dependency.dependency_types = dependency_types(&resolution, spec.type_only, dynamic);
    dependency.core_module = matches!(resolution, Resolution::Stdlib(_));
    dependency.could_not_resolve = matches!(resolution, Resolution::Unresolved(_));
    dependency.followable = matches!(resolution, Resolution::Local(_));
    dependency.dynamic = dynamic;
    dependency.type_only = spec.type_only.then_some(true);
    dependency.line = Some(spec.line);
    dependency.column = Some(spec.column);
    dependency.dependency_kind = Some(DependencyKind::Import);
    if let Resolution::Site { license, .. } = resolution {
        dependency.license = license;
    }
    dependency
}

/// A module the run does not parse but some edge leads to.
fn target_module(dependency: &Dependency) -> Module {
    let mut module = Module::new(dependency.resolved.clone());
    module.followable = Some(false);
    module.core_module = Some(dependency.core_module);
    module.could_not_resolve = Some(dependency.could_not_resolve);
    module.dependency_types = dependency.dependency_types.first().map(|t| vec![*t]);
    module.license.clone_from(&dependency.license);
    // Every module carries its language (FR-CORE-01): a namespace package's folder, a standard
    // library module, an installed distribution and an unresolved name alike.
    module.language = Some(Language::Python);
    module
}

/// Extends every class's `baseTypes` from its direct bases to the whole chain the extracted
/// code declares, nearest first.
fn close_base_chains(layer: &mut CodeLayer) {
    let direct: BTreeMap<String, Vec<String>> = layer
        .types
        .iter()
        .filter(|t| t.kind == "class")
        .map(|t| (t.full_name.clone(), t.base_types.clone()))
        .collect();
    for ty in &mut layer.types {
        let Some(bases) = direct.get(&ty.full_name) else {
            continue;
        };
        let mut chain: Vec<String> = Vec::new();
        let mut queue: std::collections::VecDeque<String> = bases.iter().cloned().collect();
        while let Some(next) = queue.pop_front() {
            if next == ty.full_name || chain.contains(&next) {
                continue;
            }
            if let Some(more) = direct.get(&next) {
                queue.extend(more.iter().cloned());
            }
            chain.push(next);
        }
        ty.base_types = chain;
    }
}

/// Extracts every Python module under `inputs` (relative to `settings.base`) with
/// `settings` from [`prepare`].
///
/// # Errors
/// An input that does not exist or a folder that cannot be listed, or no module at all. A file
/// that cannot be read or parsed is a warning naming it, and the run continues.
pub fn extract_with(inputs: &[PathBuf], settings: &Settings) -> Result<Extraction, ExtractError> {
    let files = discover::walk(&settings.base, inputs, settings.stubs)?;
    if files.is_empty() {
        return Err(ExtractError::NoModulesFound);
    }
    let index = ModuleIndex::build(&settings.layout.roots, &files);
    let parsed: Vec<Parsed> = files
        .par_iter()
        .map(|file| read_and_parse(settings, &index, file))
        .collect();
    let known: BTreeSet<&str> = files.iter().map(String::as_str).collect();
    let mut warnings = settings.warnings.clone();
    let mut modules = Vec::with_capacity(files.len());
    let mut targets: BTreeMap<String, Module> = BTreeMap::new();
    let mut code = CodeLayer::default();
    for (file, result) in files.iter().zip(parsed) {
        warnings.extend(result.warning);
        code.merge(result.code);
        let mut seen = BTreeSet::new();
        let mut dependencies: Vec<Dependency> = Vec::new();
        for spec in &result.specs {
            let Some(resolved) = resolve::resolve(
                &result.identity,
                spec,
                &index,
                &settings.stdlib,
                settings.site.as_ref(),
            ) else {
                continue;
            };
            let key = (
                resolved.module.clone(),
                resolved.resolved.clone(),
                spec.type_only,
                spec.origin == Origin::Dynamic,
            );
            if !seen.insert(key) {
                continue;
            }
            if let Some(note) = &resolved.note {
                warnings.push(Warning::about(file, note.clone()));
            }
            let dependency = to_dependency(spec, resolved);
            if !known.contains(dependency.resolved.as_str()) {
                targets
                    .entry(dependency.resolved.clone())
                    .or_insert_with(|| target_module(&dependency));
            }
            dependencies.push(dependency);
        }
        dependencies.sort_by(|a, b| {
            (&a.resolved, a.line, a.column, &a.module).cmp(&(
                &b.resolved,
                b.line,
                b.column,
                &b.module,
            ))
        });
        let mut module = Module::new(file.clone());
        module.dependencies = dependencies;
        module.language = Some(Language::Python);
        module.project = result.identity.dotted.split('.').next().map(str::to_owned);
        // The dotted module name, which a slice pattern (`app.(*)`) matches, as a .NET file's
        // namespaces are.
        module.namespaces =
            (!result.identity.dotted.is_empty()).then(|| vec![result.identity.dotted.clone()]);
        modules.push(module);
    }
    modules.extend(targets.into_values());
    modules.sort_by(|a, b| a.source.cmp(&b.source));
    close_base_chains(&mut code);
    code.normalise();
    let receipt = Receipt {
        roots: Some(settings.layout.roots.clone()),
        stdlib_version: Some(settings.stdlib.version().to_owned()),
        site: Some(
            settings
                .site
                .as_ref()
                .map_or_else(|| "none".to_owned(), |s| s.path.clone()),
        ),
        ..Receipt::counts(files.len() as u64, 0, modules.len() as u64)
    };
    Ok(Extraction {
        modules,
        code: Some(code),
        inspected: receipt,
        warnings,
    })
}

/// Extracts at `base` as the working directory, with `$VIRTUAL_ENV` given explicitly.
///
/// # Errors
/// As [`prepare`] and [`extract_with`].
pub fn extract_at(
    base: &Path,
    inputs: &[PathBuf],
    options: &PythonOptions,
    virtual_env: Option<&Path>,
) -> Result<Extraction, ExtractError> {
    let settings = prepare(base, options, virtual_env)?;
    extract_with(inputs, &settings)
}

impl Extractor for PythonExtractor {
    type Options = PythonOptions;

    fn extract(
        &self,
        roots: &[PathBuf],
        options: &PythonOptions,
    ) -> Result<Extraction, ExtractError> {
        let cwd = std::env::current_dir()?;
        let virtual_env = std::env::var_os("VIRTUAL_ENV").map(PathBuf::from);
        extract_at(&cwd, roots, options, virtual_env.as_deref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dependency_types_follow_the_resolution() {
        assert_eq!(
            dependency_types(&Resolution::Local("a.py".into()), false, false),
            [DependencyType::Local]
        );
        assert_eq!(
            dependency_types(&Resolution::Stdlib("os".into()), true, true),
            [
                DependencyType::Stdlib,
                DependencyType::TypeOnly,
                DependencyType::Dynamic
            ]
        );
        assert_eq!(
            dependency_types(
                &Resolution::Site {
                    dist: "x".into(),
                    license: None
                },
                false,
                true
            ),
            [DependencyType::Site, DependencyType::Dynamic]
        );
        assert_eq!(
            dependency_types(&Resolution::Unresolved("x".into()), true, false),
            [DependencyType::Unresolved, DependencyType::TypeOnly]
        );
        let names: Vec<String> = DEPENDENCY_TYPES.iter().map(|s| (*s).to_owned()).collect();
        for name in names {
            assert!(
                DependencyType::ALL.iter().any(|t| t.as_str() == name),
                "{name}"
            );
        }
    }

    #[test]
    fn the_version_comes_from_the_configuration_then_requires_python() {
        let mut warnings = Vec::new();
        let chosen = |c: Option<&str>, r: Option<&str>, w: &mut Vec<Warning>| {
            choose_stdlib(c, r, w).ok().map(|s| s.version())
        };
        assert_eq!(
            chosen(Some("3.11"), Some("3.9"), &mut warnings),
            Some("3.11")
        );
        assert_eq!(chosen(None, Some("3.9"), &mut warnings), Some("3.9"));
        assert_eq!(chosen(None, None, &mut warnings), Some("3.13"));
        assert!(warnings.is_empty());
        assert_eq!(chosen(None, Some("3.6"), &mut warnings), Some("3.8"));
        assert_eq!(chosen(None, Some("3.99"), &mut warnings), Some("3.14"));
        assert_eq!(chosen(None, Some("not"), &mut warnings), Some("3.13"));
        assert_eq!(warnings.len(), 2);
        assert!(warnings[0].message.contains("languages.python.version"));
        let error = choose_stdlib(Some("2.7"), None, &mut warnings)
            .err()
            .map(|e| e.to_string());
        assert!(error.is_some_and(|e| e.contains("3.8 to 3.14")));
    }

    #[test]
    fn identities_fall_back_to_the_path() {
        let identity = fallback_identity("my-scripts/run.py", true);
        assert_eq!(identity.dotted, "my-scripts.run");
        assert_eq!(identity.package, "my-scripts");
        assert_eq!(fallback_identity("x.pyi", false).package, "");
    }

    #[test]
    fn base_chains_are_closed_nearest_first() {
        let at = rb_model::Location::in_file(Language::Python, None);
        let class = |name: &str, bases: &[&str]| {
            let mut t = rb_model::TypeElement::new(name, name, "class", at.clone());
            t.base_types = bases.iter().map(|b| (*b).to_owned()).collect();
            t
        };
        let mut layer = CodeLayer {
            types: vec![
                class("m.A", &["m.B", "x.Y"]),
                class("m.B", &["m.C"]),
                class("m.C", &["m.A"]),
                rb_model::TypeElement::new("m.f", "f", "function", at.clone()),
            ],
            ..CodeLayer::default()
        };
        close_base_chains(&mut layer);
        assert_eq!(layer.types[0].base_types, ["m.B", "x.Y", "m.C"]);
        assert_eq!(layer.types[2].base_types, ["m.A", "m.B", "x.Y"]);
    }
}
