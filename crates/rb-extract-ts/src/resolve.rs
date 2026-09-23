//! Resolving a specifier to a file and classifying the result, as dependency-cruiser does over
//! enhanced-resolve, here over `oxc_resolver`, its Rust port.
//!
//! - Plan: [Wave 0, Step 8](../../../docs/plans/pending/0000-wave-0-spike.md#step-8-spike-a-rb-extract-ts-0c)
//!   (`resolve.rs`: "`oxc_resolver` configured from `TypeScriptOptions`; classifies the result into
//!   the alias family, `core`, `npm*`, `local`, `undetermined`, `unknown`")
//! - Decision: [ADR-0012](../../../docs/adr/0012-oxc-for-typescript.md)
//! - Source: [coverage § Options](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#options)
//!   (`enhancedResolveOptions`, `tsConfig`, `builtInModules`, `combinedDependencies`)
//! - Specification: dependency-cruiser 18.2.0 `src/extract/resolve/`
//!
//! The steps, in upstream's order: strip a loader prefix (`css!./x`); resolve CommonJS-style for
//! relative specifiers and for `cjs`, `es6` and `tsd` forms, AMD-style otherwise; retry an
//! unresolvable `.js`/`.mjs`/`.cjs` specifier as its TypeScript variant; then add the licence and
//! the dependency types.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use oxc_resolver::{
    AliasValue, ResolveOptions, Resolver, TsconfigDiscovery, TsconfigOptions, TsconfigReferences,
};
use rb_model::DependencyType;
use rb_model::options::BuiltInModules;

use crate::core::is_builtin;
use crate::npm::{self, Manifest};

use DependencyType as D;

/// dependency-cruiser's scannable extensions with every transpiler available, which is the
/// environment its conformance expectations were recorded in; also the default resolver
/// extensions.
pub const SCANNABLE_EXTENSIONS: &[&str] = &[
    ".js",
    ".cjs",
    ".mjs",
    ".jsx",
    ".ts",
    ".tsx",
    ".d.ts",
    ".cts",
    ".d.cts",
    ".mts",
    ".d.mts",
    ".vue",
    ".svelte",
    ".coffee",
    ".litcoffee",
    ".coffee.md",
    ".csx",
    ".cjsx",
];

const UNFOLLOWABLE: &[&str] = &[
    ".json", ".node", ".css", ".sass", ".scss", ".stylus", ".less",
];

/// Everything that decides a resolution: the enhanced-resolve options dependency-cruiser passes,
/// plus the parts of its configuration the classification reads.
#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag is an independent enhanced-resolve or dependency-cruiser option"
)]
#[derive(Debug, Clone)]
pub struct ResolveConfig {
    /// Extensions to try.
    pub extensions: Vec<String>,
    /// Folders searched for bare specifiers; relative names or absolute paths.
    pub modules: Vec<String>,
    /// `package.json` fields naming the entry point.
    pub main_fields: Vec<String>,
    /// File names tried for a directory.
    pub main_files: Vec<String>,
    /// `package.json` fields holding export maps; empty unless configured.
    pub exports_fields: Vec<String>,
    /// Conditions matched in an export map.
    pub condition_names: Vec<String>,
    /// `package.json` fields holding browser-style alias maps.
    pub alias_fields: Vec<String>,
    /// webpack-style aliases, prefix to target.
    pub alias: Vec<(String, String)>,
    /// Whether symlinks resolve to their target.
    pub symlinks: bool,
    /// The tsconfig whose `paths` apply.
    pub tsconfig: Option<PathBuf>,
    /// The tsconfig's `baseUrl`, as an absolute path, for classification.
    pub tsconfig_base_url: Option<String>,
    /// The tsconfig's `paths` keys, for classification.
    pub tsconfig_paths: Vec<String>,
    /// `builtInModules`.
    pub built_in_modules: Option<BuiltInModules>,
    /// Whether every manifest up to the base directory classifies npm dependencies.
    pub combined_dependencies: bool,
    /// Whether to read npm licences.
    pub resolve_licenses: bool,
    /// Whether to mark deprecated npm packages.
    pub resolve_deprecations: bool,
    /// `externalModuleResolutionStrategy: yarn-pnp`: bare specifiers resolve through the Yarn
    /// Plug'n'Play manifest (`.pnp.cjs`) found at or above `pnp_root`.
    pub yarn_pnp: bool,
    /// Where the search for `.pnp.cjs` starts; the working directory when `None`.
    pub pnp_root: Option<PathBuf>,
    /// Whether the tsconfig's project `references` apply to files inside the referenced
    /// projects.
    pub tsconfig_references: bool,
    /// Upstream's `bustTheCache`: every resolution builds its resolver from the options it is
    /// given. Off in a real cruise, where upstream keeps the first resolver it built for the
    /// whole run, so the TypeScript-variant retry (see [`resolve`]) searches the configured
    /// extensions rather than the variant set; on in the recorded `test/extract` cases, which
    /// all pass `bustTheCache: true`.
    pub bust_the_cache: bool,
    /// The resolvers built from these options, each made on first use and kept, so its file
    /// system cache lives for the whole run. Set every option before the first resolution.
    pub(crate) resolvers: Resolvers,
}

/// Classifying manifests by (importing folder, base directory).
type ManifestCache = Mutex<HashMap<(PathBuf, PathBuf), Option<Arc<Manifest>>>>;

/// Packages' own `package.json` files (licence, deprecation), by path, each read once.
type PackageCache = Mutex<HashMap<PathBuf, Option<Arc<serde_json::Value>>>>;

/// The resolvers one `ResolveConfig` uses: the main one, one per TypeScript retry extension set
/// (see [`typescript_variants`]) and the one that finds a package's `package.json`; and the
/// manifests and patterns read while classifying, which do not change during a run.
#[derive(Default)]
pub(crate) struct Resolvers {
    main: OnceLock<Resolver>,
    retry: [OnceLock<Resolver>; 3],
    manifest: OnceLock<Resolver>,
    manifests: ManifestCache,
    patterns: Mutex<HashMap<String, Option<regex::Regex>>>,
    followable: OnceLock<ExtensionList>,
    packages: PackageCache,
}

/// A copy starts with no resolvers: the copy's options may be changed before it resolves.
impl Clone for Resolvers {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl std::fmt::Debug for Resolvers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Resolvers")
    }
}

impl Default for ResolveConfig {
    fn default() -> Self {
        Self {
            extensions: SCANNABLE_EXTENSIONS
                .iter()
                .map(|s| (*s).to_owned())
                .collect(),
            modules: vec!["node_modules".to_owned(), "node_modules/@types".to_owned()],
            main_fields: vec!["main".to_owned()],
            main_files: vec!["index".to_owned()],
            exports_fields: Vec::new(),
            condition_names: Vec::new(),
            alias_fields: Vec::new(),
            alias: Vec::new(),
            symlinks: true,
            tsconfig: None,
            tsconfig_base_url: None,
            tsconfig_paths: Vec::new(),
            built_in_modules: None,
            combined_dependencies: false,
            resolve_licenses: false,
            resolve_deprecations: false,
            yarn_pnp: false,
            pnp_root: None,
            tsconfig_references: false,
            bust_the_cache: false,
            resolvers: Resolvers::default(),
        }
    }
}

impl ResolveConfig {
    /// The main resolver, built once.
    fn main_resolver(&self) -> &Resolver {
        self.resolvers.main.get_or_init(|| self.resolver(None))
    }

    /// The resolver for TypeScript retry set `index` of [`typescript_variants`], built once.
    fn retry_resolver(&self, index: usize, variants: &[&str]) -> Option<&Resolver> {
        let slot = self.resolvers.retry.get(index)?;
        Some(slot.get_or_init(|| {
            let extensions: Vec<String> = variants.iter().map(|s| (*s).to_owned()).collect();
            self.resolver(Some(&extensions))
        }))
    }

    /// The manifest that classifies dependencies from `folder`, read once per folder.
    fn manifest(&self, folder: &Path, base_dir: &Path) -> Option<Arc<Manifest>> {
        let key = (folder.to_path_buf(), base_dir.to_path_buf());
        if let Ok(cache) = self.resolvers.manifests.lock()
            && let Some(found) = cache.get(&key)
        {
            return found.clone();
        }
        let found = if self.combined_dependencies {
            npm::combined(folder, base_dir).map(Arc::new)
        } else {
            npm::nearest(folder).map(Arc::new)
        };
        if let Ok(mut cache) = self.resolvers.manifests.lock() {
            cache.insert(key, found.clone());
        }
        found
    }

    /// Whether `text` matches `pattern`, the pattern compiled once per run. An invalid pattern
    /// matches nothing.
    fn matches(&self, pattern: &str, text: &str) -> bool {
        if let Ok(cache) = self.resolvers.patterns.lock()
            && let Some(compiled) = cache.get(pattern)
        {
            return compiled.as_ref().is_some_and(|re| re.is_match(text));
        }
        let compiled = regex::Regex::new(pattern).ok();
        let matched = compiled.as_ref().is_some_and(|re| re.is_match(text));
        if let Ok(mut cache) = self.resolvers.patterns.lock() {
            cache.insert(pattern.to_owned(), compiled);
        }
        matched
    }

    /// The resolver that finds `<package>/package.json`: no export maps, no extensions.
    fn manifest_resolver(&self) -> &Resolver {
        self.resolvers.manifest.get_or_init(|| {
            ResolveConfig {
                exports_fields: Vec::new(),
                ..self.clone()
            }
            .resolver(Some(&[String::new()]))
        })
    }

    /// The `oxc_resolver` for these options, with `extensions` replaced when given.
    pub fn resolver(&self, extensions: Option<&[String]>) -> Resolver {
        Resolver::new(ResolveOptions {
            extensions: extensions.map_or_else(|| self.extensions.clone(), <[String]>::to_vec),
            modules: self.modules.clone(),
            main_fields: self.main_fields.clone(),
            main_files: self.main_files.clone(),
            exports_fields: self
                .exports_fields
                .iter()
                .map(|f| vec![f.clone()])
                .collect(),
            condition_names: self.condition_names.clone(),
            alias_fields: self.alias_fields.iter().map(|f| vec![f.clone()]).collect(),
            alias: self
                .alias
                .iter()
                .map(|(k, v)| (k.clone(), vec![AliasValue::Path(v.clone())]))
                .collect(),
            symlinks: self.symlinks,
            tsconfig: self.tsconfig.as_ref().map(|config_file| {
                TsconfigDiscovery::Manual(TsconfigOptions {
                    config_file: config_file.clone(),
                    references: if self.tsconfig_references {
                        TsconfigReferences::Auto
                    } else {
                        TsconfigReferences::Disabled
                    },
                })
            }),
            yarn_pnp: self.yarn_pnp,
            cwd: self.pnp_root.clone(),
            node_path: false,
            ..ResolveOptions::default()
        })
    }
}

/// What a resolution produced; the fields dependency-cruiser adds to a dependency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    /// Base-relative posix path, or the specifier when unresolved or built in.
    pub resolved: String,
    /// A runtime built-in.
    pub core_module: bool,
    /// Worth extracting in turn.
    pub followable: bool,
    /// Not found.
    pub could_not_resolve: bool,
    /// The classification followed by the form's own types.
    pub dependency_types: Vec<DependencyType>,
    /// The npm licence, when asked for and found.
    pub license: Option<String>,
    /// For a CommonJS-style resolution that found a file: the extension list it asked with,
    /// which the pipeline reads to settle upstream's followable cache
    /// ([`ResolveConfig::settle_followable`]).
    pub asked_with: Option<ExtensionList>,
}

/// Whether a specifier is relative: `./x`, `../x`, `.` or `..`.
pub fn is_relative(module: &str) -> bool {
    module.starts_with("./") || module.starts_with("../") || module == "." || module == ".."
}

/// The extension dependency-cruiser recognises, double extensions (`.d.ts`, `.coffee.md`) included.
pub fn extension(file: &str) -> &str {
    for double in [".d.ts", ".d.mts", ".d.cts", ".coffee.md"] {
        if file.ends_with(double) {
            return &file[file.len() - double.len()..];
        }
    }
    let name = file.rsplit('/').next().unwrap_or(file);
    match name.rfind('.') {
        Some(0) | None => "",
        Some(dot) => &name[dot..],
    }
}

/// `a!b!c` loads `c` through loaders `a` and `b`.
pub fn strip_loaders(module: &str) -> &str {
    module.rsplit('!').next().unwrap_or(module)
}

/// A Windows path in the one spelling this crate compares: without the `\\?\` verbatim prefix
/// that `canonicalize` adds, and with an upper-case drive letter. Other paths pass through.
pub fn simplified(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(share) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{share}"));
    }
    let rest = text.strip_prefix(r"\\?\").unwrap_or(&text);
    let mut chars = rest.chars();
    match (chars.next(), chars.next()) {
        (Some(drive), Some(':')) if drive.is_ascii_alphabetic() => {
            PathBuf::from(format!("{}{}", drive.to_ascii_uppercase(), &rest[1..]))
        }
        _ if rest.len() != text.len() => PathBuf::from(rest),
        _ => path.to_path_buf(),
    }
}

/// Lexical normalisation, as Node's `path.normalize`.
fn normalise(path: &Path) -> PathBuf {
    let path = simplified(path);
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Node's `path.relative`, posix-separated.
pub fn relative(from: &Path, to: &Path) -> String {
    let from = normalise(from);
    let to = normalise(to);
    let from_parts: Vec<_> = from.components().collect();
    let to_parts: Vec<_> = to.components().collect();
    let common = from_parts
        .iter()
        .zip(&to_parts)
        .take_while(|(a, b)| a == b)
        .count();
    let mut parts: Vec<String> =
        std::iter::repeat_n("..".to_owned(), from_parts.len() - common).collect();
    parts.extend(
        to_parts[common..]
            .iter()
            .map(|c| c.as_os_str().to_string_lossy().into_owned()),
    );
    parts.join("/")
}

fn absolute(cwd: &Path, path: &Path) -> PathBuf {
    normalise(&if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    })
}

/// Where a resolution runs: the working directory, the base directory and the importing file's
/// directory, as upstream's `resolve(dependency, baseDir, fileDir, ...)` receives them.
#[derive(Debug, Clone)]
pub struct Context<'p> {
    /// The process working directory relative paths are taken against.
    pub cwd: &'p Path,
    /// The base directory output paths are relative to.
    pub base_dir: &'p Path,
    /// The importing file's directory.
    pub file_dir: &'p Path,
}

/// The extension list a resolution asked with: the configured one, or the TypeScript variants
/// of [`typescript_variants`] set `n` for the retry of an unresolvable `.js`, `.cjs` or `.mjs`.
/// Upstream's `isFollowable` reads the list from the options of the call it is made in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtensionList {
    /// `extensions` as configured.
    Configured,
    /// The TypeScript variants, by set.
    TypeScriptVariants(usize),
}

impl ResolveConfig {
    /// Fixes the list `followable` is decided with for the rest of the run: upstream caches the
    /// followable extensions of the first successful resolution in a module-level variable
    /// that nothing clears, so every later resolution is judged by that one's list. The
    /// pipeline settles it from the first resolution in upstream's walking order; ignored
    /// under [`ResolveConfig::bust_the_cache`], and once settled.
    pub fn settle_followable(&self, list: ExtensionList) {
        let _ = self.resolvers.followable.set(list);
    }

    /// The settled list, if any.
    pub fn settled_followable(&self) -> Option<ExtensionList> {
        self.resolvers.followable.get().copied()
    }
}

fn is_followable(resolved: &str, list: ExtensionList, config: &ResolveConfig) -> bool {
    let ext = extension(resolved);
    let listed = match list {
        ExtensionList::Configured => config.extensions.iter().any(|e| e == ext),
        ExtensionList::TypeScriptVariants(index) => [".js", ".cjs", ".mjs"]
            .iter()
            .filter_map(|js| typescript_variants(js))
            .find(|(set, _)| *set == index)
            .is_some_and(|(_, variants)| variants.contains(&ext)),
    };
    listed && !UNFOLLOWABLE.contains(&ext)
}

fn strip_query(path: &str) -> &str {
    match path.find('?') {
        Some(at) if at + 1 < path.len() => &path[..at],
        _ => path,
    }
}

fn resolve_commonjs(
    module: &str,
    context: &Context<'_>,
    config: &ResolveConfig,
    (resolver, list): (&Resolver, ExtensionList),
) -> Resolution {
    let mut resolution = Resolution {
        resolved: module.to_owned(),
        core_module: false,
        followable: false,
        could_not_resolve: false,
        dependency_types: Vec::new(),
        license: None,
        asked_with: None,
    };
    if is_builtin(module, config.built_in_modules.as_ref()) {
        resolution.core_module = true;
        return resolution;
    }
    let directory = absolute(context.cwd, context.file_dir);
    match resolver.resolve(&directory, module) {
        Ok(found) => {
            let full = found.full_path().to_string_lossy().into_owned();
            let full = strip_query(&full);
            resolution.resolved =
                relative(&absolute(context.cwd, context.base_dir), Path::new(full));
            let judged_by = if config.bust_the_cache {
                list
            } else {
                config.settled_followable().unwrap_or(list)
            };
            resolution.followable = is_followable(&resolution.resolved, judged_by, config);
            resolution.asked_with = Some(list);
        }
        Err(_) => resolution.could_not_resolve = true,
    }
    resolution
}

fn resolve_amd(module: &str, context: &Context<'_>, config: &ResolveConfig) -> Resolution {
    // Upstream checks the guessed, base-relative path against the working directory.
    let exists = |path: &str| context.cwd.join(path).is_file();
    let guess = |suffix: &str| {
        relative(
            &absolute(context.cwd, context.base_dir),
            &absolute(
                context.cwd,
                &context.file_dir.join(format!("{module}{suffix}")),
            ),
        )
    };
    let resolved = [".js", ""]
        .iter()
        .map(|s| guess(s))
        .find(|p| exists(p))
        .unwrap_or_else(|| module.to_owned());
    let core_module = is_builtin(module, config.built_in_modules.as_ref());
    Resolution {
        // Case-sensitive, as upstream's `endsWith(".js")`.
        followable: exists(&resolved)
            && Path::new(&resolved).extension().is_some_and(|e| e == "js"),
        could_not_resolve: !core_module && !exists(&resolved),
        core_module,
        resolved,
        dependency_types: Vec::new(),
        license: None,
        asked_with: None,
    }
}

fn resolve_module(
    module: &str,
    form_is_commonjs_resolvable: bool,
    context: &Context<'_>,
    config: &ResolveConfig,
    resolver: (&Resolver, ExtensionList),
) -> Resolution {
    if is_relative(module) || form_is_commonjs_resolvable {
        resolve_commonjs(module, context, config, resolver)
    } else {
        resolve_amd(module, context, config)
    }
}

/// The TypeScript extensions retried for a JavaScript one, with the set's index.
fn typescript_variants(ext: &str) -> Option<(usize, &'static [&'static str])> {
    match ext {
        ".js" | ".jsx" => Some((0, &[".ts", ".tsx", ".d.ts"])),
        ".cjs" => Some((1, &[".cts", ".d.cts"])),
        ".mjs" => Some((2, &[".mts", ".d.mts"])),
        _ => None,
    }
}

/// Resolves one dependency form: the resolution, the licence and the dependency types.
///
/// `form_types` are the form's own types (`import`, `require`), appended after the classification.
pub fn resolve(
    module: &str,
    module_system: rb_model::ModuleSystem,
    form_types: &[DependencyType],
    context: &Context<'_>,
    config: &ResolveConfig,
) -> Resolution {
    use rb_model::ModuleSystem as M;
    let stripped = strip_loaders(module);
    let commonjs = matches!(module_system, M::Cjs | M::Es6 | M::Tsd);
    let mut resolution = resolve_module(
        stripped,
        commonjs,
        context,
        config,
        (config.main_resolver(), ExtensionList::Configured),
    );
    if resolution.could_not_resolve
        && let Some((index, variants)) = typescript_variants(extension(stripped))
        && let Some(retry_resolver) = if config.bust_the_cache {
            config.retry_resolver(index, variants)
        } else {
            // Upstream asks for the variant extensions, but its resolver cache is keyed on the
            // run, not on the options, so the retry gets the first resolver the run built.
            Some(config.main_resolver())
        }
    {
        let without = stripped
            .strip_suffix(extension(stripped))
            .unwrap_or(stripped);
        let candidate = resolve_module(
            without,
            commonjs,
            context,
            config,
            (retry_resolver, ExtensionList::TypeScriptVariants(index)),
        );
        // Node's `extname`, so `x.d.cts` counts as `.cts`.
        let last = candidate
            .resolved
            .rsplit('/')
            .next()
            .and_then(|n| n.rfind('.').map(|i| &n[i..]))
            .unwrap_or("");
        if matches!(last, ".ts" | ".tsx" | ".cts" | ".mts") {
            resolution = candidate;
        }
    }

    if config.resolve_licenses && is_external(&resolution.resolved, &config.modules, context) {
        resolution.license = license(stripped, context, config).filter(|l| !l.is_empty());
    }
    let manifest = manifest_for(context, config);
    resolution.dependency_types =
        dependency_types(&resolution, stripped, manifest.as_deref(), context, config);
    // Upstream returns `["unknown"]` before appending the form's own types.
    if !resolution.could_not_resolve {
        resolution.dependency_types.extend_from_slice(form_types);
    }
    if !resolution.core_module && !resolution.could_not_resolve {
        resolution.resolved = resolution.resolved.replace("\u{0}#", "#");
    }
    resolution
}

/// The manifest that classifies a dependency from `file_dir`.
pub fn manifest_for(context: &Context<'_>, config: &ResolveConfig) -> Option<Arc<Manifest>> {
    if config.combined_dependencies {
        config.manifest(context.file_dir, context.base_dir)
    } else {
        config.manifest(&absolute(context.cwd, context.file_dir), Path::new(""))
    }
}

/// Whether a resolved path lies in one of the module folders.
pub fn is_external(resolved: &str, modules: &[String], context: &Context<'_>) -> bool {
    !resolved.is_empty()
        && modules.iter().any(|folder| {
            if Path::new(folder).is_absolute() {
                absolute(context.cwd, &context.base_dir.join(resolved)).starts_with(folder)
            } else {
                resolved.contains(folder.as_str())
            }
        })
}

/// The `package.json` of the package a specifier names, found the way Node would.
fn package_json(
    module: &str,
    context: &Context<'_>,
    config: &ResolveConfig,
) -> Option<Arc<serde_json::Value>> {
    let directory = absolute(context.cwd, context.file_dir);
    let found = config
        .manifest_resolver()
        .resolve(
            &directory,
            &format!("{}/package.json", npm::package_root(module)),
        )
        .ok()?;
    let path = found.path().to_path_buf();
    if let Ok(cache) = config.resolvers.packages.lock()
        && let Some(read) = cache.get(&path)
    {
        return read.clone();
    }
    let read = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .map(Arc::new);
    if let Ok(mut cache) = config.resolvers.packages.lock() {
        cache.insert(path, read.clone());
    }
    read
}

fn license(module: &str, context: &Context<'_>, config: &ResolveConfig) -> Option<String> {
    package_json(module, context, config)?
        .get("license")?
        .as_str()
        .map(str::to_owned)
}

fn deprecated(module: &str, context: &Context<'_>, config: &ResolveConfig) -> bool {
    package_json(module, context, config)
        .and_then(|p| p.get("deprecated").cloned())
        .is_some_and(|d| !(d.is_null() || d.as_bool() == Some(false) || d.as_str() == Some("")))
}

fn strip_extension_and_index(path: &str) -> &str {
    let ext = extension(path);
    let path = &path[..path.len() - ext.len()];
    path.strip_suffix("/index").unwrap_or(path)
}

fn posix_join(a: &str, b: &str) -> String {
    let joined = format!("{}/{b}", a.trim_end_matches('/'));
    normalise(Path::new(&joined))
        .to_string_lossy()
        .replace('\\', "/")
}

fn workspace_aliased(module: &str, resolved: &str, manifest: Option<&Manifest>) -> bool {
    let Some(workspaces) = manifest.and_then(|m| m.get("workspaces")) else {
        return false;
    };
    let list = workspaces
        .as_array()
        .or_else(|| workspaces.get("packages").and_then(|p| p.as_array()));
    let globs: Vec<String> = list
        .into_iter()
        .flatten()
        .filter_map(|w| w.as_str())
        .map(|w| {
            if w.ends_with('/') {
                format!("{w}**")
            } else {
                format!("{w}/**")
            }
        })
        .collect();
    let matches = |pattern: &str, text: &str| {
        globset::GlobBuilder::new(pattern)
            .literal_separator(true)
            .build()
            .is_ok_and(|g| g.compile_matcher().is_match(text))
    };
    globs.iter().any(|g| matches(g, resolved))
        || globs
            .iter()
            .any(|g| matches(g, module) || matches(&format!("node_modules/{g}"), module))
}

/// The alias family a specifier resolved through, if any.
pub fn alias_types(
    module: &str,
    resolved: &str,
    config: &ResolveConfig,
    manifest: Option<&Manifest>,
) -> Vec<DependencyType> {
    if is_relative(module) {
        return Vec::new();
    }
    if config
        .alias
        .iter()
        .any(|(prefix, _)| module.starts_with(prefix.as_str()))
    {
        return vec![D::Aliased, D::AliasedWebpack];
    }
    if let Some(base_url) = &config.tsconfig_base_url
        && module != resolved
        && strip_extension_and_index(&posix_join(base_url, module))
            .ends_with(strip_extension_and_index(resolved))
    {
        return vec![D::Aliased, D::AliasedTsconfig, D::AliasedTsconfigBaseUrl];
    }
    if config
        .tsconfig_paths
        .iter()
        .any(|key| config.matches(&format!("^{}$", key.replace('*', ".+")), module))
    {
        return vec![D::Aliased, D::AliasedTsconfig, D::AliasedTsconfigPaths];
    }
    let subpath = module.starts_with('#')
        && manifest
            .and_then(|m| m.get("imports"))
            .and_then(|i| i.as_object())
            .is_some_and(|imports| {
                imports
                    .keys()
                    .any(|k| config.matches(&format!("^{}$", k.replace('*', ".+")), module))
            });
    if subpath {
        return vec![D::Aliased, D::AliasedSubpathImport];
    }
    if workspace_aliased(module, resolved, manifest) {
        return vec![D::Aliased, D::AliasedWorkspace];
    }
    Vec::new()
}

/// dependency-cruiser's `determineDependencyTypes`, without the form's own types.
pub fn dependency_types(
    resolution: &Resolution,
    module: &str,
    manifest: Option<&Manifest>,
    context: &Context<'_>,
    config: &ResolveConfig,
) -> Vec<DependencyType> {
    if resolution.could_not_resolve {
        return vec![D::Unknown];
    }
    let mut types = alias_types(module, &resolution.resolved, config, manifest);
    let aliased = !types.is_empty();
    let external = is_external(&resolution.resolved, &config.modules, context);
    if resolution.core_module {
        types.push(D::Core);
    } else if is_relative(module) || (aliased && !external) {
        types.push(D::Local);
    } else if external {
        if is_external(&resolution.resolved, &["node_modules".to_owned()], context) {
            let mut npm_types = npm::manifest_dependency_types(
                npm::package_root(module),
                manifest,
                &config.modules,
            );
            if config.resolve_deprecations && deprecated(module, context, config) {
                npm_types.push(D::Deprecated);
            }
            if npm::is_bundled(module, manifest) {
                npm_types.push(D::NpmBundled);
            }
            types.extend(npm_types);
        } else {
            types.push(D::Localmodule);
        }
    } else {
        types.push(D::Undetermined);
    }
    types
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_include_the_double_ones() {
        assert_eq!(extension("a/b.d.ts"), ".d.ts");
        assert_eq!(extension("a/b.d.mts"), ".d.mts");
        assert_eq!(extension("x.coffee.md"), ".coffee.md");
        assert_eq!(extension("a/b.ts"), ".ts");
        assert_eq!(extension("a/.eslintrc"), "");
        assert_eq!(extension("a/b"), "");
    }

    #[test]
    fn relative_paths_match_nodes() {
        assert_eq!(
            relative(Path::new("/a/b"), Path::new("/a/b/c/d.js")),
            "c/d.js"
        );
        assert_eq!(relative(Path::new("/a/b"), Path::new("/a/x.js")), "../x.js");
        assert_eq!(relative(Path::new("/a/b"), Path::new("/a/b")), "");
        assert_eq!(relative(Path::new("/a/./b/../b"), Path::new("/a/b/y")), "y");
    }

    #[test]
    fn windows_spellings_of_one_path_agree() {
        assert_eq!(
            simplified(Path::new(r"\\?\c:\repo\x.js")),
            PathBuf::from(r"C:\repo\x.js")
        );
        assert_eq!(simplified(Path::new(r"d:\x")), PathBuf::from(r"D:\x"));
        assert_eq!(
            simplified(Path::new(r"\\?\UNC\server\share")),
            PathBuf::from(r"\\server\share")
        );
        assert_eq!(
            simplified(Path::new("/unix/path")),
            PathBuf::from("/unix/path")
        );
    }

    #[test]
    fn small_helpers() {
        assert!(is_relative("./a") && is_relative("..") && !is_relative(".a") && !is_relative("a"));
        assert_eq!(strip_loaders("css!style!./x.css"), "./x.css");
        assert_eq!(strip_query("a.js?raw"), "a.js");
        assert_eq!(strip_query("a.js?"), "a.js?");
        assert_eq!(strip_extension_and_index("src/x/index.ts"), "src/x");
        assert_eq!(posix_join("/base/", "../y/z"), "/y/z");
        assert_eq!(
            typescript_variants(".jsx"),
            Some((0, &[".ts", ".tsx", ".d.ts"][..]))
        );
        assert_eq!(
            typescript_variants(".cjs"),
            Some((1, &[".cts", ".d.cts"][..]))
        );
        assert_eq!(
            typescript_variants(".mjs"),
            Some((2, &[".mts", ".d.mts"][..]))
        );
        assert_eq!(typescript_variants(".ts"), None);
        // One retry resolver per set, and none past the last.
        let config = ResolveConfig::default();
        assert!(config.retry_resolver(2, &[".mts"]).is_some());
        assert!(config.retry_resolver(3, &[".mts"]).is_none());
        assert!(config.clone().resolvers.main.get().is_none());
    }

    #[test]
    fn the_followable_list_settles_once_and_decides_followable() {
        let config = ResolveConfig::default();
        assert_eq!(config.settled_followable(), None);
        config.settle_followable(ExtensionList::TypeScriptVariants(0));
        config.settle_followable(ExtensionList::Configured);
        assert_eq!(
            config.settled_followable(),
            Some(ExtensionList::TypeScriptVariants(0))
        );
        assert!(config.clone().settled_followable().is_none());
        let variants = ExtensionList::TypeScriptVariants(0);
        assert!(is_followable("src/a.ts", variants, &config));
        assert!(is_followable("src/a.d.ts", variants, &config));
        assert!(!is_followable("src/a.cts", variants, &config));
        assert!(!is_followable("src/a.js", variants, &config));
        assert!(is_followable(
            "src/a.cts",
            ExtensionList::Configured,
            &config
        ));
        assert!(is_followable(
            "src/a.d.mts",
            ExtensionList::TypeScriptVariants(2),
            &config
        ));
        assert!(!is_followable(
            "src/a.json",
            ExtensionList::Configured,
            &ResolveConfig {
                extensions: vec![".json".to_owned()],
                ..ResolveConfig::default()
            }
        ));
    }

    #[test]
    fn aliases_are_classified_in_upstreams_order() {
        let mut config = ResolveConfig::default();
        config.alias.push(("@web".to_owned(), "/x".to_owned()));
        assert_eq!(
            alias_types("@web/a", "x/a.ts", &config, None),
            [D::Aliased, D::AliasedWebpack]
        );
        assert!(alias_types("./a", "a.ts", &config, None).is_empty());
        let config = ResolveConfig {
            tsconfig_paths: vec!["@shared/*".to_owned()],
            ..ResolveConfig::default()
        };
        assert_eq!(
            alias_types("@shared/x", "src/shared/x.ts", &config, None),
            [D::Aliased, D::AliasedTsconfig, D::AliasedTsconfigPaths]
        );
        let manifest: Manifest = serde_json::from_str(
            r##"{"imports": {"#utl/*": "./src/utl/*"}, "workspaces": ["packages/*"]}"##,
        )
        .unwrap_or_default();
        assert_eq!(
            alias_types(
                "#utl/x.mjs",
                "src/utl/x.mjs",
                &ResolveConfig::default(),
                Some(&manifest)
            ),
            [D::Aliased, D::AliasedSubpathImport]
        );
        assert_eq!(
            alias_types(
                "pkg-a",
                "packages/a/index.js",
                &ResolveConfig::default(),
                Some(&manifest)
            ),
            [D::Aliased, D::AliasedWorkspace]
        );
        assert!(
            alias_types(
                "lodash",
                "node_modules/lodash/index.js",
                &ResolveConfig::default(),
                Some(&manifest)
            )
            .is_empty()
        );
    }
}
