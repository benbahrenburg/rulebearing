//! `defines`: named values read from JSON files and substituted into patterns as `${name}`.
//!
//! - Source: [design § The native format](../../../docs/artifacts/design.md#the-native-format)
//!   ("the declarative replacement for computed JavaScript")
//! - Plan: [Wave 1, Step 3](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-3-extends-presets-defines-captures-regex-1a)
//! - Requirement: [FR-CFG-04](../../../docs/prd.md#fr-cfg-04)
//!
//! A define reads a JSON file relative to the configuration, selects values with a small path
//! expression (dot-separated keys, `[*]` for every element, `[n]` for one), escapes each
//! selected value so it matches only itself, and joins them with `joinWith` (default `|`).
//! Substitution happens before any pattern is compiled, and only inside pattern-valued keys, so
//! a comment that mentions `${name}` is left alone.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Map, Value};

use crate::ConfigError;
use crate::model::Define;
use crate::pattern::escape_javascript;

/// The keys whose string values (or arrays of strings) are patterns.
pub const PATTERN_KEYS: &[&str] = &[
    "path",
    "pathNot",
    "license",
    "licenseNot",
    "exoticRequire",
    "exoticRequireNot",
    "via",
    "viaOnly",
    "viaNot",
    "viaSomeNot",
    "collapse",
    "collapsePattern",
    "includeOnly",
    "focus",
    "reaches",
    "highlight",
    "exclude",
    "doNotFollow",
    "pattern",
    "layers",
];

/// One step of a `select` expression.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    Key(String),
    Every,
    Index(usize),
}

fn parse_select(select: &str) -> Result<Vec<Step>, String> {
    let mut steps = Vec::new();
    for segment in select.split('.').filter(|s| !s.is_empty()) {
        let (key, mut rest) = segment
            .split_once('[')
            .map_or((segment, ""), |(k, r)| (k, r));
        if !key.is_empty() {
            steps.push(Step::Key(key.to_owned()));
        }
        while !rest.is_empty() {
            let (inside, after) = rest
                .split_once(']')
                .ok_or_else(|| format!("unclosed `[` in `{select}`"))?;
            steps.push(if inside == "*" {
                Step::Every
            } else {
                Step::Index(
                    inside
                        .parse()
                        .map_err(|_| format!("`[{inside}]` is not `[*]` or an index"))?,
                )
            });
            rest = after.strip_prefix('[').unwrap_or(after);
            if !after.is_empty() && !after.starts_with('[') {
                return Err(format!("unexpected `{after}` in `{select}`"));
            }
        }
    }
    Ok(steps)
}

fn apply(values: Vec<Value>, step: &Step) -> Vec<Value> {
    values
        .into_iter()
        .flat_map(|value| match (step, value) {
            (Step::Key(key), Value::Object(mut map)) => map.remove(key).into_iter().collect(),
            (Step::Every, Value::Array(items)) => items,
            (Step::Every, Value::Object(map)) => map.into_iter().map(|(_, v)| v).collect(),
            (Step::Index(n), Value::Array(mut items)) if *n < items.len() => {
                vec![items.swap_remove(*n)]
            }
            _ => Vec::new(),
        })
        .collect()
}

fn scalar(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// Evaluates one define: the escaped, joined text it substitutes.
///
/// # Errors
/// [`ConfigError::Define`] when the file cannot be read, the selection is malformed or selects
/// nothing that is a string, a number or a boolean.
pub fn evaluate(name: &str, define: &Define, base_dir: &Path) -> Result<String, ConfigError> {
    let fail = |reason: String| ConfigError::Define {
        name: name.to_owned(),
        reason,
    };
    let file = base_dir.join(&define.from_json);
    let text =
        std::fs::read_to_string(&file).map_err(|e| fail(format!("{}: {e}", file.display())))?;
    let json: Value =
        serde_json::from_str(&text).map_err(|e| fail(format!("{}: {e}", file.display())))?;
    let steps = parse_select(define.select.as_deref().unwrap_or("")).map_err(fail)?;
    let mut values = vec![json];
    for step in &steps {
        values = apply(values, step);
    }
    // A selection that ends on an array selects its elements.
    if let [Value::Array(items)] = values.as_slice() {
        let items = items.clone();
        values = items;
    }
    let texts: Vec<String> = values
        .iter()
        .map(|v| scalar(v).map(|s| escape_javascript(&s)))
        .collect::<Option<_>>()
        .ok_or_else(|| fail("the selection must yield strings, numbers or booleans".into()))?;
    if texts.is_empty() {
        return Err(fail(format!(
            "`select` matched nothing in {}",
            define.from_json
        )));
    }
    Ok(texts.join(define.join_with.as_deref().unwrap_or("|")))
}

/// Substitutes every `${name}` inside pattern-valued keys of `value`.
pub fn substitute(value: &mut Value, values: &BTreeMap<String, String>) {
    substitute_in(value, values, false);
}

fn substitute_in(value: &mut Value, values: &BTreeMap<String, String>, in_pattern: bool) {
    match value {
        Value::String(text) if in_pattern && text.contains("${") => {
            for (name, replacement) in values {
                *text = text.replace(&format!("${{{name}}}"), replacement);
            }
        }
        Value::Array(items) => {
            for item in items {
                substitute_in(item, values, in_pattern);
            }
        }
        Value::Object(map) => substitute_map(map, values),
        _ => {}
    }
}

fn substitute_map(map: &mut Map<String, Value>, values: &BTreeMap<String, String>) {
    for (key, inner) in map.iter_mut() {
        substitute_in(inner, values, PATTERN_KEYS.contains(&key.as_str()));
    }
}

/// Reads `defines` from a canonical configuration, evaluates each, and substitutes them.
///
/// # Errors
/// [`ConfigError::Define`] for a define that cannot be evaluated or a `${name}` left unresolved.
pub fn apply_defines(
    canonical: &mut Map<String, Value>,
    base_dir: &Path,
) -> Result<(), ConfigError> {
    let defines = canonical
        .get("defines")
        .cloned()
        .unwrap_or_else(|| Value::Object(Map::new()));
    let defines: BTreeMap<String, Define> =
        serde_json::from_value(defines).map_err(|e| ConfigError::Define {
            name: "defines".into(),
            reason: e.to_string(),
        })?;
    let mut values = BTreeMap::new();
    for (name, define) in &defines {
        values.insert(name.clone(), evaluate(name, define, base_dir)?);
    }
    for (key, value) in canonical.iter_mut() {
        if key != "defines" {
            substitute_in(value, &values, PATTERN_KEYS.contains(&key.as_str()));
        }
    }
    let mut leftover = None;
    for (key, value) in canonical.iter() {
        if key != "defines" {
            find_unresolved(value, PATTERN_KEYS.contains(&key.as_str()), &mut leftover);
        }
    }
    match leftover {
        Some(name) => Err(ConfigError::Define {
            reason: format!("`${{{name}}}` is used but not defined under `defines`"),
            name,
        }),
        None => Ok(()),
    }
}

/// The first `${identifier}` left in a pattern-valued string.
fn find_unresolved(value: &Value, in_pattern: bool, found: &mut Option<String>) {
    if found.is_some() {
        return;
    }
    match value {
        Value::String(text) if in_pattern => {
            let mut rest = text.as_str();
            while let Some(start) = rest.find("${") {
                let after = &rest[start + 2..];
                if let Some(end) = after.find('}') {
                    let name = &after[..end];
                    if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                        *found = Some(name.to_owned());
                        return;
                    }
                }
                rest = after;
            }
        }
        Value::Array(items) => {
            for item in items {
                find_unresolved(item, in_pattern, found);
            }
        }
        Value::Object(map) => {
            for (key, inner) in map {
                find_unresolved(inner, PATTERN_KEYS.contains(&key.as_str()), found);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn dir_with(file: &str, text: &str) -> std::io::Result<std::path::PathBuf> {
        let dir = std::env::temp_dir().join(format!(
            "rb-defines-{}-{}",
            std::process::id(),
            file.replace('/', "_")
        ));
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join(file), text)?;
        Ok(dir)
    }

    fn define(from: &str, select: Option<&str>, join: Option<&str>) -> Define {
        Define {
            from_json: from.into(),
            select: select.map(str::to_owned),
            join_with: join.map(str::to_owned),
        }
    }

    #[test]
    fn the_design_example_selects_and_joins() -> Result<(), Box<dyn std::error::Error>> {
        let dir = dir_with(
            "exceptions.json",
            r#"[{"app": "legacy-a"}, {"app": "legacy.b"}, {"other": 1}]"#,
        )?;
        let value = evaluate(
            "legacyApps",
            &define("exceptions.json", Some("[*].app"), Some("|")),
            &dir,
        )?;
        assert_eq!(value, r"legacy-a|legacy\.b");
        let _ = std::fs::remove_dir_all(dir);
        Ok(())
    }

    #[test]
    fn select_expressions() -> Result<(), Box<dyn std::error::Error>> {
        let dir = dir_with(
            "data.json",
            r#"{"a": {"b": ["x", "y", "z"]}, "n": 3, "m": {"k": "v"}}"#,
        )?;
        assert_eq!(
            evaluate("d", &define("data.json", Some("a.b"), None), &dir)?,
            "x|y|z"
        );
        assert_eq!(
            evaluate("d", &define("data.json", Some("a.b[1]"), None), &dir)?,
            "y"
        );
        assert_eq!(
            evaluate("d", &define("data.json", Some("a.b[*]"), Some(",")), &dir)?,
            "x,y,z"
        );
        assert_eq!(
            evaluate("d", &define("data.json", Some("n"), None), &dir)?,
            "3"
        );
        assert_eq!(
            evaluate("d", &define("data.json", Some("m[*]"), None), &dir)?,
            "v"
        );
        for bad in ["a[", "a[x]", "a]b", "missing", "a"] {
            assert!(
                evaluate("d", &define("data.json", Some(bad), None), &dir).is_err(),
                "{bad}"
            );
        }
        assert!(evaluate("d", &define("absent.json", None, None), &dir).is_err());
        let _ = std::fs::remove_dir_all(dir);
        Ok(())
    }

    #[test]
    fn substitution_touches_patterns_only() {
        let mut value = json!({
            "forbidden": [{ "comment": "keeps ${x}", "from": { "path": "^apps/(${x})/" }, "to": { "pathNot": ["${x}", "y"] } }]
        });
        let values = BTreeMap::from([("x".to_owned(), "a|b".to_owned())]);
        if let Value::Object(map) = &mut value {
            substitute_map(map, &values);
        }
        assert_eq!(value["forbidden"][0]["from"]["path"], "^apps/(a|b)/");
        assert_eq!(value["forbidden"][0]["to"]["pathNot"][0], "a|b");
        assert_eq!(value["forbidden"][0]["comment"], "keeps ${x}");
        let mut plain = json!("${x}");
        substitute(&mut plain, &values);
        assert_eq!(plain, "${x}", "a bare string is not a pattern");
    }

    #[test]
    fn an_undefined_name_is_an_error() -> Result<(), Box<dyn std::error::Error>> {
        let dir = dir_with("v.json", r#"["a"]"#)?;
        let mut canonical = match json!({
            "defines": { "known": { "fromJson": "v.json" } },
            "forbidden": [{ "from": { "path": "${known}/${unknown}" } }]
        }) {
            Value::Object(m) => m,
            _ => Map::new(),
        };
        let error = apply_defines(&mut canonical, &dir)
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default();
        assert!(error.contains("unknown"), "{error}");
        let mut ok = match json!({
            "defines": { "known": { "fromJson": "v.json" } },
            "options": { "exclude": { "path": "^${known}/" } }
        }) {
            Value::Object(m) => m,
            _ => Map::new(),
        };
        apply_defines(&mut ok, &dir)?;
        assert_eq!(ok["options"]["exclude"]["path"], "^a/");
        let _ = std::fs::remove_dir_all(dir);
        Ok(())
    }
}
