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

use std::path::{Component, Path, PathBuf};

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
        }
    }
}

impl ResolveConfig {
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
                    references: TsconfigReferences::Disabled,
                })
            }),
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

/// Lexical normalisation, as Node's `path.normalize`.
fn normalise(path: &Path) -> PathBuf {
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

fn is_followable(resolved: &str, config: &ResolveConfig) -> bool {
    let ext = extension(resolved);
    config.extensions.iter().any(|e| e == ext) && !UNFOLLOWABLE.contains(&ext)
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
    resolver: &Resolver,
) -> Resolution {
    let mut resolution = Resolution {
        resolved: module.to_owned(),
        core_module: false,
        followable: false,
        could_not_resolve: false,
        dependency_types: Vec::new(),
        license: None,
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
            resolution.followable = is_followable(&resolution.resolved, config);
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
    }
}

fn resolve_module(
    module: &str,
    form_is_commonjs_resolvable: bool,
    context: &Context<'_>,
    config: &ResolveConfig,
    resolver: &Resolver,
) -> Resolution {
    if is_relative(module) || form_is_commonjs_resolvable {
        resolve_commonjs(module, context, config, resolver)
    } else {
        resolve_amd(module, context, config)
    }
}

fn typescript_variants(ext: &str) -> Option<&'static [&'static str]> {
    match ext {
        ".js" | ".jsx" => Some(&[".ts", ".tsx", ".d.ts"]),
        ".cjs" => Some(&[".cts", ".d.cts"]),
        ".mjs" => Some(&[".mts", ".d.mts"]),
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
    let resolver = config.resolver(None);
    let mut resolution = resolve_module(stripped, commonjs, context, config, &resolver);
    if resolution.could_not_resolve
        && let Some(variants) = typescript_variants(extension(stripped))
    {
        let without = stripped
            .strip_suffix(extension(stripped))
            .unwrap_or(stripped);
        let extensions: Vec<String> = variants.iter().map(|s| (*s).to_owned()).collect();
        let retry_resolver = config.resolver(Some(&extensions));
        let candidate = resolve_module(without, commonjs, context, config, &retry_resolver);
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
        dependency_types(&resolution, stripped, manifest.as_ref(), context, config);
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
pub fn manifest_for(context: &Context<'_>, config: &ResolveConfig) -> Option<Manifest> {
    if config.combined_dependencies {
        npm::combined(context.file_dir, context.base_dir)
    } else {
        npm::nearest(&absolute(context.cwd, context.file_dir))
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
) -> Option<serde_json::Value> {
    let manifest_config = ResolveConfig {
        exports_fields: Vec::new(),
        ..config.clone()
    };
    let resolver = manifest_config.resolver(Some(&[String::new()]));
    let directory = absolute(context.cwd, context.file_dir);
    let found = resolver
        .resolve(
            &directory,
            &format!("{}/package.json", npm::package_root(module)),
        )
        .ok()?;
    serde_json::from_str(&std::fs::read_to_string(found.path()).ok()?).ok()
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

fn regex_matches(pattern: &str, text: &str) -> bool {
    regex::Regex::new(pattern).is_ok_and(|re| re.is_match(text))
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
        .any(|key| regex_matches(&format!("^{}$", key.replace('*', ".+")), module))
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
                    .any(|k| regex_matches(&format!("^{}$", k.replace('*', ".+")), module))
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
    fn small_helpers() {
        assert!(is_relative("./a") && is_relative("..") && !is_relative(".a") && !is_relative("a"));
        assert_eq!(strip_loaders("css!style!./x.css"), "./x.css");
        assert_eq!(strip_query("a.js?raw"), "a.js");
        assert_eq!(strip_query("a.js?"), "a.js?");
        assert_eq!(strip_extension_and_index("src/x/index.ts"), "src/x");
        assert_eq!(posix_join("/base/", "../y/z"), "/y/z");
        assert_eq!(
            typescript_variants(".jsx"),
            Some(&[".ts", ".tsx", ".d.ts"][..])
        );
        assert_eq!(typescript_variants(".ts"), None);
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
