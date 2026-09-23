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

/// `Array.prototype.sort(comparefn)` as V8 runs it, where `less(a, b)` is `comparefn(a, b) < 0`.
///
/// The order matters when the comparator is inconsistent, as `compareSeverity` in
/// dependency-cruiser's `validate/index.mjs` is: a severity it has no rank for compares as `NaN`,
/// which the specification's `SortCompare` turns into `+0`, so such an entry is "equal" to every
/// other. V8's `TimSort` first takes the run at the start of the array (reversed when strictly
/// descending), then binary-inserts every further element. Below 64 elements that is the whole
/// algorithm; from 64 V8 sorts runs of that length and merges them, which gives the same order
/// whenever the comparator is consistent.
pub fn sort<T>(items: &mut [T], less: impl Fn(&T, &T) -> bool) {
    let n = items.len();
    if n < 2 {
        return;
    }
    let descending = less(&items[1], &items[0]);
    let mut run = 2;
    while run < n && less(&items[run], &items[run - 1]) == descending {
        run += 1;
    }
    if descending {
        items[..run].reverse();
    }
    for start in run..n {
        let (mut left, mut right) = (0, start);
        while left < right {
            let mid = left + (right - left) / 2;
            if less(&items[start], &items[mid]) {
                right = mid;
            } else {
                left = mid + 1;
            }
        }
        items[left..=start].rotate_right(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
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

    /// `Some(rank)`, or `None` for an entry the comparator answers `NaN` (so `+0`) for.
    fn by_rank(a: &(Option<u8>, usize), b: &(Option<u8>, usize)) -> bool {
        matches!((a.0, b.0), (Some(x), Some(y)) if x < y)
    }

    #[test]
    fn sort_follows_v8_when_the_comparator_is_inconsistent() {
        // Each expectation is what `[...].sort((a, b) => a.r - b.r)` gives on Node 24, with
        // `undefined` ranks written `None`.
        let run = |ranks: &[Option<u8>]| -> Vec<usize> {
            let mut items: Vec<(Option<u8>, usize)> = ranks.iter().copied().zip(0..).collect();
            sort(&mut items, by_rank);
            items.into_iter().map(|(_, i)| i).collect()
        };
        // A strictly descending first run is reversed, and it ends at the first "equal".
        assert_eq!(run(&[Some(3), Some(2), None]), [1, 0, 2]);
        assert_eq!(run(&[None, Some(2)]), [0, 1]);
        assert_eq!(run(&[Some(2), None, Some(1)]), [0, 1, 2]);
        assert_eq!(run(&[None, Some(2), Some(1)]), [0, 2, 1]);
        assert_eq!(run(&[Some(3), Some(2), Some(1), Some(1)]), [2, 3, 1, 0]);
        assert_eq!(run(&[Some(1), None, Some(3), Some(2)]), [0, 1, 3, 2]);
        assert!(run(&[]).is_empty());
        assert_eq!(run(&[Some(5)]), [0]);
        assert_eq!(run(&[Some(2), Some(1)]), [1, 0]);
    }

    proptest! {
        #[test]
        fn with_a_consistent_comparator_sort_is_a_stable_sort(
            keys in proptest::collection::vec(0u8..6, 0..80)
        ) {
            let mut ours: Vec<(Option<u8>, usize)> = keys.iter().map(|k| Some(*k)).zip(0..).collect();
            let mut stable = ours.clone();
            sort(&mut ours, by_rank);
            stable.sort_by_key(|(k, _)| *k);
            prop_assert_eq!(ours, stable);
        }
    }
}
