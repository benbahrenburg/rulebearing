//! Consolidation: modules folded into folders or into the first match of a pattern.
//! dependency-cruiser 18.2.0's `consolidate-*.mjs`, ported.
//!
//! - Specification: `test/graph-utl/consolidate-to-folder.spec.mjs`,
//!   `consolidate-to-pattern.spec.mjs`, run by conformance gate 1 layer 2
//! - Plan: [Wave 1, Step 6](../../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-6-graph-analysis-1b)
//!   (`folders.rs`: consolidation into `folders[]`)
//! - Requirement: [FR-RULE-08](../../../../docs/prd.md#fr-rule-08)
//!
//! The functions keep JavaScript's object-spread semantics: every key of the input survives,
//! later objects win, and `valid` is `left.valid && right.valid` with JavaScript's `&&`, which
//! returns an operand rather than a boolean.

use std::collections::HashSet;

use serde_json::{Map, Value, json};

use crate::compare::compare_rules;
use crate::js;
use crate::patterns;

/// Node's `path.posix.dirname`.
pub fn dirname(path: &str) -> String {
    if path.is_empty() {
        return ".".to_owned();
    }
    let bytes = path.as_bytes();
    let has_root = bytes[0] == b'/';
    let mut end = None;
    let mut matched_slash = true;
    for i in (1..bytes.len()).rev() {
        if bytes[i] == b'/' {
            if !matched_slash {
                end = Some(i);
                break;
            }
        } else {
            matched_slash = false;
        }
    }
    match end {
        None => (if has_root { "/" } else { "." }).to_owned(),
        Some(1) if has_root => "//".to_owned(),
        Some(end) => path[..end].to_owned(),
    }
}

/// JavaScript's `a && b`.
fn and(a: Option<&Value>, b: Option<&Value>) -> Option<Value> {
    if js::truthy(a) {
        b.cloned()
    } else {
        a.cloned()
    }
}

/// JavaScript's `a || b`.
fn or(a: Option<&Value>, b: Option<&Value>) -> Option<Value> {
    if js::truthy(a) {
        a.cloned()
    } else {
        b.cloned()
    }
}

fn put(map: &mut Map<String, Value>, key: &str, value: Option<Value>) {
    match value {
        Some(value) => {
            map.insert(key.to_owned(), value);
        }
        None => {
            map.remove(key);
        }
    }
}

/// `{ ...left, ...right }`.
fn spread(left: &Value, right: &Value) -> Map<String, Value> {
    let mut out = left.as_object().cloned().unwrap_or_default();
    if let Some(right) = right.as_object() {
        for (k, v) in right {
            out.insert(k.clone(), v.clone());
        }
    }
    out
}

/// `left.concat(right ?? []).sort(compareRules)`.
fn merged_rules(left: &Value, right: &Value) -> Value {
    let mut rules: Vec<Value> = js::array(left, "rules").to_vec();
    rules.extend(js::array(right, "rules").iter().cloned());
    rules.sort_by(compare_rules);
    Value::Array(rules)
}

fn merge_module(left: &Value, right: &Value) -> Value {
    let mut out = spread(left, right);
    let mut seen = HashSet::new();
    let dependencies: Vec<Value> = js::array(left, "dependencies")
        .iter()
        .chain(js::array(right, "dependencies"))
        .filter(|d| seen.insert(js::text(d, "resolved").into_owned()))
        .cloned()
        .collect();
    out.insert("dependencies".into(), Value::Array(dependencies));
    out.insert("rules".into(), merged_rules(left, right));
    put(
        &mut out,
        "valid",
        and(left.get("valid"), right.get("valid")),
    );
    put(
        &mut out,
        "consolidated",
        or(left.get("consolidated"), right.get("consolidated")),
    );
    Value::Object(out)
}

/// `consolidateModules`: modules with the same `source` merged into one, first-seen order.
pub fn consolidate_modules(modules: &[Value]) -> Vec<Value> {
    let mut order: Vec<String> = Vec::new();
    let mut merged: std::collections::HashMap<String, Value> = std::collections::HashMap::new();
    for module in modules {
        let source = js::text(module, "source").into_owned();
        let base = merged.remove(&source).unwrap_or_else(|| {
            order.push(source.clone());
            json!({ "dependencies": [], "rules": [], "valid": true })
        });
        merged.insert(source, merge_module(&base, module));
    }
    order
        .into_iter()
        .filter_map(|source| merged.remove(&source))
        .collect()
}

fn merge_dependency(left: &Value, right: &Value) -> Value {
    let mut out = spread(left, right);
    let mut types: Vec<Value> = js::array(left, "dependencyTypes").to_vec();
    match right.get("dependencyTypes") {
        Some(Value::Array(more)) => types.extend(more.iter().cloned()),
        // `[].concat(undefined)` appends `undefined`, which JSON writes as null.
        Some(other) => types.push(other.clone()),
        None => types.push(Value::Null),
    }
    let mut unique: Vec<Value> = Vec::with_capacity(types.len());
    for t in types {
        if !unique.contains(&t) {
            unique.push(t);
        }
    }
    out.insert("dependencyTypes".into(), Value::Array(unique));
    out.insert("rules".into(), merged_rules(left, right));
    put(
        &mut out,
        "valid",
        and(left.get("valid"), right.get("valid")),
    );
    Value::Object(out)
}

/// `consolidateModuleDependencies`: a module's dependencies with the same `resolved` merged.
pub fn consolidate_module_dependencies(module: &Value) -> Value {
    let mut order: Vec<String> = Vec::new();
    let mut merged: std::collections::HashMap<String, Value> = std::collections::HashMap::new();
    for dependency in js::array(module, "dependencies") {
        let resolved = js::text(dependency, "resolved").into_owned();
        let base = merged.remove(&resolved).unwrap_or_else(|| {
            order.push(resolved.clone());
            json!({ "dependencyTypes": [], "rules": [], "valid": true })
        });
        merged.insert(resolved, merge_dependency(&base, dependency));
    }
    let dependencies: Vec<Value> = order
        .into_iter()
        .filter_map(|r| merged.remove(&r))
        .collect();
    let mut out = module.as_object().cloned().unwrap_or_default();
    out.insert("dependencies".into(), Value::Array(dependencies));
    Value::Object(out)
}

fn with(value: &Value, key: &str, inner: Value) -> Value {
    let mut out = value.as_object().cloned().unwrap_or_default();
    out.insert(key.to_owned(), inner);
    Value::Object(out)
}

/// `consolidateToFolder`.
pub fn consolidate_to_folder(modules: &[Value]) -> Vec<Value> {
    let squashed: Vec<Value> = modules
        .iter()
        .map(|m| {
            let dependencies: Vec<Value> = js::array(m, "dependencies")
                .iter()
                .map(|d| {
                    with(
                        d,
                        "resolved",
                        Value::String(dirname(&js::text(d, "resolved"))),
                    )
                })
                .collect();
            let mut out = with(m, "source", Value::String(dirname(&js::text(m, "source"))));
            js::set(&mut out, "consolidated", Value::Bool(true));
            js::set(&mut out, "dependencies", Value::Array(dependencies));
            out
        })
        .collect();
    consolidate_modules(&squashed)
        .iter()
        .map(consolidate_module_dependencies)
        .collect()
}

/// `consolidateToPattern`.
pub fn consolidate_to_pattern(modules: &[Value], pattern: &str) -> Vec<Value> {
    let squashed: Vec<Value> = modules
        .iter()
        .map(|m| {
            let source = js::text(m, "source").into_owned();
            let matched = patterns::first_match(pattern, &source);
            let consolidated = m.get("consolidated") == Some(&Value::Bool(true))
                || matched.as_ref().is_some_and(|found| *found != source);
            let dependencies: Vec<Value> = js::array(m, "dependencies")
                .iter()
                .map(|d| {
                    let resolved = js::text(d, "resolved").into_owned();
                    let squashed = patterns::first_match(pattern, &resolved).unwrap_or(resolved);
                    with(d, "resolved", Value::String(squashed))
                })
                .collect();
            let mut out = with(m, "source", Value::String(matched.unwrap_or(source)));
            js::set(&mut out, "consolidated", Value::Bool(consolidated));
            js::set(&mut out, "dependencies", Value::Array(dependencies));
            out
        })
        .collect();
    consolidate_modules(&squashed)
        .iter()
        .map(consolidate_module_dependencies)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dirname_is_node_s() {
        for (input, expected) in [
            ("", "."),
            ("a", "."),
            ("a/b", "a"),
            ("a/b/", "a"),
            ("/a", "/"),
            ("/", "/"),
            ("//a", "//"),
            ("a//b", "a/"),
            ("src/x/y.ts", "src/x"),
        ] {
            assert_eq!(dirname(input), expected, "{input}");
        }
    }

    #[test]
    fn folders_merge_with_spread_semantics() {
        let modules = vec![
            json!({ "source": "src/a/x.ts", "valid": true, "rules": [], "extra": 1,
                    "dependencies": [
                        { "resolved": "src/b/y.ts", "dependencyTypes": ["local"], "valid": true, "rules": [] },
                        { "resolved": "src/b/z.ts", "dependencyTypes": ["npm"], "valid": false,
                          "rules": [{ "name": "r", "severity": "warn" }] }
                    ] }),
            json!({ "source": "src/a/w.ts", "valid": false, "rules": [{ "name": "q", "severity": "error" }],
                    "dependencies": [] }),
        ];
        let folders = consolidate_to_folder(&modules);
        assert_eq!(folders.len(), 1);
        let folder = &folders[0];
        assert_eq!(folder["source"], "src/a");
        assert_eq!(folder["consolidated"], true);
        assert_eq!(folder["valid"], false);
        assert_eq!(folder["extra"], 1);
        assert_eq!(folder["rules"][0]["name"], "q");
        // Upstream's consolidateModules keeps the first edge per `resolved` (uniqBy) before the
        // dependencies are merged, so the second edge into src/b is dropped, types and all.
        assert_eq!(folder["dependencies"].as_array().map(Vec::len), Some(1));
        let dependency = &folder["dependencies"][0];
        assert_eq!(dependency["resolved"], "src/b");
        assert_eq!(dependency["dependencyTypes"], json!(["local"]));
        assert_eq!(dependency["valid"], true);
        assert_eq!(dependency["rules"], json!([]));
    }

    #[test]
    fn patterns_consolidate_matching_modules_only() {
        let modules = vec![
            json!({ "source": "packages/a/src/x.ts", "dependencies": [{ "resolved": "packages/b/index.ts", "dependencyTypes": ["local"] }] }),
            json!({ "source": "packages/a/src/y.ts", "dependencies": [{ "resolved": "packages/b/other.ts" }] }),
            json!({ "source": "tools/z.ts", "dependencies": [] }),
            json!({ "source": "packages/c", "consolidated": true, "dependencies": [] }),
        ];
        let out = consolidate_to_pattern(&modules, "^packages/[^/]+");
        let sources: Vec<&str> = out.iter().filter_map(|m| m["source"].as_str()).collect();
        assert_eq!(sources, ["packages/a", "tools/z.ts", "packages/c"]);
        assert_eq!(out[0]["consolidated"], true);
        assert_eq!(out[1]["consolidated"], false);
        assert_eq!(out[2]["consolidated"], true);
        assert_eq!(out[0]["dependencies"].as_array().map(Vec::len), Some(1));
        assert_eq!(
            out[0]["dependencies"][0]["dependencyTypes"],
            json!(["local"])
        );
    }

    #[test]
    fn merging_dependencies_follows_concat() {
        let module = json!({ "source": "m", "dependencies": [
            { "resolved": "t", "dependencyTypes": ["local"], "valid": true, "rules": [{ "name": "b", "severity": "warn" }] },
            { "resolved": "t", "valid": false, "rules": [{ "name": "a", "severity": "error" }] },
            { "resolved": "u", "dependencyTypes": "odd" }
        ] });
        let merged = consolidate_module_dependencies(&module);
        let t = &merged["dependencies"][0];
        assert_eq!(
            t["dependencyTypes"],
            json!(["local", null]),
            "[].concat(undefined) appends undefined"
        );
        assert_eq!(t["valid"], false);
        assert_eq!(t["rules"][0]["name"], "a", "rules sort by severity");
        assert_eq!(merged["dependencies"][1]["dependencyTypes"], json!(["odd"]));
        assert_eq!(
            merged["dependencies"][1].get("valid"),
            None,
            "true && undefined"
        );
    }

    #[test]
    fn javascript_boolean_operators() {
        let t = json!(true);
        let f = json!(false);
        assert_eq!(and(Some(&t), None), None);
        assert_eq!(and(Some(&f), Some(&t)), Some(f.clone()));
        assert_eq!(or(None, Some(&t)), Some(t.clone()));
        assert_eq!(or(Some(&t), Some(&f)), Some(t));
        let mut m = Map::new();
        put(&mut m, "k", Some(json!(1)));
        put(&mut m, "k", None);
        assert!(m.is_empty());
    }
}
