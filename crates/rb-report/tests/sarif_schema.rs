//! The `sarif` reporter's output validates against the OASIS SARIF 2.1.0 schema, vendored in
//! `tests/schemas/` (provenance and licence in `tests/schemas/NOTICE`); no test reads the network.
//!
//! - Plan: [Wave 2, Step 10](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#210-step-10-reporters-and-baseline-semantics-2e)
//!   (schema validation of `sarif`)
//! - Contract: [Wave 2 plan § 1.5](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#15-interfaces-and-contracts-this-wave-freezes)
//! - Requirement: [FR-OUT-02](../../../docs/prd.md#fr-out-02)
//!
//! The inputs are every dependency-cruiser `test/report` result the conformance fixtures hold
//! (valid against the cruise-result schema or not, because a reporter must write valid SARIF for
//! whatever it is given) and a Rulebearing result with element and slice violations, a known
//! violation, a vacuous rule and an expired entry.

use std::error::Error;
use std::path::PathBuf;

use serde_json::{Value, json};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn validator() -> Result<jsonschema::Validator, Box<dyn Error>> {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/schemas/sarif-schema-2.1.0.json");
    let schema: Value = serde_json::from_str(&std::fs::read_to_string(path)?)?;
    Ok(jsonschema::draft4::new(&schema)?)
}

fn errors(validator: &jsonschema::Validator, value: &Value) -> Vec<String> {
    validator
        .iter_errors(value)
        .take(3)
        .map(|e| format!("{}: {e}", e.instance_path()))
        .collect()
}

fn sarif(result: &Value) -> Result<Value, Box<dyn Error>> {
    let rendered = rb_report::render("sarif", result, &rb_report::ReportOptions::default())?;
    Ok(serde_json::from_str(&rendered.output)?)
}

/// Every result the gate 1 fixtures hold.
fn upstream_results() -> Result<Vec<(String, Value)>, Box<dyn Error>> {
    let fixtures = root().join("conformance/dependency-cruiser/fixtures");
    let index: Value = serde_json::from_str(&std::fs::read_to_string(
        fixtures.join("report-json/INDEX.json"),
    )?)?;
    let mut files: Vec<String> = Vec::new();
    for entry in index["valid"].as_array().into_iter().flatten() {
        files.extend(entry.as_str().map(str::to_owned));
    }
    for entry in index["invalid"].as_array().into_iter().flatten() {
        files.extend(entry["file"].as_str().map(str::to_owned));
    }
    let mut out = Vec::new();
    for file in files {
        let value: Value = serde_json::from_str(&std::fs::read_to_string(fixtures.join(&file))?)?;
        out.push((file, value));
    }
    Ok(out)
}

fn rulebearing_result() -> Value {
    json!({
        "modules": [{ "source": "src/a.ts", "dependencies": [{ "resolved": "src/b.ts", "line": 4, "column": 1 }] }],
        "code": { "types": [{ "fullName": "S.A", "name": "A", "kind": "class", "language": "dotnet", "file": "src/A.cs", "line": 12, "column": 5 }] },
        "summary": {
            "violations": [
                { "type": "dependency", "from": "src/a.ts", "to": "src/b.ts", "rule": { "name": "no-b", "severity": "error" }, "id": "RB-4f2a9c1e", "fix": "Go through the index." },
                { "type": "element", "from": "src/A.cs", "to": "S.A", "rule": { "name": "sealed", "severity": "warn" }, "id": "RB-00000002" },
                { "type": "element", "from": "", "to": "", "rule": { "name": "exists", "severity": "info" }, "id": "RB-00000003" },
                { "type": "slice", "from": "Slice1", "to": "Slice2", "rule": { "name": "apart", "severity": "error" }, "id": "RB-00000004",
                  "via": [{ "name": "Slice1.A -> Slice2.B", "dependencyTypes": [] }] },
                { "type": "dependency", "from": "src/c.ts", "to": "src/b.ts", "rule": { "name": "no-b", "severity": "ignore" }, "id": "RB-00000005" }
            ],
            "error": 2, "warn": 1, "info": 1, "ignore": 1, "totalCruised": 3,
            "ruleSetUsed": {
                "forbidden": [{ "name": "no-b", "severity": "error", "comment": "b is private. adr:0003", "fix": "Go through the index." }],
                "elements": [{ "name": "sealed", "severity": "warn" }, { "name": "exists", "severity": "info" }],
                "slices": [{ "name": "apart", "severity": "error" }]
            },
            "vacuousRules": [{ "name": "dead", "side": "from" }],
            "expired": [{ "name": "RB-9", "expires": "2026-01-01", "kind": "knownViolation" }]
        }
    })
}

#[test]
fn every_sarif_log_validates_against_the_2_1_0_schema() -> Result<(), Box<dyn Error>> {
    let validator = validator()?;
    let mut inputs = upstream_results()?;
    assert!(inputs.len() > 100, "the gate 1 fixtures are present");
    inputs.push(("rulebearing".into(), rulebearing_result()));
    let mut failures = Vec::new();
    let mut results = 0;
    for (name, input) in &inputs {
        let log = sarif(input)?;
        results += log["runs"][0]["results"].as_array().map_or(0, Vec::len);
        let found = errors(&validator, &log);
        if !found.is_empty() {
            failures.push(format!("{name}: {}", found.join("; ")));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(results > 50, "the fixtures carry violations: {results}");
    Ok(())
}

#[test]
fn the_schema_rejects_what_it_should() -> Result<(), Box<dyn Error>> {
    // The validator is live: a log without `runs`, or with a result whose level is not SARIF's,
    // fails.
    let validator = validator()?;
    assert!(!validator.is_valid(&json!({ "version": "2.1.0" })));
    let mut log = sarif(&rulebearing_result())?;
    assert!(validator.is_valid(&log));
    log["runs"][0]["results"][0]["level"] = json!("warn");
    assert!(!validator.is_valid(&log));
    Ok(())
}

#[test]
fn fingerprints_are_the_stable_ids_and_do_not_move_between_runs() -> Result<(), Box<dyn Error>> {
    let first = sarif(&rulebearing_result())?;
    let second = sarif(&rulebearing_result())?;
    assert_eq!(first, second);
    let fingerprints: Vec<&str> = first["runs"][0]["results"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| r["partialFingerprints"]["rulebearing/v1"].as_str())
        .collect();
    assert_eq!(
        fingerprints,
        [
            "RB-4f2a9c1e",
            "RB-00000002",
            "RB-00000003",
            "RB-00000004",
            "RB-00000005"
        ]
    );
    let element = &first["runs"][0]["results"][1];
    assert_eq!(
        element["locations"][0]["physicalLocation"]["region"],
        json!({ "startLine": 12, "startColumn": 5 })
    );
    Ok(())
}
