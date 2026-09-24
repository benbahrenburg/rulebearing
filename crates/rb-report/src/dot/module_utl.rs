//! The `dot` family's per-module steps and the three preparation levels. dependency-cruiser
//! 18.2.0's `src/report/dot/module-utl.mjs`, `prepare-custom-level.mjs`,
//! `prepare-folder-level.mjs` and `prepare-flat-level.mjs`, ported.
//!
//! - Specification: `test/report/dot/module-utl.spec.mjs` and the level specs under
//!   `test/report/dot/`, run by conformance gate 1 layer 3
//!   ([ADR-0009](../../../../docs/adr/0009-conformance-suites-as-specification.md))
//! - Coverage: [coverage § Output types](../../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#output-types),
//!   row `dot`, `ddot`, `cdot` / `archi`, `fdot` / `flat`
//! - Plan: [Wave 2, Step 10](../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#210-step-10-reporters-and-baseline-semantics-2e)
//! - Requirement: [FR-OUT-01](../../../../docs/prd.md#fr-out-01)
//!
//! Each step takes a module as JSON and returns it with keys added, as upstream's object spreads
//! do, so a theme criterion can match on anything a step added (`label`, `folder`, `tooltip`).
//! A value upstream would set to `undefined` is left out, which every later read treats alike.

use std::cmp::Ordering;

use serde_json::{Map, Value, json};

use crate::dot::theme::apply_theme;
use crate::js;
use crate::style::percentage;
use crate::utl::url_for_module;

fn object(module: &Value) -> Map<String, Value> {
    module.as_object().cloned().unwrap_or_default()
}

fn dependencies(module: &Value) -> Vec<Value> {
    rb_rules::js::array(module, "dependencies").to_vec()
}

/// `extractFirstTransgression`: the first module rule's name as the tooltip, and each
/// dependency's first rule as its `rule`.
pub fn extract_first_transgression(module: &Value) -> Value {
    let mut out = object(module);
    if let Some(first) = js::get(module, "rules.0") {
        match first.get("name") {
            Some(name) => {
                out.insert("tooltip".into(), name.clone());
            }
            None => {
                out.shift_remove("tooltip");
            }
        }
    }
    let dependencies: Vec<Value> = dependencies(module)
        .into_iter()
        .map(|dependency| {
            if !js::truthy(dependency.get("rules")) {
                return dependency;
            }
            let mut out = object(&dependency);
            match js::get(&dependency, "rules.0") {
                Some(rule) => {
                    out.insert("rule".into(), rule);
                }
                None => {
                    out.shift_remove("rule");
                }
            }
            Value::Object(out)
        })
        .collect();
    out.insert("dependencies".into(), Value::Array(dependencies));
    Value::Object(out)
}

/// `makeInstabilityString`: the instability in grey after the label, for a module that has one
/// and is not consolidated, when metrics are shown.
fn instability_string(module: &Value, show_metrics: bool) -> String {
    let has = module
        .as_object()
        .is_some_and(|m| m.contains_key("instability"));
    if show_metrics && has && !js::truthy(module.get("consolidated")) {
        format!(
            " <FONT color=\"#808080\" point-size=\"8\">{}</FONT>",
            percentage(js::to_number(module.get("instability")))
        )
    } else {
        String::new()
    }
}

/// `folderify(showMetrics)`: the folder and its cluster path, the file name as label and
/// tooltip.
pub fn folderify(module: &Value, show_metrics: bool) -> Value {
    let source = js::field(module, "source");
    let directory = js::dirname(&source);
    let base = js::basename(&source);
    let mut out = object(module);
    if directory != "." {
        let parts: Vec<&str> = directory.split('/').collect();
        let path: Vec<Value> = parts
            .iter()
            .enumerate()
            .map(|(i, snippet)| {
                let above: String = parts[..i].iter().flat_map(|p| [*p, "/"]).collect();
                json!({ "snippet": snippet, "aggregateSnippet": format!("{above}{snippet}") })
            })
            .collect();
        out.insert("folder".into(), Value::String(directory));
        out.insert("path".into(), Value::Array(path));
    }
    out.insert(
        "label".into(),
        Value::String(format!(
            "<{base}{}>",
            instability_string(module, show_metrics)
        )),
    );
    out.insert("tooltip".into(), Value::String(base));
    Value::Object(out)
}

/// `flatLabel(showMetrics)`: the folder above the file name in bold.
pub fn flat_label(module: &Value, show_metrics: bool) -> Value {
    let source = js::field(module, "source");
    let base = js::basename(&source);
    let mut out = object(module);
    out.insert(
        "label".into(),
        Value::String(format!(
            "<{}/<BR/><B>{base}</B>{}>",
            js::dirname(&source),
            instability_string(module, show_metrics)
        )),
    );
    out.insert("tooltip".into(), Value::String(base));
    Value::Object(out)
}

/// `addURL(prefix, suffix)`: the module's link, unless it could not be resolved.
pub fn add_url(module: &Value, prefix: &str, suffix: &str) -> Value {
    if js::truthy(module.get("couldNotResolve")) {
        return module.clone();
    }
    let mut out = object(module);
    out.insert(
        "URL".into(),
        Value::String(url_for_module(module, prefix, suffix)),
    );
    Value::Object(out)
}

/// `stripSelfTransitions`: the dependencies on the module itself dropped.
pub fn strip_self_transitions(module: &Value) -> Value {
    let source = module.get("source");
    let mut out = object(module);
    let kept: Vec<Value> = dependencies(module)
        .into_iter()
        .filter(|d| d.get("resolved") != source)
        .collect();
    out.insert("dependencies".into(), Value::Array(kept));
    Value::Object(out)
}

/// `compareModules` as a V8 sort sees it: `a` sorts first unless its source is greater.
fn sort_modules(modules: &mut [Value]) {
    rb_rules::js::sort(modules, |a, b| {
        js::compare_utf16(&js::field(a, "source"), &js::field(b, "source")) != Ordering::Greater
    });
}

/// The three granularities' preparation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// `prepareCustomLevel`, which the module level uses too.
    Custom,
    /// `prepareFolderLevel`.
    Folder,
    /// `prepareFlatLevel`.
    Flat,
}

/// What preparing a level takes besides the modules.
#[derive(Debug, Clone)]
pub struct Prepare<'a> {
    /// The normalised theme.
    pub theme: &'a Value,
    /// The collapse pattern, when one applies (ignored by the folder level, as upstream).
    pub collapse_pattern: Option<&'a str>,
    /// `showMetrics`.
    pub show_metrics: bool,
    /// `summary.optionsUsed.prefix`.
    pub prefix: &'a str,
    /// `summary.optionsUsed.suffix`.
    pub suffix: &'a str,
}

/// Prepares the modules of a result for one granularity, step for step as upstream does.
pub fn prepare(modules: &[Value], level: Level, p: &Prepare<'_>) -> Vec<Value> {
    let mut modules: Vec<Value> = match (level, p.collapse_pattern) {
        (Level::Folder, _) => rb_rules::graph::consolidate::consolidate_to_folder(modules),
        (_, Some(pattern)) => {
            rb_rules::graph::consolidate::consolidate_to_pattern(modules, pattern)
        }
        (_, None) => modules.to_vec(),
    };
    sort_modules(&mut modules);
    modules
        .iter()
        .map(|module| {
            let module = match level {
                Level::Custom => strip_self_transitions(&extract_first_transgression(&folderify(
                    module,
                    p.show_metrics,
                ))),
                Level::Folder => strip_self_transitions(&folderify(
                    &extract_first_transgression(module),
                    p.show_metrics,
                )),
                Level::Flat => extract_first_transgression(&flat_label(module, p.show_metrics)),
            };
            add_url(&apply_theme(p.theme, &module), p.prefix, p.suffix)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_transgressions() {
        assert_eq!(
            extract_first_transgression(&json!({ "dependencies": [] })),
            json!({ "dependencies": [] })
        );
        let rules = json!([{ "name": "error-thing", "severity": "error" }, { "name": "warn-thing", "severity": "warn" }]);
        let module = extract_first_transgression(&json!({ "dependencies": [], "rules": rules }));
        assert_eq!(module["tooltip"], json!("error-thing"));
        let dependency = extract_first_transgression(
            &json!({ "dependencies": [{ "rules": rules }, { "rules": [] }, { "x": 1 }] }),
        );
        assert_eq!(dependency["dependencies"][0]["rule"], rules[0]);
        assert_eq!(dependency["dependencies"][1].get("rule"), None);
        assert_eq!(dependency["dependencies"][2], json!({ "x": 1 }));
        let nameless = extract_first_transgression(&json!({ "tooltip": "t", "rules": [{}] }));
        assert_eq!(nameless.get("tooltip"), None, "undefined, printed as such");
    }

    #[test]
    fn labels() {
        let flat = flat_label(&json!({ "source": "aap/noot/mies/wim/zus.jet" }), true);
        assert_eq!(
            flat["label"],
            json!("<aap/noot/mies/wim/<BR/><B>zus.jet</B>>")
        );
        assert_eq!(flat["tooltip"], json!("zus.jet"));
        let metric = json!({ "source": "aap/noot/mies/wim/zus.jet", "instability": "0.481" });
        assert_eq!(
            flat_label(&metric, true)["label"],
            json!(
                "<aap/noot/mies/wim/<BR/><B>zus.jet</B> <FONT color=\"#808080\" point-size=\"8\">48%</FONT>>"
            )
        );
        assert_eq!(
            flat_label(&metric, false)["label"],
            json!("<aap/noot/mies/wim/<BR/><B>zus.jet</B>>")
        );
        let folder = folderify(
            &json!({ "source": "a/b/c.js", "instability": 0.5, "consolidated": true }),
            true,
        );
        assert_eq!(
            folder["label"],
            json!("<c.js>"),
            "not for a consolidated module"
        );
        assert_eq!(folder["folder"], json!("a/b"));
        assert_eq!(
            folder["path"],
            json!([{ "snippet": "a", "aggregateSnippet": "a" }, { "snippet": "b", "aggregateSnippet": "a/b" }])
        );
        assert_eq!(
            folderify(&json!({ "source": "c.js" }), false).get("folder"),
            None
        );
    }

    #[test]
    fn urls_and_self_transitions() {
        let unresolved = json!({ "source": "x", "couldNotResolve": true });
        assert_eq!(add_url(&unresolved, "p/", ""), unresolved);
        assert_eq!(
            add_url(&json!({ "source": "x" }), "p/", "")["URL"],
            json!("p/x")
        );
        let looped =
            json!({ "source": "a", "dependencies": [{ "resolved": "a" }, { "resolved": "b" }] });
        assert_eq!(
            strip_self_transitions(&looped)["dependencies"],
            json!([{ "resolved": "b" }])
        );
    }

    #[test]
    fn levels_prepare_in_upstream_order() {
        let theme = json!({ "replace": true });
        let p = Prepare {
            theme: &theme,
            collapse_pattern: Some("^src/[^/]+"),
            show_metrics: false,
            prefix: "",
            suffix: "",
        };
        let modules = vec![
            json!({ "source": "src/b/x.js", "dependencies": [{ "resolved": "src/a/y.js" }], "rules": [{ "name": "r" }] }),
            json!({ "source": "src/a/y.js", "dependencies": [] }),
        ];
        let custom = prepare(&modules, Level::Custom, &p);
        assert_eq!(custom[0]["source"], json!("src/a"));
        assert_eq!(
            custom[1]["tooltip"],
            json!("r"),
            "the rule overrides the file name"
        );
        let folder = prepare(&modules, Level::Folder, &p);
        assert_eq!(folder[1]["source"], json!("src/b"));
        assert_eq!(
            folder[1]["tooltip"],
            json!("b"),
            "the file name overrides the rule"
        );
        let flat = prepare(
            &modules,
            Level::Flat,
            &Prepare {
                collapse_pattern: None,
                ..p.clone()
            },
        );
        assert_eq!(flat[0]["source"], json!("src/a/y.js"));
        assert_eq!(flat[1]["tooltip"], json!("r"));
    }
}
