//! The graph document's JSON schema, generated from the types so it cannot drift from them.
//!
//! - Decision: [ADR-0004](../../../docs/adr/0004-graph-document-is-cruise-result-superset.md)
//! - Plan: [Wave 0, Step 3](../../../docs/plans/pending/0000-wave-0-spike.md#step-3-rb-model-graph-document-schema-violation-id-0a)
//!   item 6 (`schema/v1.json`, the `schema-check` job)
//! - Requirement: [FR-CORE-03](../../../docs/prd.md#fr-core-03)
//!
//! `cargo run -p rb-model --example emit-schema` writes [`render`]'s output to `schema/v1.json`;
//! the `schema-check` CI job runs it and fails on a diff, and a unit test compares the committed
//! file with the generated one so `cargo test` catches a stale schema too.

use schemars::generate::SchemaSettings;

use crate::document::GraphDocument;

/// The `$id` the schema is published under, the URL the design's example config names
/// ([design § The native format](../../../docs/artifacts/design.md#the-native-format)).
pub const ID: &str = "https://benbahrenburg.github.io/rulebearing/schema/v1.json";

/// The committed schema's path, relative to the repository root.
pub const PATH: &str = "schema/v1.json";

/// Generates the schema as pretty-printed JSON with a trailing newline.
///
/// ```
/// let text = rb_model::schema::render();
/// assert!(text.contains(rb_model::schema::ID));
/// assert!(text.ends_with("}\n"));
/// ```
pub fn render() -> String {
    let generator = SchemaSettings::draft07().into_generator();
    let mut schema = generator.into_root_schema_for::<GraphDocument>();
    schema.insert("$id".to_owned(), serde_json::Value::from(ID));
    let mut text = serde_json::to_string_pretty(&schema).unwrap_or_default();
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn committed_schema_matches_the_types() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(PATH);
        let committed = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(
            committed.replace("\r\n", "\n") == render(),
            "schema/v1.json is stale; run `cargo run -p rb-model --example emit-schema`"
        );
    }

    #[test]
    fn schema_names_every_top_level_key_and_requires_the_core() {
        let schema: serde_json::Value = serde_json::from_str(&render()).unwrap_or_default();
        assert_eq!(schema["$id"], ID);
        assert_eq!(schema["$schema"], "http://json-schema.org/draft-07/schema#");
        let properties = schema["properties"]
            .as_object()
            .cloned()
            .unwrap_or_default();
        let keys: Vec<&str> = properties.keys().map(String::as_str).collect();
        for key in ["modules", "folders", "summary", "revisionData", "code"] {
            assert!(keys.contains(&key), "{key} missing from {keys:?}");
        }
        let required = schema["required"].as_array().cloned().unwrap_or_default();
        assert!(required.contains(&serde_json::Value::from("modules")));
        assert!(required.contains(&serde_json::Value::from("summary")));
        assert!(!required.contains(&serde_json::Value::from("code")));
    }
}
