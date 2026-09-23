//! The native format, mapped onto dependency-cruiser's shape and back.
//!
//! - Decision: [ADR-0005](../../../docs/adr/0005-native-config-superset-and-compat.md) (a strict
//!   superset: every dependency-cruiser key legal at the same place, nothing renamed)
//! - Source: [design § The native format](../../../docs/artifacts/design.md#the-native-format)
//! - Plan: [Wave 1, Step 1](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-1-config-model-and-the-two-front-ends-1a)
//! - Requirements: [FR-CFG-02](../../../docs/prd.md#fr-cfg-02), [FR-CFG-06](../../../docs/prd.md#fr-cfg-06)
//!
//! Internally every configuration is *canonical*: a dependency-cruiser-shaped object
//! (`forbidden`, `allowed`, `allowedSeverity`, `required`, `options`, `extends`) plus the native
//! additions at the top level (`$schema`, `defines`, `languages` for .NET and Python, `ratchets`,
//! `layers`, `independence`). [`to_canonical`] and [`from_canonical`] are the two directions;
//! a dependency-cruiser file is already canonical.
//!
//! | Native | Canonical |
//! | --- | --- |
//! | `rules.dependencies.{forbidden, allowed, allowedSeverity, required}` | the same keys at the top level |
//! | `rules.ratchets`, `rules.layers`, `rules.independence` | `ratchets`, `layers`, `independence` |
//! | `languages.typescript.*` and the flat names at the top level | `options.*` |
//! | `languages.dotnet`, `languages.python` | `languages.dotnet`, `languages.python` |
//! | `options`, `extends`, `defines`, `$schema`, `allowEmpty` | unchanged |

use serde_json::{Map, Value};

use crate::ConfigError;

/// The rule list keys dependency-cruiser defines.
pub const RULE_SET_KEYS: &[&str] = &["forbidden", "allowed", "allowedSeverity", "required"];

/// dependency-cruiser's flat option names that alias into `languages.typescript` when written at
/// the top level of a native file (design § The native format).
pub const FLAT_TYPESCRIPT_ALIASES: &[&str] = &[
    "tsConfig",
    "tsPreCompilationDeps",
    "babelConfig",
    "webpackConfig",
    "enhancedResolveOptions",
    "moduleSystems",
    "parser",
];

/// The top-level keys a native file may carry.
pub const NATIVE_TOP_LEVEL: &[&str] = &[
    "$schema",
    "extends",
    "defines",
    "allowEmpty",
    "languages",
    "options",
    "rules",
    "forbidden",
    "allowed",
    "allowedSeverity",
    "required",
];

/// The rule families under `rules` that later waves deliver.
const LATER_FAMILIES: &[(&str, u8)] = &[("elements", 2), ("slices", 2), ("diagrams", 2)];

/// Whether a parsed file is in the native shape (it has a key only the native format has).
pub fn looks_native(value: &Map<String, Value>) -> bool {
    ["rules", "languages", "defines"]
        .iter()
        .any(|k| value.contains_key(*k))
        || FLAT_TYPESCRIPT_ALIASES
            .iter()
            .any(|k| value.contains_key(*k))
}

/// Maps a native file onto the canonical shape.
///
/// # Errors
/// [`ConfigError::Invalid`] for a key the format does not define, and
/// [`ConfigError::NotYetSupported`] for a rule family a later wave delivers.
pub fn to_canonical(native: &Map<String, Value>) -> Result<Map<String, Value>, ConfigError> {
    let mut out = Map::new();
    let mut options = match native.get("options") {
        Some(Value::Object(options)) => options.clone(),
        Some(_) => return Err(ConfigError::Invalid("`options` must be an object".into())),
        None => Map::new(),
    };
    for (key, value) in native {
        match key.as_str() {
            "$schema" | "extends" | "defines" | "allowEmpty" => {
                out.insert(key.clone(), value.clone());
            }
            "options" | "rules" | "languages" => {}
            k if RULE_SET_KEYS.contains(&k) => {
                out.insert(key.clone(), value.clone());
            }
            k if FLAT_TYPESCRIPT_ALIASES.contains(&k) => {
                options.insert(key.clone(), value.clone());
            }
            other => {
                return Err(ConfigError::Invalid(format!(
                    "`{other}` is not a key of the native format; the top level holds {}",
                    NATIVE_TOP_LEVEL.join(", ")
                )));
            }
        }
    }
    if let Some(languages) = native.get("languages") {
        let Value::Object(languages) = languages else {
            return Err(ConfigError::Invalid("`languages` must be an object".into()));
        };
        let mut others = Map::new();
        for (language, block) in languages {
            match (language.as_str(), block) {
                ("typescript", Value::Object(typescript)) => {
                    for (key, value) in typescript {
                        options.insert(key.clone(), value.clone());
                    }
                }
                ("dotnet" | "python", _) => {
                    others.insert(language.clone(), block.clone());
                }
                (other, _) => {
                    return Err(ConfigError::Invalid(format!(
                        "`languages.{other}` is not a language; use typescript, dotnet or python"
                    )));
                }
            }
        }
        if !others.is_empty() {
            out.insert("languages".into(), Value::Object(others));
        }
    }
    if let Some(rules) = native.get("rules") {
        let Value::Object(rules) = rules else {
            return Err(ConfigError::Invalid("`rules` must be an object".into()));
        };
        for (family, value) in rules {
            match family.as_str() {
                "dependencies" => {
                    let Value::Object(dependencies) = value else {
                        return Err(ConfigError::Invalid(
                            "`rules.dependencies` must be an object".into(),
                        ));
                    };
                    for (key, list) in dependencies {
                        if !RULE_SET_KEYS.contains(&key.as_str()) {
                            return Err(ConfigError::Invalid(format!(
                                "`rules.dependencies.{key}` is not a rule list; use {}",
                                RULE_SET_KEYS.join(", ")
                            )));
                        }
                        append(&mut out, key, list.clone());
                    }
                }
                "ratchets" | "layers" | "independence" => {
                    out.insert(family.clone(), value.clone());
                }
                other => {
                    if let Some((_, wave)) = LATER_FAMILIES.iter().find(|(f, _)| *f == other) {
                        return Err(ConfigError::NotYetSupported {
                            key: format!("rules.{other}"),
                            wave: *wave,
                        });
                    }
                    return Err(ConfigError::Invalid(format!(
                        "`rules.{other}` is not a rule family; use dependencies, ratchets, layers or independence"
                    )));
                }
            }
        }
    }
    if !options.is_empty() {
        out.insert("options".into(), Value::Object(options));
    }
    Ok(out)
}

/// Appends a rule list to `out[key]`, or sets a scalar (`allowedSeverity`).
fn append(out: &mut Map<String, Value>, key: &str, value: Value) {
    match (out.get_mut(key), value) {
        (Some(Value::Array(existing)), Value::Array(more)) => existing.extend(more),
        (_, value) => {
            out.insert(key.to_owned(), value);
        }
    }
}

/// Maps a canonical configuration onto the native shape. The inverse of [`to_canonical`] for
/// every file [`to_canonical`] produced from a native file with no flat aliases.
pub fn from_canonical(canonical: &Map<String, Value>) -> Map<String, Value> {
    let mut out = Map::new();
    let mut dependencies = Map::new();
    let mut rules = Map::new();
    for (key, value) in canonical {
        match key.as_str() {
            k if RULE_SET_KEYS.contains(&k) => {
                dependencies.insert(key.clone(), value.clone());
            }
            "ratchets" | "layers" | "independence" => {
                rules.insert(key.clone(), value.clone());
            }
            _ => {
                out.insert(key.clone(), value.clone());
            }
        }
    }
    if !dependencies.is_empty() {
        rules.insert("dependencies".into(), Value::Object(dependencies));
    }
    if !rules.is_empty() {
        out.insert("rules".into(), Value::Object(rules));
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
    fn the_design_example_maps_onto_dependency_cruisers_shape() -> Result<(), ConfigError> {
        let native = object(json!({
            "$schema": "s",
            "extends": ["rulebearing:recommended"],
            "languages": {
                "typescript": { "tsConfig": { "fileName": "tsconfig.json" }, "tsPreCompilationDeps": true },
                "dotnet": { "solution": "a.slnx" }
            },
            "options": { "exclude": { "path": "node_modules" } },
            "rules": {
                "dependencies": { "forbidden": [{ "name": "a" }], "required": [{ "name": "b" }] },
                "ratchets": [{ "name": "r" }]
            }
        }));
        let canonical = to_canonical(&native)?;
        assert_eq!(canonical["forbidden"][0]["name"], "a");
        assert_eq!(canonical["required"][0]["name"], "b");
        assert_eq!(canonical["options"]["tsPreCompilationDeps"], true);
        assert_eq!(canonical["options"]["exclude"]["path"], "node_modules");
        assert_eq!(canonical["languages"]["dotnet"]["solution"], "a.slnx");
        assert_eq!(canonical["ratchets"][0]["name"], "r");
        assert_eq!(canonical["$schema"], "s");
        Ok(())
    }

    #[test]
    fn flat_and_nested_typescript_options_load_to_the_same_value() -> Result<(), ConfigError> {
        let flat = to_canonical(&object(json!({ "tsConfig": { "fileName": "t.json" } })))?;
        let nested = to_canonical(&object(
            json!({ "languages": { "typescript": { "tsConfig": { "fileName": "t.json" } } } }),
        ))?;
        let options = to_canonical(&object(
            json!({ "options": { "tsConfig": { "fileName": "t.json" } } }),
        ))?;
        assert_eq!(flat, nested);
        assert_eq!(flat, options);
        Ok(())
    }

    #[test]
    fn dependency_cruiser_keys_are_legal_at_the_same_place() -> Result<(), ConfigError> {
        let native = object(
            json!({ "forbidden": [{ "name": "x" }], "allowedSeverity": "error", "allowed": [] }),
        );
        let canonical = to_canonical(&native)?;
        assert_eq!(canonical, native);
        Ok(())
    }

    #[test]
    fn unknown_and_later_keys_are_refused() {
        for (value, needle) in [
            (json!({ "bogus": 1 }), "bogus"),
            (json!({ "rules": { "elements": [] } }), "wave 2"),
            (json!({ "rules": { "nope": [] } }), "nope"),
            (
                json!({ "rules": { "dependencies": { "denied": [] } } }),
                "denied",
            ),
            (json!({ "languages": { "go": {} } }), "go"),
            (json!({ "options": 1 }), "options"),
            (json!({ "rules": 1 }), "rules"),
            (json!({ "languages": 1 }), "languages"),
            (json!({ "rules": { "dependencies": 1 } }), "dependencies"),
        ] {
            let error = to_canonical(&object(value))
                .err()
                .map(|e| e.to_string())
                .unwrap_or_default();
            assert!(error.contains(needle), "{needle}: {error}");
        }
    }

    #[test]
    fn round_trip_through_both_directions() -> Result<(), ConfigError> {
        let canonical = object(json!({
            "forbidden": [{ "name": "a", "from": {}, "to": {} }],
            "allowed": [{ "from": {}, "to": {} }],
            "allowedSeverity": "warn",
            "options": { "doNotFollow": "node_modules" },
            "ratchets": [{ "name": "r" }],
            "extends": "x"
        }));
        let native = from_canonical(&canonical);
        assert!(native.contains_key("rules"));
        assert!(!native.contains_key("forbidden"));
        assert_eq!(to_canonical(&native)?, canonical);
        Ok(())
    }

    #[test]
    fn native_shape_is_detected() {
        assert!(looks_native(&object(json!({ "rules": {} }))));
        assert!(looks_native(&object(json!({ "tsConfig": {} }))));
        assert!(!looks_native(&object(json!({ "forbidden": [] }))));
    }
}
