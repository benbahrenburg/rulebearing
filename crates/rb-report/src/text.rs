//! `text`: one line per dependency, `from → to`. dependency-cruiser 18.2.0's
//! `src/report/text.mjs`, ported.
//!
//! - Specification: `test/report/text/*.spec.mjs`, run by conformance gate 1 layer 3
//! - Coverage: [coverage § Options](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#options),
//!   `reporterOptions.text.highlightFocused`
//! - Plan: [Wave 1, Step 12](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-12-reporters-1d)
//! - Requirement: [FR-OUT-01](../../../docs/prd.md#fr-out-01)

use std::collections::HashSet;
use std::fmt::Write as _;

use serde_json::Value;

use crate::style::{Style, styled};
use crate::{Rendered, text, truthy};

/// Renders `text`; `highlight_focused` underlines focused and reached modules.
pub fn render(result: &Value, highlight_focused: bool, color: bool) -> Rendered {
    let modules = result
        .get("modules")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let in_focus: HashSet<String> = modules
        .iter()
        .filter(|m| truthy(m.get("matchesFocus")) || truthy(m.get("matchesReaches")))
        .map(|m| text(m, "source"))
        .collect();
    let show = |name: &str, highlight: bool| {
        if highlight {
            styled(Some(Style::Underline), name, color)
        } else {
            name.to_owned()
        }
    };
    let mut output = String::new();
    for module in &modules {
        let source = text(module, "source");
        let from_highlight = highlight_focused && truthy(module.get("matchesFocus"));
        for dependency in module
            .get("dependencies")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let resolved = text(dependency, "resolved");
            let to_highlight = highlight_focused && in_focus.contains(&resolved);
            let _ = writeln!(
                output,
                "{} → {}",
                show(&source, from_highlight),
                show(&resolved, to_highlight)
            );
        }
    }
    if output.is_empty() {
        output.push('\n');
    }
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
    fn flat_dependencies_with_optional_highlight() {
        let result = json!({ "modules": [
            { "source": "a", "matchesFocus": true, "dependencies": [{ "resolved": "b" }] },
            { "source": "b", "matchesReaches": true, "dependencies": [] }
        ] });
        assert_eq!(render(&result, false, true).output, "a → b\n");
        assert_eq!(
            render(&result, true, true).output,
            "\u{1b}[4ma\u{1b}[24m → \u{1b}[4mb\u{1b}[24m\n"
        );
        assert_eq!(render(&json!({ "modules": [] }), false, false).output, "\n");
    }
}
