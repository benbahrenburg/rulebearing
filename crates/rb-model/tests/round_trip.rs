//! dependency-cruiser's own cruise results, read into the `rb-model` types and written back.
//!
//! - Contract: [ADR-0004](../../../docs/adr/0004-graph-document-is-cruise-result-superset.md)
//!   ("the module layer is dependency-cruiser's `cruise-result` schema, unchanged")
//! - Plan: [Wave 0, Step 3](../../../docs/plans/pending/0000-wave-0-spike.md#step-3-rb-model-graph-document-schema-violation-id-0a)
//!   item 7; quality attribute "Compatibility" in § 1.7
//! - Requirement: [FR-CORE-03](../../../docs/prd.md#fr-core-03)
//! - Fixtures: every cruise result in dependency-cruiser 18.2.0's `test/report`, vendored by
//!   `conformance/dependency-cruiser/scripts/vendor.sh` and listed in `fixtures/report-json/INDEX.json`
//!
//! The index splits the fixtures by whether dependency-cruiser's own schema accepts them. The ones
//! it accepts are what 18.2.0 can write, and every one must round-trip. The rest are hand-written
//! mocks, several deliberately malformed to test a reporter's robustness; they are listed with the
//! schema's reason, and the second test below proves the types reject at least those that break
//! a closed vocabulary or a required field, rather than accepting them silently.
//!
//! A field the fixtures carry that the types lack fails deserialisation (the types deny unknown
//! fields), and a field the types drop or reshape fails the comparison. The comparison is after
//! key sorting, byte for byte, so number formatting is checked too.

use std::error::Error;
use std::path::{Path, PathBuf};

use rb_model::GraphDocument;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance/dependency-cruiser/fixtures")
}

#[derive(serde::Deserialize)]
struct Index {
    valid: Vec<String>,
    invalid: Vec<Invalid>,
}

#[derive(serde::Deserialize)]
struct Invalid {
    file: String,
    reason: String,
}

fn index() -> Result<Index, Box<dyn Error>> {
    let text = std::fs::read_to_string(fixtures().join("report-json/INDEX.json"))?;
    Ok(serde_json::from_str(&text)?)
}

/// Serialises a value with every object's keys sorted, whatever map order the build uses.
fn canonical(value: &serde_json::Value) -> String {
    let mut sorted = value.clone();
    rb_model::schema::sort_keys(&mut sorted);
    serde_json::to_string(&sorted).unwrap_or_default()
}

#[test]
fn every_upstream_cruise_result_round_trips_without_loss() -> Result<(), Box<dyn Error>> {
    let files = index()?.valid;
    assert!(
        files.len() >= 30,
        "expected the vendored report fixtures; run conformance/dependency-cruiser/scripts/vendor.sh"
    );
    let mut failures = Vec::new();
    for file in &files {
        let text = std::fs::read_to_string(fixtures().join(file))?;
        let original: serde_json::Value = serde_json::from_str(&text)?;
        let document: GraphDocument = match serde_json::from_value(original.clone()) {
            Ok(document) => document,
            Err(error) => {
                failures.push(format!("{file}: does not deserialise: {error}"));
                continue;
            }
        };
        let written = serde_json::to_value(&document)?;
        if canonical(&written) != canonical(&original) {
            failures.push(format!("{file}: re-serialised differently"));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} fixtures lost data:\n{}",
        failures.len(),
        files.len(),
        failures.join("\n")
    );
    Ok(())
}

#[test]
fn mocks_the_upstream_schema_rejects_are_listed_with_a_reason() -> Result<(), Box<dyn Error>> {
    let invalid = index()?.invalid;
    let mut accepted_anyway = Vec::new();
    for entry in &invalid {
        assert!(!entry.reason.is_empty(), "{} has no reason", entry.file);
        let text = std::fs::read_to_string(fixtures().join(&entry.file))?;
        if serde_json::from_str::<GraphDocument>(&text).is_ok() {
            accepted_anyway.push(format!("{} ({})", entry.file, entry.reason));
        }
    }
    // The types may be no looser than the upstream schema on anything they model strictly. Two
    // leniencies are deliberate, and each mock they let through is listed so a new one is reviewed:
    // `optionsUsed` is carried verbatim as a JSON object (the option model is rb-config's, ADR-0010),
    // and an explicit `null` on an optional field reads as absent.
    let known = [
        "report-json/azure-devops/__mocks__/everything-fine.json (/summary/optionsUsed/",
        "report-json/teamcity/__mocks__/everything-fine.json (/summary/optionsUsed/",
        "report/mermaid/__mocks__/collapsed.json (/summary/optionsUsed/",
        "report/baseline/__mocks__/dc-result-with-violations.json (/summary/ignore must be number)",
    ];
    let unexpected: Vec<&String> = accepted_anyway
        .iter()
        .filter(|entry| !known.iter().any(|k| entry.starts_with(k)))
        .collect();
    assert!(
        unexpected.is_empty(),
        "the types accept mocks the upstream schema rejects:\n{unexpected:#?}"
    );
    Ok(())
}

#[test]
fn the_upstream_schema_names_no_field_the_types_lack() -> Result<(), Box<dyn Error>> {
    // A second, independent check: every property the pinned 18.2.0 schema defines for these
    // shapes is a key the generated schema also defines.
    let upstream: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(
        fixtures().join("schemas/cruise-result.schema.json"),
    )?)?;
    let ours: serde_json::Value = serde_json::from_str(&rb_model::schema::render())?;
    for (upstream_name, our_name) in [
        ("ModuleType", "Module"),
        ("DependencyType", "Dependency"),
        ("SummaryType", "Summary"),
        ("FolderType", "Folder"),
        ("ViolationType", "Violation"),
    ] {
        let theirs = upstream["definitions"][upstream_name]["properties"]
            .as_object()
            .cloned()
            .unwrap_or_default();
        let mine = ours["definitions"][our_name]["properties"]
            .as_object()
            .cloned()
            .unwrap_or_default();
        assert!(!theirs.is_empty(), "{upstream_name} missing upstream");
        let missing: Vec<&String> = theirs.keys().filter(|k| !mine.contains_key(*k)).collect();
        assert!(
            missing.is_empty(),
            "{our_name} lacks upstream fields {missing:?}"
        );
    }
    Ok(())
}
