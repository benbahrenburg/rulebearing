//! `extends`: files, npm packages, dependency-cruiser's bundled presets and Rulebearing's own.
//!
//! - Source: [design § The dependency-cruiser format](../../../docs/artifacts/design.md#the-dependency-cruiser-format)
//!   ("`extends` resolves files, npm packages, and the bundled presets, exactly as today")
//! - Plan: [Wave 1, Step 3](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-3-extends-presets-defines-captures-regex-1a)
//! - Coverage: [coverage § Rules](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#rules), row `extends`
//! - Requirement: [FR-CFG-01](../../../docs/prd.md#fr-cfg-01)
//!
//! [`merge`] is dependency-cruiser 18.2.0's `mergeConfigs`, ported: named `forbidden` and
//! `required` rules are unique by name with the extending file's keys winning over the base's,
//! anonymous rules and `allowed` rules are unique by deep equality, `allowedSeverity` is the
//! extender's or the base's or `warn`, and `options` are merged key by key with the extender
//! winning. As upstream, only those keys survive a merge; the native additions follow the same
//! rules (ratchets and shorthands unique by name, `defines` and `languages` key by key).

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::ConfigError;

/// Rulebearing's own presets, `rulebearing:<name>`.
pub const NATIVE_PRESETS: &[(&str, &str)] = &[
    (
        "recommended",
        include_str!("../../../presets/rulebearing/recommended.yaml"),
    ),
    (
        "typescript",
        include_str!("../../../presets/rulebearing/typescript.yaml"),
    ),
];

/// What an `extends` entry names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A file on disk.
    File(PathBuf),
    /// A bundled dependency-cruiser preset, by file name (`recommended-strict.cjs`).
    DependencyCruiserPreset(String),
    /// A bundled Rulebearing preset.
    NativePreset(&'static str, &'static str),
}

impl Target {
    /// A stable key for cycle detection.
    pub fn key(&self) -> String {
        match self {
            Self::File(path) => path.to_string_lossy().into_owned(),
            Self::DependencyCruiserPreset(name) => format!("dependency-cruiser/configs/{name}"),
            Self::NativePreset(name, _) => format!("rulebearing:{name}"),
        }
    }
}

/// The extensions dependency-cruiser tries for an `extends` file, then the native ones.
const EXTENSIONS: &[&str] = &[
    "", ".js", ".json", ".cjs", ".mjs", ".yaml", ".yml", ".jsonc", ".toml",
];

/// Resolves one `extends` entry written in a file in `base_dir`.
///
/// # Errors
/// [`ConfigError::Extends`] naming the entry when nothing matches.
pub fn resolve(spec: &str, base_dir: &Path) -> Result<Target, ConfigError> {
    let not_found = |reason: &str| ConfigError::Extends {
        spec: spec.to_owned(),
        reason: reason.to_owned(),
    };
    if let Some(name) = spec.strip_prefix("rulebearing:") {
        return NATIVE_PRESETS
            .iter()
            .find(|(preset, _)| *preset == name)
            .map(|(preset, text)| Target::NativePreset(preset, text))
            .ok_or_else(|| {
                not_found(&format!(
                    "no bundled preset `{name}`; the presets are {}",
                    NATIVE_PRESETS
                        .iter()
                        .map(|(n, _)| format!("rulebearing:{n}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            });
    }
    if let Some(name) = spec.strip_prefix("dependency-cruiser/configs/") {
        let name = name.trim_end_matches(".cjs");
        let file = format!("{name}.cjs");
        return crate::js::DEPENDENCY_CRUISER_PRESETS
            .iter()
            .any(|(preset, _)| *preset == file)
            .then_some(Target::DependencyCruiserPreset(file))
            .ok_or_else(|| {
                not_found("dependency-cruiser bundles recommended, recommended-strict and recommended-warn-only")
            });
    }
    let candidates: Vec<PathBuf> = if spec.starts_with('.') || Path::new(spec).is_absolute() {
        vec![base_dir.join(spec)]
    } else {
        // An npm package: `node_modules/<spec>` in this folder or any above it.
        base_dir
            .ancestors()
            .map(|dir| dir.join("node_modules").join(spec))
            .collect()
    };
    for candidate in &candidates {
        if let Some(found) = find_file(candidate) {
            return Ok(Target::File(found));
        }
    }
    Err(not_found("no file or package matches"))
}

fn find_file(candidate: &Path) -> Option<PathBuf> {
    let text = candidate.to_string_lossy();
    for ext in EXTENSIONS {
        let path = PathBuf::from(format!("{text}{ext}"));
        if path.is_file() {
            return Some(path.canonicalize().unwrap_or(path));
        }
    }
    let manifest = candidate.join("package.json");
    if let Ok(text) = std::fs::read_to_string(manifest)
        && let Ok(json) = serde_json::from_str::<Value>(&text)
        && let Some(main) = json.get("main").and_then(Value::as_str)
        && let Some(found) = find_file(&candidate.join(main))
    {
        return Some(found);
    }
    ["index.js", "index.cjs", "index.json"]
        .iter()
        .map(|index| candidate.join(index))
        .find(|path| path.is_file())
}

/// The `extends` entries of a canonical configuration, in order.
///
/// # Errors
/// [`ConfigError::Invalid`] when `extends` is neither a string nor an array of strings.
pub fn entries(canonical: &Map<String, Value>) -> Result<Vec<String>, ConfigError> {
    match canonical.get("extends") {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::String(one)) => Ok(vec![one.clone()]),
        Some(Value::Array(many)) => many
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| ConfigError::Invalid("`extends` entries must be strings".into()))
            })
            .collect(),
        Some(_) => Err(ConfigError::Invalid(
            "`extends` must be a string or an array of strings".into(),
        )),
    }
}

fn list<'a>(config: &'a Map<String, Value>, key: &str) -> Vec<&'a Value> {
    config
        .get(key)
        .and_then(Value::as_array)
        .map(|a| a.iter().collect())
        .unwrap_or_default()
}

fn name_of(value: &Value) -> Option<&str> {
    value
        .get("name")
        .and_then(Value::as_str)
        .filter(|n| !n.is_empty())
}

/// dependency-cruiser's `uniqWith(…, isDeepStrictEqual)`.
fn unique_deep(values: Vec<Value>) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::with_capacity(values.len());
    for value in values {
        if !out.contains(&value) {
            out.push(value);
        }
    }
    out
}

/// `{ ...base, ...extended }`.
fn spread(base: Option<&Value>, extended: Option<&Value>) -> Map<String, Value> {
    let mut out = base.and_then(Value::as_object).cloned().unwrap_or_default();
    if let Some(extended) = extended.and_then(Value::as_object) {
        for (k, v) in extended {
            out.insert(k.clone(), v.clone());
        }
    }
    out
}

/// dependency-cruiser's `mergeRules`: named rules unique by name, the extender's keys winning;
/// anonymous rules unique by deep equality.
fn merge_rules(extended: &[&Value], base: &[&Value]) -> Vec<Value> {
    let anonymous = unique_deep(
        extended
            .iter()
            .chain(base)
            .filter(|r| name_of(r).is_none())
            .map(|r| (*r).clone())
            .collect(),
    );
    let named: Vec<Value> = extended
        .iter()
        .filter_map(|rule| {
            let name = name_of(rule)?;
            // reduce((all, baseRule) => ({ ...baseRule, ...all }), extendedRule)
            let merged = base
                .iter()
                .filter(|b| name_of(b) == Some(name))
                .fold((*rule).clone(), |all, b| {
                    Value::Object(spread(Some(b), Some(&all)))
                });
            Some(merged)
        })
        .collect();
    let mut seen = std::collections::BTreeSet::new();
    let mut out: Vec<Value> = named
        .into_iter()
        .chain(base.iter().map(|r| (*r).clone()))
        .filter(|r| name_of(r).is_some_and(|n| seen.insert(n.to_owned())))
        .collect();
    out.extend(anonymous);
    out
}

/// Named entries (ratchets, shorthands) unique by name, the extender first.
fn merge_named(extended: &[&Value], base: &[&Value]) -> Vec<Value> {
    let mut seen = std::collections::BTreeSet::new();
    extended
        .iter()
        .chain(base)
        .filter(|v| name_of(v).is_none_or(|n| seen.insert(n.to_owned())))
        .map(|v| (*v).clone())
        .collect()
}

/// Merges `base` (the file named in `extends`) under `extended` (the file that extends it).
pub fn merge(extended: &Map<String, Value>, base: &Map<String, Value>) -> Map<String, Value> {
    let mut out = Map::new();
    let forbidden = merge_rules(&list(extended, "forbidden"), &list(base, "forbidden"));
    let required = merge_rules(&list(extended, "required"), &list(base, "required"));
    let allowed = unique_deep(
        list(extended, "allowed")
            .into_iter()
            .chain(list(base, "allowed"))
            .cloned()
            .collect(),
    );
    if !forbidden.is_empty() {
        out.insert("forbidden".into(), Value::Array(forbidden));
    }
    if !required.is_empty() {
        out.insert("required".into(), Value::Array(required));
    }
    if !allowed.is_empty() {
        out.insert("allowed".into(), Value::Array(allowed));
        let severity = extended
            .get("allowedSeverity")
            .or_else(|| base.get("allowedSeverity"))
            .cloned()
            .unwrap_or_else(|| Value::String("warn".into()));
        out.insert("allowedSeverity".into(), severity);
    }
    out.insert(
        "options".into(),
        Value::Object(spread(base.get("options"), extended.get("options"))),
    );
    for key in ["ratchets", "layers", "independence"] {
        let merged = merge_named(&list(extended, key), &list(base, key));
        if !merged.is_empty() {
            out.insert(key.into(), Value::Array(merged));
        }
    }
    for key in ["defines", "languages"] {
        let merged = spread(base.get(key), extended.get(key));
        if !merged.is_empty() {
            out.insert(key.into(), Value::Object(merged));
        }
    }
    if let Some(schema) = extended.get("$schema") {
        out.insert("$schema".into(), schema.clone());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn object(value: Value) -> Map<String, Value> {
        match value {
            Value::Object(map) => map,
            _ => Map::new(),
        }
    }

    #[test]
    fn named_rules_merge_with_the_extender_winning() {
        let extended = object(json!({ "forbidden": [{ "name": "a", "severity": "error" }] }));
        let base = object(json!({
            "forbidden": [
                { "name": "a", "severity": "warn", "from": { "path": "x" } },
                { "name": "b" }
            ]
        }));
        let merged = merge(&extended, &base);
        assert_eq!(
            merged["forbidden"],
            json!([
                { "name": "a", "severity": "error", "from": { "path": "x" } },
                { "name": "b" }
            ])
        );
        assert_eq!(merged["options"], json!({}));
    }

    #[test]
    fn anonymous_and_allowed_rules_are_unique_by_value() {
        let rule = json!({ "from": {}, "to": { "circular": true } });
        let extended = object(json!({ "forbidden": [rule.clone()], "allowed": [rule.clone()] }));
        let base = object(
            json!({ "forbidden": [rule.clone()], "allowed": [rule.clone(), { "from": {}, "to": {} }] }),
        );
        let merged = merge(&extended, &base);
        assert_eq!(merged["forbidden"].as_array().map(Vec::len), Some(1));
        assert_eq!(merged["allowed"].as_array().map(Vec::len), Some(2));
        assert_eq!(merged["allowedSeverity"], "warn");
    }

    #[test]
    fn allowed_severity_and_options_follow_upstream() {
        let extended = object(json!({ "allowed": [{}], "options": { "a": 1 } }));
        let base = object(json!({ "allowedSeverity": "error", "options": { "a": 2, "b": 3 } }));
        let merged = merge(&extended, &base);
        assert_eq!(merged["allowedSeverity"], "error");
        assert_eq!(merged["options"], json!({ "a": 1, "b": 3 }));
        assert!(!merged.contains_key("required"));
    }

    #[test]
    fn native_additions_merge_by_name_and_key() {
        let extended = object(
            json!({ "$schema": "s", "ratchets": [{ "name": "r", "budget": "a" }], "defines": { "x": 1 } }),
        );
        let base = object(
            json!({ "ratchets": [{ "name": "r", "budget": "b" }, { "name": "q" }], "defines": { "x": 2, "y": 3 }, "languages": { "python": {} } }),
        );
        let merged = merge(&extended, &base);
        assert_eq!(
            merged["ratchets"],
            json!([{ "name": "r", "budget": "a" }, { "name": "q" }])
        );
        assert_eq!(merged["defines"], json!({ "x": 1, "y": 3 }));
        assert_eq!(merged["languages"], json!({ "python": {} }));
        assert_eq!(merged["$schema"], "s");
    }

    #[test]
    fn presets_resolve() -> Result<(), ConfigError> {
        let here = Path::new("/");
        assert_eq!(
            resolve("dependency-cruiser/configs/recommended-strict", here)?,
            Target::DependencyCruiserPreset("recommended-strict.cjs".into())
        );
        assert_eq!(
            resolve("dependency-cruiser/configs/recommended.cjs", here)?.key(),
            "dependency-cruiser/configs/recommended.cjs"
        );
        assert_eq!(
            resolve("rulebearing:recommended", here)?.key(),
            "rulebearing:recommended"
        );
        assert!(resolve("rulebearing:nope", here).is_err());
        assert!(resolve("dependency-cruiser/configs/nope", here).is_err());
        assert!(resolve("./missing", here).is_err());
        assert!(resolve("some-package-that-is-not-installed", here).is_err());
        Ok(())
    }

    #[test]
    fn files_and_packages_resolve() -> Result<(), Box<dyn std::error::Error>> {
        let dir = std::env::temp_dir().join(format!("rb-extends-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("node_modules/shared-config"))?;
        std::fs::create_dir_all(dir.join("sub"))?;
        std::fs::write(dir.join("base.json"), "{}")?;
        std::fs::write(
            dir.join("node_modules/shared-config/package.json"),
            r#"{"main": "cfg.js"}"#,
        )?;
        std::fs::write(
            dir.join("node_modules/shared-config/cfg.js"),
            "module.exports={}",
        )?;
        let base = resolve("./base", &dir)?;
        assert!(matches!(base, Target::File(ref p) if p.ends_with("base.json")));
        let package = resolve("shared-config", &dir.join("sub"))?;
        assert!(matches!(package, Target::File(ref p) if p.ends_with("cfg.js")));
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn entries_accept_a_string_or_a_list() -> Result<(), ConfigError> {
        assert_eq!(entries(&object(json!({ "extends": "a" })))?, ["a"]);
        assert_eq!(
            entries(&object(json!({ "extends": ["a", "b"] })))?,
            ["a", "b"]
        );
        assert!(entries(&object(json!({})))?.is_empty());
        assert!(entries(&object(json!({ "extends": 1 }))).is_err());
        assert!(entries(&object(json!({ "extends": [1] }))).is_err());
        Ok(())
    }
}
