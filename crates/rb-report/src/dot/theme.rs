//! The `dot` family's theming: the default theme, `normalizeTheme`, `getThemeAttributes` and
//! `applyTheme`. dependency-cruiser 18.2.0's `src/report/dot/default-theme.mjs` and
//! `theming.mjs`, ported.
//!
//! - Specification: `test/report/dot/theming.spec.mjs` and the module-level specs, run by
//!   conformance gate 1 layer 3 ([ADR-0009](../../../../docs/adr/0009-conformance-suites-as-specification.md))
//! - Coverage: [coverage § Options](../../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#options),
//!   row `reporterOptions.archi` / `dot` / `ddot` / `flat`, `theme` (`graph`, `node`, `edge`,
//!   `modules[]`, `dependencies[]`, `replace`)
//! - Plan: [Wave 2, Step 10](../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#210-step-10-reporters-and-baseline-semantics-2e)
//! - Requirement: [FR-OUT-01](../../../../docs/prd.md#fr-out-01)
//!
//! Attribute order is part of the output, so every object here keeps insertion order (the crate
//! turns on `serde_json`'s `preserve_order`) and merges the way JavaScript's object spread does.
//! A criterion matches when the module's value is identical to it or, as a regular expression,
//! matches it; a module value that is falsy never matches, as upstream's `has` decides.

use serde_json::{Map, Value, json};

use crate::js;

/// dependency-cruiser's default theme, key for key in upstream's order.
pub fn default_theme() -> Value {
    json!({
        "graph": {
            "rankdir": "LR",
            "splines": "true",
            "overlap": "false",
            "nodesep": "0.16",
            "ranksep": "0.18",
            "fontname": "Helvetica-bold",
            "fontsize": "9",
            "style": "rounded,bold,filled",
            "fillcolor": "#ffffff",
            "compound": "true"
        },
        "node": {
            "shape": "box",
            "style": "rounded, filled",
            "height": "0.2",
            "color": "black",
            "fillcolor": "#ffffcc",
            "fontcolor": "black",
            "fontname": "Helvetica",
            "fontsize": 9
        },
        "edge": {
            "arrowhead": "normal",
            "arrowsize": "0.6",
            "penwidth": "2.0",
            "color": "#00000033",
            "fontname": "Helvetica",
            "fontsize": "9"
        },
        "modules": [
            { "criteria": { "consolidated": true }, "attributes": { "shape": "box3d" } },
            { "criteria": { "rules[0].severity": "error" }, "attributes": { "fontcolor": "red", "color": "red" } },
            { "criteria": { "rules[0].severity": "warn" }, "attributes": { "fontcolor": "orange", "color": "orange" } },
            { "criteria": { "rules[0].severity": "info" }, "attributes": { "fontcolor": "blue", "color": "blue" } },
            { "criteria": { "coreModule": true }, "attributes": { "color": "grey", "fontcolor": "grey" } },
            { "criteria": { "source": "node_modules" }, "attributes": { "fillcolor": "#c40b0a1a", "fontcolor": "#c40b0a" } },
            { "criteria": { "matchesDoNotFollow": true }, "attributes": { "shape": "folder" } },
            { "criteria": { "orphan": true }, "attributes": { "fillcolor": "#ccffcc" } },
            { "criteria": { "source": "\\.json$" }, "attributes": { "fillcolor": "#ffee44" } },
            { "criteria": { "source": "\\.jsx$" }, "attributes": { "fillcolor": "#ffff77" } },
            { "criteria": { "source": "\\.vue$" }, "attributes": { "fillcolor": "#41f083" } },
            { "criteria": { "source": "\\.([cm]?ts)$" }, "attributes": { "fillcolor": "#ddfeff" } },
            { "criteria": { "source": "\\.tsx$" }, "attributes": { "fillcolor": "#bbfeff" } },
            { "criteria": { "source": "\\.svelte$" }, "attributes": { "fillcolor": "#febbff" } },
            { "criteria": { "source": "(\\.coffee|\\.litcoffee|\\.coffee\\.md)$" }, "attributes": { "fillcolor": "#eeccaa" } },
            { "criteria": { "source": "(\\.csx|\\.cjsx)$" }, "attributes": { "fillcolor": "#eebb77" } },
            { "criteria": { "source": "\\.ls$/g" }, "attributes": { "fillcolor": "pink" } },
            { "criteria": { "matchesHighlight": true }, "attributes": { "fillcolor": "lime", "penwidth": 2 } }
        ],
        "dependencies": [
            { "criteria": { "rules[0].severity": "error" }, "attributes": { "fontcolor": "red", "color": "red" } },
            { "criteria": { "rules[0].severity": "warn" }, "attributes": { "fontcolor": "orange", "color": "orange" } },
            { "criteria": { "rules[0].severity": "info" }, "attributes": { "fontcolor": "blue", "color": "blue" } },
            { "criteria": { "dynamic": true }, "attributes": { "style": "dashed" } },
            { "criteria": { "circular": true }, "attributes": { "arrowhead": "normalnoneodot" } },
            {
                "criteria": { "dependencyTypes": ["pre-compilation-only", "triple-slash-type-reference", "type-import", "type-only"] },
                "attributes": { "arrowhead": "onormal", "penwidth": "1.0" }
            },
            { "criteria": { "dependencyTypes": ["export"] }, "attributes": { "arrowhead": "inv" } },
            { "criteria": { "dependencyTypes": "core" }, "attributes": { "style": "dashed", "penwidth": "1.0" } },
            { "criteria": { "dependencyTypes": "npm" }, "attributes": { "penwidth": "1.0" } }
        ]
    })
}

/// `(list || []).concat(defaults)`: an array's items, or a lone truthy value, before `defaults`.
fn concat(list: Option<&Value>, defaults: Option<&Value>) -> Value {
    let mut out: Vec<Value> = match list {
        Some(Value::Array(items)) => items.clone(),
        other if js::truthy(other) => other.into_iter().cloned().collect(),
        _ => Vec::new(),
    };
    if let Some(Value::Array(items)) = defaults {
        out.extend(items.iter().cloned());
    }
    Value::Array(out)
}

/// `normalizeTheme(theme)`: the default theme; a theme with `replace` as it is; otherwise the
/// default's `graph`, `node` and `edge` with the theme's spread over them, and the theme's
/// `modules` and `dependencies` before the default's.
pub fn normalize_theme(theme: Option<&Value>) -> Value {
    let default = default_theme();
    let Some(theme) = theme.filter(|t| js::truthy(Some(t))) else {
        return default;
    };
    if js::truthy(theme.get("replace")) {
        return theme.clone();
    }
    let section = |key: &str| {
        Value::Object(js::spread(
            default.get(key).unwrap_or(&Value::Null),
            theme.get(key).unwrap_or(&Value::Null),
        ))
    };
    let mut out = Map::new();
    out.insert("graph".into(), section("graph"));
    out.insert("node".into(), section("node"));
    out.insert("edge".into(), section("edge"));
    out.insert(
        "modules".into(),
        concat(theme.get("modules"), default.get("modules")),
    );
    out.insert(
        "dependencies".into(),
        concat(theme.get("dependencies"), default.get("dependencies")),
    );
    Value::Object(out)
}

/// `pModuleKey === pCriterion || getCachedRegExp(pCriterion).test(pModuleKey)`.
fn matches_criterion(key: &Value, criterion: &Value) -> bool {
    let identical = match (key, criterion) {
        (Value::Number(a), Value::Number(b)) => a.as_f64() == b.as_f64(),
        (Value::String(a), Value::String(b)) => a == b,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Null, Value::Null) => true,
        // Two objects are identical only when they are the same object, which a key read from a
        // module and a criterion read from a theme never are.
        _ => false,
    };
    identical
        || rb_rules::patterns::test(&js::to_string(Some(criterion)), &js::to_string(Some(key)))
}

fn matches_entry(entry: &Value, item: &Value) -> bool {
    let Some(criteria) = entry.get("criteria").and_then(Value::as_object) else {
        // `Object.keys` of a primitive is empty, so `every` holds.
        return !matches!(entry.get("criteria"), None | Some(Value::Null));
    };
    criteria.iter().all(|(key, criterion)| {
        let Some(value) = js::get(item, key) else {
            return false;
        };
        match (&value, criterion) {
            (Value::Array(values), Value::Array(criteria)) => criteria
                .iter()
                .any(|c| values.iter().any(|v| matches_criterion(v, c))),
            (Value::Array(values), c) => values.iter().any(|v| matches_criterion(v, c)),
            (v, Value::Array(criteria)) => criteria.iter().any(|c| matches_criterion(v, c)),
            (v, c) => matches_criterion(v, c),
        }
    })
}

/// `getThemeAttributes(item, entries)`: the attributes of every matching entry, the earlier entry
/// winning each key, keys in the order the spread leaves them.
pub fn theme_attributes(item: &Value, entries: Option<&Value>) -> Map<String, Value> {
    let Some(entries) = entries.and_then(Value::as_array) else {
        return Map::new();
    };
    entries
        .iter()
        .filter(|entry| matches_entry(entry, item))
        .map(|entry| entry.get("attributes").cloned().unwrap_or(Value::Null))
        .fold(Map::new(), |all, current| {
            js::spread(&current, &Value::Object(all))
        })
}

/// `attributizeObject(object)`: `key="value"` pairs joined with a space.
pub fn attributize(object: &Map<String, Value>) -> String {
    js::keys_in_order(object.keys().map(String::as_str))
        .into_iter()
        .map(|key| format!("{key}=\"{}\"", js::to_string(object.get(key))))
        .collect::<Vec<_>>()
        .join(" ")
}

/// `applyTheme(theme)` on one module: `themeAttrs` on the module and on each dependency, and
/// `hasExtraAttributes` on each dependency.
pub fn apply_theme(theme: &Value, module: &Value) -> Value {
    let module_attributes = attributize(&theme_attributes(module, theme.get("modules")));
    let dependencies: Vec<Value> = rb_rules::js::array(module, "dependencies")
        .iter()
        .map(|dependency| {
            let attributes = attributize(&theme_attributes(dependency, theme.get("dependencies")));
            let extra = js::truthy(dependency.get("rule")) || !attributes.is_empty();
            let mut out = dependency.as_object().cloned().unwrap_or_default();
            out.insert("themeAttrs".into(), Value::String(attributes));
            out.insert("hasExtraAttributes".into(), Value::Bool(extra));
            Value::Object(out)
        })
        .collect();
    let mut out = module.as_object().cloned().unwrap_or_default();
    out.insert("dependencies".into(), Value::Array(dependencies));
    out.insert("themeAttrs".into(), Value::String(module_attributes));
    Value::Object(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attributes(item: &Value, theme: &Value) -> Value {
        Value::Object(theme_attributes(
            item,
            normalize_theme(Some(theme)).get("modules"),
        ))
    }

    #[test]
    fn the_default_theme_colours_by_kind() {
        let none = json!({});
        assert_eq!(attributes(&json!({}), &none), json!({}));
        assert_eq!(
            attributes(&json!({ "coreModule": true }), &none),
            json!({ "color": "grey", "fontcolor": "grey" })
        );
        assert_eq!(
            attributes(&json!({ "source": "package.json" }), &none),
            json!({ "fillcolor": "#ffee44" })
        );
        assert_eq!(
            attributes(&json!({ "couldNotResolve": true }), &none),
            json!({})
        );
        // Earlier entries win; the spread puts the later entry's keys first.
        let module = json!({ "source": "node_modules/x.json", "rules": [{ "severity": "error" }] });
        let found = theme_attributes(&module, default_theme().get("modules"));
        assert_eq!(
            attributize(&found),
            "fillcolor=\"#c40b0a1a\" fontcolor=\"red\" color=\"red\""
        );
    }

    #[test]
    fn criteria_arrays_strings_and_regexes() {
        let module = json!({ "source": "src/heide/does.js", "dependencyTypes": ["local", "aliased-tsconfig"] });
        for (criterion, expected) in [
            (
                json!(["npm", "aliased-tsconfig"]),
                json!({ "fillcolor": "blue" }),
            ),
            (json!(["aliased-t.+"]), json!({ "fillcolor": "blue" })),
            (json!(["npm"]), json!({})),
            (json!("aliased-tsconfig"), json!({ "fillcolor": "blue" })),
        ] {
            let theme = json!({ "modules": [{ "criteria": { "dependencyTypes": criterion }, "attributes": { "fillcolor": "blue" } }] });
            assert_eq!(attributes(&module, &theme), expected);
        }
        let theme = json!({ "modules": [{ "criteria": { "source": ["package.json", "package-lock.json"] }, "attributes": { "fillcolor": "red" } }] });
        assert_eq!(
            attributes(&json!({ "source": "package.json" }), &theme),
            json!({ "fillcolor": "red" })
        );
        // A number criterion is identical to a number, or matches as a pattern.
        let theme = json!({ "replace": true, "modules": [{ "criteria": { "n": 1 }, "attributes": { "x": 1 } }] });
        assert_eq!(attributes(&json!({ "n": 1.0 }), &theme), json!({ "x": 1 }));
        assert_eq!(attributes(&json!({ "n": 21 }), &theme), json!({ "x": 1 }));
        assert_eq!(
            attributes(&json!({ "n": 0 }), &theme),
            json!({}),
            "falsy never matches"
        );
        // A criteria that is not an object has no keys, so it matches; a missing one does not.
        let theme = json!({ "replace": true, "modules": [{ "criteria": 5, "attributes": { "y": 1 } }, { "attributes": { "z": 1 } }] });
        assert_eq!(attributes(&json!({}), &theme), json!({ "y": 1 }));
        assert_eq!(theme_attributes(&json!({}), None), Map::new());
    }

    #[test]
    fn normalizing_merges_or_replaces() {
        let merged = normalize_theme(Some(
            &json!({ "graph": { "splines": "ortho", "someAttribute": 1234 }, "modules": { "criteria": {}, "attributes": {} } }),
        ));
        let graph = merged
            .get("graph")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        assert_eq!(graph.get("splines"), Some(&json!("ortho")));
        assert_eq!(
            graph.keys().next_back().map(String::as_str),
            Some("someAttribute")
        );
        assert_eq!(
            merged
                .get("modules")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(19),
            "a lone entry goes before the default's eighteen"
        );
        assert_eq!(
            normalize_theme(Some(&json!({ "replace": true }))),
            json!({ "replace": true })
        );
        assert_eq!(normalize_theme(None), default_theme());
        assert_eq!(normalize_theme(Some(&json!(null))), default_theme());
    }

    #[test]
    fn attributes_print_in_object_key_order() {
        let object = json!({ "b": 1, "2": true, "a": [1, 2], "1": null });
        let map = object.as_object().cloned().unwrap_or_default();
        assert_eq!(attributize(&map), "1=\"null\" 2=\"true\" b=\"1\" a=\"1,2\"");
    }

    #[test]
    fn applying_marks_extra_attributes() {
        let module = json!({ "source": "a.json", "dependencies": [
            { "resolved": "b", "dynamic": true },
            { "resolved": "c", "rule": { "name": "r" } },
            { "resolved": "d" }
        ] });
        let themed = apply_theme(&default_theme(), &module);
        assert_eq!(themed["themeAttrs"], json!("fillcolor=\"#ffee44\""));
        assert_eq!(
            themed["dependencies"][0]["themeAttrs"],
            json!("style=\"dashed\"")
        );
        assert_eq!(themed["dependencies"][0]["hasExtraAttributes"], json!(true));
        assert_eq!(themed["dependencies"][1]["hasExtraAttributes"], json!(true));
        assert_eq!(
            themed["dependencies"][2]["hasExtraAttributes"],
            json!(false)
        );
    }
}
