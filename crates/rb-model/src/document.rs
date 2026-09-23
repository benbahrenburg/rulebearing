//! The graph document's module layer: dependency-cruiser 18.2.0's `cruise-result` schema,
//! field for field, plus the additive fields Rulebearing records.
//!
//! - Contract: [ADR-0004](../../../docs/adr/0004-graph-document-is-cruise-result-superset.md)
//! - Architecture: [The graph document](../../../docs/architecture.md#the-graph-document)
//! - Source: [coverage § Result document](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#result-document-cruise-result-schema)
//! - Plan: [Wave 0, Step 3](../../../docs/plans/pending/0000-wave-0-spike.md#step-3-rb-model-graph-document-schema-violation-id-0a)
//! - Requirement: [FR-CORE-03](../../../docs/prd.md#fr-core-03)
//!
//! **Field names are a public contract.** Every name is the schema's camelCase name. A field the
//! upstream schema requires is a plain field here, so a document missing it does not deserialise;
//! a field it marks optional is an `Option` omitted when absent, so a document with no additions
//! serialises to exactly the keys dependency-cruiser would write. Extractors build nodes through
//! [`Module::new`] and [`Dependency::new`], which fill the required flags with the values an
//! unevaluated graph has (`valid: true`, `circular: false`); the rule engine overwrites them. The round-trip test in
//! `tests/round_trip.rs` feeds dependency-cruiser's own report fixtures through these types and
//! fails on any key they lose.
//!
//! `ruleSetUsed` and `optionsUsed` are carried as JSON objects rather than typed here: the rule
//! language is `rb-config`'s model, and `rb-model` may depend on nothing in the workspace
//! ([ADR-0010](../../../docs/adr/0010-crate-layout-and-extractor-boundary.md)). The document
//! records them verbatim, which is all a reporter needs.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::code::CodeLayer;
use crate::js_number;
use crate::vocab::{
    Attribution, ChangeType, DependencyKind, DependencyType, Language, ModuleSystem, Protocol,
    Severity, ViolationType,
};

/// The graph document: the module layer, the optional folder layer, the summary, the cache's
/// revision data and the additive code layer.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphDocument {
    /// Module layer nodes, sorted by `source` before output
    /// ([FR-CORE-07](../../../docs/prd.md#fr-core-07)).
    pub modules: Vec<Module>,
    /// The folder layer, present when metrics or folder rules asked for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folders: Option<Vec<Folder>>,
    /// Counts, violations and the options the run used.
    pub summary: Summary,
    /// The cache key: the git revision and the changes since it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision_data: Option<RevisionData>,
    /// Additive: the code layer (types, members, attributes, calls). Empty until an extractor
    /// fills it ([FR-EXT-DN-03](../../../docs/prd.md#fr-ext-dn-03)).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<CodeLayer>,
}

/// One node in the module layer: `modules[]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Module {
    /// Repository-relative, posix-separated path, or the specifier for an unresolved module.
    pub source: String,
    /// Outgoing edges.
    pub dependencies: Vec<Dependency>,
    /// `true` when no rule flagged the module.
    pub valid: bool,
    /// The resolved names of the modules that depend on this one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dependents: Option<Vec<String>>,
    /// Whether the module was followed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub followable: Option<bool>,
    /// Whether the module matched `doNotFollow`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matches_do_not_follow: Option<bool>,
    /// Whether the module matched `focus`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matches_focus: Option<bool>,
    /// Whether the module matched `reaches`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matches_reaches: Option<bool>,
    /// Whether the module matched `highlight`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matches_highlight: Option<bool>,
    /// Whether the module is a runtime built-in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub core_module: Option<bool>,
    /// Whether the module could not be resolved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub could_not_resolve: Option<bool>,
    /// The dependency types of the edges that lead to this module.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dependency_types: Option<Vec<DependencyType>>,
    /// The npm licence, when `resolveLicenses` found one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    /// Whether nothing depends on the module and it depends on nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orphan: Option<bool>,
    /// Reachability verdicts per rule.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reachable: Option<Vec<Reachable>>,
    /// What the module reaches, per rule.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reaches: Option<Vec<Reaches>>,
    /// Rules the module violated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rules: Option<Vec<RuleSummary>>,
    /// Whether the module stands for a collapsed folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consolidated: Option<bool>,
    /// Efferent over total coupling.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "js_number::option"
    )]
    pub instability: Option<f64>,
    /// Size and statement counts, when `experimentalStats` is on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experimental_stats: Option<ExperimentalStats>,
    /// Content checksum, for the cache.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
    /// Additive: which extractor produced the module.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<Language>,
    /// Additive: the `.csproj` or package that owns the file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// Additive: the namespaces the file declares.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespaces: Option<Vec<String>>,
    /// Additive: how a .NET module was attributed to a file
    /// ([ADR-0011](../../../docs/adr/0011-read-dotnet-assemblies-not-source.md)).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attribution: Option<Attribution>,
}

impl Module {
    /// A valid module with a source, no edges and no optional fields: the starting point every
    /// extractor fills.
    pub fn new(source: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            dependencies: Vec::new(),
            valid: true,
            dependents: None,
            followable: None,
            matches_do_not_follow: None,
            matches_focus: None,
            matches_reaches: None,
            matches_highlight: None,
            core_module: None,
            could_not_resolve: None,
            dependency_types: None,
            license: None,
            orphan: None,
            reachable: None,
            reaches: None,
            rules: None,
            consolidated: None,
            instability: None,
            experimental_stats: None,
            checksum: None,
            language: None,
            project: None,
            namespaces: None,
            attribution: None,
        }
    }
}

/// One edge in the module layer: `modules[].dependencies[]`.
#[expect(
    clippy::struct_excessive_bools,
    reason = "the flags are dependency-cruiser's cruise-result fields, a public contract (ADR-0004)"
)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Dependency {
    /// The specifier as written (`./x`, `lodash`), without its protocol.
    pub module: String,
    /// The protocol stripped from the specifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<Protocol>,
    /// The MIME type of a `data:` specifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// The resolved, base-relative path, or the specifier when unresolved.
    pub resolved: String,
    /// Whether the target is a runtime built-in.
    pub core_module: bool,
    /// The dependency types of the edge.
    pub dependency_types: Vec<DependencyType>,
    /// The target's npm licence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    /// Whether the extractor follows the edge.
    pub followable: bool,
    /// Whether the edge is `import()`.
    pub dynamic: bool,
    /// Whether the edge came through an exotic require name.
    pub exotically_required: bool,
    /// The exotic require name used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exotic_require: Option<String>,
    /// Whether the target matched `doNotFollow`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matches_do_not_follow: Option<bool>,
    /// Whether the specifier could not be resolved.
    pub could_not_resolve: bool,
    /// Whether the edge disappears after TypeScript compilation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pre_compilation_only: Option<bool>,
    /// Whether only types cross the edge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_only: Option<bool>,
    /// Whether following the edge returns to the source.
    pub circular: bool,
    /// One cycle through the edge, when `circular`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cycle: Option<Vec<MiniDependency>>,
    /// The module system of the dependency form.
    pub module_system: ModuleSystem,
    /// `true` when no rule flagged the edge.
    pub valid: bool,
    /// Rules the edge violated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rules: Option<Vec<RuleSummary>>,
    /// The target's instability, denormalised.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "js_number::option"
    )]
    pub instability: Option<f64>,
    /// Additive: 1-based line of the import or reference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// Additive: 1-based column of the import or reference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
    /// Additive: what kind of reference formed the edge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dependency_kind: Option<DependencyKind>,
    /// Additive: the member reference that formed the edge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member: Option<String>,
    /// Additive: the edge came from the Node sidecar
    /// ([ADR-0017](../../../docs/adr/0017-coffeescript-livescript-sidecar.md)).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sidecar: Option<bool>,
    /// Additive: the edge was declared rather than detected (wave 4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared: Option<bool>,
}

impl Dependency {
    /// A valid, resolved, non-circular edge with no dependency types and no optional fields; the
    /// extractor sets what it learns.
    pub fn new(
        module: impl Into<String>,
        resolved: impl Into<String>,
        module_system: ModuleSystem,
    ) -> Self {
        Self {
            module: module.into(),
            protocol: None,
            mime_type: None,
            resolved: resolved.into(),
            core_module: false,
            dependency_types: Vec::new(),
            license: None,
            followable: false,
            dynamic: false,
            exotically_required: false,
            exotic_require: None,
            matches_do_not_follow: None,
            could_not_resolve: false,
            pre_compilation_only: None,
            type_only: None,
            circular: false,
            cycle: None,
            module_system,
            valid: true,
            rules: None,
            instability: None,
            line: None,
            column: None,
            dependency_kind: None,
            member: None,
            sidecar: None,
            declared: None,
        }
    }
}

/// A rule reference inside a module, edge or violation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuleSummary {
    /// The rule name.
    pub name: String,
    /// The rule severity.
    pub severity: Severity,
}

/// One step of a cycle or a `via` chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MiniDependency {
    /// The module name.
    pub name: String,
    /// The dependency types of the step.
    pub dependency_types: Vec<DependencyType>,
}

/// A reachability verdict for one rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Reachable {
    /// Whether the module is reachable.
    pub value: bool,
    /// The rule that asked.
    pub as_defined_in_rule: String,
    /// The module the search started from.
    pub matched_from: String,
}

/// What a module reaches, for one rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Reaches {
    /// The reached modules and the path to each.
    pub modules: Vec<ReachedModule>,
    /// The rule that asked.
    pub as_defined_in_rule: String,
}

/// One module in a [`Reaches`] list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReachedModule {
    /// The reached module.
    pub source: String,
    /// The path to it.
    pub via: Vec<MiniDependency>,
}

/// `experimentalStats` on a module or folder.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExperimentalStats {
    /// Top-level statements in the file.
    pub top_level_statement_count: u64,
    /// Size in bytes.
    pub size: u64,
}

/// One node in the folder layer: `folders[]`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Folder {
    /// Posix folder name.
    pub name: String,
    /// Folders that depend on this one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dependents: Option<Vec<FolderDependent>>,
    /// Folders this one depends on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dependencies: Option<Vec<FolderDependency>>,
    /// Modules in the folder and its sub-folders. Signed because the upstream schema types it
    /// as a plain number and dependency-cruiser's metrics use `-1` for "not counted".
    pub module_count: i64,
    /// Modules outside that depend on modules inside.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub afferent_couplings: Option<i64>,
    /// Modules inside that depend on modules outside.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub efferent_couplings: Option<i64>,
    /// Efferent over total coupling.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "js_number::option"
    )]
    pub instability: Option<f64>,
    /// Summed statistics, when `experimentalStats` is on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experimental_stats: Option<ExperimentalStats>,
}

/// A folder that depends on a [`Folder`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FolderDependent {
    /// The dependent folder.
    pub name: String,
}

/// A folder a [`Folder`] depends on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FolderDependency {
    /// The folder depended on.
    pub name: String,
    /// Its instability, denormalised.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "js_number::option"
    )]
    pub instability: Option<f64>,
    /// `true` when no rule flagged the edge.
    pub valid: bool,
    /// Whether following the edge returns to the folder.
    pub circular: bool,
    /// One cycle through the edge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cycle: Option<Vec<MiniDependency>>,
    /// Rules the edge violated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rules: Option<Vec<RuleSummary>>,
}

/// `summary`: violations, counts, the options and rule set used, the environment, plus the
/// additive receipt and vacuous-rule list.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Summary {
    /// Every violation found.
    pub violations: Vec<Violation>,
    /// Error-severity violations.
    pub error: u64,
    /// Warn-severity violations.
    pub warn: u64,
    /// Info-severity violations.
    pub info: u64,
    /// Ignore-severity violations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ignore: Option<u64>,
    /// Modules cruised.
    pub total_cruised: u64,
    /// Dependencies cruised.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_dependencies_cruised: Option<u64>,
    /// The runtime the cruise ran on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<Environment>,
    /// The rule set, verbatim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule_set_used: Option<Map<String, Value>>,
    /// The options, verbatim.
    pub options_used: Map<String, Value>,
    /// Additive: the receipt, what the run looked at per language
    /// ([design § Precision an agent can act on](../../../docs/artifacts/design.md#precision-an-agent-can-act-on)).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inspected: Option<Inspected>,
    /// Additive: rules whose selecting side matched nothing
    /// ([ADR-0007](../../../docs/adr/0007-vacuous-rules-fail-by-default.md)).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vacuous_rules: Option<Vec<VacuousRule>>,
    /// Additive: each ratchet's count against its budget
    /// ([ADR-0029](../../../docs/adr/0029-ratchets-enforced-by-cruise-and-reported-in-the-summary.md)).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ratchets: Option<Vec<RatchetResult>>,
}

/// One violation: `summary.violations[]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Violation {
    /// The violating module.
    pub from: String,
    /// The module depended on.
    pub to: String,
    /// The specifier, when `to` did not resolve.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unresolved_to: Option<String>,
    /// The edge's dependency types.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dependency_types: Option<Vec<DependencyType>>,
    /// What kind of rule was violated.
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub violation_type: Option<ViolationType>,
    /// The rule.
    pub rule: RuleSummary,
    /// The cycle, for a cycle violation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cycle: Option<Vec<MiniDependency>>,
    /// The path, for a reachability violation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub via: Option<Vec<MiniDependency>>,
    /// The two instabilities, for a `moreUnstable` violation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metrics: Option<ViolationMetrics>,
    /// The rule's comment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    /// Additive: the stable id, `RB-` plus eight hex characters
    /// ([ADR-0015](../../../docs/adr/0015-stable-violation-id.md)).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Additive: the rule's `fix` text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
    /// Additive: the decision token parsed from the comment (`adr:0003`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<String>,
}

/// `violations[].metrics`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ViolationMetrics {
    /// The `from` module's instability.
    pub from: InstabilityMetric,
    /// The `to` module's instability.
    pub to: InstabilityMetric,
}

/// One instability figure.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstabilityMetric {
    /// Efferent over total coupling.
    #[serde(serialize_with = "js_number::plain")]
    pub instability: f64,
}

/// `summary.environment`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Environment {
    /// The tool version.
    pub version: String,
    /// The runtime range supported.
    pub node_version_supported: String,
    /// The runtime found.
    pub node_version_found: String,
    /// The operating system found.
    pub os_version_found: String,
    /// Transpilers and whether each was available.
    pub transpilers_found: Vec<TranspilerFound>,
    /// Extensions and whether each was scannable.
    pub extensions_found: Vec<ExtensionFound>,
    /// Problems that may have influenced the cruise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issues: Option<Vec<EnvironmentIssue>>,
}

/// One transpiler in [`Environment`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TranspilerFound {
    /// The package name.
    pub name: String,
    /// The supported range.
    pub version: String,
    /// Whether it was found.
    pub available: bool,
    /// The version found.
    pub current_version: String,
}

/// One extension in [`Environment`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExtensionFound {
    /// The extension, with its dot.
    pub extension: String,
    /// Whether it could be scanned.
    pub available: bool,
}

/// One issue in [`Environment`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EnvironmentIssue {
    /// How much it matters.
    pub severity: Severity,
    /// A short name.
    pub name: String,
    /// What happened.
    pub description: String,
}

/// `revisionData`, the cache key. The upstream schema allows extra keys here, so they are kept.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RevisionData {
    /// The cache format version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_format_version: Option<u64>,
    /// The git revision.
    #[serde(rename = "SHA1")]
    pub sha1: String,
    /// Changes since the revision.
    pub changes: Vec<Change>,
    /// Keys the upstream schema permits but does not name.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// One entry in [`RevisionData::changes`]. Extra keys are kept, as upstream allows them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    /// The file.
    pub name: String,
    /// What happened to it.
    #[serde(rename = "type")]
    pub change_type: ChangeType,
    /// The previous name, for a rename.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_name: Option<String>,
    /// Content checksum.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
    /// The command-line arguments of the cached run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    /// The rules file of the cached run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rules_file: Option<String>,
    /// Keys the upstream schema permits but does not name.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// The receipt: per language, what a run looked at. Keyed by language so the output order is
/// fixed ([FR-CORE-07](../../../docs/prd.md#fr-core-07)).
pub type Inspected = BTreeMap<Language, Receipt>;

/// What one language's extractor inspected.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Receipt {
    /// Source files read.
    pub files: u64,
    /// Assemblies read (.NET only).
    #[serde(default)]
    pub assemblies: u64,
    /// Modules produced.
    pub modules: u64,
}

/// A rule whose selecting side matched nothing
/// ([ADR-0007](../../../docs/adr/0007-vacuous-rules-fail-by-default.md)).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VacuousRule {
    /// The rule name.
    pub name: String,
    /// The side that matched nothing: `from`, `module` or `select`.
    pub side: String,
}

/// One ratchet's result: `summary.ratchets[]`
/// ([ADR-0029](../../../docs/adr/0029-ratchets-enforced-by-cruise-and-reported-in-the-summary.md)).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RatchetResult {
    /// The ratchet name.
    pub name: String,
    /// The budget file.
    pub budget: String,
    /// Matching direct edges.
    pub count: u64,
    /// The budget's ceiling; absent when the budget cannot be read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ceiling: Option<u64>,
    /// How the count compares with the ceiling.
    pub status: RatchetStatus,
}

/// `summary.ratchets[].status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum RatchetStatus {
    /// At or under the ceiling.
    Held,
    /// Over the ceiling: one error.
    Exceeded,
    /// The budget cannot be read: the run is untrustworthy.
    NoBudget,
}

impl GraphDocument {
    /// Sorts modules, their dependencies and the violations, and recounts the summary, so two
    /// runs over the same inputs serialise byte for byte
    /// ([design § Rules an agent writes](../../../docs/artifacts/design.md#rules-an-agent-writes-held-to-the-same-bar)).
    ///
    /// Normalising is idempotent, which is what lets a cached graph be re-reported without
    /// re-extracting:
    ///
    /// ```
    /// use rb_model::{GraphDocument, Module};
    ///
    /// let mut document = GraphDocument::default();
    /// document.modules.push(Module::new("src/b.ts"));
    /// document.modules.push(Module::new("src/a.ts"));
    ///
    /// document.normalise();
    /// assert_eq!(document.modules[0].source, "src/a.ts");
    /// assert_eq!(document.summary.total_cruised, 2);
    ///
    /// let once = serde_json::to_string(&document)?;
    /// document.normalise();
    /// assert_eq!(serde_json::to_string(&document)?, once);
    /// # Ok::<(), serde_json::Error>(())
    /// ```
    pub fn normalise(&mut self) {
        for module in &mut self.modules {
            module
                .dependencies
                .sort_by(|a, b| a.resolved.cmp(&b.resolved).then(a.module.cmp(&b.module)));
        }
        self.modules.sort_by(|a, b| a.source.cmp(&b.source));
        self.summary
            .violations
            .sort_by(|a, b| (&a.from, &a.to, &a.rule.name).cmp(&(&b.from, &b.to, &b.rule.name)));
        self.summary.total_cruised = self.modules.len() as u64;
        self.summary.total_dependencies_cruised = Some(
            self.modules
                .iter()
                .map(|m| m.dependencies.len() as u64)
                .sum(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module(source: &str, deps: &[(&str, &str)]) -> Module {
        Module {
            dependencies: deps
                .iter()
                .map(|(m, r)| Dependency {
                    dependency_types: vec![DependencyType::Local],
                    followable: true,
                    line: Some(1),
                    column: Some(1),
                    dependency_kind: Some(DependencyKind::Import),
                    ..Dependency::new(*m, *r, ModuleSystem::Es6)
                })
                .collect(),
            language: Some(Language::Typescript),
            ..Module::new(source)
        }
    }

    fn violation(from: &str, to: &str, rule: &str) -> Violation {
        Violation {
            from: from.to_owned(),
            to: to.to_owned(),
            unresolved_to: None,
            dependency_types: None,
            violation_type: Some(ViolationType::Dependency),
            rule: RuleSummary {
                name: rule.to_owned(),
                severity: Severity::Error,
            },
            cycle: None,
            via: None,
            metrics: None,
            comment: None,
            id: None,
            fix: None,
            decision: None,
        }
    }

    #[test]
    fn normalise_sorts_and_counts() {
        let mut doc = GraphDocument {
            modules: vec![
                module("src/b.ts", &[("./z", "src/z.ts"), ("./a", "src/a.ts")]),
                module("src/a.ts", &[]),
            ],
            ..GraphDocument::default()
        };
        doc.summary.violations = vec![
            violation("src/b.ts", "src/z.ts", "r"),
            violation("src/b.ts", "src/a.ts", "r"),
            violation("src/a.ts", "src/z.ts", "r"),
        ];
        doc.normalise();
        assert_eq!(doc.modules[0].source, "src/a.ts");
        assert_eq!(doc.modules[1].dependencies[0].resolved, "src/a.ts");
        assert_eq!(doc.summary.total_cruised, 2);
        assert_eq!(doc.summary.total_dependencies_cruised, Some(2));
        let order: Vec<(&str, &str)> = doc
            .summary
            .violations
            .iter()
            .map(|v| (v.from.as_str(), v.to.as_str()))
            .collect();
        assert_eq!(
            order,
            [
                ("src/a.ts", "src/z.ts"),
                ("src/b.ts", "src/a.ts"),
                ("src/b.ts", "src/z.ts")
            ]
        );
    }

    #[test]
    fn dependencies_with_the_same_target_sort_by_specifier() {
        let mut doc = GraphDocument {
            modules: vec![module("a.ts", &[("./z", "t.ts"), ("./b", "t.ts")])],
            ..GraphDocument::default()
        };
        doc.normalise();
        assert_eq!(doc.modules[0].dependencies[0].module, "./b");
    }

    #[test]
    fn optional_and_additive_fields_are_omitted_when_absent() {
        let json = serde_json::to_string(&Module::new("x.py")).unwrap_or_default();
        assert_eq!(json, r#"{"source":"x.py","dependencies":[],"valid":true}"#);
        let json = serde_json::to_string(&Dependency::new("./a", "a.ts", ModuleSystem::Cjs))
            .unwrap_or_default();
        assert_eq!(
            json,
            r#"{"module":"./a","resolved":"a.ts","coreModule":false,"dependencyTypes":[],"followable":false,"dynamic":false,"exoticallyRequired":false,"couldNotResolve":false,"circular":false,"moduleSystem":"cjs","valid":true}"#
        );
    }

    #[test]
    fn fields_the_upstream_schema_requires_are_required() {
        let missing_valid = r#"{"source":"a","dependencies":[]}"#;
        let error = serde_json::from_str::<Module>(missing_valid)
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default();
        assert!(error.contains("valid"), "{error}");
        let missing_circular = r#"{"module":"./a","resolved":"a.ts","coreModule":false,"dependencyTypes":[],"followable":false,"dynamic":false,"exoticallyRequired":false,"couldNotResolve":false,"moduleSystem":"cjs","valid":true}"#;
        let error = serde_json::from_str::<Dependency>(missing_circular)
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default();
        assert!(error.contains("circular"), "{error}");
        let summary = r#"{"violations":[],"error":0,"warn":0,"info":0,"totalCruised":0}"#;
        let error = serde_json::from_str::<Summary>(summary)
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default();
        assert!(error.contains("optionsUsed"), "{error}");
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let error = serde_json::from_str::<Module>(
            r#"{"source":"a","dependencies":[],"valid":true,"bogus":1}"#,
        )
        .err()
        .map(|e| e.to_string())
        .unwrap_or_default();
        assert!(error.contains("bogus"), "{error}");
    }

    #[test]
    fn integral_instability_prints_as_javascript_would() {
        let module = Module {
            instability: Some(1.0),
            ..Module::new("a")
        };
        let json = serde_json::to_string(&module).unwrap_or_default();
        assert!(json.contains(r#""instability":1}"#), "{json}");
        let module = Module {
            instability: Some(0.25),
            ..Module::new("a")
        };
        let json = serde_json::to_string(&module).unwrap_or_default();
        assert!(json.contains(r#""instability":0.25}"#), "{json}");
    }

    #[test]
    fn revision_data_keeps_extra_keys() {
        let text =
            r#"{"SHA1":"abc","changes":[{"name":"a.js","type":"modified","x":1}],"extra":true}"#;
        let data: Option<RevisionData> = serde_json::from_str(text).ok();
        let data = data.unwrap_or_default();
        assert_eq!(data.sha1, "abc");
        assert_eq!(data.changes[0].change_type, ChangeType::Modified);
        assert_eq!(data.extra.get("extra"), Some(&Value::Bool(true)));
        let back = serde_json::to_value(&data).unwrap_or_default();
        let original: Value = serde_json::from_str(text).unwrap_or_default();
        assert_eq!(back, original);
    }

    #[test]
    fn receipt_is_keyed_by_language_in_a_fixed_order() {
        let mut inspected = Inspected::new();
        inspected.insert(
            Language::Python,
            Receipt {
                files: 1,
                assemblies: 0,
                modules: 1,
            },
        );
        inspected.insert(
            Language::Dotnet,
            Receipt {
                files: 0,
                assemblies: 2,
                modules: 5,
            },
        );
        let json = serde_json::to_string(&inspected).unwrap_or_default();
        assert_eq!(
            json,
            r#"{"dotnet":{"files":0,"assemblies":2,"modules":5},"python":{"files":1,"assemblies":0,"modules":1}}"#
        );
    }

    #[test]
    fn serialising_twice_gives_the_same_bytes() {
        // A local run and a CI run must agree byte for byte, so nothing that reaches output may
        // depend on insertion or hash order. Two orderings of the same graph serialise the same.
        let mut first = GraphDocument::default();
        first.modules.push(module(
            "src/b.ts",
            &[("./z", "src/z.ts"), ("./a", "src/a.ts")],
        ));
        first.modules.push(module("src/a.ts", &[]));
        first.normalise();

        let mut second = GraphDocument::default();
        second.modules.push(module("src/a.ts", &[]));
        second.modules.push(module(
            "src/b.ts",
            &[("./a", "src/a.ts"), ("./z", "src/z.ts")],
        ));
        second.normalise();

        let left = serde_json::to_vec(&first).unwrap_or_default();
        let right = serde_json::to_vec(&second).unwrap_or_default();
        assert_eq!(
            left, right,
            "two orderings of the same graph serialised differently"
        );

        let mut again = first.clone();
        again.normalise();
        assert_eq!(serde_json::to_vec(&again).unwrap_or_default(), left);
    }

    #[test]
    fn round_trips_through_json() {
        let mut doc = GraphDocument::default();
        doc.modules.push(module("a.ts", &[("./b", "b.ts")]));
        doc.normalise();
        let json = serde_json::to_string(&doc).unwrap_or_default();
        let back: GraphDocument = serde_json::from_str(&json).unwrap_or_default();
        assert_eq!(back, doc);
    }
}
