//! Orderings: dependency-cruiser 18.2.0's `src/graph-utl/compare.mjs`, ported.
//!
//! - Specification: `test/graph-utl/compare.*.spec.mjs`, run by conformance gate 1 layer 2
//! - Plan: [Wave 1, Step 7](../../../docs/plans/implemented/0001-wave-1-typescript-parity.md#step-7-liveness-severity-ids-receipts-expires-ratchets-1b)
//! - Requirement: [FR-CORE-07](../../../docs/prd.md#fr-core-07) (deterministic output)
//!
//! Strings compare with JavaScript's `localeCompare` ([`rb_model::collate`]), as upstream does,
//! so violations print in the order dependency-cruiser prints them.

use std::cmp::Ordering;

use rb_model::collate;
use serde_json::Value;

use crate::js;

/// `severity2number`: `error` 1, `warn` 2, `info` 3, `ignore` 4, anything else -1.
pub fn severity_number(severity: Option<&str>) -> i32 {
    match severity {
        Some("error") => 1,
        Some("warn") => 2,
        Some("info") => 3,
        Some("ignore") => 4,
        _ => -1,
    }
}

/// `compareSeverities`.
pub fn compare_severities(first: Option<&str>, second: Option<&str>) -> Ordering {
    severity_number(first).cmp(&severity_number(second))
}

fn text_or_empty<'a>(value: &'a Value, key: &str) -> &'a str {
    js::str_of(value, key).unwrap_or("")
}

fn compare_arrays(first: &[Value], second: &[Value], by_name: bool) -> Ordering {
    for (a, b) in first.iter().zip(second) {
        let (a, b) = if by_name {
            (text_or_empty(a, "name"), text_or_empty(b, "name"))
        } else {
            (a.as_str().unwrap_or(""), b.as_str().unwrap_or(""))
        };
        let order = collate::compare(a, b);
        if order != Ordering::Equal {
            return order;
        }
    }
    first.len().cmp(&second.len())
}

/// `compareViolations`.
pub fn compare_violations(first: &Value, second: &Value) -> Ordering {
    let severity = |v: &'_ Value| {
        v.get("rule")
            .and_then(|r| js::str_of(r, "severity"))
            .map(str::to_owned)
    };
    compare_severities(severity(first).as_deref(), severity(second).as_deref())
        .then_with(|| compare_violations_ex_severities(first, second))
}

/// `compareViolationsExSeverities` (dependency-cruiser 18.7.0): every field of
/// [`compare_violations`] but the severity, which is how `diffViolationArrays` orders a baseline.
pub fn compare_violations_ex_severities(first: &Value, second: &Value) -> Ordering {
    let rule = |v: &Value| v.get("rule").cloned().unwrap_or(Value::Null);
    let (r1, r2) = (rule(first), rule(second));
    collate::compare(text_or_empty(&r1, "name"), text_or_empty(&r2, "name"))
        .then_with(|| collate::compare(text_or_empty(first, "from"), text_or_empty(second, "from")))
        .then_with(|| collate::compare(text_or_empty(first, "to"), text_or_empty(second, "to")))
        .then_with(|| {
            collate::compare(
                text_or_empty(first, "unresolvedTo"),
                text_or_empty(second, "unresolvedTo"),
            )
        })
        .then_with(|| collate::compare(text_or_empty(first, "type"), text_or_empty(second, "type")))
        .then_with(|| {
            compare_arrays(
                js::array(first, "dependencyTypes"),
                js::array(second, "dependencyTypes"),
                false,
            )
        })
        .then_with(|| compare_arrays(js::array(first, "cycle"), js::array(second, "cycle"), true))
        .then_with(|| compare_arrays(js::array(first, "via"), js::array(second, "via"), true))
}

/// What `diffViolationArrays` answers: the violations of the second list no entry of the first
/// matches (`new`), the entries of the first a violation of the second matches (`same`), and the
/// entries of the first nothing matches (`old`), each in [`compare_violations_ex_severities`]
/// order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ViolationDiff {
    /// In the second list only.
    pub new: Vec<Value>,
    /// In both, as the first list holds them.
    pub same: Vec<Value>,
    /// In the first list only.
    pub old: Vec<Value>,
}

/// `diffViolationArrays` (dependency-cruiser 18.7.0, `src/graph-utl/compare.mjs`): both lists
/// sorted by [`compare_violations_ex_severities`] and merged, so a violation that differs only in
/// its severity is the same violation, and each entry pairs with at most one violation.
pub fn diff_violation_arrays(first: &[Value], second: &[Value]) -> ViolationDiff {
    let mut first = first.to_vec();
    let mut second = second.to_vec();
    first.sort_by(compare_violations_ex_severities);
    second.sort_by(compare_violations_ex_severities);
    let mut diff = ViolationDiff::default();
    let mut second = second.into_iter().peekable();
    for entry in first {
        loop {
            match second
                .peek()
                .map(|next| compare_violations_ex_severities(&entry, next))
            {
                Some(Ordering::Greater) => diff.new.extend(second.next()),
                Some(Ordering::Equal) => {
                    second.next();
                    diff.same.push(entry);
                    break;
                }
                Some(Ordering::Less) | None => {
                    diff.old.push(entry);
                    break;
                }
            }
        }
    }
    diff.new.extend(second);
    diff
}

/// `compareRules`: severity, then name.
pub fn compare_rules(left: &Value, right: &Value) -> Ordering {
    compare_severities(js::str_of(left, "severity"), js::str_of(right, "severity"))
        .then_with(|| collate::compare(text_or_empty(left, "name"), text_or_empty(right, "name")))
}

/// `compareModules`: `source` greater is 1, anything else -1 (upstream never answers 0).
pub fn compare_modules(left: &Value, right: &Value) -> Ordering {
    if js::text(left, "source") > js::text(right, "source") {
        Ordering::Greater
    } else {
        Ordering::Less
    }
}

/// The JavaScript sign of an ordering, as the specs compare it.
pub fn sign(order: Ordering) -> i32 {
    match order {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_order_without_severities_ignores_only_the_severity() {
        let v = |sev: &str, name: &str, from: &str, to: &str| json!({ "rule": { "severity": sev, "name": name }, "from": from, "to": to });
        let (error, warn) = (v("error", "b", "x", "y"), v("warn", "a", "x", "y"));
        assert_eq!(compare_violations(&error, &warn), Ordering::Less);
        assert_eq!(
            compare_violations_ex_severities(&error, &warn),
            Ordering::Greater
        );
        assert_eq!(
            compare_violations_ex_severities(&v("error", "a", "x", "y"), &v("warn", "a", "x", "y")),
            Ordering::Equal
        );
        assert_eq!(
            compare_violations_ex_severities(&v("warn", "a", "a", "y"), &v("warn", "a", "b", "y")),
            Ordering::Less
        );
    }

    #[test]
    fn severities_order_error_first_and_unknown_before_all() {
        assert_eq!(sign(compare_severities(Some("error"), Some("warn"))), -1);
        assert_eq!(sign(compare_severities(Some("info"), Some("warn"))), 1);
        assert_eq!(sign(compare_severities(Some("ignore"), Some("ignore"))), 0);
        assert_eq!(sign(compare_severities(None, Some("error"))), -1);
        assert_eq!(severity_number(Some("ignore")), 4);
    }

    #[test]
    fn violations_compare_field_by_field() {
        let v = |sev: &str, name: &str, from: &str, to: &str| json!({ "rule": { "severity": sev, "name": name }, "from": from, "to": to });
        assert_eq!(
            compare_violations(&v("error", "b", "x", "y"), &v("warn", "a", "x", "y")),
            Ordering::Less
        );
        assert_eq!(
            compare_violations(&v("warn", "a", "x", "y"), &v("warn", "b", "a", "a")),
            Ordering::Less
        );
        assert_eq!(
            compare_violations(&v("warn", "a", "a", "y"), &v("warn", "a", "b", "a")),
            Ordering::Less
        );
        assert_eq!(
            compare_violations(&v("warn", "a", "a", "a"), &v("warn", "a", "a", "b")),
            Ordering::Less
        );
        let mut u1 = v("warn", "a", "a", "a");
        u1["unresolvedTo"] = json!("x");
        assert_eq!(
            compare_violations(&v("warn", "a", "a", "a"), &u1),
            Ordering::Less
        );
        let mut t1 = v("warn", "a", "a", "a");
        t1["type"] = json!("module");
        let mut t2 = t1.clone();
        t2["type"] = json!("dependency");
        assert_eq!(compare_violations(&t2, &t1), Ordering::Less);
        let mut d1 = t1.clone();
        d1["dependencyTypes"] = json!(["local"]);
        let mut d2 = t1.clone();
        d2["dependencyTypes"] = json!(["local", "npm"]);
        assert_eq!(compare_violations(&d1, &d2), Ordering::Less);
        let mut c1 = t1.clone();
        c1["cycle"] = json!([{ "name": "a" }]);
        let mut c2 = t1.clone();
        c2["cycle"] = json!([{ "name": "b" }]);
        assert_eq!(compare_violations(&c1, &c2), Ordering::Less);
        let mut w1 = t1.clone();
        w1["via"] = json!([{ "name": "b" }]);
        let mut w2 = t1.clone();
        w2["via"] = json!([{ "name": "a" }]);
        assert_eq!(compare_violations(&w1, &w2), Ordering::Greater);
        assert_eq!(compare_violations(&t1, &t1), Ordering::Equal);
    }

    #[test]
    fn rules_and_modules() {
        let r = |sev: &str, name: &str| json!({ "severity": sev, "name": name });
        assert_eq!(
            compare_rules(&r("warn", "a"), &r("error", "b")),
            Ordering::Greater
        );
        assert_eq!(
            compare_rules(&r("warn", "a"), &r("warn", "b")),
            Ordering::Less
        );
        assert_eq!(
            compare_modules(&json!({ "source": "b" }), &json!({ "source": "a" })),
            Ordering::Greater
        );
        assert_eq!(
            compare_modules(&json!({ "source": "a" }), &json!({ "source": "a" })),
            Ordering::Less
        );
    }

    fn edge(from: &str, to: &str, name: &str, severity: &str) -> Value {
        json!({ "from": from, "to": to, "rule": { "name": name, "severity": severity } })
    }

    #[test]
    fn violation_arrays_diff_by_value_as_upstreams_spec_says() {
        // test/graph-utl/compare.violations.spec.mjs, "diffs arrays of violations by value".
        let shared = edge("app.js", "lib.js", "no-cycles", "error");
        let old = edge("a.js", "b.js", "no-cycles", "warn");
        let new = edge("c.js", "d.js", "no-cycles", "info");
        assert_eq!(
            diff_violation_arrays(
                &[old.clone(), shared.clone()],
                &[shared.clone(), new.clone()]
            ),
            ViolationDiff {
                new: vec![new],
                same: vec![shared],
                old: vec![old]
            }
        );
        // "diffs arrays with repeated equal violations without inventing new comparison logic".
        let duplicate = edge("dup.js", "dup-target.js", "reachability", "error");
        let other = edge("other.js", "other-target.js", "reachability", "warn");
        assert_eq!(
            diff_violation_arrays(
                &[duplicate.clone(), duplicate.clone(), other.clone()],
                std::slice::from_ref(&duplicate)
            ),
            ViolationDiff {
                new: vec![],
                same: vec![duplicate.clone()],
                old: vec![duplicate, other]
            }
        );
        // A severity is not part of a violation's identity.
        let warn = edge("x.js", "y.js", "r", "warn");
        let diff = diff_violation_arrays(&[edge("x.js", "y.js", "r", "error")], &[warn]);
        assert_eq!((diff.new.len(), diff.same.len(), diff.old.len()), (0, 1, 0));
        assert_eq!(diff_violation_arrays(&[], &[]), ViolationDiff::default());
    }

    proptest::proptest! {
        #[test]
        fn every_violation_is_counted_once(
            first in proptest::collection::vec(("[a-c]", "[a-c]", "[a-b]"), 0..8),
            second in proptest::collection::vec(("[a-c]", "[a-c]", "[a-b]"), 0..8),
        ) {
            let list = |items: &[(String, String, String)]| -> Vec<Value> {
                items.iter().map(|(f, t, n)| edge(f, t, n, "error")).collect()
            };
            let (first, second) = (list(&first), list(&second));
            let diff = diff_violation_arrays(&first, &second);
            proptest::prop_assert_eq!(diff.same.len() + diff.old.len(), first.len());
            proptest::prop_assert_eq!(diff.same.len() + diff.new.len(), second.len());
            let itself = diff_violation_arrays(&first, &first);
            proptest::prop_assert_eq!(itself.same.len(), first.len());
            proptest::prop_assert!(itself.new.is_empty() && itself.old.is_empty());
            let swapped = diff_violation_arrays(&second, &first);
            proptest::prop_assert_eq!(swapped.new.len(), diff.old.len());
            proptest::prop_assert_eq!(swapped.old.len(), diff.new.len());
        }
    }
}
