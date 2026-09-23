//! `csv`: an incidence matrix. dependency-cruiser 18.2.0's `src/report/csv.mjs` and
//! `utl/dependency-to-incidence-transformer.mjs`, ported.
//!
//! - Specification: `test/report/csv/*.spec.mjs`, run by conformance gate 1 layer 3
//! - Plan: [Wave 1, Step 12](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-12-reporters-1d)
//! - Requirement: [FR-OUT-01](../../../docs/prd.md#fr-out-01)

use std::cmp::Ordering;

use serde_json::Value;

use crate::{Rendered, text, truthy};

fn sort_key(module: &Value) -> String {
    format!(
        "{}-{}",
        if truthy(module.get("coreModule")) {
            "1"
        } else {
            "0"
        },
        text(module, "source")
    )
}

fn incidence(module: &Value, column: &str) -> String {
    let found = module
        .get("dependencies")
        .and_then(Value::as_array)
        .and_then(|d| d.iter().find(|x| text(x, "resolved") == column));
    match found {
        None => "false".into(),
        Some(d) if truthy(d.get("valid")) => "true".into(),
        Some(d) => d
            .get("rules")
            .and_then(Value::as_array)
            .and_then(|r| r.first())
            .map(|r| text(r, "severity"))
            .unwrap_or_else(|| "undefined".into()),
    }
}

/// Renders `csv`.
pub fn render(result: &Value) -> Rendered {
    let mut modules = result
        .get("modules")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    modules.sort_by(|a, b| {
        let (ka, kb) = (sort_key(a), sort_key(b));
        if ka > kb {
            Ordering::Greater
        } else if ka == kb {
            Ordering::Equal
        } else {
            Ordering::Less
        }
    });
    let sources: Vec<String> = modules.iter().map(|m| text(m, "source")).collect();
    let header: Vec<String> = sources.iter().map(|s| format!("\"{s}\"")).collect();
    let rows: Vec<String> = modules
        .iter()
        .map(|m| {
            let cells: Vec<String> = sources
                .iter()
                .map(|c| format!("\"{}\"", incidence(m, c)))
                .collect();
            format!("\"{}\",{},\"\"", text(m, "source"), cells.join(","))
        })
        .collect();
    Rendered {
        output: format!("\"\",{},\"\"\n{}\n", header.join(","), rows.join("\n")),
        exit_code: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn incidence_matrix() {
        let result = json!({ "modules": [
            { "source": "fs", "coreModule": true, "dependencies": [] },
            { "source": "b", "dependencies": [{ "resolved": "a", "valid": false, "rules": [{ "severity": "error" }] }] },
            { "source": "a", "dependencies": [{ "resolved": "b", "valid": true }, { "resolved": "fs", "valid": false }] }
        ] });
        assert_eq!(
            render(&result).output,
            "\"\",\"a\",\"b\",\"fs\",\"\"\n\"a\",\"false\",\"true\",\"undefined\",\"\"\n\"b\",\"error\",\"false\",\"false\",\"\"\n\"fs\",\"false\",\"false\",\"false\",\"\"\n"
        );
        assert_eq!(render(&json!({ "modules": [] })).output, "\"\",,\"\"\n\n");
    }
}
