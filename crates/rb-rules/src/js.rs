//! JavaScript's value semantics, for the parts of dependency-cruiser's engine that depend on them.
//!
//! - Specification: dependency-cruiser 18.2.0 `src/validate` and `src/graph-utl`, run unmodified
//!   against this engine by conformance gate 1 layer 2
//!   ([ADR-0009](../../../docs/adr/0009-conformance-suites-as-specification.md))
//! - Plan: [Wave 1, Step 5](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-5-matchers-and-restriction-evaluation-1b)
//!
//! The engine walks the graph as JSON values because dependency-cruiser's matchers ask whether a
//! key is present (`Object.hasOwn`), coerce a missing string to `"undefined"` when they test a
//! pattern against it, and treat an empty string as absent. These helpers are those rules, one
//! each, so every matcher reads like the JavaScript it ports.

use serde_json::{Map, Value};

/// `Object.hasOwn(value, key)` for an object; `false` for anything else.
pub fn has(value: &Value, key: &str) -> bool {
    value.as_object().is_some_and(|m| m.contains_key(key))
}

/// The string at `key`, as JavaScript would pass it to `RegExp.prototype.test`: a missing value
/// is the string `"undefined"`, a number is its decimal form.
pub fn text<'a>(value: &'a Value, key: &str) -> std::borrow::Cow<'a, str> {
    match value.get(key) {
        Some(Value::String(s)) => std::borrow::Cow::Borrowed(s),
        Some(Value::Null) => std::borrow::Cow::Borrowed("null"),
        Some(Value::Bool(b)) => std::borrow::Cow::Owned(b.to_string()),
        Some(Value::Number(n)) => std::borrow::Cow::Owned(n.to_string()),
        Some(other) => std::borrow::Cow::Owned(other.to_string()),
        None => std::borrow::Cow::Borrowed("undefined"),
    }
}

/// The string at `key`, or `None` when absent or not a string.
pub fn str_of<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

/// JavaScript truthiness.
pub fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(_) | Value::Object(_)) => true,
    }
}

/// The array at `key`, or an empty slice.
pub fn array<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value
        .get(key)
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

/// The strings of the array at `key`.
pub fn strings<'a>(value: &'a Value, key: &str) -> Vec<&'a str> {
    array(value, key).iter().filter_map(Value::as_str).collect()
}

/// The number at `key`.
pub fn number(value: &Value, key: &str) -> Option<f64> {
    value.get(key).and_then(Value::as_f64)
}

/// Sets `key` on an object value; a no-op on anything else.
pub fn set(value: &mut Value, key: &str, inner: Value) {
    if let Value::Object(map) = value {
        map.insert(key.to_owned(), inner);
    }
}

/// An empty object.
pub fn object() -> Value {
    Value::Object(Map::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn values_behave_as_in_javascript() {
        let v = json!({ "a": "x", "b": "", "c": 0, "d": 1.5, "e": [], "f": null, "g": true, "h": [1, "s"] });
        assert!(has(&v, "b"));
        assert!(!has(&v, "z"));
        assert!(!has(&json!(3), "a"));
        assert_eq!(text(&v, "a"), "x");
        assert_eq!(text(&v, "z"), "undefined");
        assert_eq!(text(&v, "f"), "null");
        assert_eq!(text(&v, "g"), "true");
        assert_eq!(text(&v, "d"), "1.5");
        assert_eq!(text(&v, "e"), "[]");
        assert!(truthy(v.get("a")));
        assert!(!truthy(v.get("b")));
        assert!(!truthy(v.get("c")));
        assert!(truthy(v.get("d")));
        assert!(truthy(v.get("e")));
        assert!(!truthy(v.get("f")));
        assert!(truthy(v.get("g")));
        assert!(!truthy(None));
        assert_eq!(strings(&v, "h"), ["s"]);
        assert!(array(&v, "a").is_empty());
        assert_eq!(number(&v, "d"), Some(1.5));
        assert_eq!(str_of(&v, "a"), Some("x"));
        let mut o = object();
        set(&mut o, "k", json!(1));
        assert_eq!(o, json!({ "k": 1 }));
        let mut n = json!(1);
        set(&mut n, "k", json!(1));
        assert_eq!(n, json!(1));
    }
}
