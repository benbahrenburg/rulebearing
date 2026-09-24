//! `reportWrap`'s re-summary: the filters and `collapse` of a report, applied to a result, with
//! the summary recomputed. dependency-cruiser 18.2.0's `src/main/report-wrap.mjs`, ported.
//!
//! - Plan: [Wave 1, Step 13](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-13-rb-cli-cruise-fmt-exit-codes-flags-1d)
//!   (`fmt` re-reports a saved result without extracting)
//! - Source: [design § The subcommands a guard reaches for](../../../docs/artifacts/design.md#the-subcommands-a-guard-reaches-for)
//! - Requirement: [FR-CORE-02](../../../docs/prd.md#fr-core-02)
//!
//! `cruise` passes its rule set; `fmt` has none, and neither has dependency-cruiser's
//! `depcruise-fmt`, so a re-summarised violation there is typed from the annotated graph alone.
//! The Rulebearing additions a saved violation carries (`id`, `fix`, `decision`) are kept by
//! matching each recomputed violation to the saved one with the same rule, `from` and `to`.

use std::cmp::Ordering;

use rb_config::model::DependencyRules;
use rb_model::{Folder, GraphDocument, Module};
use serde_json::{Map, Value};

use crate::compare::{compare_modules, compare_violations};
use crate::evaluate::EngineError;
use crate::graph::consolidate::consolidate_to_pattern;
use crate::graph::filters::{Filters, apply};
use crate::js;
use crate::summarize::{options_used, summarize_folders, summarize_modules, violation_stats};

/// The format options a report applies.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FormatOptions {
    /// `exclude`, `includeOnly`, `focus`, `reaches`, `highlight`.
    pub filters: Filters,
    /// `collapse`: a pattern (a folder depth is turned into one by the caller).
    pub collapse: Option<String>,
    /// Options to merge into `optionsUsed` (for example `outputType`).
    pub options: Map<String, Value>,
}

/// `normalizeCollapse`: a depth `n` becomes `node_modules/[^/]+|^([^/]+/){n}`.
pub fn collapse_pattern(value: &Value) -> Option<String> {
    let depth = match value {
        Value::Number(n) => n.as_u64(),
        Value::String(s) if s.len() == 1 && s.as_bytes()[0].is_ascii_digit() => s.parse().ok(),
        Value::String(s) => return Some(s.clone()),
        _ => None,
    }?;
    let repeat = usize::try_from(depth).unwrap_or(0);
    Some(format!("node_modules/[^/]+|^{}", "[^/]+/".repeat(repeat)))
}

fn strip_self_transitions(module: &mut Value) {
    let source = js::text(module, "source").into_owned();
    if let Some(Value::Array(dependencies)) = module.get_mut("dependencies") {
        dependencies.retain(|d| js::text(d, "resolved") != source);
    }
}

/// Carries `id`, `fix` and `decision` from the saved violations onto the recomputed ones.
fn carry_additions(violations: &mut [Value], saved: &[Value]) {
    for violation in violations {
        let key = |v: &Value| {
            (
                v.get("rule").map(|r| js::text(r, "name").into_owned()),
                js::text(v, "from").into_owned(),
                js::text(v, "to").into_owned(),
            )
        };
        let wanted = key(violation);
        if let Some(old) = saved.iter().find(|s| key(s) == wanted) {
            for field in ["id", "fix", "decision"] {
                if let Some(value) = old.get(field) {
                    js::set(violation, field, value.clone());
                }
            }
        }
    }
}

/// `reSummarizeResults`.
///
/// # Errors
/// [`EngineError::Document`] when the result does not convert back into document types.
pub fn rewrap(
    document: GraphDocument,
    format: &FormatOptions,
    rules: Option<&DependencyRules>,
) -> Result<GraphDocument, EngineError> {
    let GraphDocument {
        modules,
        folders,
        summary,
        revision_data,
        code,
    } = document;
    let mut modules: Vec<Value> = modules
        .iter()
        .map(serde_json::to_value)
        .collect::<Result<_, _>>()?;
    modules = apply(modules, &format.filters);
    if let Some(pattern) = &format.collapse {
        modules = consolidate_to_pattern(&modules, pattern);
        modules.sort_by(|a, b| {
            let order = compare_modules(a, b);
            // Upstream's comparator never answers 0; a stable sort keeps equal sources in order.
            if js::text(a, "source") == js::text(b, "source") {
                Ordering::Equal
            } else {
                order
            }
        });
        for module in &mut modules {
            strip_self_transitions(module);
        }
    }
    let folder_values: Vec<Value> = folders
        .iter()
        .flatten()
        .map(serde_json::to_value)
        .collect::<Result<_, _>>()?;
    let saved: Vec<Value> = summary
        .violations
        .iter()
        .map(serde_json::to_value)
        .collect::<Result<_, _>>()?;
    let mut violations = summarize_modules(&modules, rules);
    violations.extend(summarize_folders(&folder_values, rules));
    violations.sort_by(compare_violations);
    carry_additions(&mut violations, &saved);
    // Element and slice violations belong to no module, so no module annotation can recompute
    // them: they are kept as the engine found them.
    violations.extend(
        saved
            .iter()
            .filter(|v| {
                matches!(
                    v.get("type").and_then(Value::as_str),
                    Some("element" | "slice")
                )
            })
            .cloned(),
    );
    let stats = violation_stats(&violations);
    let count = |k: &str| stats.get(k).and_then(Value::as_u64).unwrap_or(0);
    let mut merged = summary.options_used.clone();
    for (k, v) in &format.options {
        merged.insert(k.clone(), v.clone());
    }
    let args: Vec<String> = summary
        .options_used
        .get("args")
        .and_then(Value::as_str)
        .map(|a| a.split(' ').map(str::to_owned).collect())
        .unwrap_or_default();
    let mut summary = summary;
    summary.violations = violations
        .into_iter()
        .map(serde_json::from_value)
        .collect::<Result<_, _>>()?;
    summary.error = count("error");
    summary.warn = count("warn");
    summary.info = count("info");
    summary.ignore = Some(count("ignore"));
    summary.total_cruised = modules.len() as u64;
    summary.total_dependencies_cruised = Some(
        modules
            .iter()
            .map(|m| js::array(m, "dependencies").len() as u64)
            .sum(),
    );
    summary.options_used = options_used(&merged, &args);
    if let Some(rules) = rules {
        let used = crate::summarize::rule_set_used(rules);
        if !used.is_empty() {
            summary.rule_set_used = Some(used);
        }
    }
    let modules: Vec<Module> = modules
        .into_iter()
        .map(serde_json::from_value)
        .collect::<Result<_, _>>()?;
    let folders: Option<Vec<Folder>> = folders;
    Ok(GraphDocument {
        modules,
        folders,
        summary,
        revision_data,
        code,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::filters::Filter;
    use rb_model::{Dependency, ModuleSystem, RuleSummary, Severity, Violation, ViolationType};
    use serde_json::json;

    fn document() -> GraphDocument {
        let rule = RuleSummary {
            name: "r".into(),
            severity: Severity::Error,
        };
        let dependency = |to: &str| Dependency {
            valid: false,
            rules: Some(vec![rule.clone()]),
            ..Dependency::new(to, to, ModuleSystem::Es6)
        };
        let mut doc = GraphDocument {
            modules: vec![
                Module {
                    dependencies: vec![dependency("src/b/y.ts")],
                    ..Module::new("src/a/x.ts")
                },
                Module {
                    dependencies: vec![dependency("src/a/x.ts")],
                    ..Module::new("src/b/y.ts")
                },
                Module::new("lib/z.ts"),
            ],
            ..GraphDocument::default()
        };
        doc.summary
            .options_used
            .insert("args".into(), json!("src lib"));
        doc.summary.violations.push(Violation {
            from: "src/a/x.ts".into(),
            to: "src/b/y.ts".into(),
            unresolved_to: None,
            dependency_types: None,
            violation_type: Some(ViolationType::Dependency),
            rule,
            cycle: None,
            via: None,
            metrics: None,
            comment: None,
            id: Some("RB-12345678".into()),
            fix: Some("move it".into()),
            decision: None,
        });
        doc
    }

    #[test]
    fn filters_resummarise_and_keep_additions() -> Result<(), EngineError> {
        let format = FormatOptions {
            filters: Filters {
                include_only: Some(Filter {
                    path: Some("^src/a".into()),
                    depth: None,
                }),
                ..Filters::default()
            },
            options: json!({ "outputType": "err" })
                .as_object()
                .cloned()
                .unwrap_or_default(),
            ..FormatOptions::default()
        };
        let out = rewrap(document(), &format, None)?;
        assert_eq!(out.modules.len(), 1);
        assert_eq!(
            out.summary.violations.len(),
            0,
            "the edge to src/b left with includeOnly"
        );
        assert_eq!(out.summary.options_used["outputType"], "err");
        assert_eq!(out.summary.options_used["args"], "src lib");
        let all = rewrap(document(), &FormatOptions::default(), None)?;
        assert_eq!(all.summary.error, 2);
        assert_eq!(all.summary.violations[0].id.as_deref(), Some("RB-12345678"));
        assert_eq!(all.summary.violations[0].fix.as_deref(), Some("move it"));
        assert_eq!(all.summary.total_dependencies_cruised, Some(2));
        Ok(())
    }

    #[test]
    fn collapse_folds_and_strips_self_edges() -> Result<(), EngineError> {
        let format = FormatOptions {
            collapse: Some("^src".into()),
            ..FormatOptions::default()
        };
        let out = rewrap(document(), &format, None)?;
        let sources: Vec<&str> = out.modules.iter().map(|m| m.source.as_str()).collect();
        assert_eq!(sources, ["lib/z.ts", "src"]);
        assert!(out.modules[1].dependencies.is_empty());
        Ok(())
    }

    #[test]
    fn the_rule_set_is_recorded_only_when_it_has_rules() -> Result<(), EngineError> {
        let empty = DependencyRules::default();
        let out = rewrap(document(), &FormatOptions::default(), Some(&empty))?;
        assert_eq!(out.summary.rule_set_used, None);
        let rules = rb_config::normalize::rule_set(
            json!({ "forbidden": [{ "name": "r", "from": {}, "to": {} }] })
                .as_object()
                .unwrap_or(&Map::new()),
        )
        .unwrap_or_default();
        let out = rewrap(document(), &FormatOptions::default(), Some(&rules))?;
        assert!(
            out.summary
                .rule_set_used
                .is_some_and(|used| used.contains_key("forbidden"))
        );
        Ok(())
    }

    #[test]
    fn collapse_depths_become_patterns() {
        assert_eq!(
            collapse_pattern(&json!(2)).as_deref(),
            Some("node_modules/[^/]+|^[^/]+/[^/]+/")
        );
        assert_eq!(
            collapse_pattern(&json!("1")).as_deref(),
            Some("node_modules/[^/]+|^[^/]+/")
        );
        assert_eq!(collapse_pattern(&json!("^src")).as_deref(), Some("^src"));
        // Only a single digit (`/^\d$/`) is a depth; any other string is a pattern.
        assert_eq!(collapse_pattern(&json!("12")).as_deref(), Some("12"));
        assert_eq!(collapse_pattern(&json!("a")).as_deref(), Some("a"));
        assert_eq!(collapse_pattern(&json!(true)), None);
    }
}
