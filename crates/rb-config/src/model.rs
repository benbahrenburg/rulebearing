//! The configuration model: one set of types that both front-ends load into.
//!
//! - Contract: [Wave 1 plan § 1.5](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#15-interfaces-and-contracts-frozen-by-this-wave)
//!   (the config model)
//! - Decision: [ADR-0005](../../../docs/adr/0005-native-config-superset-and-compat.md) (one model,
//!   two front-ends; every dependency-cruiser key legal at the same place)
//! - Source: [design § Dependency rules](../../../docs/artifacts/design.md#dependency-rules-the-whole-of-dependency-cruiser-1820),
//!   [design § Rule metadata](../../../docs/artifacts/design.md#rule-metadata-that-says-what-to-do),
//!   [coverage § Rules](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#rules)
//! - Requirements: [FR-CFG-01](../../../docs/prd.md#fr-cfg-01), [FR-CFG-07](../../../docs/prd.md#fr-cfg-07),
//!   [FR-RULE-01](../../../docs/prd.md#fr-rule-01)
//!
//! A rule is deserialised from dependency-cruiser's JSON shape, key for key, so a rule written for
//! dependency-cruiser and a rule in a native file are the same value. Every restriction is an
//! `Option` because dependency-cruiser's matchers ask whether a key is *present*
//! (`Object.hasOwn(rule.to, "circular")`), not whether it is truthy; `None` and `Some(false)` are
//! different rules. Defaults (`severity: warn`, `name: unnamed`, `scope: module`) are applied by
//! [`crate::normalize`], exactly where dependency-cruiser applies them.

use std::collections::BTreeMap;
use std::path::PathBuf;

use chrono::NaiveDate;
use rb_model::options::Patterns;
use rb_model::{DependencyType, DotnetOptions, PythonOptions, Severity, TypeScriptOptions};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Which front-end read the configuration, and so which compatibility rules apply
/// ([ADR-0016](../../../docs/adr/0016-linear-time-regex-and-strict-compat.md)).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CompatMode {
    /// A `.dependency-cruiser.*` file: dependency-cruiser's semantics, native additions warned.
    #[default]
    DependencyCruiser,
    /// A `rulebearing.*` file.
    Native,
}

/// A loaded, validated and normalised configuration.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Config {
    /// `$schema`, as written.
    pub schema: Option<String>,
    /// The `extends` chain as written in the root file, before resolution.
    pub extends: Vec<String>,
    /// `defines`, native only.
    pub defines: BTreeMap<String, Define>,
    /// Per-language options.
    pub languages: Languages,
    /// Every option that is not per-language.
    pub options: Options,
    /// The rule families.
    pub rules: Rules,
    /// `options.knownViolations`, read path only in wave 1
    /// ([Wave 1 plan § 1.6](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#16-decisions-applied-and-decisions-to-make)).
    pub known_violations: Vec<KnownViolation>,
    /// The front-end that read the root file.
    pub compat: CompatMode,
    /// The file the configuration came from, when it came from a file.
    pub origin: Option<PathBuf>,
    /// The canonical, dependency-cruiser-shaped JSON after `extends` and `defines`, before
    /// normalisation: what `summary.ruleSetUsed` and `config convert` start from.
    pub canonical: Map<String, Value>,
    /// Problems that did not stop loading.
    pub warnings: Vec<ConfigWarning>,
    /// Every file read to build the configuration, for the `attest` receipt.
    pub files: Vec<PathBuf>,
    /// Whether JavaScript was evaluated by Node (`--config-via-node`), recorded in `optionsUsed`.
    pub via_node: bool,
    /// `allowEmpty`, native only: the rules and ratchets named as allowed to match nothing,
    /// already applied to each rule's `meta.allow_empty`
    /// ([ADR-0032](../../../docs/adr/0032-liveness-follows-the-configuration-format.md)).
    pub allow_empty: Vec<String>,
}

/// A problem found while loading that does not make the configuration invalid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigWarning {
    /// The rule concerned, when there is one.
    pub rule: Option<String>,
    /// What is wrong and what to do about it.
    pub message: String,
}

impl ConfigWarning {
    /// A warning about the configuration as a whole.
    pub fn general(message: impl Into<String>) -> Self {
        Self {
            rule: None,
            message: message.into(),
        }
    }

    /// A warning about one rule.
    pub fn about(rule: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            rule: Some(rule.into()),
            message: message.into(),
        }
    }
}

/// `languages`: what is per-language. Wave 1 reads TypeScript; the others are parsed so a file
/// written for wave 2 already loads.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Languages {
    /// `languages.typescript`, merged with dependency-cruiser's flat option names.
    pub typescript: TypeScriptOptions,
    /// `languages.dotnet`.
    pub dotnet: Option<DotnetOptions>,
    /// `languages.python`.
    pub python: Option<PythonOptions>,
}

/// Every option that is not an extractor option: the filters applied after extraction, the
/// derivation switches, the reporter settings and the rest of dependency-cruiser's option set
/// ([coverage § Options](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#options)).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Options {
    /// `focus`: the modules to show with their neighbours.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus: Option<FilterOption>,
    /// `focusDepth`, the command-line form of `focus.depth`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus_depth: Option<Value>,
    /// `reaches`: the modules whose dependents to show.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reaches: Option<FilterOption>,
    /// `highlight`: the modules to mark.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub highlight: Option<FilterOption>,
    /// `collapse`: a pattern or a folder depth.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collapse: Option<Value>,
    /// Always derive `dependents[]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub force_derive_dependents: Option<bool>,
    /// Skip derivations no rule reads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skip_analysis_not_in_rules: Option<bool>,
    /// Compute instability metrics (wave 2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metrics: Option<bool>,
    /// The link prefix reporters put before a path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    /// The link suffix reporters put after a path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suffix: Option<String>,
    /// `progress`: `{ type: none | cli-feedback | performance-log | ndjson }`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<Value>,
    /// `reporterOptions`, verbatim; each reporter reads its own key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reporter_options: Option<Value>,
    /// `cache`, accepted and recorded (the content-addressed cache is wave 3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache: Option<Value>,
    /// `outputType`, when a config pins one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_type: Option<String>,
    /// `outputTo`, when a config pins one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_to: Option<String>,
    /// `baseline` (`mode`, `staleEntriesSeverity`), accepted and recorded (wave 2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline: Option<Value>,
    /// `affected`, accepted and recorded (wave 3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub affected: Option<Value>,
    /// `webpackConfig` read from `--webpack-config-json`; its consumer is wave 2.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webpack_config_json: Option<Value>,
}

/// `focus`, `reaches`, `highlight`: the pattern form or the compound form.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FilterOption {
    /// The paths the filter matches, joined.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// `focus.depth`: how many steps of neighbours to show. Default 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<u32>,
}

/// The rule families.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Rules {
    /// `rules.dependencies`, which is dependency-cruiser's rule set.
    pub dependencies: DependencyRules,
    /// `rules.ratchets`.
    pub ratchets: Vec<Ratchet>,
    /// `rules.layers`, already expanded into `dependencies.forbidden`; kept for `config expand`.
    pub layers: Vec<LayersShorthand>,
    /// `rules.independence`, already expanded into `dependencies.forbidden`.
    pub independence: Vec<IndependenceShorthand>,
    /// `rules.elements` ([`crate::elements`]).
    pub elements: Vec<crate::elements::ElementRule>,
    /// `rules.slices`.
    pub slices: Vec<crate::elements::SliceRule>,
    /// `rules.diagrams`.
    pub diagrams: Vec<crate::elements::DiagramRule>,
}

impl Rules {
    /// Every dependency rule in the order dependency-cruiser evaluates them, with its family.
    pub fn all_dependency_rules(&self) -> impl Iterator<Item = (Family, &Rule)> {
        self.dependencies
            .forbidden
            .iter()
            .map(|r| (Family::Forbidden, r))
            .chain(
                self.dependencies
                    .allowed
                    .iter()
                    .map(|r| (Family::Allowed, r)),
            )
            .chain(
                self.dependencies
                    .required
                    .iter()
                    .map(|r| (Family::Required, r)),
            )
    }
}

/// Which list a dependency rule came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Family {
    /// `forbidden[]`.
    Forbidden,
    /// `allowed[]`.
    Allowed,
    /// `required[]`.
    Required,
}

impl Family {
    /// The key under `rules.dependencies`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Forbidden => "forbidden",
            Self::Allowed => "allowed",
            Self::Required => "required",
        }
    }
}

/// dependency-cruiser's rule set: `forbidden`, `allowed` with `allowedSeverity`, `required`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DependencyRules {
    /// `forbidden[]`, normalised: `ignore` rules removed.
    pub forbidden: Vec<Rule>,
    /// `allowed[]`, normalised: every rule named `not-in-allowed`.
    pub allowed: Vec<Rule>,
    /// `allowedSeverity`, present when `allowed` is.
    pub allowed_severity: Option<Severity>,
    /// `required[]`, normalised.
    pub required: Vec<Rule>,
}

/// The metadata every rule of every family carries
/// ([design § Rule metadata](../../../docs/artifacts/design.md#rule-metadata-that-says-what-to-do)).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RuleMeta {
    /// The rule name. Default `unnamed`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Why the rule exists; carries the decision token.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    /// `error`, `warn`, `info` or `ignore`. Default `warn`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity: Option<Severity>,
    /// The imperative to follow when the rule fires.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
    /// Edges that must pass and edges that must fail; `rulebearing test` runs them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub examples: Option<Examples>,
    /// Who answers for the rule.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    /// The last day the rule applies; the run fails the day after.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<String>")]
    pub expires: Option<NaiveDate>,
    /// Opt out of liveness ([ADR-0007](../../../docs/adr/0007-vacuous-rules-fail-by-default.md)).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub allow_empty: bool,
}

impl RuleMeta {
    /// The name, with dependency-cruiser's default.
    pub fn name(&self) -> &str {
        self.name.as_deref().unwrap_or("unnamed")
    }

    /// The severity, with dependency-cruiser's default.
    pub fn severity(&self) -> Severity {
        self.severity.unwrap_or(Severity::Warn)
    }
}

/// `examples`: `"from -> to"` strings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Examples {
    /// Edges the rule must allow.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed: Vec<String>,
    /// Edges the rule must flag.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub forbidden: Vec<String>,
}

/// `scope`: module or folder.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    /// The default: the rule compares modules.
    #[default]
    Module,
    /// The rule compares folders.
    Folder,
}

/// One dependency rule, in dependency-cruiser's shape plus the native metadata.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    /// Name, comment, severity and the native metadata.
    #[serde(flatten)]
    pub meta: RuleMeta,
    /// `module` or `folder`. Default `module`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<Scope>,
    /// The selecting side of a dependency rule.
    #[serde(default)]
    pub from: FromRestriction,
    /// The target side.
    #[serde(default)]
    pub to: ToRestriction,
    /// The module restriction of a dependents or required rule.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module: Option<ModuleRestriction>,
}

impl Rule {
    /// The name, with the default applied.
    pub fn name(&self) -> &str {
        self.meta.name()
    }

    /// The severity, with the default applied.
    pub fn severity(&self) -> Severity {
        self.meta.severity()
    }

    /// Whether the rule is about modules rather than edges: an orphan, reachability or
    /// dependents rule (dependency-cruiser's `isModuleOnlyRule`).
    pub fn is_module_only(&self) -> bool {
        self.from.orphan.is_some() || self.to.reachable.is_some() || self.module.is_some()
    }

    /// Whether the rule compares folders.
    pub fn is_folder_scope(&self) -> bool {
        self.scope == Some(Scope::Folder)
    }
}

/// `from`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FromRestriction {
    /// Paths the source must match.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<Patterns>,
    /// Paths the source must not match.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_not: Option<Patterns>,
    /// Match modules with no edges in or out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orphan: Option<bool>,
}

/// `to`: every restriction dependency-cruiser 18.2.0 defines.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ToRestriction {
    /// Paths the target must match; `$0` to `$9` substitute captures from `from.path`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<Patterns>,
    /// Paths the target must not match.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_not: Option<Patterns>,
    /// Match unresolvable targets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub could_not_resolve: Option<bool>,
    /// Match edges on a cycle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub circular: Option<bool>,
    /// Match `import()` edges.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dynamic: Option<bool>,
    /// Match edges formed by an exotic require.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exotically_required: Option<bool>,
    /// The exotic require names to match.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exotic_require: Option<Patterns>,
    /// The exotic require names not to match.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exotic_require_not: Option<Patterns>,
    /// Match edges that disappear after compilation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pre_compilation_only: Option<bool>,
    /// Dependency types, any of which matches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dependency_types: Option<Vec<DependencyType>>,
    /// Dependency types, none of which may be present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dependency_types_not: Option<Vec<DependencyType>>,
    /// Match edges whose target is depended on in more than one way.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub more_than_one_dependency_type: Option<bool>,
    /// Licences to match (wave 2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<Patterns>,
    /// Licences not to match (wave 2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license_not: Option<Patterns>,
    /// Cycle members, some of which match.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub via: Option<ViaRestriction>,
    /// Cycle members, all of which match.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub via_only: Option<ViaRestriction>,
    /// Shorthand for `viaOnly.pathNot`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub via_not: Option<Patterns>,
    /// Shorthand for `via.pathNot`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub via_some_not: Option<Patterns>,
    /// Match targets more unstable than the source (needs `metrics`, wave 2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub more_unstable: Option<bool>,
    /// Match targets in a folder above the source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ancestor: Option<bool>,
    /// Reachability: match modules the `from` modules reach (or do not reach).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reachable: Option<bool>,
}

/// `via` and `viaOnly`: the pattern form or the object form.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(from = "ViaForm", into = "ViaForm")]
pub struct ViaRestriction {
    /// Cycle members to match.
    pub path: Option<Patterns>,
    /// Cycle members not to match.
    pub path_not: Option<Patterns>,
    /// Dependency types a step must have.
    pub dependency_types: Option<Vec<DependencyType>>,
    /// Dependency types a step must not have.
    pub dependency_types_not: Option<Vec<DependencyType>>,
}

/// The two ways `via` is written.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum ViaForm {
    /// `via: "pattern"` or `via: ["a", "b"]`.
    Patterns(Patterns),
    /// `via: { path, pathNot, dependencyTypes, dependencyTypesNot }`.
    Object(ViaObject),
}

/// The object form of `via`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ViaObject {
    /// Cycle members to match.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<Patterns>,
    /// Cycle members not to match.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_not: Option<Patterns>,
    /// Dependency types a step must have.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dependency_types: Option<Vec<DependencyType>>,
    /// Dependency types a step must not have.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dependency_types_not: Option<Vec<DependencyType>>,
}

impl From<ViaForm> for ViaRestriction {
    fn from(form: ViaForm) -> Self {
        match form {
            ViaForm::Patterns(path) => Self {
                path: Some(path),
                ..Self::default()
            },
            ViaForm::Object(object) => Self {
                path: object.path,
                path_not: object.path_not,
                dependency_types: object.dependency_types,
                dependency_types_not: object.dependency_types_not,
            },
        }
    }
}

impl From<ViaRestriction> for ViaForm {
    fn from(via: ViaRestriction) -> Self {
        Self::Object(ViaObject {
            path: via.path,
            path_not: via.path_not,
            dependency_types: via.dependency_types,
            dependency_types_not: via.dependency_types_not,
        })
    }
}

/// `module`: the restriction of a dependents rule or a required rule.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ModuleRestriction {
    /// Modules to match.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<Patterns>,
    /// Modules not to match.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_not: Option<Patterns>,
    /// Match modules with fewer matching dependents than this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number_of_dependents_less_than: Option<u64>,
    /// Match modules with more matching dependents than this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number_of_dependents_more_than: Option<u64>,
}

/// `rules.ratchets[]`: a count of direct edges that may only fall
/// ([FR-RULE-06](../../../docs/prd.md#fr-rule-06)).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Ratchet {
    /// The ratchet name.
    pub name: String,
    /// Why it exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    /// The imperative to follow when the count rises.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
    /// Sources to count from.
    pub from: FromRestriction,
    /// Targets to count to; `$1` substitutes from `from.path`.
    pub to: ToRestriction,
    /// The budget file, `{ "ceiling": n }`.
    pub budget: String,
    /// Who answers for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
}

/// `rules.layers[]`: one `forbidden` rule per lower-to-higher pair
/// ([design § Shorthands](../../../docs/artifacts/design.md#shorthands)).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LayersShorthand {
    /// The rule name; each expanded rule is `<name>:<lower>-to-<higher>`.
    pub name: String,
    /// Why the layering exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    /// The fix shared by every expanded rule.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
    /// Severity of every expanded rule. Default `error`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity: Option<Severity>,
    /// Path patterns from the highest layer to the lowest; a lower layer may not depend on a
    /// higher one.
    pub layers: Vec<String>,
    /// Opt out of liveness for every expanded rule.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub allow_empty: bool,
}

/// `rules.independence[]`: one `$1` fence
/// ([design § Shorthands](../../../docs/artifacts/design.md#shorthands)).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IndependenceShorthand {
    /// The rule name.
    pub name: String,
    /// Why the modules are independent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    /// The fix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
    /// Severity. Default `error`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity: Option<Severity>,
    /// A path pattern with exactly one capturing group naming the module, for example
    /// `^apps/web/src/features/([^/]+)/`.
    pub pattern: String,
    /// Opt out of liveness.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub allow_empty: bool,
}

/// `defines.<name>`: a value read from a JSON file
/// ([FR-CFG-04](../../../docs/prd.md#fr-cfg-04)).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Define {
    /// The JSON file, relative to the configuration file.
    pub from_json: String,
    /// A path expression: keys, `[*]`, `[n]`, separated by dots.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub select: Option<String>,
    /// Joins an array of strings. Default `|`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub join_with: Option<String>,
}

/// One `knownViolations` entry: dependency-cruiser's shape, or a Rulebearing id with lifecycle
/// fields.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct KnownViolation {
    /// The stable id, when Rulebearing wrote the entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The violating module.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    /// The module depended on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    /// The rule.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule: Option<KnownRule>,
    /// The cycle, for a cycle violation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cycle: Option<Vec<Value>>,
    /// The via chain, for a reachability violation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub via: Option<Vec<Value>>,
    /// The last day the exception applies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<String>")]
    pub expires: Option<NaiveDate>,
    /// Who answers for the exception.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    /// Every other key, kept so an entry dependency-cruiser wrote round-trips.
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: BTreeMap<String, Value>,
}

/// The rule of a `knownViolations` entry.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct KnownRule {
    /// The rule name.
    pub name: String,
    /// The severity it had.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity: Option<Severity>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dependency_cruiser_rule_deserialises_key_for_key() {
        let rule: Rule = serde_json::from_str(
            r#"{"name":"no-cross","severity":"error","comment":"c adr:0003",
                "from":{"path":"^apps/([^/]+)/"},
                "to":{"path":"^apps/([^/]+)/","pathNot":["^apps/$1/","^x"],"circular":false,
                      "dependencyTypesNot":["type-only"],"via":"^a","viaOnly":{"pathNot":"b"}}}"#,
        )
        .unwrap_or_default();
        assert_eq!(rule.name(), "no-cross");
        assert_eq!(rule.severity(), Severity::Error);
        assert_eq!(rule.to.circular, Some(false));
        assert_eq!(
            rule.to.path_not.as_ref().map(Patterns::joined).as_deref(),
            Some("^apps/$1/|^x")
        );
        assert_eq!(
            rule.to.via.as_ref().and_then(|v| v.path.as_ref()),
            Some(&Patterns::One("^a".into()))
        );
        assert_eq!(
            rule.to.via_only.as_ref().and_then(|v| v.path_not.as_ref()),
            Some(&Patterns::One("b".into()))
        );
        assert_eq!(
            rule.to.dependency_types_not,
            Some(vec![DependencyType::TypeOnly])
        );
        assert!(!rule.is_module_only());
        assert!(!rule.is_folder_scope());
    }

    #[test]
    fn defaults_are_dependency_cruisers() {
        let rule = Rule::default();
        assert_eq!(rule.name(), "unnamed");
        assert_eq!(rule.severity(), Severity::Warn);
    }

    #[test]
    fn module_only_and_folder_scope_are_classified() {
        let orphan: Rule =
            serde_json::from_str(r#"{"from":{"orphan":true},"to":{}}"#).unwrap_or_default();
        assert!(orphan.is_module_only());
        let reach: Rule =
            serde_json::from_str(r#"{"from":{},"to":{"reachable":false}}"#).unwrap_or_default();
        assert!(reach.is_module_only());
        let dependents: Rule =
            serde_json::from_str(r#"{"from":{},"module":{"numberOfDependentsLessThan":2}}"#)
                .unwrap_or_default();
        assert!(dependents.is_module_only());
        let folder: Rule =
            serde_json::from_str(r#"{"scope":"folder","from":{},"to":{"circular":true}}"#)
                .unwrap_or_default();
        assert!(folder.is_folder_scope());
    }

    #[test]
    fn native_metadata_round_trips() {
        let text = r#"{"name":"r","fix":"do x","owner":"@me","expires":"2026-12-31","allowEmpty":true,"examples":{"allowed":["a -> b"]},"from":{},"to":{}}"#;
        let rule: Rule = serde_json::from_str(text).unwrap_or_default();
        assert_eq!(rule.meta.fix.as_deref(), Some("do x"));
        assert!(rule.meta.allow_empty);
        assert_eq!(rule.meta.expires, NaiveDate::from_ymd_opt(2026, 12, 31));
        let back = serde_json::to_value(&rule).unwrap_or_default();
        assert_eq!(back["expires"], "2026-12-31");
        assert_eq!(back["examples"]["allowed"][0], "a -> b");
    }

    #[test]
    fn families_iterate_in_evaluation_order() {
        let mut rules = Rules::default();
        rules.dependencies.required.push(Rule::default());
        rules.dependencies.forbidden.push(Rule::default());
        rules.dependencies.allowed.push(Rule::default());
        let order: Vec<Family> = rules.all_dependency_rules().map(|(f, _)| f).collect();
        assert_eq!(
            order,
            [Family::Forbidden, Family::Allowed, Family::Required]
        );
        assert_eq!(Family::Allowed.as_str(), "allowed");
    }

    #[test]
    fn warnings_carry_their_rule() {
        assert_eq!(ConfigWarning::about("r", "m").rule.as_deref(), Some("r"));
        assert_eq!(ConfigWarning::general("m").rule, None);
    }
}
