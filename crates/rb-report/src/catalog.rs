//! What the `sarif`, `junit` and `trx` reporters share: every rule of the run with its family,
//! severity, comment and `fix`, the violations of each, and where a violation sits.
//!
//! - Contract: [Wave 2 plan § 1.5](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#15-interfaces-and-contracts-this-wave-freezes)
//!   (one SARIF rule and one test case per configuration rule)
//! - Plan: [Wave 2, Step 10](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#210-step-10-reporters-and-baseline-semantics-2e)
//! - Source: [design § Reporters](../../../docs/artifacts/design.md#reporters)
//! - Requirement: [FR-OUT-02](../../../docs/prd.md#fr-out-02)
//!
//! The rules come from `summary.ruleSetUsed` in the order a configuration lists them:
//! `forbidden`, the `allowed` list as the one rule its violations name (`not-in-allowed`, at
//! `allowedSeverity`, dependency-cruiser's default `warn`), `required`, then the element, slice
//! and diagram rules. A violation of a rule the rule set does not list (a result without
//! `ruleSetUsed`) adds that rule, in name order, so no violation goes unreported. The ratchets
//! of `summary.ratchets` follow, then any vacuous entry that names none of these.

use serde_json::Value;

use crate::text;

/// One rule of the run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogRule {
    /// The name violations carry.
    pub name: String,
    /// `forbidden`, `allowed`, `required`, `elements`, `slices`, `diagrams`, `ratchets` or
    /// `rules` for one known only from its violations.
    pub family: String,
    /// The configured severity.
    pub severity: String,
    /// The rule's comment.
    pub comment: Option<String>,
    /// The rule's `fix`.
    pub fix: Option<String>,
}

fn string(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn summary(result: &Value) -> Option<&Value> {
    result.get("summary")
}

/// The violations of `summary.violations`.
pub fn violations(result: &Value) -> Vec<&Value> {
    summary(result)
        .and_then(|s| s.get("violations"))
        .and_then(Value::as_array)
        .map(|v| v.iter().collect())
        .unwrap_or_default()
}

/// A violation's rule name.
pub fn rule_name(violation: &Value) -> String {
    violation
        .get("rule")
        .and_then(|r| r.get("name"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// The entries of a summary list (`vacuousRules`, `ratchets`, `expired`).
pub fn list<'a>(result: &'a Value, key: &str) -> Vec<&'a Value> {
    summary(result)
        .and_then(|s| s.get(key))
        .and_then(Value::as_array)
        .map(|v| v.iter().collect())
        .unwrap_or_default()
}

/// Every rule of the run, in the order the module documentation gives.
pub fn rules(result: &Value) -> Vec<CatalogRule> {
    let mut out: Vec<CatalogRule> = Vec::new();
    let rule_set = summary(result).and_then(|s| s.get("ruleSetUsed"));
    let entry = |family: &str, rule: &Value, default_severity: &str| CatalogRule {
        name: string(rule, "name").unwrap_or_default(),
        family: family.to_owned(),
        severity: string(rule, "severity").unwrap_or_else(|| default_severity.to_owned()),
        comment: string(rule, "comment"),
        fix: string(rule, "fix"),
    };
    let family = |key: &str| {
        rule_set
            .and_then(|r| r.get(key))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    for rule in family("forbidden") {
        out.push(entry("forbidden", &rule, "warn"));
    }
    let allowed = family("allowed");
    if let Some(first) = allowed.first() {
        let severity = rule_set
            .and_then(|r| string(r, "allowedSeverity"))
            .unwrap_or_else(|| "warn".into());
        out.push(CatalogRule {
            name: "not-in-allowed".into(),
            family: "allowed".into(),
            severity,
            comment: string(first, "comment"),
            fix: string(first, "fix"),
        });
    }
    for key in ["required", "elements", "slices", "diagrams"] {
        for rule in family(key) {
            out.push(entry(key, &rule, "warn"));
        }
    }
    let mut unlisted: Vec<CatalogRule> = Vec::new();
    for violation in violations(result) {
        let name = rule_name(violation);
        if !out.iter().chain(&unlisted).any(|r| r.name == name) {
            unlisted.push(CatalogRule {
                name,
                family: "rules".into(),
                severity: crate::severity(violation),
                comment: string(violation, "comment"),
                fix: string(violation, "fix"),
            });
        }
    }
    unlisted.sort_by(|a, b| a.name.cmp(&b.name));
    out.extend(unlisted);
    for ratchet in list(result, "ratchets") {
        out.push(CatalogRule {
            name: text(ratchet, "name"),
            family: "ratchets".into(),
            severity: "error".into(),
            comment: None,
            fix: None,
        });
    }
    for vacuous in list(result, "vacuousRules") {
        let name = text(vacuous, "name");
        if !out.iter().any(|r| r.name == name) {
            out.push(CatalogRule {
                name,
                family: "rules".into(),
                severity: "error".into(),
                comment: None,
                fix: None,
            });
        }
    }
    out
}

/// The violations of the rule named `name`, in the result's order.
pub fn violations_of<'a>(result: &'a Value, name: &str) -> Vec<&'a Value> {
    violations(result)
        .into_iter()
        .filter(|v| rule_name(v) == name)
        .collect()
}

/// A violation's `fix`, else its rule's.
pub fn fix_of(violation: &Value, rule: &CatalogRule) -> Option<String> {
    string(violation, "fix").or_else(|| rule.fix.clone())
}

/// Where a violation sits: the edge's line and column for a dependency, the type's declaration
/// for an element violation, when the extractor recorded them.
pub fn position(result: &Value, violation: &Value) -> Option<(u64, u64)> {
    let from = text(violation, "from");
    let to = text(violation, "to");
    if violation.get("type").and_then(Value::as_str) == Some("element") {
        let declared = result
            .get("code")
            .and_then(|c| c.get("types"))
            .and_then(Value::as_array)?
            .iter()
            .find(|t| t.get("fullName").and_then(Value::as_str) == Some(to.as_str()))?;
        let line = declared.get("line").and_then(Value::as_u64)?;
        return Some((
            line,
            declared.get("column").and_then(Value::as_u64).unwrap_or(1),
        ));
    }
    crate::edge_position(result, &from, &to)
}

/// One line per violation: its id, `from -> to` and the line, as the failure messages list them.
pub fn describe(result: &Value, violation: &Value) -> String {
    let id = violation
        .get("id")
        .and_then(Value::as_str)
        .map(|id| format!("{id} "))
        .unwrap_or_default();
    let from = text(violation, "from");
    let to = text(violation, "to");
    let at = position(result, violation)
        .map(|(line, column)| format!(" (line {line}, column {column})"))
        .unwrap_or_default();
    let known = if crate::severity(violation) == "ignore" {
        " [known]"
    } else {
        ""
    };
    format!("{id}{from} -> {to}{at}{known}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn result() -> Value {
        json!({
            "modules": [{ "source": "a.ts", "dependencies": [{ "resolved": "b.ts", "line": 3, "column": 8 }] }],
            "code": { "types": [{ "fullName": "S.A", "line": 7, "column": 2 }, { "fullName": "S.B" }] },
            "summary": {
                "violations": [
                    { "type": "dependency", "from": "a.ts", "to": "b.ts", "rule": { "name": "no-b", "severity": "error" }, "id": "RB-1" },
                    { "type": "element", "from": "a.cs", "to": "S.A", "rule": { "name": "sealed", "severity": "ignore" }, "id": "RB-2", "fix": "Seal it." },
                    { "type": "element", "from": "b.cs", "to": "S.B", "rule": { "name": "sealed", "severity": "error" } },
                    { "type": "dependency", "from": "x", "to": "y", "rule": { "name": "zeta", "severity": "warn" }, "comment": "z" },
                    { "type": "dependency", "from": "x", "to": "z", "rule": { "name": "alpha", "severity": "info" } }
                ],
                "ruleSetUsed": {
                    "forbidden": [{ "name": "no-b", "severity": "error", "comment": "c", "fix": "f" }, { "name": "quiet" }],
                    "allowed": [{ "from": {}, "to": {}, "comment": "only these", "fix": "Use one." }],
                    "required": [{ "name": "needs", "severity": "info" }],
                    "elements": [{ "name": "sealed", "severity": "error" }],
                    "slices": [{ "name": "apart", "severity": "warn" }],
                    "diagrams": [{ "name": "drawn", "severity": "error" }]
                },
                "ratchets": [{ "name": "budget", "budget": "b.json", "count": 3, "ceiling": 2, "status": "exceeded" }],
                "vacuousRules": [{ "name": "quiet", "side": "from" }, { "name": "allowed[0]", "side": "from" }]
            }
        })
    }

    #[test]
    fn every_rule_in_configuration_order_then_the_unlisted_ones() {
        let rules = rules(&result());
        let names: Vec<(&str, &str, &str)> = rules
            .iter()
            .map(|r| (r.name.as_str(), r.family.as_str(), r.severity.as_str()))
            .collect();
        assert_eq!(
            names,
            [
                ("no-b", "forbidden", "error"),
                ("quiet", "forbidden", "warn"),
                ("not-in-allowed", "allowed", "warn"),
                ("needs", "required", "info"),
                ("sealed", "elements", "error"),
                ("apart", "slices", "warn"),
                ("drawn", "diagrams", "error"),
                ("alpha", "rules", "info"),
                ("zeta", "rules", "warn"),
                ("budget", "ratchets", "error"),
                ("allowed[0]", "rules", "error"),
            ]
        );
        assert_eq!(rules[0].comment.as_deref(), Some("c"));
        assert_eq!(rules[2].fix.as_deref(), Some("Use one."));
        assert_eq!(rules[8].comment.as_deref(), Some("z"));
        let mut strict = result();
        strict["summary"]["ruleSetUsed"]["allowedSeverity"] = json!("error");
        assert!(
            super::rules(&strict)
                .iter()
                .any(|r| r.name == "not-in-allowed" && r.severity == "error")
        );
        assert!(super::rules(&json!({})).is_empty());
    }

    #[test]
    fn violations_positions_and_descriptions() {
        let result = result();
        assert_eq!(violations_of(&result, "sealed").len(), 2);
        let all = violations(&result);
        let sealed = &rules(&result)[4];
        assert_eq!(fix_of(all[1], sealed).as_deref(), Some("Seal it."));
        assert_eq!(fix_of(all[2], sealed), None);
        assert_eq!(position(&result, all[0]), Some((3, 8)));
        assert_eq!(position(&result, all[1]), Some((7, 2)));
        assert_eq!(position(&result, all[2]), None, "no line recorded");
        assert_eq!(position(&result, all[3]), None);
        assert_eq!(
            describe(&result, all[0]),
            "RB-1 a.ts -> b.ts (line 3, column 8)"
        );
        assert_eq!(
            describe(&result, all[1]),
            "RB-2 a.cs -> S.A (line 7, column 2) [known]"
        );
        assert_eq!(describe(&result, all[2]), "b.cs -> S.B");
        assert_eq!(list(&result, "ratchets").len(), 1);
        assert!(list(&json!({}), "ratchets").is_empty());
    }
}
