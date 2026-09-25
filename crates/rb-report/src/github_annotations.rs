//! `github-annotations`: one GitHub workflow command per violation, so findings appear inline on
//! the pull request.
//!
//! - Contract: [Wave 1 plan § 1.5](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#15-interfaces-and-contracts-frozen-by-this-wave)
//!   (the line format is the plan's decision, fixed by a snapshot)
//! - Source: [design § Reporters](../../../docs/artifacts/design.md#reporters)
//! - Requirement: [FR-OUT-02](../../../docs/prd.md#fr-out-02)
//!
//! `::error file=<from>,line=<line>,col=<column>,title=<rule>::<from> -> <to>: <comment> Fix: <fix>`,
//! with `warning` for `warn` and `notice` for `info`; `ignore` is not printed. `line` and `col`
//! come from the edge; a module violation has none. `: <comment>` and ` Fix: <fix>` appear only
//! when the rule has them. Property values and messages are escaped as the workflow command
//! syntax requires.

use serde_json::Value;
use std::fmt::Write as _;

use crate::{Rendered, edge_position, find_rule, severity, text};

/// Escapes a message.
pub fn escape_data(text: &str) -> String {
    text.replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

/// Escapes a property value.
pub fn escape_property(text: &str) -> String {
    escape_data(text).replace(':', "%3A").replace(',', "%2C")
}

/// Renders `github-annotations`, each `file=` behind `path_prefix` (the run's folder from the
/// repository root, empty at the root).
pub fn render(result: &Value, path_prefix: &str) -> Rendered {
    let summary = result.get("summary").cloned().unwrap_or(Value::Null);
    let rule_set = summary.get("ruleSetUsed");
    let mut output = String::new();
    for v in summary
        .get("violations")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let command = match severity(v).as_str() {
            "error" => "error",
            "warn" => "warning",
            "info" => "notice",
            _ => continue,
        };
        let from = text(v, "from");
        let to = text(v, "to");
        let name = v.get("rule").map(|r| text(r, "name")).unwrap_or_default();
        let rule = find_rule(rule_set, &name);
        let mut properties = vec![format!(
            "file={}",
            escape_property(&format!("{path_prefix}{from}"))
        )];
        if let Some((line, column)) = edge_position(result, &from, &to) {
            properties.push(format!("line={line}"));
            properties.push(format!("col={column}"));
        }
        properties.push(format!("title={}", escape_property(&name)));
        let mut message = if v.get("type").and_then(Value::as_str) == Some("module") {
            from.clone()
        } else {
            format!("{from} -> {to}")
        };
        if let Some(comment) = rule
            .and_then(|r| r.get("comment"))
            .and_then(Value::as_str)
            .filter(|c| !c.is_empty())
        {
            message.push_str(": ");
            message.push_str(comment);
        }
        let fix = v
            .get("fix")
            .and_then(Value::as_str)
            .or_else(|| rule.and_then(|r| r.get("fix")).and_then(Value::as_str));
        if let Some(fix) = fix.filter(|f| !f.is_empty()) {
            message.push_str(" Fix: ");
            message.push_str(fix);
        }
        let _ = writeln!(
            output,
            "::{command} {}::{}",
            properties.join(","),
            escape_data(&message)
        );
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

    #[test]
    fn snapshot() {
        let result = json!({
            "modules": [{ "source": "apps/web/x.ts", "dependencies": [{ "resolved": "apps/api/y.ts", "line": 3, "column": 1 }] }],
            "summary": {
                "error": 1,
                "violations": [
                    { "type": "dependency", "from": "apps/web/x.ts", "to": "apps/api/y.ts", "rule": { "name": "no-cross-app", "severity": "error" }, "fix": "Call the API." },
                    { "type": "module", "from": "lonely.ts", "to": "lonely.ts", "rule": { "name": "no-orphans", "severity": "warn" } },
                    { "type": "dependency", "from": "a,b.ts", "to": "c", "rule": { "name": "info-rule", "severity": "info" } },
                    { "type": "dependency", "from": "q", "to": "r", "rule": { "name": "quiet", "severity": "ignore" } }
                ],
                "ruleSetUsed": { "forbidden": [{ "name": "no-cross-app", "comment": "Apps share packages. adr:0003" }] }
            }
        });
        let out = render(&result, "");
        assert_eq!(
            out.output,
            "::error file=apps/web/x.ts,line=3,col=1,title=no-cross-app::apps/web/x.ts -> apps/api/y.ts: Apps share packages. adr:0003 Fix: Call the API.\n\
             ::warning file=lonely.ts,title=no-orphans::lonely.ts\n\
             ::notice file=a%2Cb.ts,title=info-rule::a,b.ts -> c\n"
        );
        assert_eq!(out.exit_code, 1);
        // Run below the repository root, each file is placed by its path from the root; the
        // message keeps the paths the rules speak in.
        let nested = render(&result, "web/");
        assert!(nested.output.starts_with(
            "::error file=web/apps/web/x.ts,line=3,col=1,title=no-cross-app::apps/web/x.ts -> apps/api/y.ts"
        ));
        assert!(nested.output.contains("::warning file=web/lonely.ts,"));
        assert_eq!(escape_data("50%\n"), "50%25%0A");
        assert_eq!(escape_property("a:b\r"), "a%3Ab%0D");
    }
}
