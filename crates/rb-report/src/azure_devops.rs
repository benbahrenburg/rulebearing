//! `azure-devops`: Azure Pipelines logging commands. dependency-cruiser 18.2.0's
//! `src/report/azure-devops.mjs`, ported.
//!
//! - Specification: `test/report/azure-devops/*.spec.mjs`, run by conformance gate 1 layer 3
//! - Plan: [Wave 1, Step 12](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-12-reporters-1d)
//! - Requirement: [FR-OUT-01](../../../docs/prd.md#fr-out-01)

use serde_json::Value;

use crate::style::percentage;
use crate::{Rendered, js_number, num, severity, text};

fn names(steps: Option<&Value>) -> String {
    steps
        .and_then(Value::as_array)
        .map(|s| {
            s.iter()
                .map(|x| text(x, "name"))
                .collect::<Vec<_>>()
                .join(" -> ")
        })
        .unwrap_or_default()
}

/// The violation text shared by the `azure-devops` and `teamcity` reporters, which differ only in
/// how a reachability violation prints its path.
pub fn violators(v: &Value, reachability_with_via: bool) -> String {
    let dependency = format!("{} -> {}", text(v, "from"), text(v, "to"));
    match v.get("type").and_then(Value::as_str) {
        Some("module") => text(v, "from"),
        Some("cycle") => format!("{} -> {}", text(v, "from"), names(v.get("cycle"))),
        Some("reachability") if reachability_with_via => {
            format!("{dependency} (via {})", names(v.get("via")))
        }
        Some("reachability") => format!("{dependency} {}", names(v.get("via"))),
        Some("instability") => {
            let metric = |side: &str| {
                percentage(num(
                    v.get("metrics").and_then(|m| m.get(side)),
                    "instability",
                ))
            };
            format!(
                "{dependency} (instability: {} -> {})",
                metric("from"),
                metric("to")
            )
        }
        _ => dependency,
    }
}

/// Renders `azure-devops`.
pub fn render(result: &Value) -> Rendered {
    let summary = result.get("summary").cloned().unwrap_or(Value::Null);
    let mut output = String::new();
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
        let kind = match sev.as_str() {
            "error" => "error",
            _ => "warning",
        };
        output.push_str(&format!(
            "##vso[task.logissue type={kind};sourcepath={};code={};]{}\n",
            text(v, "from"),
            v.get("rule").map(|r| text(r, "name")).unwrap_or_default(),
            violators(v, true)
        ));
    }
    let n = |k: &str| summary.get(k).and_then(Value::as_u64).unwrap_or(0);
    let total = n("error") + n("warn") + n("info");
    let stats = format!(
        "{} modules, {} dependencies cruised",
        js_number(summary.get("totalCruised")),
        summary
            .get("totalDependenciesCruised")
            .map_or_else(|| "0".into(), |v| js_number(Some(v)))
    );
    let ignored = n("ignore");
    let message = if total > 0 {
        let ignore = if ignored > 0 {
            format!(", {ignored} ignored")
        } else {
            String::new()
        };
        format!(
            "{total} dependency violations ({} error, {} warning/ informational{ignore}). {stats}",
            n("error"),
            n("warn") + n("info")
        )
    } else {
        let ignore = if ignored > 0 {
            format!(" - {ignored} violations ignored ")
        } else {
            String::new()
        };
        format!("no dependency violations found{ignore} ({stats})")
    };
    let status = if n("error") > 0 {
        "Failed"
    } else {
        "Succeeded"
    };
    output.push_str(&format!("##vso[task.complete result={status};]{message}\n"));
    Rendered {
        output,
        exit_code: n("error"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn logging_commands_and_summary() {
        let result = json!({ "summary": { "violations": [
            { "type": "dependency", "from": "a", "to": "b", "rule": { "name": "d", "severity": "error" } },
            { "type": "reachability", "from": "a", "to": "c", "via": [{ "name": "b" }, { "name": "c" }], "rule": { "name": "r", "severity": "info" } },
            { "type": "cycle", "from": "a", "to": "b", "cycle": [{ "name": "b" }, { "name": "a" }], "rule": { "name": "c", "severity": "warn" } },
            { "type": "instability", "from": "a", "to": "b", "metrics": { "from": { "instability": 0.1 }, "to": { "instability": 0.9 } }, "rule": { "name": "s", "severity": "warn" } },
            { "type": "module", "from": "m", "to": "m", "rule": { "name": "o", "severity": "ignore" } }
        ], "error": 1, "warn": 2, "info": 1, "ignore": 1, "totalCruised": 3, "totalDependenciesCruised": 2 } });
        let out = render(&result);
        assert_eq!(out.exit_code, 1);
        assert!(
            out.output
                .contains("##vso[task.logissue type=error;sourcepath=a;code=d;]a -> b\n")
        );
        assert!(
            out.output
                .contains("type=warning;sourcepath=a;code=r;]a -> c (via b -> c)\n")
        );
        assert!(out.output.contains("]a -> b -> a\n"));
        assert!(out.output.contains("(instability: 10% -> 90%)"));
        assert!(!out.output.contains("code=o"));
        assert!(out.output.ends_with("##vso[task.complete result=Failed;]4 dependency violations (1 error, 3 warning/ informational, 1 ignored). 3 modules, 2 dependencies cruised\n"));
        let clean = render(
            &json!({ "summary": { "violations": [], "error": 0, "warn": 0, "info": 0, "ignore": 2, "totalCruised": 1 } }),
        );
        assert_eq!(
            clean.output,
            "##vso[task.complete result=Succeeded;]no dependency violations found - 2 violations ignored  (1 modules, 0 dependencies cruised)\n"
        );
        assert_eq!(
            violators(&json!({ "type": "module", "from": "m" }), false),
            "m"
        );
        assert_eq!(
            violators(
                &json!({ "type": "reachability", "from": "a", "to": "b", "via": [{ "name": "b" }] }),
                false
            ),
            "a -> b b"
        );
    }
}
