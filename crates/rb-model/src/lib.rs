//! `rb-model`: the graph document that every other crate reads or writes.
//!
//! The module layer is dependency-cruiser 18.2.0's `cruise-result` schema, unchanged; the code
//! layer and every other field here are additive. See:
//!
//! - Architecture: [`docs/architecture.md#the-graph-document`](../../../docs/architecture.md#the-graph-document)
//! - Decision: [ADR-0004](../../../docs/adr/0004-graph-document-is-cruise-result-superset.md),
//!   [ADR-0015](../../../docs/adr/0015-stable-violation-id.md)
//! - Plan: [Wave 0, sub-wave 0A](../../../docs/plans/pending/0000-wave-0-spike.md)
//! - Requirements: [FR-CORE-03](../../../docs/prd.md#fr-core-03), [FR-CORE-04](../../../docs/prd.md#fr-core-04)
//! - Source: [design § The five stages](../../../docs/artifacts/design.md#the-five-stages), stage 3
//!
//! This crate depends on nothing inside the workspace. It must not learn which language a node
//! came from beyond carrying the `language` string.
//!
//! | Module | Holds |
//! | --- | --- |
//! | [`document`] | the module layer: `modules`, `folders`, `summary`, `revisionData` |
//! | [`code`] | the additive code layer: types, members, attributes, calls |
//! | [`collate`] | JavaScript's `localeCompare` order, which dependency-cruiser sorts with |
//! | [`vocab`] | the closed string vocabularies, one declaration each |
//! | [`options`] | the per-language options an extractor receives |
//! | [`extract`] | the [`Extractor`] trait and its errors |
//! | [`violation_id`] | the stable violation id |
//! | [`schema`] | the generated JSON schema for `schema/v1.json` |

pub mod code;
pub mod collate;
pub mod document;
pub mod extract;
pub mod js_number;
pub mod options;
pub mod schema;
pub mod violation_id;
pub mod vocab;

pub use code::{AttributeElement, CallElement, CodeLayer, Location, MemberElement, TypeElement};
pub use document::{
    Change, Dependency, Environment, EnvironmentIssue, ExperimentalStats, ExpiredEntry,
    ExtensionFound, Folder, FolderDependency, FolderDependent, GraphDocument, Inspected,
    InstabilityMetric, MiniDependency, Module, RatchetResult, RatchetStatus, Reachable,
    ReachedModule, Reaches, Receipt, RevisionData, RuleSummary, Summary, TranspilerFound,
    VacuousRule, Violation, ViolationMetrics,
};
pub use extract::{ExtractError, Extraction, Extractor, Warning};
pub use options::{DotnetOptions, PythonOptions, TypeScriptOptions};
pub use vocab::{
    Attribution, ChangeType, DependencyKind, DependencyType, ExternalModuleResolutionStrategy,
    Language, ModuleSystem, Parser, Protocol, Severity, UnknownValue, ViolationType,
};
