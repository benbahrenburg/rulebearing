//! `json`: the result document, and `--strict-schema`, which strips every Rulebearing addition so
//! the output validates against dependency-cruiser 18.2.0's `cruise-result` schema.
//!
//! - Decision: [ADR-0004](../../../docs/adr/0004-graph-document-is-cruise-result-superset.md)
//!   (the additions are additive; `--strict-schema` strips them; layer 4 validates)
//! - Plan: [Wave 1, Step 12](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-12-reporters-1d)
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
];
/// Violation keys Rulebearing adds.
pub const VIOLATION_ADDITIONS: &[&str] = &["id", "fix", "decision"];
/// Summary keys Rulebearing adds.
pub const SUMMARY_ADDITIONS: &[&str] = &["inspected", "vacuousRules"];
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

/// Removes every Rulebearing addition.
pub fn strip(result: &mut Value) {
    remove(result, &["code"]);
    each(result, "modules", &mut |module| {
        remove(module, MODULE_ADDITIONS);
        each(module, "dependencies", &mut |d| {
            remove(d, DEPENDENCY_ADDITIONS)
        });
    });
    if let Some(summary) = result.get_mut("summary") {
        remove(summary, SUMMARY_ADDITIONS);
        each(summary, "violations", &mut |v| {
            remove(v, VIOLATION_ADDITIONS)
        });
        if let Some(rules) = summary.get_mut("ruleSetUsed") {
            for list in ["forbidden", "allowed", "required"] {
                each(rules, list, &mut |r| remove(r, RULE_ADDITIONS));
            }
        }
    }
}

/// Renders `json`: two-space indentation and a trailing newline, as `JSON.stringify(r, null, "  ")`.
pub fn render(result: &Value, strict_schema: bool) -> Rendered {
    let mut result = result.clone();
    if strict_schema {
        strip(&mut result);
    }
    let mut output = serde_json::to_string_pretty(&result).unwrap_or_default();
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
    fn strict_schema_strips_every_addition() {
        let result = json!({
            "modules": [{ "source": "a", "language": "typescript", "valid": true, "dependencies": [{ "resolved": "b", "line": 1, "column": 2, "dependencyKind": "import" }] }],
            "summary": { "violations": [{ "from": "a", "to": "b", "id": "RB-1", "fix": "f", "decision": "adr:1" }], "inspected": {}, "vacuousRules": [],
                         "ruleSetUsed": { "forbidden": [{ "name": "r", "fix": "f", "allowEmpty": true }] } },
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
            "allowEmpty",
            "\"code\"",
        ] {
            assert!(!stripped.contains(gone), "{gone}");
        }
        let kept = render(&result, false).output;
        assert!(kept.contains("\"line\": 1") && kept.ends_with("}\n"));
        assert!(kept.contains("\n  \"modules\""), "two-space indentation");
    }
}
