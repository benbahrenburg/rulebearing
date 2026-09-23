//! The `layers` and `independence` shorthands, expanded into `forbidden` rules.
//!
//! - Source: [design § Shorthands](../../../docs/artifacts/design.md#shorthands)
//! - Plan: [Wave 1, Step 4](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-4-config-convert-config-expand-config-lint-shorthands-1a)
//! - Requirement: [FR-RULE-07](../../../docs/prd.md#fr-rule-07)
//!
//! `layers` lists path patterns from the highest layer to the lowest, as import-linter's layers
//! contract does. Each lower layer gets one rule forbidding it to depend on each higher layer,
//! named `<name>:<lower>-to-<higher>` with 1-based layer numbers. `independence` takes a pattern
//! with exactly one capturing group and becomes one `$1` fence: a module in one group may not
//! depend on a module in another. `rulebearing config expand` prints the result.

use serde_json::{Map, Value, json};

use crate::ConfigError;
use crate::model::{IndependenceShorthand, LayersShorthand};

/// Expands one `layers` entry.
pub fn expand_layers(layers: &LayersShorthand) -> Vec<Value> {
    let severity = layers.severity.map_or("error", |s| s.as_str());
    let mut rules = Vec::new();
    for (lower, lower_path) in layers.layers.iter().enumerate().skip(1) {
        for (higher, higher_path) in layers.layers.iter().enumerate().take(lower) {
            let mut rule = Map::new();
            rule.insert(
                "name".into(),
                json!(format!("{}:{}-to-{}", layers.name, lower + 1, higher + 1)),
            );
            if let Some(comment) = &layers.comment {
                rule.insert("comment".into(), json!(comment));
            }
            if let Some(fix) = &layers.fix {
                rule.insert("fix".into(), json!(fix));
            }
            rule.insert("severity".into(), json!(severity));
            rule.insert("from".into(), json!({ "path": lower_path }));
            rule.insert("to".into(), json!({ "path": higher_path }));
            if layers.allow_empty {
                rule.insert("allowEmpty".into(), json!(true));
            }
            rules.push(Value::Object(rule));
        }
    }
    rules
}

/// The span of the first capturing group of a JavaScript pattern, and how many there are.
fn capture_groups(pattern: &str) -> (Option<(usize, usize)>, usize) {
    let bytes = pattern.as_bytes();
    let mut first_start = None;
    let mut first = None;
    let mut count = 0;
    let mut open: Vec<usize> = Vec::new();
    let mut in_class = false;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 1,
            b'[' if !in_class => in_class = true,
            b']' if in_class => in_class = false,
            b'(' if !in_class => {
                let capturing = bytes.get(i + 1) != Some(&b'?')
                    || (bytes.get(i + 2) == Some(&b'<')
                        && !matches!(bytes.get(i + 3), Some(b'=' | b'!')));
                if capturing {
                    count += 1;
                    first_start.get_or_insert(i);
                }
                open.push(i);
            }
            b')' if !in_class => {
                let closed = open.pop();
                if closed == first_start && first.is_none() {
                    first = first_start.map(|start| (start, i + 1));
                }
            }
            _ => {}
        }
        i += 1;
    }
    (first, count)
}

/// Expands one `independence` entry.
///
/// # Errors
/// [`ConfigError::Invalid`] when the pattern does not have exactly one capturing group.
pub fn expand_independence(entry: &IndependenceShorthand) -> Result<Value, ConfigError> {
    let (first, count) = capture_groups(&entry.pattern);
    let Some((start, end)) = first.filter(|_| count == 1) else {
        return Err(ConfigError::Invalid(format!(
            "independence `{}`: the pattern must have exactly one capturing group naming the module, found {count}",
            entry.name
        )));
    };
    let fence = format!("{}$1{}", &entry.pattern[..start], &entry.pattern[end..]);
    let mut rule = Map::new();
    rule.insert("name".into(), json!(entry.name));
    if let Some(comment) = &entry.comment {
        rule.insert("comment".into(), json!(comment));
    }
    if let Some(fix) = &entry.fix {
        rule.insert("fix".into(), json!(fix));
    }
    rule.insert(
        "severity".into(),
        json!(entry.severity.map_or("error", |s| s.as_str())),
    );
    rule.insert("from".into(), json!({ "path": entry.pattern }));
    rule.insert(
        "to".into(),
        json!({ "path": entry.pattern, "pathNot": fence }),
    );
    if entry.allow_empty {
        rule.insert("allowEmpty".into(), json!(true));
    }
    Ok(Value::Object(rule))
}

/// Parsed shorthands and the rules they expand to.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Expanded {
    /// The `layers` entries.
    pub layers: Vec<LayersShorthand>,
    /// The `independence` entries.
    pub independence: Vec<IndependenceShorthand>,
}

/// Removes `layers` and `independence` from a canonical configuration and appends their
/// expansion to `forbidden`.
///
/// # Errors
/// [`ConfigError::Invalid`] for a malformed entry.
pub fn expand(canonical: &mut Map<String, Value>) -> Result<Expanded, ConfigError> {
    let invalid =
        |what: &str, e: serde_json::Error| ConfigError::Invalid(format!("`rules.{what}`: {e}"));
    let layers: Vec<LayersShorthand> = canonical
        .remove("layers")
        .map(serde_json::from_value)
        .transpose()
        .map_err(|e| invalid("layers", e))?
        .unwrap_or_default();
    let independence: Vec<IndependenceShorthand> = canonical
        .remove("independence")
        .map(serde_json::from_value)
        .transpose()
        .map_err(|e| invalid("independence", e))?
        .unwrap_or_default();
    let mut added: Vec<Value> = layers.iter().flat_map(expand_layers).collect();
    for entry in &independence {
        added.push(expand_independence(entry)?);
    }
    if !added.is_empty() {
        match canonical.get_mut("forbidden") {
            Some(Value::Array(forbidden)) => forbidden.extend(added),
            _ => {
                canonical.insert("forbidden".into(), Value::Array(added));
            }
        }
    }
    Ok(Expanded {
        layers,
        independence,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layers(names: &[&str]) -> LayersShorthand {
        LayersShorthand {
            name: "clean".into(),
            comment: Some("adr:0001".into()),
            fix: Some("invert it".into()),
            severity: None,
            layers: names.iter().map(|s| (*s).to_owned()).collect(),
            allow_empty: true,
        }
    }

    #[test]
    fn layers_give_one_rule_per_lower_to_higher_pair() {
        let rules = expand_layers(&layers(&["^src/Web/", "^src/Application/", "^src/Domain/"]));
        let pairs: Vec<(String, String, String)> = rules
            .iter()
            .map(|r| {
                (
                    r["name"].as_str().unwrap_or_default().to_owned(),
                    r["from"]["path"].as_str().unwrap_or_default().to_owned(),
                    r["to"]["path"].as_str().unwrap_or_default().to_owned(),
                )
            })
            .collect();
        assert_eq!(
            pairs,
            [
                (
                    "clean:2-to-1".into(),
                    "^src/Application/".into(),
                    "^src/Web/".into()
                ),
                (
                    "clean:3-to-1".into(),
                    "^src/Domain/".into(),
                    "^src/Web/".into()
                ),
                (
                    "clean:3-to-2".into(),
                    "^src/Domain/".into(),
                    "^src/Application/".into()
                ),
            ]
        );
        assert_eq!(rules[0]["severity"], "error");
        assert_eq!(rules[0]["fix"], "invert it");
        assert_eq!(rules[0]["allowEmpty"], true);
        assert!(expand_layers(&layers(&["^a/"])).is_empty());
    }

    #[test]
    fn independence_is_one_dollar_one_fence() -> Result<(), ConfigError> {
        let entry = IndependenceShorthand {
            name: "features".into(),
            comment: None,
            fix: None,
            severity: None,
            pattern: "^apps/web/src/features/([^/]+)/".into(),
            allow_empty: false,
        };
        let rule = expand_independence(&entry)?;
        assert_eq!(rule["from"]["path"], "^apps/web/src/features/([^/]+)/");
        assert_eq!(rule["to"]["pathNot"], "^apps/web/src/features/$1/");
        assert!(rule.get("comment").is_none());
        Ok(())
    }

    #[test]
    fn independence_needs_exactly_one_group() {
        for pattern in ["^a/", "^(a)/(b)/", "^(?:a)/"] {
            let entry = IndependenceShorthand {
                name: "x".into(),
                pattern: pattern.into(),
                ..IndependenceShorthand::default()
            };
            assert!(expand_independence(&entry).is_err(), "{pattern}");
        }
        assert_eq!(capture_groups(r"^\(x\)/([a(]+)/"), (Some((7, 14)), 1));
        assert_eq!(capture_groups("((a))").1, 2);
        assert_eq!(capture_groups("(?<n>a)"), (Some((0, 7)), 1));
    }

    #[test]
    fn expand_moves_shorthands_into_forbidden() -> Result<(), ConfigError> {
        let mut canonical = match json!({
            "forbidden": [{ "name": "existing" }],
            "layers": [{ "name": "l", "layers": ["^a/", "^b/"] }],
            "independence": [{ "name": "i", "pattern": "^f/([^/]+)/" }]
        }) {
            Value::Object(m) => m,
            _ => Map::new(),
        };
        let expanded = expand(&mut canonical)?;
        assert_eq!(expanded.layers.len(), 1);
        assert_eq!(expanded.independence.len(), 1);
        assert_eq!(canonical["forbidden"].as_array().map(Vec::len), Some(3));
        assert!(!canonical.contains_key("layers"));
        let mut empty = Map::new();
        empty.insert(
            "layers".into(),
            json!([{ "name": "l", "layers": ["^a/", "^b/"] }]),
        );
        expand(&mut empty)?;
        assert_eq!(empty["forbidden"].as_array().map(Vec::len), Some(1));
        let mut bad = Map::new();
        bad.insert("layers".into(), json!([{ "nope": 1 }]));
        assert!(expand(&mut bad).is_err());
        Ok(())
    }
}
