//! `teamcity`: TeamCity service messages. dependency-cruiser 18.2.0's `src/report/teamcity.mjs`,
//! ported.
//!
//! - Specification: `test/report/teamcity/*.spec.mjs`, run by conformance gate 1 layer 3
//! - Plan: [Wave 1, Step 12](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-12-reporters-1d)
//! - Requirements: [FR-OUT-01](../../../docs/prd.md#fr-out-01), [FR-CORE-07](../../../docs/prd.md#fr-core-07)
//!
//! Upstream draws the `flowId` at random and stamps each message with the time. Here the flow id is
//! derived from the violations, so two runs over the same result agree, and the timestamp is the
//! one the caller passes (the command line passes the time, or `SOURCE_DATE_EPOCH`).

use serde_json::Value;

use crate::azure_devops::violators;
use crate::{Rendered, severity, text};

const CATEGORY: &str = "dependency-cruiser";

/// TeamCity's escaping.
pub fn escape(message: &str) -> String {
    message
        .replace('|', "||")
        .replace('\n', "|n")
        .replace('\r', "|r")
        .replace('[', "|[")
        .replace(']', "|]")
        .replace('\u{85}', "|x")
        .replace('\u{2028}', "|l")
        .replace('\u{2029}', "|p")
        .replace('\'', "|'")
}

/// Ten digits derived from the violations (FNV-1a).
fn flow_id(violations: &[&Value]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for v in violations {
        for byte in v.to_string().bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
    }
    format!("{:010}", hash % 10_000_000_000)
}

fn inspection_type(id: &str, description: &str, flow: &str, timestamp: &str) -> String {
    format!(
        "##teamcity[inspectionType id='{id}' name='{id}' description='{}' category='{CATEGORY}' flowId='{flow}' timestamp='{timestamp}']",
        escape(description)
    )
}

fn inspection(
    type_id: &str,
    message: &str,
    file: Option<&str>,
    severity: &str,
    flow: &str,
    timestamp: &str,
) -> String {
    let file = file
        .filter(|f| !f.is_empty())
        .map(|f| format!(" file='{f}'"))
        .unwrap_or_default();
    format!(
        "##teamcity[inspection typeId='{type_id}' message='{}'{file} SEVERITY='{severity}' flowId='{flow}' timestamp='{timestamp}']",
        escape(message)
    )
}

/// Renders `teamcity`; `timestamp` is ISO 8601 without the trailing `Z`.
pub fn render(result: &Value, timestamp: &str) -> Rendered {
    let summary = result.get("summary").cloned().unwrap_or(Value::Null);
    let violations: Vec<&Value> = summary
        .get("violations")
        .and_then(Value::as_array)
        .map(|v| v.iter().filter(|x| severity(x) != "ignore").collect())
        .unwrap_or_default();
    let ignored = summary.get("ignore").and_then(Value::as_u64).unwrap_or(0);
    let flow = flow_id(&violations);
    let rule_set = summary.get("ruleSetUsed").cloned().unwrap_or(Value::Null);
    let violated = |name: &str| {
        violations
            .iter()
            .any(|v| v.get("rule").map(|r| text(r, "name")).as_deref() == Some(name))
    };
    let mut lines = Vec::new();
    let mut rules = |key: &str| {
        for rule in rule_set
            .get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let name = text(rule, "name");
            if violated(&name) {
                let comment = rule
                    .get("comment")
                    .and_then(Value::as_str)
                    .filter(|c| !c.is_empty())
                    .unwrap_or(&name)
                    .to_owned();
                lines.push(inspection_type(&name, &comment, &flow, timestamp));
            }
        }
    };
    rules("forbidden");
    let allowed = rule_set
        .get("allowed")
        .and_then(Value::as_array)
        .is_some_and(|a| !a.is_empty());
    let mut rest = Vec::new();
    if allowed && violated("not-in-allowed") {
        rest.push(inspection_type(
            "not-in-allowed",
            "dependency is not in the 'allowed' set of rules",
            &flow,
            timestamp,
        ));
    }
    lines.extend(rest);
    let mut required = Vec::new();
    for rule in rule_set
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let name = text(rule, "name");
        if violated(&name) {
            let comment = rule
                .get("comment")
                .and_then(Value::as_str)
                .filter(|c| !c.is_empty())
                .unwrap_or(&name)
                .to_owned();
            required.push(inspection_type(&name, &comment, &flow, timestamp));
        }
    }
    lines.extend(required);
    if ignored > 0 {
        lines.push(inspection_type(
            "ignored-known-violations",
            "some dependency violations were ignored; run with --no-ignore-known to see them",
            &flow,
            timestamp,
        ));
    }
    for v in &violations {
        let sev = match severity(v).as_str() {
            "error" => "ERROR",
            "warn" => "WARNING",
            _ => "INFO",
        };
        let name = v.get("rule").map(|r| text(r, "name")).unwrap_or_default();
        lines.push(inspection(
            &name,
            &violators(v, false),
            v.get("from").and_then(Value::as_str),
            sev,
            &flow,
            timestamp,
        ));
    }
    if ignored > 0 {
        lines.push(inspection(
            "ignored-known-violations",
            &format!("{ignored} known violations ignored. Run with --no-ignore-known to see them."),
            None,
            "WARNING",
            &flow,
            timestamp,
        ));
    }
    let mut output: String = lines.iter().map(|l| format!("{l}\n")).collect();
    if output.is_empty() {
        output.push('\n');
    }
    Rendered {
        output,
        exit_code: summary.get("error").and_then(Value::as_u64).unwrap_or(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const T: &str = "2026-09-22T00:00:00.000";

    #[test]
    fn service_messages() {
        let result = json!({ "summary": {
            "violations": [
                { "type": "dependency", "from": "a", "to": "b", "rule": { "name": "no-x", "severity": "error" } },
                { "type": "dependency", "from": "a", "to": "c", "rule": { "name": "not-in-allowed", "severity": "warn" } },
                { "type": "module", "from": "m", "to": "m", "rule": { "name": "req", "severity": "info" } }
            ],
            "error": 1, "ignore": 1,
            "ruleSetUsed": { "forbidden": [{ "name": "no-x", "comment": "it's [bad]" }, { "name": "unused" }], "allowed": [{}], "required": [{ "name": "req" }] }
        } });
        let out = render(&result, T);
        let lines: Vec<&str> = out.output.lines().collect();
        assert_eq!(lines.len(), 8);
        assert!(lines[0].starts_with("##teamcity[inspectionType id='no-x' name='no-x' description='it|'s |[bad|]' category='dependency-cruiser' flowId='"));
        assert!(lines[1].contains("id='not-in-allowed'"));
        assert!(lines[2].contains("id='req'") && lines[2].contains("description='req'"));
        assert!(lines[3].contains("id='ignored-known-violations'"));
        assert!(lines[4].contains("typeId='no-x' message='a -> b' file='a' SEVERITY='ERROR'"));
        assert!(lines[5].contains("SEVERITY='WARNING'"));
        assert!(lines[6].contains("message='m' file='m' SEVERITY='INFO'"));
        assert!(lines[7].contains("message='1 known violations ignored. Run with --no-ignore-known to see them.' SEVERITY='WARNING'"));
        assert!(lines[7].ends_with(&format!("timestamp='{T}']")));
        assert_eq!(out.exit_code, 1);
        assert_eq!(render(&result, T).output, out.output, "deterministic");
        assert_eq!(
            render(&json!({ "summary": { "violations": [] } }), T).output,
            "\n"
        );
    }

    #[test]
    fn escaping() {
        assert_eq!(
            escape("a|b\nc\rd[e]f\u{85}g\u{2028}h\u{2029}i'j"),
            "a||b|nc|rd|[e|]f|xg|lh|pi|'j"
        );
        assert_eq!(flow_id(&[]).len(), 10);
    }
}
