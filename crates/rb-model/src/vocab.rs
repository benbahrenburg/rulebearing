//! The closed vocabularies of the graph document: dependency types, module systems, languages,
//! attribution states, edge kinds, severities, protocols and violation types.
//!
//! - Contract: [ADR-0004](../../../docs/adr/0004-graph-document-is-cruise-result-superset.md)
//!   (the strings are dependency-cruiser's, unchanged, plus additive values)
//! - Source: [coverage § Dependency types and module systems](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#dependency-types-and-module-systems),
//!   [design § Dependency rules](../../../docs/artifacts/design.md#dependency-rules-the-whole-of-dependency-cruiser-1820)
//! - Plan: [Wave 0, Step 3](../../../docs/plans/pending/0000-wave-0-spike.md#step-3-rb-model-graph-document-schema-violation-id-0a)
//! - Requirement: [FR-CORE-03](../../../docs/prd.md#fr-core-03)
//!
//! Every vocabulary is declared once through `vocabulary!`, which generates the enum, its string
//! form, the parser, the serde implementation and the JSON schema from the same table, so the
//! string a reporter prints, the string a config names and the string the schema allows cannot
//! drift apart. A string outside the table is a deserialisation error, never a silent default.

use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A string that is not a member of a closed vocabulary.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{value}` is not a valid {vocabulary}")]
pub struct UnknownValue {
    /// The vocabulary that was asked, for example `dependency type`.
    pub vocabulary: &'static str,
    /// The string that did not match.
    pub value: String,
}

macro_rules! vocabulary {
    (
        $(#[$meta:meta])*
        $name:ident, $label:literal {
            $( $(#[$vmeta:meta])* $variant:ident => $text:literal, )+
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum $name {
            $( $(#[$vmeta])* $variant, )+
        }

        impl $name {
            /// Every member, in declaration order.
            pub const ALL: &'static [Self] = &[ $( Self::$variant, )+ ];

            /// The string form, exactly as the document carries it.
            pub const fn as_str(self) -> &'static str {
                match self {
                    $( Self::$variant => $text, )+
                }
            }
        }

        impl FromStr for $name {
            type Err = UnknownValue;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                match value {
                    $( $text => Ok(Self::$variant), )+
                    _ => Err(UnknownValue { vocabulary: $label, value: value.to_owned() }),
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.as_str())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let text = Cow::<'de, str>::deserialize(deserializer)?;
                text.parse().map_err(serde::de::Error::custom)
            }
        }

        impl JsonSchema for $name {
            fn schema_name() -> Cow<'static, str> {
                Cow::Borrowed(stringify!($name))
            }

            fn json_schema(_: &mut SchemaGenerator) -> Schema {
                let values: Vec<&'static str> = Self::ALL.iter().map(|v| v.as_str()).collect();
                json_schema!({ "type": "string", "enum": values })
            }
        }
    };
}

vocabulary! {
    /// `dependencyTypes` values. The first forty are dependency-cruiser 18.2.0's, in its schema's
    /// order; the rest are the .NET and Python additions from
    /// [design § Dependency rules](../../../docs/artifacts/design.md#dependency-rules-the-whole-of-dependency-cruiser-1820).
    /// `local` and `type-only` are shared across languages.
    DependencyType, "dependency type" {
        /// `package.json` `imports` subpath alias (`#x`).
        AliasedSubpathImport => "aliased-subpath-import",
        /// Resolved through `compilerOptions.baseUrl`.
        AliasedTsconfigBaseUrl => "aliased-tsconfig-base-url",
        /// Resolved through `compilerOptions.paths`.
        AliasedTsconfigPaths => "aliased-tsconfig-paths",
        /// Resolved through any tsconfig mechanism.
        AliasedTsconfig => "aliased-tsconfig",
        /// Resolved through a webpack `resolve.alias`.
        AliasedWebpack => "aliased-webpack",
        /// Resolved to a workspace package.
        AliasedWorkspace => "aliased-workspace",
        /// Resolved through any alias.
        Aliased => "aliased",
        /// AMD `define([...])`.
        AmdDefine => "amd-define",
        /// AMD `require([...])`.
        AmdRequire => "amd-require",
        /// AMD require through an exotic name.
        AmdExoticRequire => "amd-exotic-require",
        /// A runtime built-in (`fs`, `node:path`).
        Core => "core",
        /// An npm package marked deprecated.
        Deprecated => "deprecated",
        /// `import()`.
        DynamicImport => "dynamic-import",
        /// `require` through a name listed in `exoticRequireStrings`.
        ExoticRequire => "exotic-require",
        /// `export ... from`.
        Export => "export",
        /// TypeScript `import x = require(...)`.
        ImportEquals => "import-equals",
        /// ES `import`.
        Import => "import",
        /// Any JSDoc import.
        Jsdoc => "jsdoc",
        /// JSDoc `{import('x').T}`.
        JsdocBracketImport => "jsdoc-bracket-import",
        /// JSDoc `@import`.
        JsdocImportTag => "jsdoc-import-tag",
        /// A file inside the repository.
        Local => "local",
        /// A module resolved from a `modules` folder that is not `node_modules`.
        Localmodule => "localmodule",
        /// An npm package in `bundledDependencies`.
        NpmBundled => "npm-bundled",
        /// An npm package in `devDependencies`.
        NpmDev => "npm-dev",
        /// An npm package in no `package.json`.
        NpmNoPkg => "npm-no-pkg",
        /// An npm package in `optionalDependencies`.
        NpmOptional => "npm-optional",
        /// An npm package in `peerDependencies`.
        NpmPeer => "npm-peer",
        /// An npm package with no `package.json` to classify it against.
        NpmUnknown => "npm-unknown",
        /// An npm package in `dependencies`.
        Npm => "npm",
        /// Present before TypeScript compilation only.
        PreCompilationOnly => "pre-compilation-only",
        /// `process.getBuiltinModule('x')`.
        ProcessGetBuiltinModule => "process-get-builtin-module",
        /// CommonJS `require`.
        Require => "require",
        /// `/// <amd-dependency path="x" />`.
        TripleSlashAmdDependency => "triple-slash-amd-dependency",
        /// Any triple-slash directive.
        TripleSlashDirective => "triple-slash-directive",
        /// `/// <reference path="x" />`.
        TripleSlashFileReference => "triple-slash-file-reference",
        /// `/// <reference types="x" />`.
        TripleSlashTypeReference => "triple-slash-type-reference",
        /// `import type`.
        TypeImport => "type-import",
        /// Only types cross the edge (TypeScript) or only annotations do (Python).
        TypeOnly => "type-only",
        /// Resolved, but not classifiable.
        Undetermined => "undetermined",
        /// Not resolved.
        Unknown => "unknown",
        /// .NET: another project in the solution.
        Project => "project",
        /// .NET: a NuGet package.
        Package => "package",
        /// .NET: the shared framework.
        Framework => "framework",
        /// .NET: reachable from a test project only.
        TestOnly => "test-only",
        /// .NET: referenced from a signature only.
        SignatureOnly => "signature-only",
        /// .NET and Python: the target could not be resolved.
        Unresolved => "unresolved",
        /// Python: the standard library.
        Stdlib => "stdlib",
        /// Python: an installed distribution.
        Site => "site",
        /// Python: a literal `importlib.import_module` or `__import__`.
        Dynamic => "dynamic",
    }
}

impl DependencyType {
    /// How many values dependency-cruiser 18.2.0 defines; the rest are Rulebearing's additions.
    pub const DEPENDENCY_CRUISER_COUNT: usize = 40;

    /// Whether dependency-cruiser 18.2.0 defines this value, which is what
    /// `--strict-schema` keeps.
    pub fn is_dependency_cruiser(self) -> bool {
        Self::ALL[..Self::DEPENDENCY_CRUISER_COUNT].contains(&self)
    }
}

vocabulary! {
    /// `moduleSystem`. The first four are dependency-cruiser's; `clr` and `py` are additive.
    ModuleSystem, "module system" {
        /// CommonJS.
        Cjs => "cjs",
        /// ES modules.
        Es6 => "es6",
        /// AMD.
        Amd => "amd",
        /// TypeScript triple-slash directives.
        Tsd => "tsd",
        /// .NET metadata references.
        Clr => "clr",
        /// Python imports.
        Py => "py",
    }
}

vocabulary! {
    /// The language an extractor attributes a module to. Carried as a string so that a reporter
    /// or a rule never has to `match` on it
    /// ([ADR-0010](../../../docs/adr/0010-crate-layout-and-extractor-boundary.md)).
    Language, "language" {
        /// `.ts`, `.tsx`, `.mts`, `.cts`, `.d.ts`
        Typescript => "typescript",
        /// `.js`, `.mjs`, `.cjs`, `.jsx`
        Javascript => "javascript",
        /// Types read from built assemblies and portable PDBs.
        Dotnet => "dotnet",
        /// `.py`
        Python => "python",
    }
}

vocabulary! {
    /// How a .NET type was attributed to a source file
    /// ([ADR-0011](../../../docs/adr/0011-read-dotnet-assemblies-not-source.md)).
    Attribution, "attribution" {
        /// From the portable PDB's `Document` and `MethodDebugInformation` tables.
        Pdb => "pdb",
        /// By naming convention, because the type has no method with a sequence point.
        Inferred => "inferred",
        /// No attribution; path-based rules skip the type with a warning.
        None => "none",
    }
}

vocabulary! {
    /// The additive `dependencyKind` of an edge
    /// ([architecture § The graph document](../../../docs/architecture.md#the-graph-document)).
    DependencyKind, "dependency kind" {
        /// A module import of any form.
        Import => "import",
        /// A base class.
        Inherits => "inherits",
        /// An implemented interface.
        Implements => "implements",
        /// A field's type.
        Field => "field",
        /// A parameter or return type.
        Signature => "signature",
        /// A reference inside a method body.
        Body => "body",
        /// An attribute or decorator.
        Attribute => "attribute",
        /// A generic type argument.
        GenericArgument => "generic-argument",
        /// A `typeof` or `ldtoken` reference.
        Typeof => "typeof",
        /// A call.
        Call => "call",
    }
}

vocabulary! {
    /// A rule's severity. Only `error` counts toward the exit code
    /// ([ADR-0008](../../../docs/adr/0008-exit-code-contract.md)). Ordered from least to most
    /// severe, so `max()` over a set of findings gives the one that decides the outcome.
    Severity, "severity" {
        /// Not reported.
        Ignore => "ignore",
        /// Reported, does not affect the exit code.
        Info => "info",
        /// Reported, does not affect the exit code.
        Warn => "warn",
        /// Reported and counted in the exit code.
        Error => "error",
    }
}

vocabulary! {
    /// The URL-style protocol dependency-cruiser strips from a specifier (`node:fs`).
    Protocol, "protocol" {
        /// `data:` URLs.
        Data => "data:",
        /// `file:` URLs.
        File => "file:",
        /// Node built-ins.
        Node => "node:",
        /// Bun built-ins.
        Bun => "bun:",
    }
}

vocabulary! {
    /// `summary.violations[].type`.
    ViolationType, "violation type" {
        /// A forbidden, allowed or required dependency rule.
        Dependency => "dependency",
        /// A rule over a module on its own (orphans, dependents counts).
        Module => "module",
        /// A reachability rule.
        Reachability => "reachability",
        /// A cycle.
        Cycle => "cycle",
        /// A `moreUnstable` rule.
        Instability => "instability",
        /// A folder-scope rule.
        Folder => "folder",
    }
}

vocabulary! {
    /// `revisionData.changes[].type`, git's change vocabulary as dependency-cruiser records it.
    ChangeType, "change type" {
        /// Added.
        Added => "added",
        /// Copied.
        Copied => "copied",
        /// Deleted.
        Deleted => "deleted",
        /// Modified.
        Modified => "modified",
        /// Renamed.
        Renamed => "renamed",
        /// The file type changed.
        TypeChanged => "type changed",
        /// Unmerged.
        Unmerged => "unmerged",
        /// Pairing broken.
        PairingBroken => "pairing broken",
        /// Unknown.
        Unknown => "unknown",
        /// Unmodified.
        Unmodified => "unmodified",
        /// Untracked.
        Untracked => "untracked",
        /// Ignored.
        Ignored => "ignored",
    }
}

vocabulary! {
    /// The parser dependency-cruiser's `parser` option names. Rulebearing parses every one of
    /// them with `oxc_parser` ([ADR-0012](../../../docs/adr/0012-oxc-for-typescript.md)); the
    /// value is kept because it changes which dependency forms dependency-cruiser reports.
    Parser, "parser" {
        /// acorn, dependency-cruiser's default for JavaScript.
        Acorn => "acorn",
        /// The TypeScript compiler.
        Tsc => "tsc",
        /// swc.
        Swc => "swc",
    }
}

vocabulary! {
    /// `externalModuleResolutionStrategy`.
    ExternalModuleResolutionStrategy, "external module resolution strategy" {
        /// Plain `node_modules` lookup.
        NodeModules => "node_modules",
        /// Yarn Plug'n'Play.
        YarnPnp => "yarn-pnp",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Round-trips every member of a vocabulary through its string, serde and the schema.
    fn round_trip<T>(all: &[T], as_str: fn(T) -> &'static str)
    where
        T: Copy + PartialEq + fmt::Debug + FromStr<Err = UnknownValue> + Serialize,
        T: for<'de> Deserialize<'de> + JsonSchema + fmt::Display,
    {
        let schema = schemars::schema_for!(T);
        let allowed = schema
            .get("enum")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        assert_eq!(allowed.len(), all.len(), "schema lists every member");
        for (index, value) in all.iter().enumerate() {
            let text = as_str(*value);
            assert_eq!(text.parse::<T>().ok(), Some(*value), "{text} parses back");
            assert_eq!(value.to_string(), text);
            let json = serde_json::to_string(value).unwrap_or_default();
            assert_eq!(json, format!("\"{text}\""));
            let back: Option<T> = serde_json::from_str(&json).ok();
            assert_eq!(back, Some(*value));
            assert_eq!(allowed[index], serde_json::Value::from(text));
        }
        let unique: std::collections::BTreeSet<_> = all.iter().map(|v| as_str(*v)).collect();
        assert_eq!(unique.len(), all.len(), "no two members share a string");
    }

    #[test]
    fn every_vocabulary_round_trips() {
        round_trip(DependencyType::ALL, DependencyType::as_str);
        round_trip(ModuleSystem::ALL, ModuleSystem::as_str);
        round_trip(Language::ALL, Language::as_str);
        round_trip(Attribution::ALL, Attribution::as_str);
        round_trip(DependencyKind::ALL, DependencyKind::as_str);
        round_trip(Severity::ALL, Severity::as_str);
        round_trip(Protocol::ALL, Protocol::as_str);
        round_trip(ViolationType::ALL, ViolationType::as_str);
        round_trip(ChangeType::ALL, ChangeType::as_str);
        round_trip(Parser::ALL, Parser::as_str);
        round_trip(
            ExternalModuleResolutionStrategy::ALL,
            ExternalModuleResolutionStrategy::as_str,
        );
    }

    #[test]
    fn dependency_cruiser_values_are_exactly_its_forty() {
        // The pinned 18.2.0 schema's enum, in its order. Adding or renaming one is a breaking
        // change under ADR-0004.
        let upstream = [
            "aliased-subpath-import",
            "aliased-tsconfig-base-url",
            "aliased-tsconfig-paths",
            "aliased-tsconfig",
            "aliased-webpack",
            "aliased-workspace",
            "aliased",
            "amd-define",
            "amd-require",
            "amd-exotic-require",
            "core",
            "deprecated",
            "dynamic-import",
            "exotic-require",
            "export",
            "import-equals",
            "import",
            "jsdoc",
            "jsdoc-bracket-import",
            "jsdoc-import-tag",
            "local",
            "localmodule",
            "npm-bundled",
            "npm-dev",
            "npm-no-pkg",
            "npm-optional",
            "npm-peer",
            "npm-unknown",
            "npm",
            "pre-compilation-only",
            "process-get-builtin-module",
            "require",
            "triple-slash-amd-dependency",
            "triple-slash-directive",
            "triple-slash-file-reference",
            "triple-slash-type-reference",
            "type-import",
            "type-only",
            "undetermined",
            "unknown",
        ];
        assert_eq!(upstream.len(), DependencyType::DEPENDENCY_CRUISER_COUNT);
        let ours: Vec<&str> = DependencyType::ALL[..DependencyType::DEPENDENCY_CRUISER_COUNT]
            .iter()
            .map(|t| t.as_str())
            .collect();
        assert_eq!(ours, upstream);
        assert!(DependencyType::Npm.is_dependency_cruiser());
        assert!(DependencyType::Unknown.is_dependency_cruiser());
        assert!(!DependencyType::Project.is_dependency_cruiser());
        assert!(!DependencyType::Dynamic.is_dependency_cruiser());
    }

    #[test]
    fn unknown_strings_are_errors_that_name_the_vocabulary() {
        let error = "npm-devv".parse::<DependencyType>().err();
        assert_eq!(
            error.map(|e| e.to_string()),
            Some("`npm-devv` is not a valid dependency type".to_owned())
        );
        assert!(serde_json::from_str::<Severity>("\"fatal\"").is_err());
        assert!(serde_json::from_str::<ModuleSystem>("\"ES6\"").is_err());
    }

    #[test]
    fn severity_orders_from_ignore_to_error() {
        assert!(Severity::Error > Severity::Warn);
        assert!(Severity::Warn > Severity::Info);
        assert!(Severity::Info > Severity::Ignore);
        assert_eq!(Severity::ALL.iter().max(), Some(&Severity::Error));
    }
}
