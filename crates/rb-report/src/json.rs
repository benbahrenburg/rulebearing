//! `json`: the result document, and `--strict-schema`, which strips every Rulebearing addition so
//! the output validates against dependency-cruiser 18.2.0's `cruise-result` schema.
//!
//! - Decision: [ADR-0004](../../../docs/adr/0004-graph-document-is-cruise-result-superset.md)
//!   (the additions are additive; `--strict-schema` strips them; layer 4 validates)
//! - Plan: [Wave 1, Step 12](../../../docs/plans/implemented/0001-wave-1-typescript-parity.md#step-12-reporters-1d)
//! - Requirements: [FR-OUT-01](../../../docs/prd.md#fr-out-01), [FR-CORE-03](../../../docs/prd.md#fr-core-03)

use serde_json::Value;

use crate::Rendered;

/// Module keys Rulebearing adds.
pub const MODULE_ADDITIONS: &[&str] = &["language", "project", "namespaces", "attribution"];
/// Dependency keys Rulebearing adds.
pub const DEPENDENCY_ADDITIONS: &[&str] = &[
    "line",
    "column",
    "dependencyKind",
    "member",
    "sidecar",
    "declared",
    "approximate",
];
/// Violation keys Rulebearing adds.
pub const VIOLATION_ADDITIONS: &[&str] = &["id", "fix", "decision"];
/// Summary keys Rulebearing adds.
pub const SUMMARY_ADDITIONS: &[&str] = &[
    "inspected",
    "vacuousRules",
    "ratchets",
    "expired",
    "affected",
    "cache",
    "sidecar",
    "plugins",
];
/// Rule-set keys Rulebearing adds (inside `summary.ruleSetUsed`): the element, slice and diagram
/// rules.
pub const RULE_SET_ADDITIONS: &[&str] = &["elements", "slices", "diagrams"];
/// Rule keys Rulebearing adds (inside `summary.ruleSetUsed`).
pub const RULE_ADDITIONS: &[&str] = &["fix", "examples", "owner", "expires", "allowEmpty"];

fn remove(value: &mut Value, keys: &[&str]) {
    if let Value::Object(map) = value {
        for key in keys {
            map.remove(*key);
        }
    }
}

fn each(value: &mut Value, key: &str, f: &mut dyn FnMut(&mut Value)) {
    if let Some(Value::Array(items)) = value.get_mut(key) {
        items.iter_mut().for_each(f);
    }
}

/// The names of the project-scoped rules in `ruleSetUsed`, which only a native configuration has
/// ([ADR-0066](../../../docs/adr/0066-project-scope-and-the-project-layer.md)).
fn project_rules(result: &Value) -> Vec<String> {
    let Some(rules) = result.get("summary").and_then(|s| s.get("ruleSetUsed")) else {
        return Vec::new();
    };
    ["forbidden", "allowed", "required"]
        .iter()
        .filter_map(|list| rules.get(*list).and_then(Value::as_array))
        .flatten()
        .filter(|r| r.get("scope").and_then(Value::as_str) == Some("project"))
        .filter_map(|r| r.get("name").and_then(Value::as_str).map(str::to_owned))
        .collect()
}

/// Removes every Rulebearing addition: the additive keys, the project layer, and the
/// project-scoped rules with the violations they found, since upstream's `scope` is `module` or
/// `folder`.
pub fn strip(result: &mut Value) {
    let project_rules = project_rules(result);
    remove(result, &["code", "projects"]);
    each(result, "modules", &mut |module| {
        remove(module, MODULE_ADDITIONS);
        each(module, "dependencies", &mut |d| {
            remove(d, DEPENDENCY_ADDITIONS);
        });
    });
    if let Some(summary) = result.get_mut("summary") {
        remove(summary, SUMMARY_ADDITIONS);
        if let Some(Value::Array(violations)) = summary.get_mut("violations") {
            violations.retain(|v| {
                let rule = v
                    .get("rule")
                    .and_then(|r| r.get("name"))
                    .and_then(Value::as_str);
                v.get("type").and_then(Value::as_str) != Some("project")
                    && rule.is_none_or(|name| !project_rules.iter().any(|p| p == name))
            });
        }
        each(summary, "violations", &mut |v| {
            remove(v, VIOLATION_ADDITIONS);
        });
        if let Some(rules) = summary.get_mut("ruleSetUsed") {
            remove(rules, RULE_SET_ADDITIONS);
            for list in ["forbidden", "allowed", "required"] {
                if let Some(Value::Array(items)) = rules.get_mut(list) {
                    items.retain(|r| r.get("scope").and_then(Value::as_str) != Some("project"));
                }
                each(rules, list, &mut |r| remove(r, RULE_ADDITIONS));
            }
        }
    }
}

/// Renders `json`: two-space indentation and a trailing newline, as `JSON.stringify(r, null, "  ")`.
pub fn render(result: &Value, strict_schema: bool) -> Rendered {
    // Copied only to strip it: a large graph's result is gigabytes as a value.
    let mut output = if strict_schema {
        let mut stripped = result.clone();
        strip(&mut stripped);
        serde_json::to_string_pretty(&stripped)
    } else {
        serde_json::to_string_pretty(result)
    }
    .unwrap_or_default();
    output.push('\n');
    Rendered {
        output,
        exit_code: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn strict_schema_strips_the_project_layer_and_its_rules() {
        let result = json!({
            "modules": [],
            "projects": [{ "name": "CoreProject", "moduleCount": 1 }],
            "summary": {
                "violations": [
                    { "type": "project", "from": "Web", "to": "Core", "rule": { "name": "projectRule" } },
                    { "type": "cycle", "from": "Core", "to": "Web", "rule": { "name": "projectCycles" } },
                    { "type": "folder", "from": "src/a", "to": "src/b", "rule": { "name": "keptFolder" } }
                ],
                "ruleSetUsed": { "forbidden": [
                    { "name": "projectRule", "scope": "project" },
                    { "name": "projectCycles", "scope": "project" },
                    { "name": "keptFolder", "scope": "folder" }
                ] }
            }
        });
        let stripped = render(&result, true).output;
        for gone in ["CoreProject", "projectRule", "projectCycles", "\"project\""] {
            assert!(!stripped.contains(gone), "{gone}");
        }
        assert_eq!(stripped.matches("keptFolder").count(), 2, "{stripped}");
        assert!(render(&result, false).output.contains("CoreProject"));
    }

    #[test]
    fn strict_schema_strips_every_addition() {
        let result = json!({
            "modules": [{ "source": "a", "language": "typescript", "valid": true, "dependencies": [{ "resolved": "b", "line": 1, "column": 2, "dependencyKind": "import", "sidecar": true, "approximate": true }] }],
            "summary": { "violations": [{ "from": "a", "to": "b", "id": "RB-1", "fix": "f", "decision": "adr:1" }], "inspected": {}, "vacuousRules": [], "cache": { "hit": true, "strategy": "metadata" }, "sidecar": { "tool": "dependency-cruiser", "version": "18.2.0", "files": 2 }, "affected": { "revision": "main", "changed": ["affectedFile"], "closure": [] }, "plugins": ["pluginFile.mjs"],
                         "ruleSetUsed": { "forbidden": [{ "name": "r", "fix": "f", "allowEmpty": true }],
                                          "elements": [{ "name": "sealedElement" }], "slices": [{ "name": "apartSlice" }],
                                          "diagrams": [{ "name": "drawnDiagram" }] } },
            "code": {}
        });
        let stripped = render(&result, true).output;
        for gone in [
            "language",
            "\"line\"",
            "dependencyKind",
            "RB-1",
            "inspected",
            "vacuousRules",
            "affectedFile",
            "pluginFile",
            "allowEmpty",
            "\"code\"",
            "sealedElement",
            "apartSlice",
            "drawnDiagram",
            "\"hit\"",
            "dependency-cruiser",
            "\"sidecar\"",
            "approximate",
        ] {
            assert!(!stripped.contains(gone), "{gone}");
        }
        let kept = render(&result, false).output;
        assert!(kept.contains("\"line\": 1") && kept.ends_with("}\n"));
        assert!(kept.contains("\n  \"modules\""), "two-space indentation");
    }
}
