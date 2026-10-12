//! `fleet.yaml`: the workspace manifest `rulebearing fleet` reads, the sibling repositories it
//! cruises and the fleet-wide rules it evaluates over their union.
//!
//! - Architecture: [Configuration and the rule language](../../../docs/architecture.md#configuration-and-the-rule-language)
//! - Decisions: [ADR-0005](../../../docs/adr/0005-native-config-superset-and-compat.md) (the
//!   fleet rules are the native format's `rules`),
//!   [ADR-0078](../../../docs/adr/0078-fleet-and-declared-edges.md) (what a fleet rule may be)
//! - Plan: [plan 0004, Step 11](../../../docs/plans/pending/0004-wave-4-reach.md#24-steps-for-sub-wave-4d-fleet-and-declared-edges),
//!   [§ 1.6](../../../docs/plans/pending/0004-wave-4-reach.md#16-interfaces-and-contracts-this-wave-freezes)
//!   (the frozen `fleet.yaml`)
//! - Requirement: [FR-REACH-03](../../../docs/prd.md#fr-reach-03)
//!
//! A fleet rule is a dependency rule, or a `layers` or `independence` entry that expands to
//! dependency rules: the union a fleet evaluates is the repositories' module graphs, prefixed
//! with their names. Element, slice and diagram rules read the code layer, whose type names are
//! not prefixed and may repeat across repositories, and a ratchet's budget lives with a
//! repository; all four belong in a repository's own configuration, and the manifest refuses
//! them (exit 3).

use std::collections::BTreeSet;
use std::path::Path;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::ConfigError;
use crate::LoadOptions;
use crate::model::{Config, IndependenceShorthand, LayersShorthand};
use crate::read::Syntax;
use crate::schema::DependencyRulesSchema;

/// The `rules` keys a fleet evaluates.
pub const RULE_KEYS: [&str; 3] = ["dependencies", "layers", "independence"];

/// `fleet.yaml`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(title = "Rulebearing fleet manifest")]
pub struct FleetManifest {
    /// The schema this file is written against.
    #[serde(rename = "$schema", default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    /// The repositories, each cruised with its own configuration and cache.
    pub repos: Vec<FleetRepo>,
    /// The fleet-wide rules, in the native format's `rules` shape, over module paths prefixed
    /// with each repository's name and `/` (`web/src/app.ts`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<FleetRules>")]
    pub rules: Option<Value>,
}

/// One repository of a fleet.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FleetRepo {
    /// The name its module paths are prefixed with: letters, digits, `.`, `_` and `-`, unique in
    /// the manifest.
    pub name: String,
    /// The repository's folder, relative to the manifest.
    pub path: String,
    /// Its configuration file, relative to the repository; without it, the run looks for one
    /// as `cruise` does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<String>,
}

/// `rules` in a fleet manifest: the keys a fleet evaluates, for the schema.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FleetRules {
    /// dependency-cruiser's rule set, over the prefixed paths.
    #[serde(default)]
    pub dependencies: Option<DependencyRulesSchema>,
    /// Layer models, expanded to one `forbidden` rule per lower-to-higher pair.
    #[serde(default)]
    pub layers: Option<Vec<LayersShorthand>>,
    /// Independence contracts, expanded to one `$1` fence each.
    #[serde(default)]
    pub independence: Option<Vec<IndependenceShorthand>>,
}

fn invalid(message: impl std::fmt::Display) -> ConfigError {
    ConfigError::Invalid(format!("fleet manifest: {message}"))
}

/// Whether `name` is a repository name a manifest may use.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// Reads and checks a manifest: at least one repository, every name valid and unique, every
/// path non-empty, and `rules` holding only [`RULE_KEYS`].
///
/// # Errors
/// [`ConfigError::Invalid`] naming what is wrong.
pub fn read_manifest(text: &str) -> Result<FleetManifest, ConfigError> {
    let manifest: FleetManifest = serde_yaml::from_str(text).map_err(invalid)?;
    if manifest.repos.is_empty() {
        return Err(invalid("`repos` lists no repository"));
    }
    let mut names = BTreeSet::new();
    for (index, repo) in manifest.repos.iter().enumerate() {
        if !valid_name(&repo.name) {
            return Err(invalid(format!(
                "repos[{index}].name `{}`: use letters, digits, `.`, `_` and `-`; it becomes the first segment of every module path",
                repo.name
            )));
        }
        if !names.insert(repo.name.as_str()) {
            return Err(invalid(format!(
                "repos[{index}].name `{}` is used twice; each repository needs its own prefix",
                repo.name
            )));
        }
        if repo.path.trim().is_empty() {
            return Err(invalid(format!("repos[{index}].path is empty")));
        }
    }
    match &manifest.rules {
        None => {}
        Some(Value::Object(rules)) => {
            if let Some(key) = rules.keys().find(|k| !RULE_KEYS.contains(&k.as_str())) {
                return Err(invalid(format!(
                    "rules.{key}: a fleet evaluates dependency rules (`dependencies`, `layers`, `independence`) over the union of the repositories' module graphs; element, slice and diagram rules and ratchets belong in a repository's own configuration"
                )));
            }
        }
        Some(_) => return Err(invalid("`rules` is not an object")),
    }
    Ok(manifest)
}

/// The fleet rules as a configuration, loaded as a native file in `dir` (the manifest's folder)
/// would be; an empty configuration when the manifest has none.
///
/// # Errors
/// What [`crate::load_text`] returns for the rules.
pub fn rules_config(manifest: &FleetManifest, dir: &Path) -> Result<Config, ConfigError> {
    let mut file = Map::new();
    file.insert(
        "rules".to_owned(),
        manifest
            .rules
            .clone()
            .unwrap_or_else(|| Value::Object(Map::new())),
    );
    let text = Value::Object(file).to_string();
    // `{ "rules": ... }` is the native shape, so the file is read as native, as a
    // `rulebearing.yaml` beside the manifest would be.
    crate::load_text(&text, Syntax::Json, dir, &LoadOptions::default())
}

/// The manifest's JSON schema, published as `schema/fleet-v1.json`.
pub mod schema {
    use serde_json::Value;

    /// The `$id` the schema is published under.
    pub const ID: &str = "https://benbahrenburg.github.io/rulebearing/schema/fleet-v1.json";

    /// The committed schema's path, relative to the repository root.
    pub const PATH: &str = "schema/fleet-v1.json";

    /// The schema as pretty JSON with sorted keys and a trailing newline.
    pub fn generate() -> String {
        let mut schema = schemars::schema_for!(super::FleetManifest);
        schema.insert("$id".into(), Value::String(ID.into()));
        let mut text = serde_json::to_string_pretty(&crate::schema::sorted(schema.to_value()))
            .unwrap_or_default();
        text.push('\n');
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = "repos:
  - { name: web,      path: ../web,      config: rulebearing.yaml }
  - { name: api,      path: ../api,      config: rulebearing.yaml }
  - { name: pipeline, path: ../pipeline }
rules:
  dependencies:
    forbidden:
      - name: web-never-reaches-pipeline
        comment: \"adr:0021\"
        from: { path: \"^web/\" }
        to:   { path: \"^pipeline/\" }
";

    #[test]
    fn reads_the_frozen_format() {
        let manifest = read_manifest(MANIFEST).unwrap_or_default();
        let names: Vec<(&str, &str, Option<&str>)> = manifest
            .repos
            .iter()
            .map(|r| (r.name.as_str(), r.path.as_str(), r.config.as_deref()))
            .collect();
        assert_eq!(
            names,
            [
                ("web", "../web", Some("rulebearing.yaml")),
                ("api", "../api", Some("rulebearing.yaml")),
                ("pipeline", "../pipeline", None),
            ]
        );
        let config = rules_config(&manifest, Path::new("."));
        let forbidden = config
            .map(|c| {
                c.rules
                    .dependencies
                    .forbidden
                    .iter()
                    .map(|r| r.name().to_owned())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        assert_eq!(forbidden, ["web-never-reaches-pipeline"]);
        let compat = rules_config(&manifest, Path::new(".")).map(|c| c.compat);
        assert!(
            matches!(compat, Ok(crate::CompatMode::Native)),
            "{compat:?}"
        );
    }

    #[test]
    fn a_manifest_without_rules_is_an_empty_configuration() {
        let manifest = read_manifest("repos: [{ name: a, path: a }]").unwrap_or_default();
        assert!(manifest.rules.is_none());
        let config = rules_config(&manifest, Path::new("."));
        assert!(config.is_ok_and(|c| c.rules.all_dependency_rules().count() == 0));
    }

    #[test]
    fn refuses_what_is_not_the_format() {
        for (text, says) in [
            ("repos: []", "lists no repository"),
            ("rules: {}", "missing field `repos`"),
            (
                "repos: [{ name: a, path: a, cache: x }]",
                "unknown field `cache`",
            ),
            ("repos: [{ name: a/b, path: a }]", "repos[0].name `a/b`"),
            ("repos: [{ name: '', path: a }]", "repos[0].name ``"),
            ("repos: [{ name: '..', path: a }]", "repos[0].name `..`"),
            (
                "repos: [{ name: a, path: a }, { name: a, path: b }]",
                "repos[1].name `a` is used twice",
            ),
            ("repos: [{ name: a, path: ' ' }]", "repos[0].path is empty"),
            (
                "repos: [{ name: a, path: a }]\nrules: { elements: [] }",
                "rules.elements: a fleet evaluates dependency rules",
            ),
            (
                "repos: [{ name: a, path: a }]\nrules: { ratchets: [] }",
                "rules.ratchets",
            ),
            (
                "repos: [{ name: a, path: a }]\nrules: []",
                "`rules` is not an object",
            ),
            (
                "extends: x\nrepos: [{ name: a, path: a }]",
                "unknown field `extends`",
            ),
        ] {
            let message = match read_manifest(text) {
                Err(ConfigError::Invalid(message)) => message,
                other => format!("not refused: {other:?}"),
            };
            assert!(
                message.starts_with("fleet manifest: ") && message.contains(says),
                "{text}: {message}"
            );
        }
    }

    #[test]
    fn a_rule_the_native_format_refuses_is_refused_here() {
        let manifest = read_manifest(
            "repos: [{ name: a, path: a }]\nrules: { dependencies: { forbidden: [{ name: x, from: { nonsense: 1 }, to: {} }] } }",
        )
        .unwrap_or_default();
        assert!(rules_config(&manifest, Path::new(".")).is_err());
    }

    #[test]
    fn names_are_one_path_segment() {
        for (name, valid) in [
            ("web", true),
            ("api-v2", true),
            ("a.b_c", true),
            ("", false),
            (".", false),
            ("..", false),
            ("a/b", false),
            ("a b", false),
            ("é", false),
        ] {
            assert_eq!(valid_name(name), valid, "{name}");
        }
    }

    #[test]
    fn the_committed_schema_is_current() -> Result<(), Box<dyn std::error::Error>> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(schema::PATH);
        let generated = schema::generate();
        if std::env::var_os("RB_UPDATE_SCHEMA").is_some() {
            std::fs::write(&path, &generated)?;
        }
        let committed = std::fs::read_to_string(&path)?;
        assert!(
            committed == generated,
            "{} is stale; run RB_UPDATE_SCHEMA=1 cargo test -p rb-config fleet",
            schema::PATH
        );
        let value: Value = serde_json::from_str(&generated)?;
        assert_eq!(value["$id"], Value::String(schema::ID.into()));
        assert!(value["properties"]["repos"].is_object());
        Ok(())
    }
}
