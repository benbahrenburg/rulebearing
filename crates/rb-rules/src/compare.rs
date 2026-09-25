//! Orderings: dependency-cruiser 18.2.0's `src/graph-utl/compare.mjs`, ported.
//!
//! - Specification: `test/graph-utl/compare.*.spec.mjs`, run by conformance gate 1 layer 2
//! - Plan: [Wave 1, Step 7](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-7-liveness-severity-ids-receipts-expires-ratchets-1b)
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
    let rule = |v: &Value| v.get("rule").cloned().unwrap_or(Value::Null);
    let (r1, r2) = (rule(first), rule(second));
    compare_severities(js::str_of(&r1, "severity"), js::str_of(&r2, "severity"))
        .then_with(|| collate::compare(text_or_empty(&r1, "name"), text_or_empty(&r2, "name")))
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
}
