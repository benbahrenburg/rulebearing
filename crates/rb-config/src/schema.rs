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

use rb_model::{DotnetOptions, Language, PythonOptions, Severity, TypeScriptOptions};
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::capability::{Capability, capability};
use crate::elements::{Concept, Side, VOCABULARY, ValueKind, spellings, split_key};

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
    /// The rules allowed to match nothing: names of dependency rules (`allowed[N]` for the Nth
    /// `allowed` entry), ratchets, and element, slice and diagram rules. A name that is none of
    /// these is an error, so an exception cannot outlive its rule (ADR-0032).
    #[serde(default)]
    pub allow_empty: Option<Vec<String>>,
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
    /// Element rules: `ArchUnitNET`'s predicates and conditions over types, members and modules
    /// of every language, each key's per-language answer in its description.
    #[serde(default)]
    #[schemars(schema_with = "element_rules")]
    pub elements: Option<Vec<Value>>,
    /// Slice rules: types grouped by a namespace, module or path pattern.
    #[serde(default)]
    #[schemars(schema_with = "slice_rules")]
    pub slices: Option<Vec<Value>>,
    /// Diagram rules: types that must adhere to a `PlantUML` component diagram.
    #[serde(default)]
    #[schemars(schema_with = "diagram_rules")]
    pub diagrams: Option<Vec<Value>>,
}

/// A reference to the definition `name`, added by `define` when missing.
fn reference(
    generator: &mut SchemaGenerator,
    name: &str,
    define: impl FnOnce(&mut SchemaGenerator) -> Value,
) -> Value {
    if !generator.definitions().contains_key(name) {
        // Reserve the name first, so a definition that refers to itself terminates.
        generator
            .definitions_mut()
            .insert(name.to_owned(), Value::Bool(true));
        let definition = define(generator);
        generator
            .definitions_mut()
            .insert(name.to_owned(), definition);
    }
    json!({ "$ref": format!("#{}/{name}", generator.settings().definitions_path) })
}

/// The fields every rule of the three families shares.
fn rule_metadata() -> serde_json::Map<String, Value> {
    let severity: Vec<&str> = ["error", "warn", "info", "ignore"].to_vec();
    json!({
        "name": { "type": "string", "description": "The rule's name, unique in the configuration." },
        "comment": { "type": "string", "description": "Why the rule exists; the decision token (`adr:NNNN`) goes here." },
        "fix": { "type": "string", "description": "What to do about a violation; every reporter prints it." },
        "severity": { "enum": severity, "description": "Default `error`." },
        "owner": { "type": "string", "description": "Who answers for the rule." },
        "expires": { "type": "string", "format": "date", "description": "The last day the rule applies, YYYY-MM-DD; the run fails the day after, as for a dependency rule." },
    })
    .as_object()
    .cloned()
    .unwrap_or_default()
}

/// `select`: the objects a rule is about.
fn selector(generator: &mut SchemaGenerator) -> Value {
    reference(generator, "ElementSelector", |generator| {
        let predicate = expression(generator, Side::Where);
        json!({
            "type": "object",
            "description": "The objects a rule selects: `ArchUnitNET`'s `Types()`, `Classes()`, ..., then `That()`.",
            "required": ["kind"],
            "additionalProperties": false,
            "properties": {
                "kind": {
                    "enum": ["type", "class", "interface", "attribute", "member", "field", "method", "property", "function", "module"],
                    "description": "`Types()`, `Classes()`, `Interfaces()`, `Attributes()`, `Members()`, `FieldMembers()`, `MethodMembers()`, `PropertyMembers()`; `function` and `module` are TypeScript and Python additions.",
                },
                "language": {
                    "description": "Only objects of these languages: scopes a rule that uses a key some language cannot answer (ADR-0014).",
                    "anyOf": [
                        { "enum": LANGUAGES },
                        { "type": "array", "minItems": 1, "items": { "enum": LANGUAGES } },
                    ],
                },
                "includeReferenced": {
                    "type": "boolean",
                    "description": "Also select the types the code references but does not define (`Types(true)`). Default false.",
                },
                "where": predicate,
            },
        })
    })
}

const LANGUAGES: [&str; 4] = ["typescript", "javascript", "dotnet", "python"];

/// How each language answers `concept`, for a key's description.
fn capabilities(concept: Concept) -> String {
    let row = |language: Language| match capability(concept, language) {
        Capability::Answerable => "answered as `ArchUnitNET` defines it".to_owned(),
        Capability::Mapped(text) => text.to_owned(),
        Capability::Unanswerable(why) => {
            format!("unanswerable ({why}): exit 3 unless `select.language` leaves it out")
        }
    };
    format!(
        " .NET: {}. TypeScript: {}. JavaScript: {}. Python: {}.",
        row(Language::Dotnet),
        row(Language::Typescript),
        row(Language::Javascript),
        row(Language::Python)
    )
}

/// The value a key of `kind` takes.
fn value_schema(generator: &mut SchemaGenerator, kind: ValueKind) -> Value {
    let names = json!({ "anyOf": [{ "type": "string" }, { "type": "array", "items": { "type": "string" } }] });
    match kind {
        ValueKind::Flag => json!({ "type": "boolean" }),
        ValueKind::Names => names,
        ValueKind::Pattern | ValueKind::Diagram => json!({ "type": "string" }),
        ValueKind::Objects => {
            let nested = selector(generator);
            json!({ "anyOf": [{ "type": "string" }, { "type": "array", "items": { "type": "string" } }, nested] })
        }
        ValueKind::AttributeArguments | ValueKind::AttributeNamedArguments => {
            let nested = selector(generator);
            let arguments = if kind == ValueKind::AttributeArguments {
                json!({ "type": "array" })
            } else {
                json!({ "type": "object" })
            };
            json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "attribute": { "anyOf": [{ "type": "string" }, nested] },
                    "arguments": arguments,
                },
            })
        }
        ValueKind::ArgumentValues => json!({ "type": "array" }),
        ValueKind::NamedArgumentValues => json!({ "type": "object" }),
    }
}

/// `getterVisibility` and `setterVisibility`. Each access level is its own concept, answered per
/// language on its own, so each value carries its own description.
fn accessor_visibility(properties: &mut serde_json::Map<String, Value>) {
    for (key, family, levels) in [
        (
            "getterVisibility",
            "Getter",
            [
                ("public", Concept::HavePublicGetter),
                ("protected", Concept::HaveProtectedGetter),
                ("internal", Concept::HaveInternalGetter),
                ("protected-internal", Concept::HaveProtectedInternalGetter),
                ("private", Concept::HavePrivateGetter),
                ("private-protected", Concept::HavePrivateProtectedGetter),
            ],
        ),
        (
            "setterVisibility",
            "Setter",
            [
                ("public", Concept::HavePublicSetter),
                ("protected", Concept::HaveProtectedSetter),
                ("internal", Concept::HaveInternalSetter),
                ("protected-internal", Concept::HaveProtectedInternalSetter),
                ("private", Concept::HavePrivateSetter),
                ("private-protected", Concept::HavePrivateProtectedSetter),
            ],
        ),
    ] {
        let values: Vec<Value> = levels
            .iter()
            .map(|(level, concept)| {
                let (name, _, _) = concept.entry();
                json!({
                    "const": level,
                    "description": format!("`{name}`.{}", capabilities(*concept)),
                })
            })
            .collect();
        let summary: Vec<String> = levels
            .iter()
            .map(|(level, concept)| format!("`{level}`:{}", capabilities(*concept)))
            .collect();
        properties.insert(
            key.into(),
            json!({
                "oneOf": values,
                "description": format!(
                    "The property's {} has this visibility (`HavePublic{family}` and its twins). Per value: {}",
                    family.to_lowercase(),
                    summary.join(" ")
                ),
            }),
        );
    }
}

/// `where` (predicates) or `should` (conditions): one key, or `all`, `any`, `not` around more.
fn expression(generator: &mut SchemaGenerator, side: Side) -> Value {
    let (name, what) = match side {
        Side::Where => ("ElementPredicate", "predicates (`That()`)"),
        Side::Should => ("ElementCondition", "conditions (`Should()`)"),
    };
    reference(generator, name, |generator| {
        let own = json!({ "$ref": format!("#{}/{name}", generator.settings().definitions_path) });
        let mut properties = serde_json::Map::new();
        properties.insert(
            "all".into(),
            json!({ "type": "array", "minItems": 1, "items": own, "description": "Every item holds (`And()`, `AndShould()`)." }),
        );
        properties.insert(
            "any".into(),
            json!({ "type": "array", "minItems": 1, "items": own, "description": "At least one item holds (`Or()`, `OrShould()`)." }),
        );
        properties.insert(
            "not".into(),
            json!({ "allOf": [own], "description": "The item does not hold." }),
        );
        accessor_visibility(&mut properties);
        for spelling in spellings(side) {
            let (base, negated, _) = split_key(&spelling, side);
            let Some((_, concept, kind, _)) = VOCABULARY.iter().find(|(n, ..)| *n == base) else {
                continue;
            };
            let mut value = value_schema(generator, *kind);
            if let Value::Object(map) = &mut value {
                map.insert(
                    "description".into(),
                    Value::String(format!(
                        "`{}`{}.{}",
                        if base.is_empty() {
                            "are"
                        } else {
                            base.as_str()
                        },
                        if negated { ", negated" } else { "" },
                        capabilities(*concept)
                    )),
                );
            }
            properties.insert(spelling, value);
        }
        let mut definition = json!({
            "type": "object",
            "description": format!("One of `ArchUnitNET`'s {what}, or `all`, `any`, `not` around more. The loader rejects any other key and names the nearest one, and an empty expression (only an empty `select.where` means \"no filter\"); `...That` forms (`dependOnAnyTypesThat`) take a nested selector."),
            "properties": properties,
        });
        if side == Side::Should
            && let Value::Object(map) = &mut definition
        {
            map.insert("minProperties".into(), json!(1));
        }
        definition
    })
}

/// `rules.elements`.
fn element_rules(generator: &mut SchemaGenerator) -> Schema {
    let select = selector(generator);
    let should = expression(generator, Side::Should);
    let mut properties = rule_metadata();
    properties.insert(
        "because".into(),
        json!({ "type": "string", "description": "`Because(reason)`." }),
    );
    properties.insert(
        "allowEmpty".into(),
        json!({ "type": "boolean", "description": "An empty selection passes (`WithoutRequiringPositiveResults()`). Default false: it is vacuous (ADR-0007)." }),
    );
    properties.insert("select".into(), select);
    properties.insert("should".into(), should);
    Schema::try_from(json!({
        "type": "array",
        "items": {
            "type": "object",
            "required": ["name", "select", "should"],
            "additionalProperties": false,
            "properties": properties,
        },
    }))
    .unwrap_or_default()
}

/// `rules.slices`.
fn slice_rules(_: &mut SchemaGenerator) -> Schema {
    let condition = json!({ "enum": ["notDependOnEachOther", "beFreeOfCycles"] });
    let mut properties = rule_metadata();
    properties.insert(
        "matching".into(),
        json!({ "type": "string", "description": "`Matching(\"Ns.(*)\")` or `MatchingWithPackages(\"Ns.(**)\")`: a namespace, dotted module or path pattern; either names a slice by what follows the prefix, and `Ns.(**)..` by its first segment." }),
    );
    properties.insert(
        "should".into(),
        json!({ "anyOf": [condition, { "type": "array", "minItems": 1, "items": condition }], "description": "`NotDependOnEachOther()`, `BeFreeOfCycles()`, or both." }),
    );
    properties.insert(
        "ignore".into(),
        json!({ "anyOf": [{ "type": "string" }, { "type": "array", "items": { "type": "string" } }], "description": "Slice names left out." }),
    );
    properties.insert("where".into(), json!({ "type": "string", "description": "A pattern a slice name must match to take part." }));
    properties.insert(
        "segments".into(),
        json!({ "type": "integer", "minimum": 1, "description": "Keep the first this many segments of each slice name, so a package and everything below it are one slice (import-linter's `acyclic_siblings`). A Rulebearing addition." }),
    );
    properties.insert(
        "allowEmpty".into(),
        json!({ "type": "boolean", "description": "An empty slicing is not vacuous." }),
    );
    Schema::try_from(json!({
        "type": "array",
        "items": {
            "type": "object",
            "required": ["name", "matching", "should"],
            "additionalProperties": false,
            "properties": properties,
        },
    }))
    .unwrap_or_default()
}

/// `rules.diagrams`.
fn diagram_rules(generator: &mut SchemaGenerator) -> Schema {
    let select = selector(generator);
    let mut properties = rule_metadata();
    properties.insert("select".into(), select);
    properties.insert(
        "allowEmpty".into(),
        json!({ "type": "boolean", "description": "An empty selection is not vacuous. Default false (ADR-0007)." }),
    );
    properties.insert(
        "adhereTo".into(),
        json!({ "type": "string", "description": "The `PlantUML` component diagram, relative to the configuration (`AdhereToPlantUmlDiagram`)." }),
    );
    Schema::try_from(json!({
        "type": "array",
        "items": {
            "type": "object",
            "required": ["name", "select", "adhereTo"],
            "additionalProperties": false,
            "properties": properties,
        },
    }))
    .unwrap_or_default()
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

    /// Every key of every `where` and `should` in the ported conformance cases.
    fn case_keys(value: &Value, side: Side, out: &mut BTreeMap<String, Side>) {
        let Value::Object(map) = value else {
            return;
        };
        for (key, inner) in map {
            match key.as_str() {
                "all" | "any" => {
                    for item in inner.as_array().into_iter().flatten() {
                        case_keys(item, side, out);
                    }
                }
                "not" => case_keys(inner, side, out),
                _ => {
                    out.insert(key.clone(), side);
                    if let Some(nested) = inner.get("where") {
                        case_keys(nested, Side::Where, out);
                    }
                    if let Some(nested) = inner.get("attribute").and_then(|a| a.get("where")) {
                        case_keys(nested, Side::Where, out);
                    }
                }
            }
        }
    }

    #[test]
    fn every_key_the_conformance_cases_use_is_described() -> Result<(), Box<dyn std::error::Error>>
    {
        let schema: Value = serde_json::from_str(&generate())?;
        let described = |definition: &str| -> Vec<String> {
            schema["$defs"][definition]["properties"]
                .as_object()
                .map(|m| m.keys().cloned().collect())
                .unwrap_or_default()
        };
        let (predicates, conditions) =
            (described("ElementPredicate"), described("ElementCondition"));
        assert!(
            predicates.contains(&"arePublic".to_owned())
                && conditions.contains(&"bePublic".to_owned())
        );
        let mut keys = BTreeMap::new();
        for suite in ["archunitnet", "netarchtest"] {
            let folder = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../conformance")
                .join(suite)
                .join("ported");
            for entry in std::fs::read_dir(folder)?.flatten() {
                let doc: Value = serde_yaml::from_str(&std::fs::read_to_string(entry.path())?)?;
                for case in doc["cases"].as_array().into_iter().flatten() {
                    case_keys(&case["rule"]["select"]["where"], Side::Where, &mut keys);
                    case_keys(&case["rule"]["should"], Side::Should, &mut keys);
                }
            }
        }
        assert!(keys.len() > 100, "{} keys", keys.len());
        let missing: Vec<&String> = keys
            .iter()
            .filter(|(key, side)| {
                let list = if **side == Side::Where {
                    &predicates
                } else {
                    &conditions
                };
                !list.contains(key) && !key.ends_with("That")
            })
            .map(|(key, _)| key)
            .collect();
        assert!(
            missing.is_empty(),
            "keys the schema does not describe: {missing:?}"
        );
        Ok(())
    }

    #[test]
    fn each_rule_family_lists_exactly_the_keys_the_loader_accepts()
    -> Result<(), Box<dyn std::error::Error>> {
        let schema: Value = serde_json::from_str(&generate())?;
        for family in ["elements", "slices", "diagrams"] {
            let listed: std::collections::BTreeSet<String> =
                schema["$defs"]["NativeRules"]["properties"][family]["items"]["properties"]
                    .as_object()
                    .map(|m| m.keys().cloned().collect())
                    .unwrap_or_default();
            let accepted: std::collections::BTreeSet<String> = crate::elements::family_keys(family)
                .into_iter()
                .map(str::to_owned)
                .collect();
            assert_eq!(listed, accepted, "{family}");
        }
        Ok(())
    }

    #[test]
    fn accessor_visibility_describes_each_access_level() -> Result<(), Box<dyn std::error::Error>> {
        let schema: Value = serde_json::from_str(&generate())?;
        let values = schema["$defs"]["ElementCondition"]["properties"]["getterVisibility"]["oneOf"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let describe = |level: &str| {
            values
                .iter()
                .find(|v| v["const"] == level)
                .and_then(|v| v["description"].as_str())
                .unwrap_or_default()
                .to_owned()
        };
        assert_eq!(values.len(), 6);
        assert!(describe("public").contains("havePublicGetter"));
        assert!(
            !describe("public").contains("TypeScript: unanswerable"),
            "{}",
            describe("public")
        );
        assert!(
            describe("internal").contains("TypeScript: unanswerable"),
            "{}",
            describe("internal")
        );
        Ok(())
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
