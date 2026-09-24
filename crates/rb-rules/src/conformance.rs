//! The `rulebearing validate` protocol: dependency-cruiser's own `test/validate` and
//! `test/graph-utl` specs call this engine through it, unmodified.
//!
//! - Protocol: `conformance/dependency-cruiser/harness/shim.mjs` (its header is the contract)
//! - Decision: [ADR-0009](../../../docs/adr/0009-conformance-suites-as-specification.md)
//! - Plan: [Wave 1, Step 8](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-8-rulebearing-validate-for-gate-1-layer-2-1b)
//! - Requirement: [NFR-CONF-01](../../../docs/prd.md#nfr-conf-01)
//!
//! A request names an upstream module, an export, a path into it (a method of a class, or a
//! function of an object export), the class's constructor arguments when there is one, and one
//! argument list per application of a curried function. [`dispatch`] maps each to the Rust
//! function that ports it and answers with the value the JavaScript would have returned. A rule
//! set arrives as the spec built it (usually through upstream's own `parseRuleSet`) and is read as
//! it is, without normalising it again, so a spec that bypasses normalisation sees upstream's
//! behaviour.

use rb_config::Rule;
use rb_config::model::DependencyRules;
use rb_model::Severity;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::compare::{
    compare_modules, compare_rules, compare_severities, compare_violations, sign,
};
use crate::graph::consolidate::{consolidate_to_folder, consolidate_to_pattern};
use crate::graph::filters::{Filter, Filters, add_focus, apply};
use crate::graph::indexed::{DependencySet, IndexedGraph};
use crate::js;
use crate::matchers::ModuleFacts;
use crate::validate::{
    Matcher, folder_match, matches_dependents_rule, matches_orphan_rule, matches_reachable_rule,
    matches_reaches_rule, module_match, validate_dependency, validate_folder, validate_module,
    violates_required_rule,
};

/// One request, as `shim.mjs` sends it.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    /// The upstream module specifier, for example `#validate/index.mjs`.
    pub module: String,
    /// The export name.
    pub export: String,
    /// The path to the function inside the export.
    #[serde(default)]
    pub path: Vec<String>,
    /// Constructor arguments, for a method of a class export.
    #[serde(default)]
    pub constructor_args: Option<Vec<Value>>,
    /// One argument list per application.
    #[serde(default)]
    pub calls: Vec<Vec<Value>>,
}

/// Why a request could not be answered.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProtocolError {
    /// The request names a function this engine does not port.
    #[error("{module} {export}{path}: not ported")]
    NotPorted {
        /// The module.
        module: String,
        /// The export.
        export: String,
        /// The path, as `.a.b`.
        path: String,
    },
    /// An argument has the wrong shape.
    #[error("argument {index}: {reason}")]
    Argument {
        /// The argument position, flattened across applications.
        index: usize,
        /// What was wrong.
        reason: String,
    },
}

/// A rule set read as it arrived: no defaults applied, no rule dropped.
///
/// # Errors
///
/// [`ProtocolError::Argument`] when a rule in `forbidden`, `allowed` or `required` does not
/// deserialise as a [`Rule`].
pub fn raw_rule_set(value: &Value) -> Result<DependencyRules, ProtocolError> {
    let list = |key: &str| -> Result<Vec<Rule>, ProtocolError> {
        js::array(value, key)
            .iter()
            .map(|r| {
                serde_json::from_value(r.clone()).map_err(|e| ProtocolError::Argument {
                    index: 0,
                    reason: format!("{key}: {e}"),
                })
            })
            .collect()
    };
    Ok(DependencyRules {
        forbidden: list("forbidden")?,
        allowed: list("allowed")?,
        allowed_severity: js::str_of(value, "allowedSeverity")
            .and_then(|s| s.parse::<Severity>().ok()),
        required: list("required")?,
    })
}

fn rule(value: &Value, index: usize) -> Result<Rule, ProtocolError> {
    serde_json::from_value(value.clone()).map_err(|e| ProtocolError::Argument {
        index,
        reason: e.to_string(),
    })
}

fn modules(value: &Value) -> Vec<Value> {
    value.as_array().cloned().unwrap_or_default()
}

fn string(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) => "null".into(),
        Some(other) => other.to_string(),
        None => "undefined".into(),
    }
}

fn depth(value: Option<&Value>) -> u32 {
    value
        .and_then(Value::as_u64)
        .and_then(|d| u32::try_from(d).ok())
        .unwrap_or(0)
}

/// JavaScript's `undefined`: a reply without a `result` key.
const UNDEFINED: &str = "\u{0}undefined";

/// `findVertexByName`: the module with its edges' `name` filled from `resolved`.
fn vertex(modules: &[Value], attribute: &str, name: &str) -> Value {
    let Some(found) = modules
        .iter()
        .rev()
        .find(|m| js::text(m, attribute) == name)
    else {
        return Value::String(UNDEFINED.into());
    };
    let mut out = found.clone();
    let dependencies: Vec<Value> = js::array(found, "dependencies")
        .iter()
        .map(|d| {
            let mut d = d.clone();
            if !js::truthy(d.get("name")) {
                let resolved = d.get("resolved").cloned().unwrap_or(Value::Null);
                js::set(&mut d, "name", resolved);
            }
            d
        })
        .collect();
    js::set(&mut out, "dependencies", Value::Array(dependencies));
    out
}

fn filters(value: &Value) -> Filters {
    let get = |k: &str| value.get(k).map(Filter::from_value);
    Filters {
        exclude: get("exclude"),
        include_only: get("includeOnly"),
        focus: value.get("focus").map(|f| {
            let mut filter = Filter::from_value(f);
            if filter.depth.is_none()
                && let Some(d) = value.get("focusDepth").and_then(Value::as_u64)
            {
                filter.depth = u32::try_from(d).ok();
            }
            filter
        }),
        reaches: get("reaches"),
        highlight: get("highlight"),
    }
}

fn find_rule_by_name(rule_set: &Value, name: &str) -> Value {
    js::array(rule_set, "forbidden")
        .iter()
        .chain(js::array(rule_set, "required"))
        .find(|r| js::str_of(r, "name") == Some(name))
        .cloned()
        .unwrap_or_else(|| Value::String(UNDEFINED.into()))
}

fn has_to(rule: &Value, key: &str) -> bool {
    rule.get("to").is_some_and(|to| js::has(to, key))
}

/// Answers one request. A value JavaScript would leave `undefined` is a string no JSON input can
/// produce, which [`answer`] turns into a reply without `result`.
///
/// # Errors
/// [`ProtocolError`] for a function not ported or an argument of the wrong shape.
pub fn dispatch(request: &Request) -> Result<Value, ProtocolError> {
    let args: Vec<&Value> = request.calls.iter().flatten().collect();
    let path = request.path.join(".");
    let key = (
        request.module.as_str(),
        request.export.as_str(),
        path.as_str(),
    );
    if key.0.starts_with("#validate/") {
        dispatch_validate(request, &args, key)
    } else {
        dispatch_graph_utl(request, &args, key)
    }
}

/// The `#validate/*` half of [`dispatch`].
fn dispatch_validate(
    request: &Request,
    args: &[&Value],
    key: (&str, &str, &str),
) -> Result<Value, ProtocolError> {
    let arg = |i: usize| args.get(i).copied().unwrap_or(&Value::Null);
    // The upstream specifications carry no cross-language keys, so no module facts.
    let facts = ModuleFacts::default();
    let answer = match key {
        ("#validate/index.mjs", "validateModule", "") => {
            validate_module(&raw_rule_set(arg(0))?, arg(1), &facts)
        }
        ("#validate/index.mjs", "validateDependency", "") => {
            validate_dependency(&raw_rule_set(arg(0))?, arg(1), arg(2), &facts)
        }
        ("#validate/index.mjs", "validateFolder", "") => {
            validate_folder(&raw_rule_set(arg(0))?, arg(1), arg(2))
        }
        ("#validate/matchers.mjs", "matchesAncestor", "") => {
            json!(crate::matchers::matches_ancestor(
                &rule(arg(0), 0)?,
                arg(1),
                arg(2)
            ))
        }
        ("#validate/match-module-rule-helpers.mjs", name, "") => {
            let r = rule(arg(0), 0)?;
            json!(match name {
                "matchesOrphanRule" => matches_orphan_rule(&r, arg(1), &facts),
                "matchesReachableRule" => matches_reachable_rule(&r, arg(1)),
                "matchesReachesRule" => matches_reaches_rule(&r, arg(1)),
                "matchesDependentsRule" => matches_dependents_rule(&r, arg(1)),
                _ => return Err(not_ported(request)),
            })
        }
        ("#validate/match-module-rule.mjs", "default", "match") => {
            json!(module_match(&rule(arg(1), 1)?, arg(0), &facts))
        }
        ("#validate/match-module-rule.mjs", "default", "isInteresting") => {
            json!(Matcher::Module.is_interesting(&rule(arg(0), 0)?))
        }
        ("#validate/match-dependency-rule.mjs", "default", "match") => {
            json!(crate::validate::dependency_match(
                &rule(arg(2), 2)?,
                arg(0),
                arg(1),
                &facts
            ))
        }
        ("#validate/match-dependency-rule.mjs", "default", "isInteresting") => {
            json!(Matcher::Dependency.is_interesting(&rule(arg(0), 0)?))
        }
        ("#validate/match-folder-dependency-rule.mjs", "default", "match") => {
            json!(folder_match(&rule(arg(2), 2)?, arg(0), arg(1)))
        }
        ("#validate/match-folder-dependency-rule.mjs", "default", "isInteresting") => {
            json!(Matcher::Folder.is_interesting(&rule(arg(0), 0)?))
        }
        ("#validate/violates-required-rule.mjs", "default", "") => {
            json!(violates_required_rule(&rule(arg(0), 0)?, arg(1)))
        }
        _ => return Err(not_ported(request)),
    };
    Ok(answer)
}

/// The `#graph-utl/*` half of [`dispatch`].
fn dispatch_graph_utl(
    request: &Request,
    args: &[&Value],
    key: (&str, &str, &str),
) -> Result<Value, ProtocolError> {
    let arg = |i: usize| args.get(i).copied().unwrap_or(&Value::Null);
    let answer = match key {
        ("#graph-utl/compare.mjs", "compareSeverities", "") => {
            json!(sign(compare_severities(arg(0).as_str(), arg(1).as_str())))
        }
        ("#graph-utl/compare.mjs", "compareViolations", "") => {
            json!(sign(compare_violations(arg(0), arg(1))))
        }
        ("#graph-utl/compare.mjs", "compareRules", "") => {
            json!(sign(compare_rules(arg(0), arg(1))))
        }
        ("#graph-utl/compare.mjs", "compareModules", "") => {
            json!(sign(compare_modules(arg(0), arg(1))))
        }
        ("#graph-utl/filter-bank.mjs", "applyFilters", "") => {
            if js::truthy(Some(arg(1))) {
                Value::Array(apply(modules(arg(0)), &filters(arg(1))))
            } else {
                arg(0).clone()
            }
        }
        ("#graph-utl/add-focus.mjs", "default", "") => {
            // `pFilter?.path`: `get` on a non-object is `None`, as the optional chain is.
            if js::truthy(arg(1).get("path")) {
                Value::Array(add_focus(modules(arg(0)), &Filter::from_value(arg(1))))
            } else {
                arg(0).clone()
            }
        }
        ("#graph-utl/consolidate-to-folder.mjs", "default", "") => {
            Value::Array(consolidate_to_folder(&modules(arg(0))))
        }
        ("#graph-utl/consolidate-to-pattern.mjs", "default", "") => Value::Array(
            consolidate_to_pattern(&modules(arg(0)), &string(Some(arg(1)))),
        ),
        ("#graph-utl/indexed-module-graph.mjs", "default", method) => {
            indexed_module_graph(request, args, method)?
        }
        ("#graph-utl/module-graph-with-dependency-set.mjs", "default", method) => {
            let constructor = request.constructor_args.clone().unwrap_or_default();
            let set = DependencySet::new(&modules(constructor.first().unwrap_or(&Value::Null)));
            match method {
                "moduleHasDependents" => json!(set.has_dependents(arg(0))),
                "getDependents" => json!(set.dependents(arg(0))),
                _ => return Err(not_ported(request)),
            }
        }
        ("#graph-utl/rule-set.mjs", "findRuleByName", "") => {
            find_rule_by_name(arg(0), &string(Some(arg(1))))
        }
        ("#graph-utl/rule-set.mjs", "ruleSetHasLicenseRule", "") => json!(
            js::array(arg(0), "forbidden")
                .iter()
                .chain(js::array(arg(0), "allowed"))
                .any(|r| has_to(r, "license") || has_to(r, "licenseNot"))
        ),
        ("#graph-utl/rule-set.mjs", "ruleSetHasDeprecationRule", "") => json!(
            js::array(arg(0), "forbidden")
                .iter()
                .chain(js::array(arg(0), "allowed"))
                .any(|r| r
                    .get("to")
                    .is_some_and(|to| js::strings(to, "dependencyTypes").contains(&"deprecated")))
        ),
        _ => return Err(not_ported(request)),
    };
    Ok(answer)
}

/// `IndexedModuleGraph`, constructed from the request and asked `method`.
fn indexed_module_graph(
    request: &Request,
    args: &[&Value],
    method: &str,
) -> Result<Value, ProtocolError> {
    let arg = |i: usize| args.get(i).copied().unwrap_or(&Value::Null);
    let constructor = request.constructor_args.clone().unwrap_or_default();
    let all = modules(constructor.first().unwrap_or(&Value::Null));
    let attribute = constructor
        .get(1)
        .and_then(Value::as_str)
        .unwrap_or("source")
        .to_owned();
    let graph = IndexedGraph::new(&all, &attribute);
    let answer =
        match method {
            "findVertexByName" => vertex(&all, &attribute, &string(args.first().copied())),
            "findTransitiveDependents" => {
                json!(graph.transitive_dependents(
                    &string(args.first().copied()),
                    depth(args.get(1).copied())
                ))
            }
            "findTransitiveDependencies" => {
                json!(graph.transitive_dependencies(
                    &string(args.first().copied()),
                    depth(args.get(1).copied())
                ))
            }
            "getPath" => json!(graph.path(&string(Some(arg(0))), &string(Some(arg(1))))),
            "getCycle" => json!(graph.cycle(&string(Some(arg(0))), &string(Some(arg(1))))),
            _ => return Err(not_ported(request)),
        };
    Ok(answer)
}

fn not_ported(request: &Request) -> ProtocolError {
    ProtocolError::NotPorted {
        module: request.module.clone(),
        export: request.export.clone(),
        path: request.path.iter().fold(String::new(), |mut out, p| {
            out.push('.');
            out.push_str(p);
            out
        }),
    }
}

/// Answers a request given as JSON text, as `rulebearing validate` reads it from stdin; the
/// reply is `{ "result": <value> }`. A value JavaScript would leave `undefined` is omitted.
///
/// # Errors
/// [`ProtocolError`], or an argument error when the text is not a request.
pub fn answer(text: &str) -> Result<String, ProtocolError> {
    let request: Request = serde_json::from_str(text).map_err(|e| ProtocolError::Argument {
        index: 0,
        reason: format!("not a request: {e}"),
    })?;
    let result = dispatch(&request)?;
    let mut reply = Map::new();
    if result.as_str() != Some(UNDEFINED) {
        reply.insert("result".into(), result);
    }
    Ok(Value::Object(reply).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(
        module: &str,
        export: &str,
        path: &[&str],
        calls: Value,
    ) -> Result<Value, ProtocolError> {
        dispatch(&Request {
            module: module.into(),
            export: export.into(),
            path: path.iter().map(|p| (*p).to_owned()).collect(),
            constructor_args: None,
            calls: serde_json::from_value(calls).unwrap_or_default(),
        })
    }

    #[test]
    fn validate_functions() -> Result<(), ProtocolError> {
        let set = json!({ "forbidden": [{ "name": "x", "severity": "error", "from": {}, "to": { "circular": true } }] });
        let r = call(
            "#validate/index.mjs",
            "validateDependency",
            &[],
            json!([[set, { "source": "a" }, { "circular": true }]]),
        )?;
        assert_eq!(
            r,
            json!({ "valid": false, "rules": [{ "severity": "error", "name": "x" }] })
        );
        assert_eq!(
            call(
                "#validate/index.mjs",
                "validateModule",
                &[],
                json!([[{}, { "source": "a" }]])
            )?,
            json!({ "valid": true })
        );
        assert_eq!(
            call(
                "#validate/index.mjs",
                "validateFolder",
                &[],
                json!([[{}, { "name": "a" }, { "name": "b" }]])
            )?,
            json!({ "valid": true })
        );
        let raw = raw_rule_set(&json!({ "allowed": [{ "from": {}, "to": { "path": "^x" } }] }))?;
        assert_eq!(raw.allowed_severity, None, "no defaults when read raw");
        Ok(())
    }

    #[test]
    fn matchers_and_curried_calls() -> Result<(), ProtocolError> {
        let ancestor = json!([[{ "from": {}, "to": { "ancestor": true } }, { "source": "a/b/c.ts" }, { "resolved": "a/x.ts" }]]);
        assert_eq!(
            call("#validate/matchers.mjs", "matchesAncestor", &[], ancestor)?,
            json!(true)
        );
        let orphan =
            json!([[{ "from": { "orphan": true }, "to": {} }, { "source": "x", "orphan": true }]]);
        for name in [
            "matchesOrphanRule",
            "matchesReachableRule",
            "matchesReachesRule",
            "matchesDependentsRule",
        ] {
            let r = call(
                "#validate/match-module-rule-helpers.mjs",
                name,
                &[],
                orphan.clone(),
            )?;
            assert_eq!(r, json!(name == "matchesOrphanRule"), "{name}");
        }
        let curried = json!([[{ "source": "x", "orphan": true }], [{ "from": { "orphan": true }, "to": {} }]]);
        assert_eq!(
            call(
                "#validate/match-module-rule.mjs",
                "default",
                &["match"],
                curried
            )?,
            json!(true)
        );
        assert_eq!(
            call(
                "#validate/match-module-rule.mjs",
                "default",
                &["isInteresting"],
                json!([[{ "from": { "orphan": true } }]])
            )?,
            json!(true)
        );
        let dep = json!([[{ "source": "a" }, { "resolved": "b" }], [{ "from": {}, "to": { "path": "^b" } }]]);
        assert_eq!(
            call(
                "#validate/match-dependency-rule.mjs",
                "default",
                &["match"],
                dep
            )?,
            json!(true)
        );
        assert_eq!(
            call(
                "#validate/match-dependency-rule.mjs",
                "default",
                &["isInteresting"],
                json!([[{ "from": {}, "to": {} }]])
            )?,
            json!(true)
        );
        let folder = json!([[{ "name": "a" }, { "name": "b" }], [{ "scope": "folder", "from": {}, "to": {} }]]);
        assert_eq!(
            call(
                "#validate/match-folder-dependency-rule.mjs",
                "default",
                &["match"],
                folder
            )?,
            json!(true)
        );
        assert_eq!(
            call(
                "#validate/match-folder-dependency-rule.mjs",
                "default",
                &["isInteresting"],
                json!([[{ "scope": "folder", "from": {}, "to": {} }]])
            )?,
            json!(true)
        );
        let required = json!([[{ "module": { "path": "^a" }, "to": { "path": "^b" } }, { "source": "a", "dependencies": [] }]]);
        assert_eq!(
            call(
                "#validate/violates-required-rule.mjs",
                "default",
                &[],
                required
            )?,
            json!(true)
        );
        Ok(())
    }

    #[test]
    fn graph_utilities() -> Result<(), ProtocolError> {
        assert_eq!(
            call(
                "#graph-utl/compare.mjs",
                "compareSeverities",
                &[],
                json!([["error", "warn"]])
            )?,
            json!(-1)
        );
        assert_eq!(
            call(
                "#graph-utl/compare.mjs",
                "compareRules",
                &[],
                json!([[{ "name": "a" }, { "name": "a" }]])
            )?,
            json!(0)
        );
        assert_eq!(
            call(
                "#graph-utl/compare.mjs",
                "compareModules",
                &[],
                json!([[{ "source": "b" }, { "source": "a" }]])
            )?,
            json!(1)
        );
        let v = json!({ "rule": { "name": "a", "severity": "error" }, "from": "x", "to": "y" });
        assert_eq!(
            call(
                "#graph-utl/compare.mjs",
                "compareViolations",
                &[],
                json!([[v, v]])
            )?,
            json!(0)
        );
        let graph = json!([{ "source": "a", "dependencies": [{ "resolved": "b" }] }, { "source": "b", "dependencies": [] }]);
        assert_eq!(
            call(
                "#graph-utl/filter-bank.mjs",
                "applyFilters",
                &[],
                json!([[graph, null]])
            )?,
            graph
        );
        let filtered = call(
            "#graph-utl/filter-bank.mjs",
            "applyFilters",
            &[],
            json!([[graph, { "includeOnly": "^a" }]]),
        )?;
        assert_eq!(filtered.as_array().map(Vec::len), Some(1));
        assert_eq!(
            call(
                "#graph-utl/add-focus.mjs",
                "default",
                &[],
                json!([[graph, {}]])
            )?,
            graph
        );
        assert_eq!(
            call(
                "#graph-utl/add-focus.mjs",
                "default",
                &[],
                json!([[graph, { "path": "^a" }]])
            )?
            .as_array()
            .map(Vec::len),
            Some(2)
        );
        assert_eq!(
            call(
                "#graph-utl/consolidate-to-folder.mjs",
                "default",
                &[],
                json!([[graph]])
            )?
            .as_array()
            .map(Vec::len),
            Some(1)
        );
        assert_eq!(
            call(
                "#graph-utl/consolidate-to-pattern.mjs",
                "default",
                &[],
                json!([[graph, "^a"]])
            )?
            .as_array()
            .map(Vec::len),
            Some(2)
        );
        Ok(())
    }

    #[test]
    fn classes_replay_their_constructor() -> Result<(), ProtocolError> {
        let graph = json!([{ "source": "a", "dependencies": [{ "resolved": "b" }], "dependents": [] },
                           { "source": "b", "dependencies": [{ "resolved": "a" }], "dependents": ["a"] }]);
        let method = |class: &str, name: &str, calls: Value| {
            dispatch(&Request {
                module: class.into(),
                export: "default".into(),
                path: vec![name.into()],
                constructor_args: Some(vec![graph.clone()]),
                calls: serde_json::from_value(calls).unwrap_or_default(),
            })
        };
        let indexed = "#graph-utl/indexed-module-graph.mjs";
        assert_eq!(
            method(indexed, "findVertexByName", json!([["a"]]))?["dependencies"][0]["name"],
            "b"
        );
        assert_eq!(
            method(indexed, "findVertexByName", json!([["zz"]]))?,
            json!(UNDEFINED)
        );
        assert_eq!(
            method(indexed, "findTransitiveDependents", json!([["b"]]))?,
            json!(["b", "a"])
        );
        assert_eq!(
            method(indexed, "findTransitiveDependencies", json!([["a", 1]]))?,
            json!(["a", "b"])
        );
        assert_eq!(
            method(indexed, "getPath", json!([["a", "b"]]))?
                .as_array()
                .map(Vec::len),
            Some(1)
        );
        assert_eq!(
            method(indexed, "getCycle", json!([["a", "b"]]))?
                .as_array()
                .map(Vec::len),
            Some(2)
        );
        assert!(method(indexed, "nope", json!([[]])).is_err());
        let set = "#graph-utl/module-graph-with-dependency-set.mjs";
        assert_eq!(
            method(set, "moduleHasDependents", json!([[{ "source": "a" }]]))?,
            json!(true)
        );
        assert_eq!(
            method(set, "getDependents", json!([[{ "source": "a" }]]))?,
            json!(["b"])
        );
        assert!(method(set, "nope", json!([[]])).is_err());
        Ok(())
    }

    #[test]
    fn rule_set_helpers_and_protocol() -> Result<(), ProtocolError> {
        let set = json!({ "forbidden": [{ "name": "a", "to": { "license": "GPL" } }], "allowed": [{ "to": { "dependencyTypes": ["deprecated"] } }] });
        assert_eq!(
            call(
                "#graph-utl/rule-set.mjs",
                "findRuleByName",
                &[],
                json!([[set, "a"]])
            )?["name"],
            "a"
        );
        assert_eq!(
            call(
                "#graph-utl/rule-set.mjs",
                "findRuleByName",
                &[],
                json!([[set, "zz"]])
            )?,
            json!(UNDEFINED)
        );
        let undefined = answer(
            r##"{"module":"#graph-utl/rule-set.mjs","export":"findRuleByName","calls":[[{}, "x"]]}"##,
        )?;
        assert_eq!(undefined, "{}");
        assert_eq!(
            call(
                "#graph-utl/rule-set.mjs",
                "ruleSetHasLicenseRule",
                &[],
                json!([[set]])
            )?,
            json!(true)
        );
        assert_eq!(
            call(
                "#graph-utl/rule-set.mjs",
                "ruleSetHasDeprecationRule",
                &[],
                json!([[set]])
            )?,
            json!(true)
        );
        assert!(
            call("#nope.mjs", "x", &["y"], json!([]))
                .err()
                .map(|e| e.to_string())
                .unwrap_or_default()
                .contains("#nope.mjs x.y")
        );
        let reply = answer(
            r##"{"module":"#graph-utl/compare.mjs","export":"compareSeverities","path":[],"calls":[["warn","error"]]}"##,
        )?;
        assert_eq!(reply, r#"{"result":1}"#);
        assert!(answer("nope").is_err());
        assert!(rule(&json!({ "severity": "loud" }), 3).is_err());
        assert_eq!(string(None), "undefined");
        assert_eq!(string(Some(&json!(null))), "null");
        assert_eq!(string(Some(&json!(3))), "3");
        assert_eq!(depth(Some(&json!(2))), 2);
        assert_eq!(depth(Some(&json!("2"))), 0);
        assert_eq!(depth(None), 0);
        let neither = json!({ "forbidden": [{ "to": { "path": "x" } }, { "from": {} }], "allowed": [{ "to": {} }] });
        assert_eq!(
            call(
                "#graph-utl/rule-set.mjs",
                "ruleSetHasLicenseRule",
                &[],
                json!([[neither]])
            )?,
            json!(false)
        );
        let license_not = json!({ "allowed": [{ "to": { "licenseNot": "MIT" } }] });
        assert_eq!(
            call(
                "#graph-utl/rule-set.mjs",
                "ruleSetHasLicenseRule",
                &[],
                json!([[license_not]])
            )?,
            json!(true)
        );
        Ok(())
    }
}
