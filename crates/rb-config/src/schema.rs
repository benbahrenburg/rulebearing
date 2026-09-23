//! The native format's JSON schema, published as `schema/config-v1.json`.
//!
//! - Decision: [ADR-0005](../../../docs/adr/0005-native-config-superset-and-compat.md) ("the
//!   native schema is published with descriptions at the `$schema` URL")
//! - Plan: [Wave 1, Step 1](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-1-config-model-and-the-two-front-ends-1a)
//! - Requirement: [FR-CFG-02](../../../docs/prd.md#fr-cfg-02)
//!
//! The schema is generated from the types the loader deserialises into, so a key the schema
//! names is a key the loader reads. The test below fails when the committed file is stale;
//! `RB_UPDATE_SCHEMA=1 cargo test -p rb-config schema` rewrites it.

use std::collections::BTreeMap;

use rb_model::{DotnetOptions, PythonOptions, Severity, TypeScriptOptions};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model::{
    Define, FilterOption, IndependenceShorthand, KnownViolation, LayersShorthand, Ratchet, Rule,
};

/// A native configuration file: `rulebearing.yaml`, `.json`, `.jsonc` or `.toml`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(title = "Rulebearing configuration")]
pub struct NativeFile {
    /// The schema this file is written against.
    #[serde(rename = "$schema", default)]
    pub schema: Option<String>,
    /// Files, npm packages, `dependency-cruiser/configs/<preset>` or `rulebearing:<preset>` to
    /// extend; later entries are merged under earlier ones and this file wins.
    #[serde(default)]
    pub extends: Option<ExtendsValue>,
    /// Named values read from JSON files, substituted into patterns as `${name}`.
    #[serde(default)]
    pub defines: Option<BTreeMap<String, Define>>,
    /// Per-language settings.
    #[serde(default)]
    pub languages: Option<NativeLanguages>,
    /// Every dependency-cruiser option, unchanged.
    #[serde(default)]
    pub options: Option<OptionsSchema>,
    /// The rule families.
    #[serde(default)]
    pub rules: Option<NativeRules>,
    /// dependency-cruiser's `forbidden`, legal at the same place.
    #[serde(default)]
    pub forbidden: Option<Vec<Rule>>,
    /// dependency-cruiser's `allowed`, legal at the same place.
    #[serde(default)]
    pub allowed: Option<Vec<Rule>>,
    /// dependency-cruiser's `allowedSeverity`, legal at the same place.
    #[serde(default)]
    pub allowed_severity: Option<Severity>,
    /// dependency-cruiser's `required`, legal at the same place.
    #[serde(default)]
    pub required: Option<Vec<Rule>>,
}

/// `extends`: one entry or several.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum ExtendsValue {
    /// One entry.
    One(String),
    /// Several, merged in order.
    Many(Vec<String>),
}

/// `languages`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NativeLanguages {
    /// The TypeScript and JavaScript extractor's options (dependency-cruiser's flat option names).
    #[serde(default)]
    pub typescript: Option<TypeScriptOptions>,
    /// The .NET extractor's options (wave 2).
    #[serde(default)]
    pub dotnet: Option<DotnetOptions>,
    /// The Python extractor's options (wave 2).
    #[serde(default)]
    pub python: Option<PythonOptions>,
}

/// `options`: the TypeScript block's keys and every other dependency-cruiser option.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OptionsSchema {
    /// The extractor options, also accepted here.
    #[serde(flatten)]
    pub typescript: TypeScriptOptions,
    /// `focus`: a pattern, a list, or `{ path, depth }`.
    #[serde(default)]
    pub focus: Option<FilterValue>,
    /// `reaches`: a pattern, a list, or `{ path }`.
    #[serde(default)]
    pub reaches: Option<FilterValue>,
    /// `highlight`: a pattern, a list, or `{ path }`.
    #[serde(default)]
    pub highlight: Option<FilterValue>,
    /// `collapse`: a pattern or a folder depth.
    #[serde(default)]
    pub collapse: Option<Value>,
    /// Always derive `dependents[]`.
    #[serde(default)]
    pub force_derive_dependents: Option<bool>,
    /// Skip derivations no rule reads.
    #[serde(default)]
    pub skip_analysis_not_in_rules: Option<bool>,
    /// Compute instability metrics (wave 2).
    #[serde(default)]
    pub metrics: Option<bool>,
    /// The link prefix reporters put before a path.
    #[serde(default)]
    pub prefix: Option<String>,
    /// The link suffix reporters put after a path.
    #[serde(default)]
    pub suffix: Option<String>,
    /// `{ type: none | cli-feedback | performance-log | ndjson }`.
    #[serde(default)]
    pub progress: Option<Value>,
    /// Per-reporter settings.
    #[serde(default)]
    pub reporter_options: Option<Value>,
    /// Accepted and recorded; the content-addressed cache is wave 3.
    #[serde(default)]
    pub cache: Option<Value>,
    /// Violations to report at a lower severity, keyed by stable id or by rule, from and to.
    #[serde(default)]
    pub known_violations: Option<Vec<KnownViolation>>,
}

/// A filter in any of its three forms.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum FilterValue {
    /// One pattern.
    One(String),
    /// Several patterns, joined with `|`.
    Many(Vec<String>),
    /// The object form.
    Object(FilterOption),
}

/// `rules`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NativeRules {
    /// dependency-cruiser's rule set.
    #[serde(default)]
    pub dependencies: Option<DependencyRulesSchema>,
    /// Counts of direct edges that may only fall.
    #[serde(default)]
    pub ratchets: Option<Vec<Ratchet>>,
    /// Layer models, expanded to one `forbidden` rule per lower-to-higher pair.
    #[serde(default)]
    pub layers: Option<Vec<LayersShorthand>>,
    /// Independence contracts, expanded to one `$1` fence each.
    #[serde(default)]
    pub independence: Option<Vec<IndependenceShorthand>>,
}

/// `rules.dependencies`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DependencyRulesSchema {
    /// Edges that must not exist.
    #[serde(default)]
    pub forbidden: Option<Vec<Rule>>,
    /// The only edges that may exist.
    #[serde(default)]
    pub allowed: Option<Vec<Rule>>,
    /// The severity of an edge outside `allowed`. Default `warn`.
    #[serde(default)]
    pub allowed_severity: Option<Severity>,
    /// Edges that must exist.
    #[serde(default)]
    pub required: Option<Vec<Rule>>,
}

/// Rebuilds every object with its keys sorted, so the output does not depend on whether a
/// dependency enabled `serde_json`'s `preserve_order` (feature unification differs between
/// `cargo test -p rb-config` and `cargo test --workspace`).
fn sorted(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<(String, Value)> = map.into_iter().collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            Value::Object(entries.into_iter().map(|(k, v)| (k, sorted(v))).collect())
        }
        Value::Array(items) => Value::Array(items.into_iter().map(sorted).collect()),
        other => other,
    }
}

/// The schema as pretty JSON with sorted keys and a trailing newline.
pub fn generate() -> String {
    let mut schema = schemars::schema_for!(NativeFile);
    schema.insert(
        "$id".into(),
        Value::String("https://benbahrenburg.github.io/rulebearing/schema/config-v1.json".into()),
    );
    let mut text = serde_json::to_string_pretty(&sorted(schema.to_value())).unwrap_or_default();
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn committed() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../schema/config-v1.json")
    }

    #[test]
    fn schema_is_current() -> Result<(), Box<dyn std::error::Error>> {
        let generated = generate();
        if std::env::var_os("RB_UPDATE_SCHEMA").is_some() {
            std::fs::write(committed(), &generated)?;
        }
        let on_disk = std::fs::read_to_string(committed()).unwrap_or_default();
        assert!(
            on_disk == generated,
            "schema/config-v1.json is stale; run RB_UPDATE_SCHEMA=1 cargo test -p rb-config schema"
        );
        Ok(())
    }

    #[test]
    fn the_design_example_validates_against_the_types() -> Result<(), Box<dyn std::error::Error>> {
        let text = r#"
$schema: https://benbahrenburg.github.io/rulebearing/schema/config-v1.json
extends: [rulebearing:recommended]
languages:
  typescript: { tsConfig: { fileName: tsconfig.json }, tsPreCompilationDeps: true }
options:
  exclude: { path: ["(^|/)node_modules/"] }
  skipAnalysisNotInRules: true
rules:
  dependencies:
    forbidden:
      - { name: a, severity: error, from: { path: "^apps/([^/]+)/" }, to: { path: "^apps/([^/]+)/", pathNot: "^apps/$1/" } }
  ratchets:
    - { name: r, from: { path: "^a" }, to: { path: "^b" }, budget: b.json }
"#;
        let _: NativeFile = serde_yaml::from_str(text)?;
        assert!(serde_yaml::from_str::<NativeFile>("bogus: 1").is_err());
        Ok(())
    }
}
