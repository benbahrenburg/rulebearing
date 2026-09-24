//! JavaScript's value semantics for the graph reporters: `ToString`, `ToNumber`, template-literal
//! interpolation, `String.prototype.replace`, dependency-cruiser's `object-util` `get`, and Node's
//! `path.posix` helpers.
//!
//! - Specification: dependency-cruiser 18.2.0 `src/utl/object-util.mjs`, `src/report/utl/index.mjs`,
//!   and the ECMAScript `ToString`, `Number::toString` and `GetSubstitution` operations the
//!   reporters lean on; byte-compared by conformance gate 1 layer 3
//!   ([ADR-0009](../../../docs/adr/0009-conformance-suites-as-specification.md))
//! - Plan: [Wave 2, Step 10](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#210-step-10-reporters-and-baseline-semantics-2e)
//! - Requirement: [FR-OUT-01](../../../docs/prd.md#fr-out-01)
//!
//! The reporters interpolate whatever the cruise result carries, so a missing value prints as
//! `undefined`, a number prints the way JavaScript prints it, and a theme's `criteria` compare as
//! JavaScript compares them. Each helper is one of those rules.

use std::cmp::Ordering;

use serde_json::{Map, Value};

/// `Number.prototype.toString()` for a double: the shortest round-trip digits, laid out by the
/// ECMAScript `Number::toString` rules (plain up to 21 integer digits, `0.000001` down to six
/// leading zeros, exponent form beyond).
pub fn number_to_string(value: f64) -> String {
    if value.is_nan() {
        return "NaN".into();
    }
    if value == 0.0 {
        return "0".into();
    }
    if value.is_infinite() {
        return if value > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    let sign = if value < 0.0 { "-" } else { "" };
    // Rust's `{:e}` prints the shortest digits that round-trip, as `d.ddde<exp>`.
    let scientific = format!("{:e}", value.abs());
    let (mantissa, exponent) = scientific.split_once('e').unwrap_or((&scientific, "0"));
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let exponent: i64 = exponent.parse().unwrap_or(0);
    let k = i64::try_from(digits.len()).unwrap_or(i64::MAX);
    let n = exponent + 1;
    let body = if k <= n && n <= 21 {
        format!(
            "{digits}{}",
            "0".repeat(usize::try_from(n - k).unwrap_or(0))
        )
    } else if 0 < n && n <= 21 {
        let at = usize::try_from(n).unwrap_or(0);
        format!("{}.{}", &digits[..at], &digits[at..])
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", "0".repeat(usize::try_from(-n).unwrap_or(0)))
    } else {
        let e = n - 1;
        let exp = if e >= 0 {
            format!("+{e}")
        } else {
            e.to_string()
        };
        if digits.len() == 1 {
            format!("{digits}e{exp}")
        } else {
            format!("{}.{}e{exp}", &digits[..1], &digits[1..])
        }
    };
    format!("{sign}{body}")
}

/// JavaScript's `ToString` (what `${value}` and `String(value)` print); `None` is `undefined`.
pub fn to_string(value: Option<&Value>) -> String {
    match value {
        None => "undefined".into(),
        Some(Value::Null) => "null".into(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Number(n)) => n.as_f64().map_or_else(|| n.to_string(), number_to_string),
        Some(Value::String(s)) => s.clone(),
        // `Array.prototype.join(",")`, where null and undefined elements are empty.
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| match item {
                Value::Null => String::new(),
                other => to_string(Some(other)),
            })
            .collect::<Vec<_>>()
            .join(","),
        Some(Value::Object(_)) => "[object Object]".into(),
    }
}

/// The value at `key` as `${object.key}` prints it.
pub fn field(value: &Value, key: &str) -> String {
    to_string(value.get(key))
}

fn string_to_number(text: &str) -> f64 {
    let trimmed = text.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
    if trimmed.is_empty() {
        return 0.0;
    }
    let radix = |prefix: &str, radix: u32| {
        trimmed
            .strip_prefix(prefix)
            .map(|digits| u64::from_str_radix(digits, radix).map_or(f64::NAN, precise))
    };
    if let Some(n) = radix("0x", 16)
        .or_else(|| radix("0X", 16))
        .or_else(|| radix("0o", 8))
        .or_else(|| radix("0O", 8))
        .or_else(|| radix("0b", 2))
        .or_else(|| radix("0B", 2))
    {
        return n;
    }
    match trimmed {
        "Infinity" | "+Infinity" => f64::INFINITY,
        "-Infinity" => f64::NEG_INFINITY,
        other
            if other
                .chars()
                .all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-')) =>
        {
            other.parse().unwrap_or(f64::NAN)
        }
        _ => f64::NAN,
    }
}

#[expect(
    clippy::cast_precision_loss,
    reason = "JavaScript's ToNumber rounds a large literal to the nearest double too"
)]
fn precise(n: u64) -> f64 {
    n as f64
}

/// JavaScript's `ToNumber`; `None` (undefined) is `NaN`.
pub fn to_number(value: Option<&Value>) -> f64 {
    match value {
        None | Some(Value::Object(_)) => f64::NAN,
        Some(Value::Null) => 0.0,
        Some(Value::Bool(b)) => f64::from(u8::from(*b)),
        Some(Value::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(s)) => string_to_number(s),
        Some(Value::Array(_)) => string_to_number(&to_string(value)),
    }
}

/// `Number.isInteger(value)`.
pub fn is_integer(value: Option<&Value>) -> bool {
    value
        .and_then(Value::as_f64)
        .is_some_and(|f| f.is_finite() && f.fract() == 0.0)
}

/// JavaScript truthiness.
pub fn truthy(value: Option<&Value>) -> bool {
    rb_rules::js::truthy(value)
}

/// `a < b` and friends on strings: UTF-16 code unit order, which differs from UTF-8 byte order
/// above the Basic Multilingual Plane.
pub fn compare_utf16(a: &str, b: &str) -> Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

/// `string.length`: UTF-16 code units.
pub fn length(text: &str) -> usize {
    text.encode_utf16().count()
}

/// `text.padEnd(width)`.
pub fn pad_end(text: &str, width: usize) -> String {
    let len = length(text);
    format!("{text}{}", " ".repeat(width.saturating_sub(len)))
}

/// `text.padStart(width)`.
pub fn pad_start(text: &str, width: usize) -> String {
    let len = length(text);
    format!("{}{text}", " ".repeat(width.saturating_sub(len)))
}

/// One step of property access, `object[key]`, on a JSON value: an object's own key, an array's
/// index or `length`, a string's character or `length`.
fn property(value: &Value, key: &str) -> Option<Value> {
    match value {
        Value::Object(map) => map.get(key).cloned(),
        Value::Array(items) => {
            if key == "length" {
                Some(Value::from(items.len()))
            } else {
                canonical_index(key).and_then(|i| items.get(i).cloned())
            }
        }
        Value::String(text) => {
            let units: Vec<u16> = text.encode_utf16().collect();
            if key == "length" {
                Some(Value::from(units.len()))
            } else {
                canonical_index(key)
                    .and_then(|i| units.get(i))
                    .map(|u| Value::String(String::from_utf16_lossy(&[*u])))
            }
        }
        _ => None,
    }
}

/// A key that is an array index: decimal digits with no leading zero.
fn canonical_index(key: &str) -> Option<usize> {
    if key.is_empty() || (key.len() > 1 && key.starts_with('0')) {
        return None;
    }
    if !key.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    key.parse().ok()
}

/// dependency-cruiser's `get(object, path)`: `path` split on `.`, `[` and `]`, each step taken
/// while the value so far is truthy; a falsy result is `None` (the default).
pub fn get(object: &Value, path: &str) -> Option<Value> {
    if !truthy(Some(object)) || path.is_empty() {
        return None;
    }
    let mut current = Some(object.clone());
    for key in path.split(['.', '[', ']']).filter(|k| !k.is_empty()) {
        current = match current {
            Some(value) if truthy(Some(&value)) => property(&value, key),
            other => other,
        };
    }
    current.filter(|v| truthy(Some(v)))
}

/// `{ ...left, ...right }` for two objects; a non-object spreads nothing.
pub fn spread(left: &Value, right: &Value) -> Map<String, Value> {
    let mut out = left.as_object().cloned().unwrap_or_default();
    if let Some(right) = right.as_object() {
        for (k, v) in right {
            out.insert(k.clone(), v.clone());
        }
    }
    out
}

/// `Object.keys(object)` in JavaScript's order: array-index keys ascending, then the rest in
/// insertion order.
pub fn keys_in_order<'a, I>(keys: I) -> Vec<&'a str>
where
    I: IntoIterator<Item = &'a str>,
{
    let mut indices: Vec<(usize, &str)> = Vec::new();
    let mut rest: Vec<&str> = Vec::new();
    for key in keys {
        match canonical_index(key).filter(|i| *i < u32::MAX as usize) {
            Some(i) => indices.push((i, key)),
            None => rest.push(key),
        }
    }
    indices.sort_by_key(|(i, _)| *i);
    indices.into_iter().map(|(_, k)| k).chain(rest).collect()
}

/// `haystack.replace(needle, replacement)` with a string needle: the first occurrence only, and the
/// replacement's `$$`, `$&`, `` $` `` and `$'` patterns expanded (`GetSubstitution` without
/// captures).
pub fn replace_first(haystack: &str, needle: &str, replacement: &str) -> String {
    let Some(at) = haystack.find(needle) else {
        return haystack.to_owned();
    };
    let before = &haystack[..at];
    let after = &haystack[at + needle.len()..];
    let mut expanded = String::with_capacity(replacement.len());
    let mut chars = replacement.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '$' {
            expanded.push(c);
            continue;
        }
        match chars.peek() {
            Some('$') => {
                expanded.push('$');
                chars.next();
            }
            Some('&') => {
                expanded.push_str(needle);
                chars.next();
            }
            Some('`') => {
                expanded.push_str(before);
                chars.next();
            }
            Some('\'') => {
                expanded.push_str(after);
                chars.next();
            }
            _ => expanded.push('$'),
        }
    }
    format!("{before}{expanded}{after}")
}

/// Node's `path.posix.basename(path)`.
pub fn basename(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return String::new();
    }
    trimmed
        .rsplit_once('/')
        .map_or(trimmed, |(_, base)| base)
        .to_owned()
}

/// Node's `path.posix.dirname(path)`.
pub fn dirname(path: &str) -> String {
    rb_rules::graph::consolidate::dirname(path)
}

/// Node's `path.posix.normalize` for a joined path.
fn normalize(path: &str) -> String {
    if path.is_empty() {
        return ".".into();
    }
    let absolute = path.starts_with('/');
    let trailing = path.ends_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                if parts.last().is_some_and(|p| *p != "..") {
                    parts.pop();
                } else if !absolute {
                    parts.push("..");
                }
            }
            other => parts.push(other),
        }
    }
    let mut out = parts.join("/");
    if out.is_empty() && !absolute {
        out.push('.');
    }
    if trailing && !out.is_empty() {
        out.push('/');
    }
    if absolute {
        out.insert(0, '/');
    }
    out
}

/// Node's `path.posix.join(left, right)`.
pub fn join(left: &str, right: &str) -> String {
    let joined: Vec<&str> = [left, right]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect();
    if joined.is_empty() {
        return ".".into();
    }
    normalize(&joined.join("/"))
}

/// Whether a JSON array (or string, with `String.prototype.includes`) includes `needle`.
pub fn includes(value: Option<&Value>, needle: &str) -> bool {
    match value {
        Some(Value::Array(items)) => items.iter().any(|i| i.as_str() == Some(needle)),
        Some(Value::String(s)) => s.contains(needle),
        _ => false,
    }
}

/// Whether some string element of a JSON array satisfies `test`.
pub fn some_str(value: Option<&Value>, test: impl Fn(&str) -> bool) -> bool {
    value
        .and_then(Value::as_array)
        .is_some_and(|items| items.iter().filter_map(Value::as_str).any(test))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use serde_json::json;

    #[test]
    fn numbers_print_as_javascript_prints_them() {
        for (value, expected) in [
            (1.0, "1"),
            (0.5, "0.5"),
            (-2.25, "-2.25"),
            (0.481, "0.481"),
            (1e21, "1e+21"),
            (123_456_789_012_345_680_000.0, "123456789012345680000"),
            (0.000_001, "0.000001"),
            (1e-7, "1e-7"),
            (1.5e-7, "1.5e-7"),
            (2.5e25, "2.5e+25"),
            (f64::NAN, "NaN"),
            (f64::INFINITY, "Infinity"),
            (f64::NEG_INFINITY, "-Infinity"),
            (0.0, "0"),
            (100.0, "100"),
        ] {
            assert_eq!(number_to_string(value), expected, "{value}");
        }
    }

    #[test]
    fn to_string_is_javascript_s() {
        assert_eq!(to_string(None), "undefined");
        assert_eq!(to_string(Some(&json!(null))), "null");
        assert_eq!(to_string(Some(&json!(true))), "true");
        assert_eq!(to_string(Some(&json!(2))), "2");
        assert_eq!(to_string(Some(&json!("x"))), "x");
        assert_eq!(to_string(Some(&json!(["a", null, 1]))), "a,,1");
        assert_eq!(to_string(Some(&json!({ "a": 1 }))), "[object Object]");
        assert_eq!(field(&json!({ "a": 1.5 }), "a"), "1.5");
    }

    #[test]
    fn to_number_is_javascript_s() {
        let number = |v: Option<&Value>| number_to_string(to_number(v));
        assert!(to_number(None).is_nan());
        assert_eq!(number(Some(&json!(null))), number_to_string(0.0));
        assert_eq!(number(Some(&json!(true))), number_to_string(1.0));
        assert_eq!(number(Some(&json!("0.481"))), number_to_string(0.481));
        assert_eq!(number(Some(&json!("  12 "))), number_to_string(12.0));
        assert_eq!(number(Some(&json!(""))), number_to_string(0.0));
        assert_eq!(number(Some(&json!("0x10"))), number_to_string(16.0));
        assert_eq!(number(Some(&json!("0b11"))), number_to_string(3.0));
        assert_eq!(number(Some(&json!("0o7"))), number_to_string(7.0));
        assert!(to_number(Some(&json!("0xZZ"))).is_nan());
        assert_eq!(
            number(Some(&json!("-Infinity"))),
            number_to_string(f64::NEG_INFINITY)
        );
        assert!(to_number(Some(&json!("abc"))).is_nan());
        assert!(to_number(Some(&json!({}))).is_nan());
        assert_eq!(number(Some(&json!([]))), number_to_string(0.0));
        assert_eq!(number(Some(&json!([5]))), number_to_string(5.0));
        assert!(is_integer(Some(&json!(3))));
        assert!(!is_integer(Some(&json!(3.5))));
        assert!(!is_integer(Some(&json!(null))));
    }

    #[test]
    fn get_follows_upstream_paths() {
        let module = json!({ "rules": [{ "severity": "error" }], "name": "abc", "zero": 0 });
        assert_eq!(get(&module, "rules[0].severity"), Some(json!("error")));
        assert_eq!(get(&module, "rules.length"), Some(json!(1)));
        assert_eq!(get(&module, "name[1]"), Some(json!("b")));
        assert_eq!(get(&module, "name.length"), Some(json!(3)));
        assert_eq!(get(&module, "zero"), None, "a falsy result is the default");
        assert_eq!(get(&module, "zero.x"), None);
        assert_eq!(get(&module, "missing.x"), None);
        assert_eq!(get(&module, "rules[01]"), None, "not an array index");
        assert_eq!(get(&module, ""), None);
        assert_eq!(get(&json!(null), "a"), None);
        assert_eq!(get(&json!(5), "a"), None);
    }

    #[test]
    fn replace_expands_substitution_patterns() {
        assert_eq!(replace_first("a{{x}}b{{x}}", "{{x}}", "1"), "a1b{{x}}");
        assert_eq!(replace_first("a{{x}}b", "{{x}}", "$$"), "a$b");
        assert_eq!(replace_first("a{{x}}b", "{{x}}", "[$&]"), "a[{{x}}]b");
        assert_eq!(replace_first("a{{x}}b", "{{x}}", "$`"), "aab");
        assert_eq!(replace_first("a{{x}}b", "{{x}}", "$'"), "abb");
        assert_eq!(replace_first("a{{x}}b", "{{x}}", "$1$"), "a$1$b");
        assert_eq!(replace_first("ab", "{{x}}", "z"), "ab");
    }

    #[test]
    fn paths_are_node_s() {
        assert_eq!(basename("a/b/c.js"), "c.js");
        assert_eq!(basename("c.js"), "c.js");
        assert_eq!(basename("a/b/"), "b");
        assert_eq!(basename("/"), "");
        assert_eq!(dirname("a/b/c.js"), "a/b");
        assert_eq!(join("prefix", "src/a.js"), "prefix/src/a.js");
        assert_eq!(join("../x/", "./src/a.js"), "../x/src/a.js");
        assert_eq!(join("/abs/", "../a"), "/a");
        assert_eq!(join("", ""), ".");
        assert_eq!(join("a", ".."), ".");
        assert_eq!(join("/", ".."), "/");
        assert_eq!(join("a/", "b/"), "a/b/");
        assert_eq!(join("..", "../b"), "../../b");
    }

    #[test]
    fn keys_order_indices_first() {
        assert_eq!(
            keys_in_order(["src", "10", "2", "01", "a"]),
            vec!["2", "10", "src", "01", "a"]
        );
    }

    #[test]
    fn small_helpers() {
        assert_eq!(compare_utf16("\u{ffff}", "\u{10000}"), Ordering::Greater);
        assert_eq!(length("é😀"), 3);
        assert_eq!(pad_end("ab", 4), "ab  ");
        assert_eq!(pad_start("ab", 4), "  ab");
        assert_eq!(pad_start("abcde", 4), "abcde");
        assert!(includes(Some(&json!(["core"])), "core"));
        assert!(includes(Some(&json!("x-core")), "core"));
        assert!(!includes(None, "core"));
        assert!(some_str(Some(&json!(["npm-dev"])), |t| t.starts_with("npm")));
        assert!(!some_str(Some(&json!("npm")), |t| t.starts_with("npm")));
        let spread = spread(&json!({ "a": 1, "b": 2 }), &json!({ "b": 3 }));
        assert_eq!(Value::Object(spread), json!({ "a": 1, "b": 3 }));
        assert!(truthy(Some(&json!("x"))));
    }

    proptest! {
        #[test]
        fn number_strings_round_trip(value in proptest::num::f64::NORMAL) {
            let printed = number_to_string(value);
            prop_assert_eq!(printed.parse::<f64>().ok(), Some(value));
        }

        #[test]
        fn joined_paths_never_have_double_slashes(a in "[a-z./]{0,12}", b in "[a-z./]{0,12}") {
            prop_assert!(!join(&a, &b).contains("//"));
        }
    }
}
