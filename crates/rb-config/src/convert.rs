//! `config convert` and `config expand`.
//!
//! - Source: [design § The native format](../../../docs/artifacts/design.md#the-native-format)
//!   ("native to dependency-cruiser is lossy and says exactly what it dropped; dependency-cruiser
//!   to native is lossless"), [§ Shorthands](../../../docs/artifacts/design.md#shorthands)
//! - Decision: [ADR-0005](../../../docs/adr/0005-native-config-superset-and-compat.md)
//! - Plan: [Wave 1, Step 4](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-4-config-convert-config-expand-config-lint-shorthands-1a)
//! - Requirement: [FR-CFG-05](../../../docs/prd.md#fr-cfg-05)
//!
//! Both work on one file as written, not on the merged result of its `extends` chain, so a
//! converted file still extends what it extended. dependency-cruiser to native moves the rule
//! lists under `rules.dependencies` and changes nothing else; native to dependency-cruiser
//! substitutes `defines`, expands the shorthands, and drops what dependency-cruiser has no place
//! for, listing every drop.

use std::fmt::Write as _;
use std::path::Path;

use serde_json::{Map, Value};

use crate::normalize::{CROSS_LANGUAGE_KEYS, EDGE_KEYS, GRAPH_KEY, NATIVE_RULE_KEYS};
use crate::{ConfigError, defines, native, shorthands};

/// Something native-to-dependency-cruiser conversion left out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dropped {
    /// Where it was, as a key path.
    pub at: String,
    /// Why dependency-cruiser cannot hold it.
    pub reason: String,
}

/// dependency-cruiser to native: lossless.
pub fn to_native(dependency_cruiser: &Map<String, Value>) -> Map<String, Value> {
    native::from_canonical(dependency_cruiser)
}

/// Native to dependency-cruiser: lossy, with the list of what was dropped.
///
/// # Errors
/// [`ConfigError`] when the native file is malformed or a define cannot be evaluated.
pub fn to_dependency_cruiser(
    native_file: &Map<String, Value>,
    base_dir: &Path,
) -> Result<(Map<String, Value>, Vec<Dropped>), ConfigError> {
    let mut canonical = native::to_canonical(native_file)?;
    let mut dropped = Vec::new();
    if canonical.contains_key("defines") {
        defines::apply_defines(&mut canonical, base_dir)?;
        canonical.remove("defines");
        dropped.push(Dropped {
            at: "defines".into(),
            reason: "substituted into the patterns; dependency-cruiser has no defines".into(),
        });
    }
    if canonical.contains_key("layers") || canonical.contains_key("independence") {
        shorthands::expand(&mut canonical)?;
        dropped.push(Dropped {
            at: "rules.layers, rules.independence".into(),
            reason: "expanded into forbidden rules".into(),
        });
    }
    if let Some(Value::Array(ratchets)) = canonical.remove("ratchets") {
        for ratchet in ratchets {
            let name = ratchet.get("name").and_then(Value::as_str).unwrap_or("?");
            dropped.push(Dropped {
                at: format!("rules.ratchets[{name}]"),
                reason: "dependency-cruiser has no ratchets; count them with `rulebearing count`"
                    .into(),
            });
        }
    }
    if let Some(Value::Object(languages)) = canonical.remove("languages") {
        for language in languages.keys() {
            dropped.push(Dropped {
                at: format!("languages.{language}"),
                reason: "dependency-cruiser reads TypeScript and JavaScript only".into(),
            });
        }
    }
    if canonical.remove("$schema").is_some() {
        dropped.push(Dropped {
            at: "$schema".into(),
            reason: "points at the native schema".into(),
        });
    }
    if let Some(Value::String(one)) = canonical.get("extends").cloned()
        && one.starts_with("rulebearing:")
    {
        canonical.remove("extends");
        dropped.push(Dropped {
            at: format!("extends[{one}]"),
            reason: "a Rulebearing preset; expand it with `rulebearing config expand` first".into(),
        });
    }
    if let Some(Value::Array(entries)) = canonical.get_mut("extends") {
        entries.retain(|entry| {
            let native_preset = entry
                .as_str()
                .is_some_and(|e| e.starts_with("rulebearing:"));
            if native_preset {
                dropped.push(Dropped {
                    at: format!("extends[{}]", entry.as_str().unwrap_or_default()),
                    reason:
                        "a Rulebearing preset; expand it with `rulebearing config expand` first"
                            .into(),
                });
            }
            !native_preset
        });
        if entries.is_empty() {
            canonical.remove("extends");
        }
    }
    for list in ["forbidden", "allowed", "required"] {
        if let Some(Value::Array(rules)) = canonical.get_mut(list) {
            drop_cross_language_rules(rules, list, &mut dropped);
            for rule in rules.iter_mut() {
                let Value::Object(rule) = rule else { continue };
                let name = rule
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("unnamed")
                    .to_owned();
                for key in NATIVE_RULE_KEYS {
                    if rule.remove(*key).is_some() {
                        dropped.push(Dropped {
                            at: format!("{list}[{name}].{key}"),
                            reason: "a Rulebearing rule field".into(),
                        });
                    }
                }
            }
        }
    }
    Ok((canonical, dropped))
}

/// Leaves out every rule a cross-language key narrows: without the key the rule would match
/// more, so the whole rule goes, never the key alone.
fn drop_cross_language_rules(rules: &mut Vec<Value>, list: &str, dropped: &mut Vec<Dropped>) {
    rules.retain(|rule| {
        let keys: Vec<String> = ["from", "to"]
            .iter()
            .flat_map(|side| {
                rule.get(*side).map_or_else(Vec::new, |value| {
                    CROSS_LANGUAGE_KEYS
                        .iter()
                        .chain(EDGE_KEYS)
                        .filter(|k| value.get(**k).is_some())
                        .map(|k| format!("{side}.{k}"))
                        .collect()
                })
            })
            .collect();
        let mut keys = keys;
        if rule.get(GRAPH_KEY).is_some() {
            keys.push(GRAPH_KEY.to_owned());
        }
        if keys.is_empty() {
            return true;
        }
        let name = rule.get("name").and_then(Value::as_str).unwrap_or("unnamed");
        dropped.push(Dropped {
            at: format!("{list}[{name}]"),
            reason: format!(
                "narrowed by {}, a Rulebearing addition; without it the rule would match more, so the whole rule is left out",
                keys.join(", ")
            ),
        });
        false
    });
}

/// `config expand`: the native file with `defines` substituted and the shorthands expanded, so
/// nothing is hidden.
///
/// # Errors
/// See [`to_dependency_cruiser`].
pub fn expand(
    native_file: &Map<String, Value>,
    base_dir: &Path,
) -> Result<Map<String, Value>, ConfigError> {
    let mut canonical = native::to_canonical(native_file)?;
    defines::apply_defines(&mut canonical, base_dir)?;
    canonical.remove("defines");
    shorthands::expand(&mut canonical)?;
    Ok(native::from_canonical(&canonical))
}

/// Writes a configuration as YAML (the native default) or JSON.
///
/// # Errors
/// [`ConfigError::Invalid`] when serialisation fails.
pub fn render(value: &Map<String, Value>, yaml: bool) -> Result<String, ConfigError> {
    if yaml {
        serde_yaml::to_string(value).map_err(|e| ConfigError::Invalid(e.to_string()))
    } else {
        serde_json::to_string_pretty(value)
            .map(|mut s| {
                s.push('\n');
                s
            })
            .map_err(|e| ConfigError::Invalid(e.to_string()))
    }
}

/// Renders the drop list for the terminal.
pub fn describe(dropped: &[Dropped]) -> String {
    if dropped.is_empty() {
        return "nothing dropped\n".to_owned();
    }
    let mut out = format!(
        "dropped {} item(s) dependency-cruiser cannot hold:\n",
        dropped.len()
    );
    for d in dropped {
        let _ = writeln!(out, "  {}: {}", d.at, d.reason);
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
    fn dependency_cruiser_to_native_and_back_is_identity() -> Result<(), ConfigError> {
        let dc = object(json!({
            "extends": "dependency-cruiser/configs/recommended",
            "forbidden": [{ "name": "a", "severity": "error", "from": { "path": ["x", "y"] }, "to": { "circular": true } }],
            "allowed": [{ "from": {}, "to": { "path": "z" } }],
            "allowedSeverity": "info",
            "required": [{ "name": "r", "module": { "path": "m" }, "to": { "path": "t" } }],
            "options": { "tsConfig": { "fileName": "tsconfig.json" }, "doNotFollow": "node_modules" }
        }));
        let native_file = to_native(&dc);
        assert!(native_file.contains_key("rules"));
        let (back, dropped) = to_dependency_cruiser(&native_file, Path::new("."))?;
        assert!(dropped.is_empty(), "{dropped:?}");
        assert_eq!(back, dc);
        let one = object(json!({ "extends": "rulebearing:typescript" }));
        let (back, dropped) = to_dependency_cruiser(&to_native(&one), Path::new("."))?;
        assert!(!back.contains_key("extends"));
        assert_eq!(dropped.len(), 1);
        Ok(())
    }

    #[test]
    fn native_to_dependency_cruiser_says_what_it_dropped() -> Result<(), ConfigError> {
        let native_file = object(json!({
            "$schema": "s",
            "extends": ["rulebearing:recommended"],
            "languages": { "dotnet": { "solution": "a.sln" } },
            "rules": {
                "dependencies": { "forbidden": [{ "name": "a", "fix": "f", "examples": { "allowed": [] }, "owner": "o", "expires": "2030-01-01", "allowEmpty": true, "from": {}, "to": {} }] },
                "ratchets": [{ "name": "r", "from": {}, "to": {}, "budget": "b.json" }],
                "layers": [{ "name": "l", "layers": ["^a/", "^b/"] }]
            }
        }));
        let (dc, dropped) = to_dependency_cruiser(&native_file, Path::new("."))?;
        let at: Vec<&str> = dropped.iter().map(|d| d.at.as_str()).collect();
        for expected in [
            "rules.ratchets[r]",
            "languages.dotnet",
            "$schema",
            "extends[rulebearing:recommended]",
            "forbidden[a].fix",
            "forbidden[a].examples",
            "forbidden[a].owner",
            "forbidden[a].expires",
            "forbidden[a].allowEmpty",
            "rules.layers, rules.independence",
        ] {
            assert!(at.contains(&expected), "{expected} in {at:?}");
        }
        assert_eq!(dc["forbidden"].as_array().map(Vec::len), Some(2));
        assert!(!dc.contains_key("extends"));
        assert!(describe(&dropped).contains("dropped 10"));
        assert_eq!(describe(&[]), "nothing dropped\n");
        Ok(())
    }

    #[test]
    fn a_rule_with_cross_language_keys_is_dropped_whole() -> Result<(), ConfigError> {
        let native_file = object(json!({ "rules": { "dependencies": {
            "forbidden": [
                { "name": "web-not-infra", "from": { "language": "dotnet", "namespace": "^Web" }, "to": { "dependencyKind": "inherits" } },
                { "name": "kept", "from": { "path": "^a" }, "to": { "path": "^b" } }
            ],
            "allowed": [{ "from": {}, "to": { "assemblyNot": "^Legacy" } }]
        } } }));
        let (dc, dropped) = to_dependency_cruiser(&native_file, Path::new("."))?;
        assert_eq!(
            dc["forbidden"],
            json!([{ "name": "kept", "from": { "path": "^a" }, "to": { "path": "^b" } }])
        );
        assert_eq!(dc["allowed"], json!([]));
        let described: Vec<(&str, &str)> = dropped
            .iter()
            .map(|d| (d.at.as_str(), d.reason.as_str()))
            .collect();
        assert_eq!(described.len(), 2);
        assert_eq!(described[0].0, "forbidden[web-not-infra]");
        assert!(
            described[0]
                .1
                .starts_with("narrowed by from.language, from.namespace, to.dependencyKind,"),
            "{}",
            described[0].1
        );
        assert_eq!(described[1].0, "allowed[unnamed]");
        assert!(described[1].1.contains("to.assemblyNot"));
        Ok(())
    }

    #[test]
    fn a_rule_with_graph_is_dropped_whole() -> Result<(), ConfigError> {
        let native_file = object(json!({ "rules": { "dependencies": { "forbidden": [
            { "name": "narrowed", "from": { "path": "^a" }, "to": { "path": "^b", "reachable": true }, "graph": { "chainsThrough": "^a" } },
            { "name": "both", "from": { "language": "python" }, "to": {}, "graph": { "modulesNot": "^n" } },
            { "name": "kept", "from": { "path": "^a" }, "to": { "path": "^b" } }
        ] } } }));
        let (dc, dropped) = to_dependency_cruiser(&native_file, Path::new("."))?;
        assert_eq!(
            dc["forbidden"],
            json!([{ "name": "kept", "from": { "path": "^a" }, "to": { "path": "^b" } }])
        );
        let described: Vec<(&str, &str)> = dropped
            .iter()
            .map(|d| (d.at.as_str(), d.reason.as_str()))
            .collect();
        assert_eq!(described.len(), 2);
        assert_eq!(described[0].0, "forbidden[narrowed]");
        assert!(
            described[0]
                .1
                .starts_with("narrowed by graph, a Rulebearing addition"),
            "{}",
            described[0].1
        );
        assert!(
            described[1]
                .1
                .starts_with("narrowed by from.language, graph,"),
            "{}",
            described[1].1
        );
        Ok(())
    }

    #[test]
    fn expand_shows_the_shorthands_as_rules() -> Result<(), ConfigError> {
        let native_file = object(
            json!({ "rules": { "independence": [{ "name": "i", "pattern": "^f/([^/]+)/" }] } }),
        );
        let expanded = expand(&native_file, Path::new("."))?;
        assert_eq!(
            expanded["rules"]["dependencies"]["forbidden"][0]["to"]["pathNot"],
            "^f/$1/"
        );
        let yaml = render(&expanded, true)?;
        assert!(yaml.contains("pathNot"));
        let json_text = render(&expanded, false)?;
        assert!(json_text.ends_with("}\n"));
        Ok(())
    }
}
