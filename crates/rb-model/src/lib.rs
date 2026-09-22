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

pub mod violation_id;

use serde::{Deserialize, Serialize};

/// The language an extractor attributes a module to.
///
/// Carried as a string in the document so that a reporter or a rule never has to `match` on it
/// ([ADR-0010](../../../docs/adr/0010-crate-layout-and-extractor-boundary.md)).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    /// `.ts`, `.tsx`, `.mts`, `.cts`, `.d.ts`
    Typescript,
    /// `.js`, `.mjs`, `.cjs`, `.jsx`
    Javascript,
    /// Types read from built assemblies and portable PDBs
    Dotnet,
    /// `.py`
    Python,
}

/// How a .NET type was attributed to a source file
/// ([ADR-0011](../../../docs/adr/0011-read-dotnet-assemblies-not-source.md)).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Attribution {
    /// From the portable PDB's `Document` and `MethodDebugInformation` tables.
    Pdb,
    /// By naming convention, because the type has no methods and no PDB row.
    Inferred,
    /// No attribution; path-based rules skip the type with a warning.
    None,
}

/// One edge in the module layer: `modules[].dependencies[]` in the `cruise-result` schema,
/// plus the additive `line`, `column`, `dependency_kind` and `member` fields.
///
/// Only the fields wave 0 needs are present; the plan for wave 1 completes the schema
/// ([FR-CORE-03](../../../docs/prd.md#fr-core-03)).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Dependency {
    /// The specifier as written in the source (`./x`, `lodash`, `System.Net.Http`).
    pub module: String,
    /// The resolved, repository-relative path or the unresolved specifier.
    pub resolved: String,
    /// dependency-cruiser's `dependencyTypes` vocabulary, or the per-language additions.
    #[serde(default)]
    pub dependency_types: Vec<String>,
    /// Whether the resolver could not resolve the specifier.
    #[serde(default)]
    pub could_not_resolve: bool,
    /// Additive: 1-based line of the import or reference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// Additive: 1-based column of the import or reference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
    /// Additive: `import`, `inherits`, `implements`, `field`, `signature`, `body`, `attribute`,
    /// `generic-argument`, `typeof`, `call`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dependency_kind: Option<String>,
    /// Additive: the member reference that formed the edge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member: Option<String>,
}

/// One node in the module layer: `modules[]` in the `cruise-result` schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Module {
    /// Repository-relative path.
    pub source: String,
    /// Outgoing edges.
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
    /// Additive: which extractor produced the module.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<Language>,
    /// Additive: how a .NET module was attributed to a file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attribution: Option<Attribution>,
}

/// The receipt: what a run inspected, per language
/// ([design § Precision an agent can act on](../../../docs/artifacts/design.md#precision-an-agent-can-act-on)).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Inspected {
    /// Source files read.
    pub files: u64,
    /// Assemblies read (.NET only).
    pub assemblies: u64,
    /// Modules produced.
    pub modules: u64,
}

/// `summary` in the `cruise-result` schema, wave 0 subset plus the additive receipt.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    /// Number of modules cruised.
    pub total_cruised: u64,
    /// Number of dependencies cruised.
    pub total_dependencies_cruised: u64,
    /// Additive: the receipt.
    #[serde(default)]
    pub inspected: Inspected,
    /// Additive: names of rules whose selection was empty
    /// ([ADR-0007](../../../docs/adr/0007-vacuous-rules-fail-by-default.md)).
    #[serde(default)]
    pub vacuous_rules: Vec<String>,
}

/// The graph document. Wave 0 carries the module layer skeleton; the `code` section arrives in
/// wave 2 ([FR-EXT-DN-03](../../../docs/prd.md#fr-ext-dn-03)).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphDocument {
    /// Module layer nodes, sorted by `source` before output for determinism
    /// ([FR-CORE-07](../../../docs/prd.md#fr-core-07)).
    pub modules: Vec<Module>,
    /// Counts and the receipt.
    pub summary: Summary,
}

impl GraphDocument {
    /// Sorts modules and their dependencies so two runs over the same inputs serialise
    /// byte for byte ([design § Rules an agent writes](../../../docs/artifacts/design.md#rules-an-agent-writes-held-to-the-same-bar)).
    ///
    /// Normalising is idempotent, which is what lets a cached graph be re-reported without
    /// re-extracting:
    ///
    /// ```
    /// use rb_model::{Dependency, GraphDocument, Module};
    ///
    /// let mut document = GraphDocument::default();
    /// document.modules.push(Module {
    ///     source: "src/b.ts".to_owned(),
    ///     dependencies: vec![],
    ///     language: None,
    ///     attribution: None,
    /// });
    /// document.modules.push(Module {
    ///     source: "src/a.ts".to_owned(),
    ///     dependencies: vec![],
    ///     language: None,
    ///     attribution: None,
    /// });
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
        self.summary.total_cruised = self.modules.len() as u64;
        self.summary.total_dependencies_cruised = self
            .modules
            .iter()
            .map(|m| m.dependencies.len() as u64)
            .sum();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module(source: &str, deps: &[(&str, &str)]) -> Module {
        Module {
            source: source.to_owned(),
            dependencies: deps
                .iter()
                .map(|(m, r)| Dependency {
                    module: (*m).to_owned(),
                    resolved: (*r).to_owned(),
                    dependency_types: vec!["local".to_owned()],
                    could_not_resolve: false,
                    line: Some(1),
                    column: Some(1),
                    dependency_kind: Some("import".to_owned()),
                    member: None,
                })
                .collect(),
            language: Some(Language::Typescript),
            attribution: None,
        }
    }

    #[test]
    fn normalise_sorts_and_counts() {
        let mut doc = GraphDocument {
            modules: vec![
                module("src/b.ts", &[("./z", "src/z.ts"), ("./a", "src/a.ts")]),
                module("src/a.ts", &[]),
            ],
            summary: Summary::default(),
        };
        doc.normalise();
        assert_eq!(doc.modules[0].source, "src/a.ts");
        assert_eq!(doc.modules[1].dependencies[0].resolved, "src/a.ts");
        assert_eq!(doc.summary.total_cruised, 2);
        assert_eq!(doc.summary.total_dependencies_cruised, 2);
    }

    #[test]
    fn additive_fields_are_omitted_when_absent() {
        let m = Module {
            source: "x.py".to_owned(),
            dependencies: vec![],
            language: None,
            attribution: None,
        };
        let json = serde_json::to_string(&m).unwrap_or_default();
        assert!(!json.contains("language"));
        assert!(!json.contains("attribution"));
        assert!(json.contains("\"source\":\"x.py\""));
    }

    #[test]
    fn language_and_attribution_serialise_lowercase() {
        let json = serde_json::to_string(&Language::Dotnet).unwrap_or_default();
        assert_eq!(json, "\"dotnet\"");
        let json = serde_json::to_string(&Attribution::Inferred).unwrap_or_default();
        assert_eq!(json, "\"inferred\"");
    }

    #[test]
    fn serialising_twice_gives_the_same_bytes() {
        // A local run and a CI run must agree byte for byte, so nothing that reaches output may
        // depend on hash iteration order (docs/adr/0023 is the gate, the promise is the design's
        // "hermetic runs"). Building the same document from a different insertion order must
        // produce the same JSON.
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

        // Normalising again changes nothing.
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
