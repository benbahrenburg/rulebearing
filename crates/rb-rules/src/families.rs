//! The `ArchUnitNET` rule families in a run: element, slice and diagram rules evaluated and turned
//! into violations beside the dependency rules'.
//!
//! - Architecture: [The rule engine](../../../docs/architecture.md#the-rule-engine)
//! - Plan: [Wave 2, Steps 5 and 6](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#25-step-5-the-element-rule-engine-and-the-capability-table-2c)
//! - Decisions: [ADR-0015](../../../docs/adr/0015-stable-violation-id.md) (the id hashes the
//!   rule, the object's file, the object and the condition key),
//!   [ADR-0007](../../../docs/adr/0007-vacuous-rules-fail-by-default.md) (an empty selection is
//!   reported vacuous with side `select`)
//!
//! | Family | One violation per |
//! | --- | --- |
//! | element | selected object that fails `should` (`from` its file, `to` the object) |
//! | diagram | type that does not adhere to the diagram |
//! | slice | slice that depends on another, or cycle of slices (`from` and `to` the slices) |

use rb_config::Config;
use rb_config::elements::{ElementRule, Expr, Test};
use rb_model::{GraphDocument, VacuousRule, violation_id::violation_id};
use serde_json::{Value, json};

use crate::elements::{Architecture, ElementError};

/// The decision token a comment carries (`adr:0003`).
fn decision(comment: Option<&str>) -> Option<String> {
    let comment = comment?;
    let at = comment.find("adr:")?;
    let token: String = comment[at..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == ':')
        .collect();
    (token.len() > 4).then_some(token)
}

/// The key a rule's `should` is known by, for the violation id: the first test's key.
fn condition_key(expr: &Expr) -> String {
    match expr {
        Expr::Test(Test { key, .. }) => key.clone(),
        Expr::All(items) | Expr::Any(items) => items.first().map(condition_key).unwrap_or_default(),
        Expr::Not(inner) => format!("not {}", condition_key(inner)),
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "the violation's fields, as the reporters read them"
)]
fn violation(
    rule: &str,
    severity: rb_model::Severity,
    comment: Option<&str>,
    fix: Option<&str>,
    kind: &str,
    from: &str,
    to: &str,
    key: &str,
) -> Value {
    let mut v = json!({
        "from": from,
        "to": to,
        "type": kind,
        "rule": { "name": rule, "severity": severity.as_str() },
        "id": violation_id(rule, from, to, key),
    });
    if let Some(c) = comment {
        v["comment"] = json!(c);
    }
    if let Some(f) = fix {
        v["fix"] = json!(f);
    }
    if let Some(d) = decision(comment) {
        v["decision"] = json!(d);
    }
    v
}

fn element_violations(
    architecture: &Architecture<'_>,
    rule: &ElementRule,
    found: &mut Vec<Value>,
    empty: &mut Vec<VacuousRule>,
) -> Result<(), ElementError> {
    if rule.severity == rb_model::Severity::Ignore {
        return Ok(());
    }
    let outcome = crate::elements::evaluate(architecture, rule)?;
    let key = condition_key(&rule.should);
    if outcome.vacuous {
        empty.push(VacuousRule::new(rule.name.clone(), "select"));
    }
    if outcome.existence_failed {
        found.push(violation(
            &rule.name,
            rule.severity,
            rule.comment.as_deref(),
            rule.fix.as_deref(),
            "element",
            "",
            "",
            &key,
        ));
    }
    for failure in outcome.failures() {
        let from = failure.file.as_deref().unwrap_or(&failure.object);
        found.push(violation(
            &rule.name,
            rule.severity,
            rule.comment.as_deref(),
            rule.fix.as_deref(),
            "element",
            from,
            &failure.object,
            &key,
        ));
    }
    Ok(())
}

/// One family rule as `summary.ruleSetUsed` lists it: the fields a reporter or a test adapter
/// reads to give every rule its own result, passing or not.
fn used(
    name: &str,
    severity: rb_model::Severity,
    comment: Option<&str>,
    fix: Option<&str>,
) -> Value {
    let mut v = json!({ "name": name, "severity": severity.as_str() });
    if let Some(c) = comment {
        v["comment"] = json!(c);
    }
    if let Some(f) = fix {
        v["fix"] = json!(f);
    }
    v
}

/// The additive `summary.ruleSetUsed` keys `elements`, `slices` and `diagrams`, each present
/// only when the configuration has such rules, so the `junit`, `trx` and `sarif` reporters and
/// the test adapters list every rule
/// ([Wave 2 plan § 1.5](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#15-interfaces-and-contracts-this-wave-freezes),
/// the test adapter contract). `--strict-schema` removes them with the other additions.
pub fn rule_set_used(rules: &rb_config::model::Rules) -> serde_json::Map<String, Value> {
    let mut out = serde_json::Map::new();
    let elements: Vec<Value> = rules
        .elements
        .iter()
        .map(|r| used(&r.name, r.severity, r.comment.as_deref(), r.fix.as_deref()))
        .collect();
    let slices: Vec<Value> = rules
        .slices
        .iter()
        .map(|r| used(&r.name, r.severity, r.comment.as_deref(), r.fix.as_deref()))
        .collect();
    let diagrams: Vec<Value> = rules
        .diagrams
        .iter()
        .map(|r| used(&r.name, r.severity, r.comment.as_deref(), r.fix.as_deref()))
        .collect();
    for (key, list) in [
        ("elements", elements),
        ("slices", slices),
        ("diagrams", diagrams),
    ] {
        if !list.is_empty() {
            out.insert(key.into(), Value::Array(list));
        }
    }
    out
}

/// Every element, slice and diagram violation of a run, and the rules whose selection is empty.
///
/// # Errors
/// [`ElementError`] for a rule that cannot be evaluated as written.
pub fn evaluate(
    document: &GraphDocument,
    config: &Config,
) -> Result<(Vec<Value>, Vec<VacuousRule>), ElementError> {
    let mut architecture = Architecture::new(document);
    if let Some(folder) = config.files.first().and_then(|f| f.parent()) {
        architecture.base = folder.to_path_buf();
    }
    let mut found = Vec::new();
    let mut empty = Vec::new();
    for rule in &config.rules.elements {
        element_violations(&architecture, rule, &mut found, &mut empty)?;
    }
    for rule in &config.rules.diagrams {
        let as_element = rule.as_element_rule();
        element_violations(&architecture, &as_element, &mut found, &mut empty)?;
    }
    for rule in &config.rules.slices {
        if rule.severity == rb_model::Severity::Ignore {
            continue;
        }
        let outcome = crate::slices::evaluate(&architecture, rule)?;
        if outcome.vacuous {
            empty.push(VacuousRule::new(rule.name.clone(), "select"));
        }
        for failure in &outcome.failures {
            let from = failure.slices.first().cloned().unwrap_or_default();
            let to = failure
                .slices
                .get(1)
                .cloned()
                .or_else(|| failure.edges.first().map(|(_, t)| t.clone()))
                .unwrap_or_default();
            let key = match failure.condition {
                rb_config::elements::SliceCondition::NotDependOnEachOther => "notDependOnEachOther",
                rb_config::elements::SliceCondition::BeFreeOfCycles => "beFreeOfCycles",
            };
            let mut v = violation(
                &rule.name,
                rule.severity,
                rule.comment.as_deref(),
                rule.fix.as_deref(),
                "slice",
                &from,
                &to,
                key,
            );
            v["via"] = json!(
                failure
                    .edges
                    .iter()
                    .map(|(a, b)| json!({ "name": format!("{a} -> {b}"), "dependencyTypes": [] }))
                    .collect::<Vec<_>>()
            );
            found.push(v);
        }
    }
    found.sort_by(|a, b| {
        let key = |v: &Value| {
            (
                v["rule"]["name"].as_str().unwrap_or_default().to_owned(),
                v["from"].as_str().unwrap_or_default().to_owned(),
                v["to"].as_str().unwrap_or_default().to_owned(),
            )
        };
        key(a).cmp(&key(b))
    });
    Ok((found, empty))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decisions_and_condition_keys() {
        assert_eq!(
            decision(Some("Keep it sealed. adr:0004 please")),
            Some("adr:0004".into())
        );
        assert_eq!(decision(Some("no token")), None);
        assert_eq!(decision(None), None);
        let expr = rb_config::elements::parse_expr(
            &json!({ "all": [{ "beSealed": true }] }),
            rb_config::elements::Side::Should,
            "t",
        );
        assert_eq!(
            expr.map(|e| condition_key(&e)).ok().as_deref(),
            Some("beSealed")
        );
    }

    #[test]
    fn the_rule_set_lists_every_family_rule() -> Result<(), Box<dyn std::error::Error>> {
        assert!(rule_set_used(&rb_config::model::Rules::default()).is_empty());
        let rules = rb_config::model::Rules {
            elements: rb_config::elements::parse_elements(&json!([
                { "name": "sealed", "severity": "warn", "comment": "c adr:0001", "fix": "Seal it.",
                  "select": { "kind": "type" }, "should": { "beSealed": true } }
            ]))?,
            slices: rb_config::elements::parse_slices(&json!([
                { "name": "apart", "matching": "A.(*)", "should": "notDependOnEachOther" }
            ]))?,
            diagrams: rb_config::elements::parse_diagrams(&json!([
                { "name": "drawn", "severity": "info", "select": { "kind": "type" }, "adhereTo": "a.puml" }
            ]))?,
            ..rb_config::model::Rules::default()
        };
        let used = rule_set_used(&rules);
        assert_eq!(
            Value::Object(used),
            json!({
                "elements": [{ "name": "sealed", "severity": "warn", "comment": "c adr:0001", "fix": "Seal it." }],
                "slices": [{ "name": "apart", "severity": "error" }],
                "diagrams": [{ "name": "drawn", "severity": "info" }]
            })
        );
        Ok(())
    }
}
