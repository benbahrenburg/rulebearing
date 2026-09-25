//! `rb-ingest`: dependency-cruiser JSON, ArchUnitNET-style JSON and the C# fallback extractor's
//! output in; the graph document out. The migration bridge.
//!
//! - Architecture: [`docs/architecture.md#crate-layout`](../../../docs/architecture.md#crate-layout)
//! - Decisions: [ADR-0004](../../../docs/adr/0004-graph-document-is-cruise-result-superset.md),
//!   [ADR-0003](../../../docs/adr/0003-dotnet-extractor-fallback.md)
//! - Plans: [Wave 1](../../../docs/plans/pending/0001-wave-1-typescript-parity.md) (`fmt --from dependency-cruiser`),
//!   [Wave 2](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md) (importers)
//! - Requirements: [FR-CLI-01](../../../docs/prd.md#fr-cli-01), [FR-CLI-04](../../../docs/prd.md#fr-cli-04)
//!
//! | Module | Reads |
//! | --- | --- |
//! | [`dependency_cruiser`] | a result dependency-cruiser wrote (`fmt --from dependency-cruiser`) |

pub mod dependency_cruiser;

use rb_model::GraphDocument;

/// Reads a `cruise-result` JSON produced by dependency-cruiser or Rulebearing. Because the module
/// layer is the same schema ([ADR-0004](../../../docs/adr/0004-graph-document-is-cruise-result-superset.md)),
/// no translation is needed for the fields wave 0 models.
///
/// # Errors
/// Returns the serde error when the text is not a valid document.
pub fn read_cruise_result(json: &str) -> Result<GraphDocument, serde_json::Error> {
    let mut doc: GraphDocument = serde_json::from_str(json)?;
    doc.normalise();
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_dependency_cruiser_shaped_document() {
        let json = r#"{
          "modules": [
            {"source": "src/b.ts", "valid": true, "dependencies": [{
              "module": "./a", "resolved": "src/a.ts", "dependencyTypes": ["local"],
              "couldNotResolve": false, "circular": false, "coreModule": false,
              "exoticallyRequired": false, "dynamic": false, "followable": true,
              "moduleSystem": "es6", "valid": true
            }]},
            {"source": "src/a.ts", "valid": true, "dependencies": []}
          ],
          "summary": {"violations": [], "error": 0, "warn": 0, "info": 0,
                      "totalCruised": 0, "totalDependenciesCruised": 0, "optionsUsed": {}}
        }"#;
        let doc = read_cruise_result(json).unwrap_or_default();
        assert_eq!(doc.modules.len(), 2);
        assert_eq!(doc.modules[0].source, "src/a.ts");
        assert_eq!(doc.summary.total_cruised, 2);
        assert_eq!(doc.summary.total_dependencies_cruised, Some(1));
    }

    #[test]
    fn rejects_garbage() {
        assert!(read_cruise_result("not json").is_err());
    }
}
