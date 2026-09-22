//! The options each extractor receives, one struct per language.
//!
//! - Decision: [ADR-0010](../../../docs/adr/0010-crate-layout-and-extractor-boundary.md) rule 2
//!   (an extractor may not depend on `rb-config`, so its options live here)
//! - Source: [design § The native format](../../../docs/artifacts/design.md#the-native-format)
//!   (the `languages` block), [coverage § Options](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#options)
//! - Plan: [Wave 0, Step 3](../../../docs/plans/pending/0000-wave-0-spike.md#step-3-rb-model-graph-document-schema-violation-id-0a)
//! - Requirement: [FR-CORE-03](../../../docs/prd.md#fr-core-03)
//!
//! Every field is an `Option` so that "not set" and "set to the default" stay distinguishable
//! when `config convert` writes a file back. The documented default is applied by the accessor
//! of the same name, never by filling the field. Key names are dependency-cruiser's, so a flat
//! dependency-cruiser option deserialises into [`TypeScriptOptions`] unchanged.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::vocab::{DependencyType, ExternalModuleResolutionStrategy, ModuleSystem, Parser};

/// One regular expression or several, dependency-cruiser's `REAsStringsType`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Patterns {
    /// A single expression.
    One(String),
    /// Several expressions, matched as an alternation.
    Many(Vec<String>),
}

impl Patterns {
    /// The expressions as a list.
    pub fn as_slice(&self) -> &[String] {
        match self {
            Self::One(one) => std::slice::from_ref(one),
            Self::Many(many) => many,
        }
    }

    /// One expression that matches when any member does, which is how dependency-cruiser joins
    /// an array.
    pub fn joined(&self) -> String {
        match self {
            Self::One(one) => one.clone(),
            Self::Many(many) => many.join("|"),
        }
    }
}

/// `doNotFollow`, `exclude` and `includeOnly`: either patterns or the compound form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum PathFilter {
    /// The short form, patterns only.
    Patterns(Patterns),
    /// The compound form.
    Compound(CompoundFilter),
}

impl PathFilter {
    /// The path patterns, whichever form was written.
    pub fn path(&self) -> Option<&Patterns> {
        match self {
            Self::Patterns(patterns) => Some(patterns),
            Self::Compound(compound) => compound.path.as_ref(),
        }
    }

    /// The dependency types (compound `doNotFollow` only).
    pub fn dependency_types(&self) -> &[DependencyType] {
        match self {
            Self::Patterns(_) => &[],
            Self::Compound(compound) => compound.dependency_types.as_deref().unwrap_or_default(),
        }
    }

    /// Whether dynamic imports are filtered (compound `exclude` only).
    pub fn dynamic(&self) -> Option<bool> {
        match self {
            Self::Patterns(_) => None,
            Self::Compound(compound) => compound.dynamic,
        }
    }
}

/// The compound form of a path filter.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompoundFilter {
    /// Paths to match.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<Patterns>,
    /// Dependency types to match (`doNotFollow`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dependency_types: Option<Vec<DependencyType>>,
    /// Whether to match dynamic imports (`exclude`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dynamic: Option<bool>,
}

/// A configuration file the extractor reads: `tsConfig`, `babelConfig`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FileReference {
    /// Path, relative to the working directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
}

/// `webpackConfig`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WebpackConfig {
    /// Path, relative to the working directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
    /// The `env` passed to a function-shaped config.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<serde_json::Value>,
    /// The `arguments` passed to a function-shaped config.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<serde_json::Value>,
}

/// `enhancedResolveOptions`: the resolver settings `oxc_resolver` receives.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EnhancedResolveOptions {
    /// `package.json` fields holding export maps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exports_fields: Option<Vec<String>>,
    /// Conditions to match in an export map.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition_names: Option<Vec<String>>,
    /// Extensions to try.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extensions: Option<Vec<String>>,
    /// `package.json` fields naming the entry point.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main_fields: Option<Vec<String>>,
    /// File names tried for a directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main_files: Option<Vec<String>>,
    /// `package.json` fields holding browser-style alias maps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias_fields: Option<Vec<String>>,
    /// The file-system cache settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_input_file_system: Option<CachedInputFileSystem>,
}

/// `enhancedResolveOptions.cachedInputFileSystem`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CachedInputFileSystem {
    /// Milliseconds a cached entry stays valid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_duration: Option<u32>,
}

/// `builtInModules`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BuiltInModules {
    /// Replaces the runtime's list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#override: Option<Vec<String>>,
    /// Adds to the runtime's list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub add: Option<Vec<String>>,
}

/// `tsPreCompilationDeps`: `true`, `false` or `"specify"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum TsPreCompilationDeps {
    /// Include (`true`) or drop (`false`) edges that vanish after compilation.
    Enabled(bool),
    /// Include them and mark each with `preCompilationOnly`.
    Specify(SpecifyLiteral),
}

/// The literal string `"specify"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum SpecifyLiteral {
    /// `"specify"`.
    #[serde(rename = "specify")]
    Specify,
}

/// `languages.typescript`, and dependency-cruiser's flat options that alias into it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TypeScriptOptions {
    /// Paths in the output are relative to this directory. Default: the working directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_dir: Option<String>,
    /// The tsconfig to resolve `paths` and `baseUrl` from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ts_config: Option<FileReference>,
    /// Whether edges that disappear after compilation are kept. Default: `false`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ts_pre_compilation_deps: Option<TsPreCompilationDeps>,
    /// The Babel config whose aliases apply.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub babel_config: Option<FileReference>,
    /// The webpack config whose `resolve` applies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webpack_config: Option<WebpackConfig>,
    /// Resolver settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enhanced_resolve_options: Option<EnhancedResolveOptions>,
    /// Which dependency forms to extract. Default: `es6`, `cjs`, `tsd`, `amd`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module_systems: Option<Vec<ModuleSystem>>,
    /// Which parser dependency-cruiser would have used. Default: `acorn`, or `tsc` for
    /// TypeScript sources.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parser: Option<Parser>,
    /// Names other than `require` that load a module.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exotic_require_strings: Option<Vec<String>>,
    /// Whether JSDoc imports are dependencies. Default: `false`.
    #[serde(
        rename = "detectJSDocImports",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub detect_js_doc_imports: Option<bool>,
    /// Whether `process.getBuiltinModule` calls are dependencies. Default: `false`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detect_process_builtin_module_calls: Option<bool>,
    /// Whether symlinks keep their own path. Default: `false`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preserve_symlinks: Option<bool>,
    /// Whether every `package.json` up the tree counts for npm classification. Default: `false`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub combined_dependencies: Option<bool>,
    /// `node_modules` or Yarn Plug'n'Play. Default: `node_modules`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_module_resolution_strategy: Option<ExternalModuleResolutionStrategy>,
    /// Changes to the runtime's built-in module list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub built_in_modules: Option<BuiltInModules>,
    /// Extensions to scan beyond the parser's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra_extensions_to_scan: Option<Vec<String>>,
    /// Modules to record but not follow.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub do_not_follow: Option<PathFilter>,
    /// Modules to leave out entirely.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude: Option<PathFilter>,
    /// The only modules to include.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_only: Option<PathFilter>,
    /// How deep to follow from the roots; `0` is unlimited. Default: `0`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_depth: Option<u8>,
    /// Whether to record size and statement counts. Default: `false`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experimental_stats: Option<bool>,
}

impl TypeScriptOptions {
    /// dependency-cruiser's default `moduleSystems`.
    pub const DEFAULT_MODULE_SYSTEMS: [ModuleSystem; 4] = [
        ModuleSystem::Es6,
        ModuleSystem::Cjs,
        ModuleSystem::Tsd,
        ModuleSystem::Amd,
    ];

    /// The module systems to extract, with the default applied.
    pub fn module_systems(&self) -> Vec<ModuleSystem> {
        self.module_systems
            .clone()
            .unwrap_or_else(|| Self::DEFAULT_MODULE_SYSTEMS.to_vec())
    }

    /// Whether pre-compilation edges are kept, with the default applied.
    pub fn keeps_pre_compilation_deps(&self) -> bool {
        match self.ts_pre_compilation_deps {
            None | Some(TsPreCompilationDeps::Enabled(false)) => false,
            Some(TsPreCompilationDeps::Enabled(true) | TsPreCompilationDeps::Specify(_)) => true,
        }
    }

    /// The exotic require names, with the default (none) applied.
    pub fn exotic_require_strings(&self) -> &[String] {
        self.exotic_require_strings.as_deref().unwrap_or_default()
    }

    /// The resolution strategy, with the default applied.
    pub fn external_module_resolution_strategy(&self) -> ExternalModuleResolutionStrategy {
        self.external_module_resolution_strategy
            .unwrap_or(ExternalModuleResolutionStrategy::NodeModules)
    }

    /// The maximum depth, with the default (unlimited, `0`) applied.
    pub fn max_depth(&self) -> u8 {
        self.max_depth.unwrap_or(0)
    }
}

/// `languages.dotnet`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DotnetOptions {
    /// The `.sln` or `.slnx` to read. Default: the only one in the working directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub solution: Option<String>,
    /// The build configuration whose output is read. Default: `Debug`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration: Option<String>,
    /// The target framework to read when a project has several. Default: the first listed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_framework: Option<String>,
    /// Project paths to leave out, as patterns.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude_projects: Option<Patterns>,
}

impl DotnetOptions {
    /// The configuration, with the default applied.
    pub fn configuration(&self) -> &str {
        self.configuration.as_deref().unwrap_or("Debug")
    }
}

/// `languages.python`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PythonOptions {
    /// The Python version whose standard library is `stdlib`. Default: `3.13`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Import roots. Default: `src` when it exists, else the working directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roots: Option<Vec<String>>,
    /// Whether `.pyi` stubs are modules. Default: `false`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stubs: Option<bool>,
}

impl PythonOptions {
    /// The Python version, with the default applied.
    pub fn version(&self) -> &str {
        self.version.as_deref().unwrap_or("3.13")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flat_dependency_cruiser_options_block_deserialises() {
        let json = r#"{
            "tsConfig": {"fileName": "tsconfig.json"},
            "tsPreCompilationDeps": "specify",
            "moduleSystems": ["cjs", "es6"],
            "detectJSDocImports": true,
            "exoticRequireStrings": ["need", "window.require"],
            "doNotFollow": {"path": "node_modules", "dependencyTypes": ["npm", "core"]},
            "exclude": ["^dist/", "^coverage/"],
            "enhancedResolveOptions": {"exportsFields": ["exports"], "conditionNames": ["import"]},
            "externalModuleResolutionStrategy": "yarn-pnp",
            "maxDepth": 3
        }"#;
        let options: TypeScriptOptions = serde_json::from_str(json).unwrap_or_default();
        assert_eq!(
            options.module_systems(),
            [ModuleSystem::Cjs, ModuleSystem::Es6]
        );
        assert!(options.keeps_pre_compilation_deps());
        assert_eq!(options.detect_js_doc_imports, Some(true));
        assert_eq!(options.exotic_require_strings(), ["need", "window.require"]);
        let dnf = options.do_not_follow.as_ref();
        assert_eq!(
            dnf.and_then(PathFilter::path).map(Patterns::joined),
            Some("node_modules".to_owned())
        );
        assert_eq!(
            dnf.map(PathFilter::dependency_types),
            Some(&[DependencyType::Npm, DependencyType::Core][..])
        );
        assert_eq!(
            options
                .exclude
                .as_ref()
                .and_then(PathFilter::path)
                .map(Patterns::joined),
            Some("^dist/|^coverage/".to_owned())
        );
        assert_eq!(
            options.external_module_resolution_strategy(),
            ExternalModuleResolutionStrategy::YarnPnp
        );
        assert_eq!(options.max_depth(), 3);
        // Round-trips without inventing defaults.
        let back = serde_json::to_value(&options).unwrap_or_default();
        assert!(back.get("parser").is_none());
        assert_eq!(back["tsPreCompilationDeps"], "specify");
    }

    #[test]
    fn defaults_are_applied_by_accessors_not_by_filling_fields() {
        let options = TypeScriptOptions::default();
        assert_eq!(
            options.module_systems(),
            TypeScriptOptions::DEFAULT_MODULE_SYSTEMS
        );
        assert!(!options.keeps_pre_compilation_deps());
        assert!(options.exotic_require_strings().is_empty());
        assert_eq!(
            options.external_module_resolution_strategy(),
            ExternalModuleResolutionStrategy::NodeModules
        );
        assert_eq!(options.max_depth(), 0);
        assert_eq!(serde_json::to_string(&options).unwrap_or_default(), "{}");
        assert_eq!(DotnetOptions::default().configuration(), "Debug");
        assert_eq!(PythonOptions::default().version(), "3.13");
    }

    #[test]
    fn pre_compilation_deps_accepts_all_three_forms() {
        for (text, keeps) in [("true", true), ("false", false), ("\"specify\"", true)] {
            let options: TypeScriptOptions =
                serde_json::from_str(&format!(r#"{{"tsPreCompilationDeps": {text}}}"#))
                    .unwrap_or_default();
            assert_eq!(options.keeps_pre_compilation_deps(), keeps, "{text}");
        }
        assert!(
            serde_json::from_str::<TypeScriptOptions>(r#"{"tsPreCompilationDeps": "maybe"}"#)
                .is_err()
        );
    }

    #[test]
    fn filters_report_their_parts() {
        let short = PathFilter::Patterns(Patterns::One("x".to_owned()));
        assert!(short.dependency_types().is_empty());
        assert_eq!(short.dynamic(), None);
        let compound = PathFilter::Compound(CompoundFilter {
            path: None,
            dependency_types: None,
            dynamic: Some(true),
        });
        assert_eq!(compound.path(), None);
        assert_eq!(compound.dynamic(), Some(true));
        assert!(compound.dependency_types().is_empty());
        assert_eq!(
            Patterns::Many(vec!["a".to_owned(), "b".to_owned()]).as_slice(),
            ["a", "b"]
        );
        assert_eq!(Patterns::One("a".to_owned()).as_slice(), ["a"]);
    }

    #[test]
    fn unknown_keys_are_rejected_with_their_name() {
        let error = serde_json::from_str::<DotnetOptions>(r#"{"solutionFile": "a.sln"}"#)
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default();
        assert!(error.contains("solutionFile"), "{error}");
    }

    #[test]
    fn dotnet_and_python_options_read_the_design_example() {
        let dotnet: DotnetOptions = serde_json::from_str(
            r#"{"solution": "CleanArchitecture.slnx", "configuration": "Release", "excludeProjects": ["^tools/"]}"#,
        )
        .unwrap_or_default();
        assert_eq!(dotnet.configuration(), "Release");
        assert_eq!(
            dotnet.exclude_projects.as_ref().map(Patterns::joined),
            Some("^tools/".to_owned())
        );
        let python: PythonOptions =
            serde_json::from_str(r#"{"version": "3.12", "roots": ["src"]}"#).unwrap_or_default();
        assert_eq!(python.version(), "3.12");
        assert_eq!(python.roots, Some(vec!["src".to_owned()]));
    }
}
