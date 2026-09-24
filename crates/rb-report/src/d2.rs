//! `d2`: the result in the D2 diagram language, folders as nested containers. dependency-cruiser
//! 18.2.0's `src/report/d2.mjs`, ported.
//!
//! - Specification: `test/report/d2/d2.spec.mjs` (one case per mock), run unmodified by
//!   conformance gate 1 layer 3 ([ADR-0009](../../../docs/adr/0009-conformance-suites-as-specification.md))
//! - Coverage: [coverage § Output types](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#output-types),
//!   row `mermaid`, `d2`
//! - Plan: [Wave 2, Step 10](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#210-step-10-reporters-and-baseline-semantics-2e)
//! - Requirement: [FR-OUT-01](../../../docs/prd.md#fr-out-01)
//!
//! An invalid module or dependency is stroked in the colour of its most severe rule (orange when
//! the severity is not one of `error`, `warn`, `info`), with the rule names as tooltip or label.

use std::fmt::Write as _;

use serde_json::Value;

use crate::utl::url_for_module;
use crate::{Rendered, js};

fn severity_rank(severity: Option<&str>) -> u8 {
    match severity {
        Some("error") => 3,
        Some("warn") => 2,
        Some("info") => 1,
        _ => 0,
    }
}

fn severity_colour(severity: Option<&str>) -> &'static str {
    match severity {
        Some("error") => "red",
        Some("info") => "blue",
        // `warn`, and the fallback for a severity with no colour.
        _ => "orange",
    }
}

/// `getMaxSeverity`: the reduction keeps the later severity unless the earlier ranks higher.
fn max_severity(rules: &[Value]) -> Option<&str> {
    let mut severities = rules
        .iter()
        .map(|r| r.get("severity").and_then(Value::as_str));
    let first = severities.next()?;
    Some(severities.fold(first, |max, current| {
        if severity_rank(max) > severity_rank(current) {
            max
        } else {
            current
        }
    }))
    .flatten()
}

/// `rules.map(rule => rule.name).join("\\n")`: null and undefined names are empty.
fn rule_names(rules: &[Value]) -> String {
    rules
        .iter()
        .map(|r| match r.get("name") {
            None | Some(Value::Null) => String::new(),
            name => js::to_string(name),
        })
        .collect::<Vec<_>>()
        .join("\\n")
}

/// `getVertexName`: each folder and the file name quoted, joined with dots.
fn vertex(source: &str) -> String {
    let folder = js::dirname(source)
        .split('/')
        .map(|p| format!("\"{p}\""))
        .collect::<Vec<_>>()
        .join(".");
    let base = format!("\"{}\"", js::basename(source));
    if folder == "\".\"" {
        base
    } else {
        format!("{folder}.{base}")
    }
}

fn module_attributes(module: &Value, options_used: Option<&Value>) -> String {
    let mut out = String::from("class: module");
    if js::truthy(module.get("consolidated")) {
        out.push_str("; style.multiple: true");
    }
    if ["matchesFocus", "matchesHighlight", "matchesReaches"]
        .iter()
        .any(|k| js::truthy(module.get(*k)))
    {
        out.push_str("; style.fill: yellow");
    }
    if module.get("valid") == Some(&Value::Bool(false)) {
        let rules = rb_rules::js::array(module, "rules");
        let _ = write!(
            out,
            "; style.stroke: {}; tooltip: \"{}\"",
            severity_colour(max_severity(rules)),
            rule_names(rules)
        );
    }
    if js::some_str(module.get("dependencyTypes"), |t| t.contains("npm")) {
        out.push_str("; shape: package");
    }
    let option = |key: &str| {
        options_used
            .and_then(|o| o.get(key))
            .filter(|v| js::truthy(Some(v)))
            .map(|v| js::to_string(Some(v)))
            .unwrap_or_default()
    };
    let _ = write!(
        out,
        "; link: \"{}\"",
        url_for_module(module, &option("prefix"), &option("suffix"))
    );
    out
}

fn dependency_attributes(dependency: &Value) -> String {
    let mut thing = String::new();
    if dependency.get("valid") == Some(&Value::Bool(false)) {
        let rules = rb_rules::js::array(dependency, "rules");
        thing = format!(
            "style: {{stroke: {}}}; label: \"{}\"",
            severity_colour(max_severity(rules)),
            rule_names(rules)
        );
    }
    for (key, shape) in [("circular", "circle"), ("dynamic", "arrow")] {
        if js::truthy(dependency.get(key)) {
            let before = if thing.is_empty() {
                thing.clone()
            } else {
                format!("{thing};")
            };
            thing = format!("{before} target-arrowhead: {{shape: {shape}}}");
        }
    }
    if thing.is_empty() {
        thing
    } else {
        format!(": {{{thing}}}")
    }
}

/// Renders `d2`.
pub fn render(result: &Value) -> Rendered {
    let modules = rb_rules::js::array(result, "modules");
    let options_used = result.get("summary").and_then(|s| s.get("optionsUsed"));
    let vertices: Vec<String> = modules
        .iter()
        .map(|m| {
            format!(
                "{}: {{{}}}",
                vertex(&js::field(m, "source")),
                module_attributes(m, options_used)
            )
        })
        .collect();
    let edges: Vec<String> = modules
        .iter()
        .flat_map(|m| {
            let from = vertex(&js::field(m, "source"));
            rb_rules::js::array(m, "dependencies")
                .iter()
                .map(|d| {
                    format!(
                        "{from} -> {}{}",
                        vertex(&js::field(d, "resolved")),
                        dependency_attributes(d)
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect();
    let styles = "classes: {\n  module: {\n    height: 30;\n    style.border-radius: 10;\n  }\n}";
    Rendered {
        output: format!(
            "# modules\n\n{}\n\n# dependencies\n\n{}\n\n# styling\n\n{styles}\n",
            vertices.join("\n"),
            edges.join("\n")
        ),
        exit_code: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn modules_and_dependencies() {
        let result = json!({
            "modules": [
                { "source": "src/a.js", "valid": false, "rules": [{ "name": "one", "severity": "info" }, { "name": "two", "severity": "error" }, { "severity": "warn" }],
                  "dependencies": [
                    { "resolved": "b.js", "valid": false, "rules": [{ "name": "x", "severity": "loud" }], "circular": true, "dynamic": true },
                    { "resolved": "b.js", "circular": true },
                    { "resolved": "b.js" }
                  ] },
                { "source": "node_modules/pk/i.js", "consolidated": true, "matchesReaches": true, "dependencyTypes": ["npm"], "dependencies": [] }
            ],
            "summary": { "optionsUsed": { "prefix": "https://x/" } }
        });
        assert_eq!(
            render(&result).output,
            concat!(
                "# modules\n\n",
                "\"src\".\"a.js\": {class: module; style.stroke: red; tooltip: \"one\\ntwo\\n\"; link: \"https://x/src/a.js\"}\n",
                "\"node_modules\".\"pk\".\"i.js\": {class: module; style.multiple: true; style.fill: yellow; shape: package; link: \"https://www.npmjs.com/package/pk\"}\n\n",
                "# dependencies\n\n",
                "\"src\".\"a.js\" -> \"b.js\": {style: {stroke: orange}; label: \"x\"; target-arrowhead: {shape: circle}; target-arrowhead: {shape: arrow}}\n",
                "\"src\".\"a.js\" -> \"b.js\": { target-arrowhead: {shape: circle}}\n",
                "\"src\".\"a.js\" -> \"b.js\"\n\n",
                "# styling\n\n",
                "classes: {\n  module: {\n    height: 30;\n    style.border-radius: 10;\n  }\n}\n"
            )
        );
        assert_eq!(
            render(&json!({ "modules": [], "summary": {} })).output,
            "# modules\n\n\n\n# dependencies\n\n\n\n# styling\n\nclasses: {\n  module: {\n    height: 30;\n    style.border-radius: 10;\n  }\n}\n"
        );
    }

    #[test]
    fn severities() {
        assert_eq!(max_severity(&[]), None);
        assert_eq!(
            max_severity(&[json!({ "severity": "warn" }), json!({ "severity": "info" })]),
            Some("warn")
        );
        assert_eq!(
            max_severity(&[json!({ "severity": "loud" }), json!({})]),
            None
        );
        assert_eq!(severity_colour(Some("warn")), "orange");
        assert_eq!(severity_colour(Some("info")), "blue");
    }
}
