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
//! `ruleSetUsed`, or an element rule, which `ruleSetUsed` does not carry) adds that rule, in name
//! order, so no violation goes unreported; so does one whose name only rules of another family
//! carry (an element rule and an anonymous dependency rule both named `unnamed`). The ratchets
//! of `summary.ratchets` follow, then any vacuous entry that names none of these.
//!
//! Names are not unique (every rule without one is `unnamed`), so each rule also has an
//! [`CatalogRule::id`]: its name the first time the name occurs, then `name#2`, `name#3` and so
//! on (skipping any id another rule already carries as its name). The id is what `junit` and
//! `trx` name a test case, what `sarif` names a rule, and what the TRX GUIDs hash, so no two
//! entries collide. A violation carries only its rule's name and severity; [`rule_index`] gives
//! it to one rule of that name: the first whose family can produce its `type` and whose severity
//! is its own, else the first whose family can produce it, else the first of the name. Vacuous and
//! expired entries go to the first rule of their name. The adapters port this line for line.

use std::fmt::Write as _;

use serde_json::Value;

use crate::text;

/// One rule of the run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogRule {
    /// The rule's identity within the run: the name, with `#n` for the n-th rule of a name
    /// already taken (module documentation).
    pub id: String,
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
        id: String::new(),
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
            id: String::new(),
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
        let kind = violation.get("type").and_then(Value::as_str);
        if !out
            .iter()
            .chain(&unlisted)
            .any(|r| r.name == name && produces(&r.family, kind))
        {
            unlisted.push(CatalogRule {
                id: String::new(),
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
            id: String::new(),
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
                id: String::new(),
                name,
                family: "rules".into(),
                severity: "error".into(),
                comment: None,
                fix: None,
            });
        }
    }
    identify(&mut out);
    out
}

/// Gives each rule its [`CatalogRule::id`]: the name at its first occurrence, then `name#n` for
/// the n-th, counting up past any id already taken.
fn identify(rules: &mut [CatalogRule]) {
    let mut taken: std::collections::BTreeSet<String> =
        rules.iter().map(|r| r.name.clone()).collect();
    let mut seen: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for rule in rules.iter_mut() {
        let count = seen.entry(rule.name.clone()).or_insert(0);
        *count += 1;
        if *count == 1 {
            rule.id = rule.name.clone();
            continue;
        }
        let mut n = *count;
        let mut id = format!("{}#{n}", rule.name);
        while taken.contains(&id) {
            n += 1;
            id = format!("{}#{n}", rule.name);
        }
        taken.insert(id.clone());
        rule.id = id;
    }
}

/// Whether a rule of `family` can produce a violation of `kind` (`summary.violations[].type`). A
/// rule known only from its violations (`rules`) can produce any.
fn produces(family: &str, kind: Option<&str>) -> bool {
    if family == "rules" {
        return true;
    }
    match kind {
        Some("element") => matches!(family, "elements" | "diagrams"),
        Some("slice") => family == "slices",
        _ => matches!(family, "forbidden" | "allowed" | "required" | "rules"),
    }
}

/// The index in `rules` of the rule a violation belongs to (module documentation), `None` when
/// no rule other than a ratchet has its name.
pub fn rule_index(rules: &[CatalogRule], violation: &Value) -> Option<usize> {
    let name = rule_name(violation);
    let named: Vec<usize> = (0..rules.len())
        .filter(|&i| rules[i].name == name && rules[i].family != "ratchets")
        .collect();
    let kind = violation.get("type").and_then(Value::as_str);
    let fitting: Vec<usize> = named
        .iter()
        .copied()
        .filter(|&i| produces(&rules[i].family, kind))
        .collect();
    let fitting = if fitting.is_empty() { named } else { fitting };
    let severity = crate::severity(violation);
    fitting
        .iter()
        .copied()
        .find(|&i| rules[i].severity == severity)
        .or_else(|| fitting.first().copied())
}

/// The violations of `rules[index]`, in the result's order.
pub fn violations_of<'a>(result: &'a Value, rules: &[CatalogRule], index: usize) -> Vec<&'a Value> {
    violations(result)
        .into_iter()
        .filter(|v| rule_index(rules, v) == Some(index))
        .collect()
}

/// Whether `rules[index]` is the first rule named `name`, the one vacuous and expired entries of
/// that name go to.
fn first_of(rules: &[CatalogRule], index: usize, name: &str) -> bool {
    rules.iter().position(|r| r.name == name) == Some(index)
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

/// How many violations a failure message lists before it says how many more there are.
pub const SHOWN: usize = 5;

/// One test case: a rule's result, as `junit` and `trx` report it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Case {
    /// The rule, or the expired known violation, the case is for.
    pub rule: CatalogRule,
    /// A failure: its message (the `fix`, then the first violations) and every violation, one
    /// per line. Only error-severity findings fail a case, as only they fail the gate.
    pub failure: Option<(String, String)>,
    /// Errors: the rule could not be trusted (vacuous, expired, a ratchet without a budget), as
    /// `(type, message)`.
    pub errors: Vec<(String, String)>,
    /// What the case reports without failing: warn, info and known findings, a vacuous rule the
    /// run only warns about, a ratchet under its ceiling.
    pub output: Vec<String>,
}

fn vacuous_message(entry: &Value) -> String {
    format!(
        "rule `{}` is vacuous: its {} side matched nothing, so it checks nothing (ADR-0007)",
        text(entry, "name"),
        text(entry, "side")
    )
}

fn expired_message(entry: &Value) -> String {
    format!(
        "{} `{}` expired on {}; it no longer applies and the run fails",
        text(entry, "kind"),
        text(entry, "name"),
        text(entry, "expires")
    )
}

fn ratchet_case(case: &mut Case, ratchet: &Value) {
    let count = crate::js_number(ratchet.get("count"));
    let budget = text(ratchet, "budget");
    match ratchet.get("status").and_then(Value::as_str) {
        Some("exceeded") => {
            let ceiling = crate::js_number(ratchet.get("ceiling"));
            let message = format!(
                "ratchet `{}`: {count} edges exceed the ceiling of {ceiling} in {budget}",
                case.rule.name
            );
            case.failure = Some((message.clone(), message));
        }
        Some("no-budget") => case.errors.push((
            "no-budget".into(),
            format!(
                "ratchet `{}`: the budget {budget} cannot be read, so the count {count} is checked against nothing",
                case.rule.name
            ),
        )),
        _ => case.output.push(format!(
            "{count} edges, within the ceiling of {} in {budget}",
            crate::js_number(ratchet.get("ceiling"))
        )),
    }
}

/// One case per rule of [`rules`], then one per expired known violation.
pub fn cases(result: &Value) -> Vec<Case> {
    let vacuous = list(result, "vacuousRules");
    let expired = list(result, "expired");
    let ratchets = list(result, "ratchets");
    let mut out: Vec<Case> = Vec::new();
    let all = rules(result);
    let mut ratchet_at = 0;
    for (index, rule) in all.iter().enumerate() {
        let mut case = Case {
            rule: rule.clone(),
            failure: None,
            errors: Vec::new(),
            output: Vec::new(),
        };
        let name = case.rule.name.clone();
        let first = first_of(&all, index, &name);
        if case.rule.family == "ratchets" {
            // The ratchet rules are summary.ratchets, in order.
            if let Some(ratchet) = ratchets.get(ratchet_at) {
                ratchet_case(&mut case, ratchet);
            }
            ratchet_at += 1;
        } else {
            let found = violations_of(result, &all, index);
            let errors: Vec<String> = found
                .iter()
                .filter(|v| crate::severity(v) == "error")
                .map(|v| describe(result, v))
                .collect();
            for v in found.iter().filter(|v| crate::severity(v) != "error") {
                case.output
                    .push(format!("{}: {}", crate::severity(v), describe(result, v)));
            }
            if !errors.is_empty() {
                let fix = found
                    .first()
                    .and_then(|v| fix_of(v, &case.rule))
                    .unwrap_or_else(|| format!("{} violation(s) of `{name}`", errors.len()));
                let mut message = fix;
                for line in errors.iter().take(SHOWN) {
                    message.push('\n');
                    message.push_str(line);
                }
                if errors.len() > SHOWN {
                    let _ = write!(message, "\n... and {} more", errors.len() - SHOWN);
                }
                case.failure = Some((message, errors.join("\n")));
            }
        }
        for entry in vacuous.iter().filter(|v| first && text(v, "name") == name) {
            if entry.get("severity").and_then(Value::as_str) == Some("warn") {
                case.output
                    .push(format!("warning: {}", vacuous_message(entry)));
            } else {
                case.errors.push(("vacuous".into(), vacuous_message(entry)));
            }
        }
        for entry in expired
            .iter()
            .filter(|e| first && text(e, "kind") == "rule" && text(e, "name") == name)
        {
            case.errors.push(("expired".into(), expired_message(entry)));
        }
        out.push(case);
    }
    for entry in expired.iter().filter(|e| text(e, "kind") != "rule") {
        out.push(Case {
            rule: CatalogRule {
                id: text(entry, "name"),
                name: text(entry, "name"),
                family: "knownViolations".into(),
                severity: "error".into(),
                comment: None,
                fix: None,
            },
            failure: None,
            errors: vec![("expired".into(), expired_message(entry))],
            output: Vec::new(),
        });
    }
    out
}

/// The receipt, `summary.inspected`, and the counts, flattened to `name = value` pairs in key
/// order: what `junit` writes as properties and `trx` in the run's output.
pub fn receipt(result: &Value) -> Vec<(String, String)> {
    fn flatten(prefix: &str, value: &Value, out: &mut Vec<(String, String)>) {
        match value {
            Value::Object(map) => {
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort();
                for key in keys {
                    flatten(&format!("{prefix}.{key}"), &map[key], out);
                }
            }
            Value::Null => {}
            other => out.push((prefix.to_owned(), crate::js_number(Some(other)))),
        }
    }
    let mut out = Vec::new();
    let Some(summary) = summary(result) else {
        return out;
    };
    if let Some(inspected) = summary.get("inspected") {
        flatten("inspected", inspected, &mut out);
    }
    for key in [
        "totalCruised",
        "totalDependenciesCruised",
        "error",
        "warn",
        "info",
        "ignore",
    ] {
        if let Some(value) = summary.get(key).filter(|v| !v.is_null()) {
            out.push((key.to_owned(), crate::js_number(Some(value))));
        }
    }
    out
}

/// Text made safe for XML 1.0: the five markup characters escaped, and, in an attribute, line
/// breaks and tabs as character references so they survive attribute normalisation. Characters
/// XML 1.0 cannot carry become U+FFFD.
pub fn xml(text: &str, attribute: bool) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\n' if attribute => out.push_str("&#10;"),
            '\r' if attribute => out.push_str("&#13;"),
            '\t' if attribute => out.push_str("&#9;"),
            '\n' | '\r' | '\t' => out.push(c),
            c if (c as u32) < 0x20 || matches!(c, '\u{FFFE}' | '\u{FFFF}') => out.push('\u{FFFD}'),
            c => out.push(c),
        }
    }
    out
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
        let catalog = rules(&result);
        assert_eq!(violations_of(&result, &catalog, 4).len(), 2);
        let all = violations(&result);
        let sealed = &catalog[4];
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

    /// A fixed vector: every anonymous rule is `unnamed`, and each must stay one rule with one
    /// identity, holding only its own violations.
    #[test]
    fn rules_sharing_a_name_keep_distinct_identities_and_their_own_violations() {
        let result = json!({ "summary": {
            "violations": [
                { "type": "dependency", "from": "a", "to": "b", "rule": { "name": "unnamed", "severity": "error" } },
                { "type": "dependency", "from": "c", "to": "d", "rule": { "name": "unnamed", "severity": "warn" } },
                { "type": "element", "from": "e.cs", "to": "E", "rule": { "name": "unnamed", "severity": "error" } },
                { "type": "dependency", "from": "f", "to": "g", "rule": { "name": "unnamed", "severity": "ignore" } }
            ],
            "ruleSetUsed": {
                "forbidden": [
                    { "name": "unnamed", "severity": "error" },
                    { "name": "unnamed#2" },
                    { "severity": "warn", "name": "unnamed" }
                ],
                "elements": [{ "name": "unnamed", "severity": "error" }]
            },
            "vacuousRules": [{ "name": "unnamed", "side": "from" }]
        } });
        let catalog = rules(&result);
        let ids: Vec<(&str, &str)> = catalog
            .iter()
            .map(|r| (r.id.as_str(), r.family.as_str()))
            .collect();
        assert_eq!(
            ids,
            [
                ("unnamed", "forbidden"),
                ("unnamed#2", "forbidden"),
                ("unnamed#3", "forbidden"),
                ("unnamed#4", "elements"),
            ]
        );
        let all = violations(&result);
        let owners: Vec<Option<usize>> = all.iter().map(|v| rule_index(&catalog, v)).collect();
        assert_eq!(owners, [Some(0), Some(2), Some(3), Some(0)]);
        assert_eq!(
            rule_index(&catalog, &json!({ "rule": { "name": "none" } })),
            None
        );
        let cases = cases(&result);
        let failures: Vec<Option<&str>> = cases
            .iter()
            .map(|c| c.failure.as_ref().map(|f| f.1.as_str()))
            .collect();
        assert_eq!(failures, [Some("a -> b"), None, None, Some("e.cs -> E")]);
        assert_eq!(cases[0].output, ["ignore: f -> g [known]"]);
        assert_eq!(cases[2].output, ["warn: c -> d"]);
        assert_eq!(
            cases[0].errors.len(),
            1,
            "the vacuous entry goes to the first rule"
        );
        assert!(cases[1..].iter().all(|c| c.errors.is_empty()));
        // ruleSetUsed does not carry element rules: an element violation whose name only a
        // dependency rule has is a rule of its own, not that rule's.
        let unlisted = json!({ "summary": {
            "violations": [
                { "type": "dependency", "from": "a", "to": "b", "rule": { "name": "unnamed", "severity": "error" } },
                { "type": "element", "from": "e.cs", "to": "E", "rule": { "name": "unnamed", "severity": "error" } }
            ],
            "ruleSetUsed": { "forbidden": [{ "name": "unnamed", "severity": "error" }] }
        } });
        let catalog = rules(&unlisted);
        let ids: Vec<(&str, &str)> = catalog
            .iter()
            .map(|r| (r.id.as_str(), r.family.as_str()))
            .collect();
        assert_eq!(ids, [("unnamed", "forbidden"), ("unnamed#2", "rules")]);
        let owners: Vec<Option<usize>> = violations(&unlisted)
            .iter()
            .map(|v| rule_index(&catalog, v))
            .collect();
        assert_eq!(owners, [Some(0), Some(1)]);
    }

    #[test]
    fn xml_escapes_markup_breaks_in_attributes_and_what_xml_cannot_carry() {
        assert_eq!(xml("a<b>&\"c'", true), "a&lt;b&gt;&amp;&quot;c&apos;");
        assert_eq!(xml("1\n2\r3\t4", true), "1&#10;2&#13;3&#9;4");
        assert_eq!(xml("1\n2\r3\t4", false), "1\n2\r3\t4");
        assert_eq!(
            xml("x\u{1}y\u{FFFE}\u{FFFF}", false),
            "x\u{FFFD}y\u{FFFD}\u{FFFD}"
        );
        assert_eq!(xml("é ☃", true), "é ☃");
    }

    #[test]
    fn ratchets_expiry_and_long_failures_become_cases() {
        let many: Vec<Value> = (0..7)
            .map(|i| json!({ "type": "dependency", "from": format!("f{i}"), "to": "t", "rule": { "name": "wide", "severity": "error" } }))
            .collect();
        let result = json!({ "summary": {
            "violations": many,
            "ruleSetUsed": { "forbidden": [{ "name": "wide", "severity": "error" }, { "name": "old", "expires": "2020-01-01" }, { "name": "soft" }] },
            "ratchets": [
                { "name": "over", "budget": "o.json", "count": 3, "ceiling": 2, "status": "exceeded" },
                { "name": "lost", "budget": "l.json", "count": 1, "status": "no-budget" },
                { "name": "held", "budget": "h.json", "count": 1, "ceiling": 4, "status": "held" }
            ],
            "vacuousRules": [{ "name": "soft", "side": "from", "severity": "warn" }],
            "expired": [{ "name": "old", "expires": "2020-01-01", "kind": "rule" }]
        } });
        let cases = cases(&result);
        let by = |name: &str| cases.iter().find(|c| c.rule.name == name).cloned();
        let wide = by("wide").and_then(|c| c.failure).unwrap_or_default();
        assert_eq!(
            wide.0,
            "7 violation(s) of `wide`\nf0 -> t\nf1 -> t\nf2 -> t\nf3 -> t\nf4 -> t\n... and 2 more"
        );
        assert_eq!(wide.1.lines().count(), 7, "the body lists every violation");
        let old = by("old").map(|c| c.errors).unwrap_or_default();
        assert_eq!(
            old,
            [(
                "expired".to_owned(),
                "rule `old` expired on 2020-01-01; it no longer applies and the run fails"
                    .to_owned()
            )]
        );
        let soft = by("soft").unwrap_or_else(|| cases[0].clone());
        assert!(soft.errors.is_empty());
        assert_eq!(
            soft.output,
            [
                "warning: rule `soft` is vacuous: its from side matched nothing, so it checks nothing (ADR-0007)"
            ]
        );
        assert_eq!(
            by("over").and_then(|c| c.failure).map(|f| f.0).as_deref(),
            Some("ratchet `over`: 3 edges exceed the ceiling of 2 in o.json")
        );
        let lost = by("lost").unwrap_or_else(|| cases[0].clone());
        assert_eq!(lost.errors[0].0, "no-budget");
        assert!(lost.failure.is_none());
        let held = by("held").unwrap_or_else(|| cases[0].clone());
        assert_eq!(held.output, ["1 edges, within the ceiling of 4 in h.json"]);
        assert!(held.failure.is_none() && held.errors.is_empty());
        assert_eq!(cases.len(), 6);
    }

    #[test]
    fn the_receipt_is_flat_and_sorted() {
        let result = json!({ "summary": {
            "inspected": { "python": { "modules": 2, "files": 3 }, "dotnet": { "files": 0, "attribution": { "pdb": 4 }, "projects": null } },
            "totalCruised": 5, "error": 0, "ignore": null
        } });
        assert_eq!(
            receipt(&result),
            [
                (
                    "inspected.dotnet.attribution.pdb".to_owned(),
                    "4".to_owned()
                ),
                ("inspected.dotnet.files".to_owned(), "0".to_owned()),
                ("inspected.python.files".to_owned(), "3".to_owned()),
                ("inspected.python.modules".to_owned(), "2".to_owned()),
                ("totalCruised".to_owned(), "5".to_owned()),
                ("error".to_owned(), "0".to_owned()),
            ]
        );
        assert!(receipt(&json!({})).is_empty());
    }
}
