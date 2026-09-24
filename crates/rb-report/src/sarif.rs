//! `sarif`: SARIF 2.1.0 for code scanning (GitHub, Azure DevOps Advanced Security).
//!
//! - Contract: [Wave 2 plan § 1.5](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#15-interfaces-and-contracts-this-wave-freezes)
//!   (one `rule` per configuration rule, `comment` as `help.text`, `fix` as the recommendation,
//!   `partialFingerprints.rulebearing/v1` the violation id)
//! - Plan: [Wave 2, Step 10](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#210-step-10-reporters-and-baseline-semantics-2e)
//! - Source: [design § Reporters](../../../docs/artifacts/design.md#reporters)
//! - Decision: [ADR-0015](../../../docs/adr/0015-stable-violation-id.md) (the id is the fingerprint)
//! - Requirement: [FR-OUT-02](../../../docs/prd.md#fr-out-02)
//! - Specification: the OASIS SARIF 2.1.0 schema, vendored in `tests/schemas/`, which
//!   `tests/sarif_schema.rs` validates every output against
//!
//! One run, whose driver lists every rule of the configuration ([`crate::catalog`]):
//! `shortDescription` the name, `fullDescription` and `help.text` the comment, `help.markdown` the
//! comment and then the `fix` under **Fix**, `defaultConfiguration.level` from the severity
//! (`error`, `warning` for `warn`, `note` for `info`, `none` for `ignore`), and `properties` with
//! the family, the `fix` and the decision token. One result per violation, with the rule's index,
//! its level, a message naming the edge or object and the `fix`, the location of `from` (with the
//! line and column when the extractor recorded them) and the fingerprint. A known violation
//! (severity `ignore`) is reported with an external suppression, so code scanning shows it as
//! baselined rather than dropping it. A vacuous rule and an expired entry are configuration
//! notifications on the invocation. Exits 0, as the data reporters do
//! ([ADR-0030](../../../docs/adr/0030-the-reporter-decides-the-error-count-exit.md)).

use serde_json::{Map, Value, json};

use crate::catalog::{self, CatalogRule};
use crate::{Rendered, text};

/// The schema URI the log names.
pub const SCHEMA: &str = "https://json.schemastore.org/sarif-2.1.0.json";
/// The fingerprint key: the stable id, versioned so a later scheme can sit beside it.
pub const FINGERPRINT: &str = "rulebearing/v1";

/// SARIF's level for a Rulebearing severity.
pub fn level(severity: &str) -> &'static str {
    match severity {
        "error" => "error",
        "warn" => "warning",
        "info" => "note",
        _ => "none",
    }
}

fn rule_descriptor(rule: &CatalogRule) -> Value {
    let mut descriptor = Map::new();
    descriptor.insert("id".into(), json!(rule.name));
    descriptor.insert("shortDescription".into(), json!({ "text": rule.name }));
    let help = rule.comment.clone().unwrap_or_else(|| rule.name.clone());
    if let Some(comment) = &rule.comment {
        descriptor.insert("fullDescription".into(), json!({ "text": comment }));
    }
    let markdown = match &rule.fix {
        Some(fix) => format!("{help}\n\n**Fix:** {fix}"),
        None => help.clone(),
    };
    descriptor.insert("help".into(), json!({ "text": help, "markdown": markdown }));
    descriptor.insert(
        "defaultConfiguration".into(),
        json!({ "level": level(&rule.severity) }),
    );
    let mut properties = Map::new();
    properties.insert("family".into(), json!(rule.family));
    if let Some(fix) = &rule.fix {
        properties.insert("fix".into(), json!(fix));
    }
    if let Some(token) = rule.comment.as_deref().and_then(crate::decision) {
        properties.insert("decision".into(), json!(token));
    }
    descriptor.insert("properties".into(), Value::Object(properties));
    Value::Object(descriptor)
}

fn message(violation: &Value, rule: &CatalogRule) -> String {
    let from = text(violation, "from");
    let to = text(violation, "to");
    let subject = if from.is_empty() && to.is_empty() {
        "the selection as a whole".to_owned()
    } else {
        format!("{from} -> {to}")
    };
    match catalog::fix_of(violation, rule) {
        Some(fix) => format!("{}: {subject}. {fix}", rule.name),
        None => format!("{}: {subject}", rule.name),
    }
}

fn result_of(
    result: &Value,
    violation: &Value,
    index: usize,
    rule: &CatalogRule,
    prefix: &str,
) -> Value {
    let severity = crate::severity(violation);
    let mut out = Map::new();
    out.insert("ruleId".into(), json!(rule.name));
    out.insert("ruleIndex".into(), json!(index));
    out.insert("level".into(), json!(level(&severity)));
    out.insert(
        "message".into(),
        json!({ "text": message(violation, rule) }),
    );
    let from = text(violation, "from");
    if !from.is_empty() && from != "undefined" {
        let mut physical = Map::new();
        physical.insert(
            "artifactLocation".into(),
            json!({ "uri": format!("{prefix}{from}") }),
        );
        if let Some((line, column)) = catalog::position(result, violation).filter(|(l, _)| *l > 0) {
            physical.insert(
                "region".into(),
                json!({ "startLine": line, "startColumn": column.max(1) }),
            );
        }
        out.insert(
            "locations".into(),
            json!([{ "physicalLocation": Value::Object(physical) }]),
        );
    }
    if let Some(id) = violation.get("id").and_then(Value::as_str) {
        out.insert("partialFingerprints".into(), json!({ FINGERPRINT: id }));
    }
    if severity == "ignore" {
        out.insert(
            "suppressions".into(),
            json!([{ "kind": "external", "justification": "listed in knownViolations" }]),
        );
    }
    let mut properties = Map::new();
    properties.insert("to".into(), json!(text(violation, "to")));
    if let Some(kind) = violation.get("type").and_then(Value::as_str) {
        properties.insert("type".into(), json!(kind));
    }
    out.insert("properties".into(), Value::Object(properties));
    Value::Object(out)
}

fn notifications(result: &Value) -> Vec<Value> {
    let mut out = Vec::new();
    for vacuous in catalog::list(result, "vacuousRules") {
        let name = text(vacuous, "name");
        let warn = vacuous.get("severity").and_then(Value::as_str) == Some("warn");
        out.push(json!({
            "descriptor": { "id": name },
            "level": if warn { "warning" } else { "error" },
            "message": { "text": format!(
                "rule `{name}` is vacuous: its {} side matched nothing, so it checks nothing",
                text(vacuous, "side")
            ) }
        }));
    }
    for expired in catalog::list(result, "expired") {
        out.push(json!({
            "descriptor": { "id": text(expired, "name") },
            "level": "error",
            "message": { "text": format!(
                "{} `{}` expired on {}; it no longer applies",
                text(expired, "kind"), text(expired, "name"), text(expired, "expires")
            ) }
        }));
    }
    out
}

/// Renders `sarif`. `prefix` is put before each path, the run's folder from the repository
/// root, because code scanning places a result by its path from the root.
pub fn render(result: &Value, prefix: &str) -> Rendered {
    let rules = catalog::rules(result);
    let mut results = Vec::new();
    for violation in catalog::violations(result) {
        let name = catalog::rule_name(violation);
        if let Some(index) = rules.iter().position(|r| r.name == name) {
            results.push(result_of(result, violation, index, &rules[index], prefix));
        }
    }
    let log = json!({
        "$schema": SCHEMA,
        "version": "2.1.0",
        "runs": [{
            "tool": { "driver": {
                "name": "rulebearing",
                "informationUri": "https://github.com/benbahrenburg/rulebearing",
                "semanticVersion": env!("CARGO_PKG_VERSION"),
                "rules": rules.iter().map(rule_descriptor).collect::<Vec<_>>()
            } },
            "invocations": [{
                "executionSuccessful": true,
                "toolConfigurationNotifications": notifications(result)
            }],
            "results": results
        }]
    });
    let mut output = serde_json::to_string_pretty(&log).unwrap_or_default();
    output.push('\n');
    Rendered {
        output,
        exit_code: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result() -> Value {
        json!({
            "modules": [{ "source": "src/a.ts", "dependencies": [{ "resolved": "src/b.ts", "line": 3, "column": 8 }] }],
            "summary": {
                "violations": [
                    { "type": "dependency", "from": "src/a.ts", "to": "src/b.ts", "rule": { "name": "no-b", "severity": "error" }, "id": "RB-4f2a9c1e" },
                    { "type": "element", "from": "", "to": "", "rule": { "name": "exists", "severity": "warn" } },
                    { "type": "dependency", "from": "src/c.ts", "to": "src/b.ts", "rule": { "name": "no-b", "severity": "ignore" }, "id": "RB-00000001" }
                ],
                "ruleSetUsed": {
                    "forbidden": [{ "name": "no-b", "severity": "error", "comment": "B is private. adr:0003", "fix": "Import b through its index." }],
                    "elements": [{ "name": "exists", "severity": "warn" }]
                },
                "vacuousRules": [{ "name": "dead", "side": "from" }, { "name": "soft", "side": "select", "severity": "warn" }],
                "expired": [{ "name": "RB-9", "expires": "2026-01-01", "kind": "knownViolation" }]
            }
        })
    }

    #[test]
    fn levels() {
        assert_eq!(
            ["error", "warn", "info", "ignore", "x"].map(level),
            ["error", "warning", "note", "none", "none"]
        );
    }

    #[test]
    fn one_rule_per_configuration_rule_and_one_result_per_violation()
    -> Result<(), serde_json::Error> {
        let rendered = render(&result(), "web/");
        assert_eq!(rendered.exit_code, 0);
        assert!(rendered.output.ends_with("}\n"));
        let log: Value = serde_json::from_str(&rendered.output)?;
        assert_eq!(log["version"], "2.1.0");
        assert_eq!(log["$schema"], SCHEMA);
        let run = &log["runs"][0];
        let rules = &run["tool"]["driver"]["rules"];
        assert_eq!(rules[0]["id"], "no-b");
        assert_eq!(rules[0]["help"]["text"], "B is private. adr:0003");
        assert_eq!(
            rules[0]["help"]["markdown"],
            "B is private. adr:0003\n\n**Fix:** Import b through its index."
        );
        assert_eq!(
            rules[0]["fullDescription"]["text"],
            "B is private. adr:0003"
        );
        assert_eq!(rules[0]["defaultConfiguration"]["level"], "error");
        assert_eq!(
            rules[0]["properties"],
            json!({ "family": "forbidden", "fix": "Import b through its index.", "decision": "adr:0003" })
        );
        assert_eq!(
            rules[1]["help"],
            json!({ "text": "exists", "markdown": "exists" })
        );
        assert_eq!(rules[1].get("fullDescription"), None);
        assert_eq!(rules[1]["properties"], json!({ "family": "elements" }));
        // dead and soft are vacuous rules the rule set does not list.
        assert_eq!(rules.as_array().map(Vec::len), Some(4));

        let results = &run["results"];
        assert_eq!(
            results[0],
            json!({
                "ruleId": "no-b", "ruleIndex": 0, "level": "error",
                "message": { "text": "no-b: src/a.ts -> src/b.ts. Import b through its index." },
                "locations": [{ "physicalLocation": {
                    "artifactLocation": { "uri": "web/src/a.ts" },
                    "region": { "startLine": 3, "startColumn": 8 } } }],
                "partialFingerprints": { "rulebearing/v1": "RB-4f2a9c1e" },
                "properties": { "to": "src/b.ts", "type": "dependency" }
            })
        );
        assert_eq!(results[1]["ruleIndex"], 1);
        assert_eq!(results[1]["level"], "warning");
        assert_eq!(
            results[1]["message"]["text"],
            "exists: the selection as a whole"
        );
        assert_eq!(results[1].get("locations"), None);
        assert_eq!(results[1].get("partialFingerprints"), None);
        assert_eq!(results[2]["level"], "none");
        assert_eq!(
            results[2]["suppressions"],
            json!([{ "kind": "external", "justification": "listed in knownViolations" }])
        );
        assert_eq!(
            results[2]["locations"][0]["physicalLocation"].get("region"),
            None
        );

        let notes = &run["invocations"][0]["toolConfigurationNotifications"];
        assert_eq!(notes[0]["level"], "error");
        assert_eq!(notes[0]["descriptor"]["id"], "dead");
        assert_eq!(
            notes[0]["message"]["text"],
            "rule `dead` is vacuous: its from side matched nothing, so it checks nothing"
        );
        assert_eq!(notes[1]["level"], "warning");
        assert_eq!(
            notes[2]["message"]["text"],
            "knownViolation `RB-9` expired on 2026-01-01; it no longer applies"
        );
        assert_eq!(run["invocations"][0]["executionSuccessful"], true);
        Ok(())
    }

    #[test]
    fn fingerprints_are_stable_across_runs() {
        assert_eq!(render(&result(), "").output, render(&result(), "").output);
        let empty = render(&json!({}), "");
        assert!(empty.output.contains("\"results\": []"));
    }
}
