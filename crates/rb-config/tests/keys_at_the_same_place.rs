//! Every dependency-cruiser key is legal in a native file at the same place, with the same
//! meaning ([ADR-0005](../../../docs/adr/0005-native-config-superset-and-compat.md)).
//!
//! - Plan: [Wave 1, Step 1](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-1-config-model-and-the-two-front-ends-1a)
//!   ("a table test that every dependency-cruiser key is legal in a native file at the same path")
//! - Requirements: [FR-CFG-02](../../../docs/prd.md#fr-cfg-02), [FR-CFG-06](../../../docs/prd.md#fr-cfg-06)
//!
//! The table is dependency-cruiser 18.2.0's own configuration schema, vendored under
//! `conformance/dependency-cruiser/fixtures/schemas/`: every top-level key, every option key and
//! every restriction key it defines must load in a native file and give the same rules and
//! options as the same file loaded as dependency-cruiser's format.

use std::error::Error;
use std::path::PathBuf;

use rb_config::normalize::OPTION_KEYS;
use rb_config::read::Syntax;
use rb_config::{ConfigFormat, LoadOptions, load_text};
use serde_json::Value;

fn schema() -> Result<Value, Box<dyn Error>> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../conformance/dependency-cruiser/fixtures/schemas/configuration.schema.json");
    Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
}

fn keys(schema: &Value, definition: &str) -> Vec<String> {
    schema["definitions"][definition]["properties"]
        .as_object()
        .map(|p| p.keys().cloned().collect())
        .unwrap_or_default()
}

#[test]
fn every_option_key_of_the_schema_is_known() -> Result<(), Box<dyn Error>> {
    let schema = schema()?;
    for key in keys(&schema, "OptionsType") {
        assert!(OPTION_KEYS.contains(&key.as_str()), "options.{key}");
    }
    Ok(())
}

#[test]
fn both_formats_load_the_same_rules() -> Result<(), Box<dyn Error>> {
    let schema = schema()?;
    for definition in ["FromRestrictionType", "ToRestrictionType"] {
        assert!(!keys(&schema, definition).is_empty(), "{definition}");
    }
    let dc = r#"{
      "forbidden": [{ "name": "a", "severity": "error", "comment": "c", "scope": "module",
        "from": { "path": "^a", "pathNot": "^b" },
        "to": { "path": "^c", "pathNot": "^d", "couldNotResolve": false, "circular": false, "dynamic": false,
                "exoticallyRequired": false, "exoticRequire": "x", "exoticRequireNot": "y", "preCompilationOnly": false,
                "dependencyTypes": ["local"], "dependencyTypesNot": ["npm"], "moreThanOneDependencyType": false,
                "via": { "path": "v" }, "viaOnly": { "pathNot": "w" }, "ancestor": false } },
        { "name": "orphans", "from": { "orphan": true }, "to": {} },
        { "name": "dependents", "module": { "path": "^m", "numberOfDependentsLessThan": 2 }, "from": { "path": "^n" } },
        { "name": "reach", "from": { "path": "^r" }, "to": { "path": "^s", "reachable": false } }],
      "allowed": [{ "from": { "path": "^a" }, "to": { "path": "^b" } }],
      "allowedSeverity": "error",
      "required": [{ "name": "req", "module": { "path": "^m" }, "to": { "path": "^t", "reachable": true } }],
      "options": { "doNotFollow": { "path": "node_modules" }, "exclude": "^dist", "maxDepth": 3, "tsPreCompilationDeps": true,
                   "prefix": "p", "focus": "^src", "skipAnalysisNotInRules": true }
    }"#;
    let options = |format| LoadOptions {
        format: Some(format),
        root: Some(std::env::temp_dir()),
        ..LoadOptions::default()
    };
    let dir = std::env::temp_dir();
    let as_dc = load_text(
        dc,
        Syntax::Json,
        &dir,
        &options(ConfigFormat::DependencyCruiser),
    )?;
    let as_native = load_text(dc, Syntax::Json, &dir, &options(ConfigFormat::Native))?;
    assert_eq!(as_dc.rules, as_native.rules);
    assert_eq!(as_dc.languages, as_native.languages);
    assert_eq!(as_dc.options, as_native.options);
    Ok(())
}
