//! `agent`: violations shaped for a model, grouped by rule, cheapest fix first, token-budgeted.
//!
//! - Contract: [Wave 1 plan § 1.5](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#15-interfaces-and-contracts-frozen-by-this-wave)
//!   (the shape, fix-cost ordering, `--max-findings`)
//! - Source: [design § Precision an agent can act on](../../../docs/artifacts/design.md#precision-an-agent-can-act-on),
//!   [design § The agentic engineering hat](../../../docs/artifacts/design.md#the-agentic-engineering-hat-turn-two)
//! - Decision: [ADR-0021](../../../docs/adr/0021-agent-surface-cli-first.md)
//! - Requirement: [FR-OUT-02](../../../docs/prd.md#fr-out-02)
//!
//! Each violation carries `cost`: `edgesToMove` (one for a dependency, the steps of the cycle or
//! the reachability path, zero for a module finding) plus `targetFanIn` (how many modules depend on
//! the target), summed into `score`. Violations are ordered by `score`, then `from` and `to`; rules
//! by their cheapest violation, then name. `maxFindings` caps each rule's shown violations; `count`
//! keeps the total and `budget.truncated` says whether anything was cut.

use std::collections::HashMap;

use serde_json::{Map, Value, json};

use crate::{Rendered, edge_position, find_rule, severity, text};

/// The default number of violations shown per rule.
pub const DEFAULT_MAX_FINDINGS: usize = 5;

fn fan_in(result: &Value) -> HashMap<String, usize> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for module in result
        .get("modules")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        for d in module
            .get("dependencies")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            *counts.entry(text(d, "resolved")).or_default() += 1;
        }
    }
    counts
}

fn edge_kind(result: &Value, from: &str, to: &str) -> Value {
    result
        .get("modules")
        .and_then(Value::as_array)
        .and_then(|m| m.iter().find(|x| text(x, "source") == from))
        .and_then(|m| m.get("dependencies").and_then(Value::as_array))
        .and_then(|d| d.iter().find(|x| text(x, "resolved") == to))
        .and_then(|d| d.get("dependencyKind").cloned())
        .unwrap_or(Value::Null)
}

fn steps(v: &Value, key: &str) -> usize {
    v.get(key).and_then(Value::as_array).map_or(0, Vec::len)
}

/// Renders `agent`.
pub fn render(result: &Value, max_findings: usize) -> Rendered {
    let summary = result.get("summary").cloned().unwrap_or(Value::Null);
    let rule_set = summary.get("ruleSetUsed");
    let fan = fan_in(result);
    let mut groups: Vec<(String, String, Vec<(u64, Value)>)> = Vec::new();
    for v in summary
        .get("violations")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let sev = severity(v);
        if sev == "ignore" {
            continue;
        }
        let from = text(v, "from");
        let to = text(v, "to");
        let edges = match v.get("type").and_then(Value::as_str) {
            Some("module") => 0,
            Some("cycle") => steps(v, "cycle"),
            Some("reachability") => steps(v, "via"),
            _ => 1,
        };
        let target_fan_in = if v.get("type").and_then(Value::as_str) == Some("module") {
            0
        } else {
            fan.get(&to).copied().unwrap_or(0)
        };
        let score = (edges + target_fan_in) as u64;
        let (line, column) = edge_position(result, &from, &to)
            .map_or((Value::Null, Value::Null), |(l, c)| (json!(l), json!(c)));
        let finding = json!({
            "id": v.get("id").cloned().unwrap_or(Value::Null),
            "from": from, "to": to, "line": line, "column": column,
            "member": Value::Null,
            "dependencyKind": edge_kind(result, &text(v, "from"), &text(v, "to")),
            "cost": { "edgesToMove": edges, "targetFanIn": target_fan_in, "score": score }
        });
        let name = v.get("rule").map(|r| text(r, "name")).unwrap_or_default();
        match groups.iter_mut().find(|(n, _, _)| *n == name) {
            Some((_, _, list)) => list.push((score, finding)),
            None => groups.push((name, sev, vec![(score, finding)])),
        }
    }
    for (_, _, list) in &mut groups {
        list.sort_by(|(a, x), (b, y)| {
            a.cmp(b)
                .then_with(|| text(x, "from").cmp(&text(y, "from")))
                .then_with(|| text(x, "to").cmp(&text(y, "to")))
        });
    }
    groups.sort_by(|(na, _, a), (nb, _, b)| {
        let min = |l: &Vec<(u64, Value)>| l.first().map_or(u64::MAX, |(s, _)| *s);
        min(a).cmp(&min(b)).then_with(|| na.cmp(nb))
    });
    let mut truncated = false;
    let rules: Vec<Value> = groups
        .into_iter()
        .map(|(name, sev, list)| {
            let count = list.len();
            let shown: Vec<Value> = list
                .into_iter()
                .take(max_findings)
                .map(|(_, f)| f)
                .collect();
            truncated |= shown.len() < count;
            let rule = find_rule(rule_set, &name);
            let comment = rule.and_then(|r| r.get("comment")).and_then(Value::as_str);
            let fix = rule
                .and_then(|r| r.get("fix"))
                .cloned()
                .unwrap_or(Value::Null);
            let decision = comment
                .and_then(crate::decision)
                .map_or(Value::Null, Value::String);
            let mut out = Map::new();
            out.insert("name".into(), json!(name));
            out.insert("severity".into(), json!(sev));
            out.insert("count".into(), json!(count));
            out.insert("shown".into(), json!(shown.len()));
            out.insert("fix".into(), fix);
            out.insert("decision".into(), decision);
            out.insert("violations".into(), Value::Array(shown));
            Value::Object(out)
        })
        .collect();
    let report = json!({
        "inspected": summary.get("inspected").cloned().unwrap_or_else(|| json!({})),
        "vacuousRules": summary.get("vacuousRules").cloned().unwrap_or_else(|| json!([])),
        "rules": rules,
        "budget": { "maxFindings": max_findings, "truncated": truncated }
    });
    let mut output = serde_json::to_string_pretty(&report).unwrap_or_default();
    output.push('\n');
    Rendered {
        output,
        exit_code: summary.get("error").and_then(Value::as_u64).unwrap_or(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grouped_cost_ordered_and_budgeted() -> Result<(), serde_json::Error> {
        let result = json!({
            "modules": [
                { "source": "a", "dependencies": [{ "resolved": "hub", "line": 3, "column": 1, "dependencyKind": "import" }, { "resolved": "leaf", "line": 4, "column": 1 }] },
                { "source": "b", "dependencies": [{ "resolved": "hub" }] },
                { "source": "c", "dependencies": [{ "resolved": "hub" }] }
            ],
            "summary": {
                "error": 3, "inspected": { "typescript": { "files": 3, "assemblies": 0, "modules": 5 } },
                "violations": [
                    { "type": "dependency", "from": "a", "to": "hub", "id": "RB-1", "rule": { "name": "z-rule", "severity": "error" } },
                    { "type": "dependency", "from": "b", "to": "hub", "id": "RB-2", "rule": { "name": "z-rule", "severity": "error" } },
                    { "type": "dependency", "from": "a", "to": "leaf", "id": "RB-3", "rule": { "name": "a-rule", "severity": "error" } },
                    { "type": "module", "from": "o", "to": "o", "rule": { "name": "orphans", "severity": "warn" } },
                    { "type": "module", "from": "q", "to": "q", "rule": { "name": "quiet", "severity": "ignore" } }
                ],
                "ruleSetUsed": { "forbidden": [{ "name": "z-rule", "comment": "why plan:wave-1", "fix": "move" }] }
            }
        });
        let out = render(&result, 1);
        let report: Value = serde_json::from_str(&out.output)?;
        let names: Vec<&str> = report["rules"]
            .as_array()
            .map(|r| r.iter().filter_map(|x| x["name"].as_str()).collect())
            .unwrap_or_default();
        assert_eq!(names, ["orphans", "a-rule", "z-rule"], "cheapest first");
        let z = &report["rules"][2];
        assert_eq!(z["count"], 2);
        assert_eq!(z["shown"], 1);
        assert_eq!(z["fix"], "move");
        assert_eq!(z["decision"], "plan:wave-1");
        assert_eq!(
            z["violations"][0]["cost"],
            json!({ "edgesToMove": 1, "targetFanIn": 3, "score": 4 })
        );
        assert_eq!(z["violations"][0]["line"], 3);
        assert_eq!(z["violations"][0]["dependencyKind"], "import");
        assert_eq!(z["violations"][0]["member"], Value::Null);
        assert_eq!(
            report["budget"],
            json!({ "maxFindings": 1, "truncated": true })
        );
        assert_eq!(report["inspected"]["typescript"]["files"], 3);
        assert_eq!(out.exit_code, 3);
        let cycle = json!({ "summary": { "violations": [{ "type": "cycle", "from": "a", "to": "b", "cycle": [{ "name": "b" }, { "name": "a" }], "rule": { "name": "c", "severity": "error" } },
                                                         { "type": "reachability", "from": "a", "to": "b", "via": [{ "name": "b" }], "rule": { "name": "r", "severity": "info" } }] } });
        let report: Value = serde_json::from_str(&render(&cycle, DEFAULT_MAX_FINDINGS).output)?;
        assert_eq!(
            report["rules"][0]["violations"][0]["cost"]["edgesToMove"],
            1
        );
        assert_eq!(
            report["rules"][1]["violations"][0]["cost"]["edgesToMove"],
            2
        );
        assert_eq!(report["budget"]["truncated"], false);
        Ok(())
    }
}
