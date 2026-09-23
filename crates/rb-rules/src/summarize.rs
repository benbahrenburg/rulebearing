//! `summary`: violations extracted from the annotated graph, counted and ordered, plus the
//! options and rule set the run used. dependency-cruiser 18.2.0's `src/analyze/summarize`, ported.
//!
//! - Plan: [Wave 1, Step 7](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-7-liveness-severity-ids-receipts-expires-ratchets-1b)
//!   (`summary.rs`)
//! - Coverage: [coverage § Result document](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#result-document-cruise-result-schema)
//! - Decisions: [ADR-0004](../../../docs/adr/0004-graph-document-is-cruise-result-superset.md)
//!   (additions are additive), [ADR-0015](../../../docs/adr/0015-stable-violation-id.md) (ids)
//! - Requirements: [FR-CORE-04](../../../docs/prd.md#fr-core-04), [FR-CORE-07](../../../docs/prd.md#fr-core-07)

use rb_config::Rule;
use rb_config::model::DependencyRules;
use serde_json::{Map, Value, json};

use crate::compare::compare_violations;
use crate::js;

/// `findRuleByName`: the first `forbidden` or `required` rule with the name.
pub fn find_rule_by_name<'a>(rules: Option<&'a DependencyRules>, name: &str) -> Option<&'a Rule> {
    let rules = rules?;
    rules
        .forbidden
        .iter()
        .chain(&rules.required)
        .find(|r| r.name() == name)
}

fn names(steps: &Value) -> Vec<&str> {
    steps
        .as_array()
        .map(|s| s.iter().filter_map(|x| js::str_of(x, "name")).collect())
        .unwrap_or_default()
}

/// `isSameViolation`.
pub fn is_same_violation(left: &Value, right: &Value) -> bool {
    let rule_name = |v: &Value| {
        v.get("rule")
            .and_then(|r| js::str_of(r, "name"))
            .map(str::to_owned)
    };
    if rule_name(left) != rule_name(right) {
        return false;
    }
    let same_from_to =
        || left.get("from") == right.get("from") && left.get("to") == right.get("to");
    let same_steps = |key: &str| {
        let (l, r) = (names(&left[key]), names(&right[key]));
        l.len() == r.len() && l.iter().all(|n| r.contains(n))
    };
    if js::truthy(left.get("cycle")) && js::truthy(right.get("cycle")) {
        return same_steps("cycle");
    }
    if js::truthy(left.get("via")) && js::truthy(right.get("via")) {
        return same_from_to() && same_steps("via");
    }
    same_from_to()
}

fn dependency_violation(
    rule: &Value,
    module: &Value,
    dependency: &Value,
    rules: Option<&DependencyRules>,
) -> Value {
    let mut out = Map::new();
    out.insert("type".into(), json!("dependency"));
    out.insert("from".into(), json!(js::text(module, "source")));
    out.insert("to".into(), json!(js::text(dependency, "resolved")));
    if let Some(module_name) = dependency.get("module") {
        out.insert("unresolvedTo".into(), module_name.clone());
    }
    if let Some(types) = dependency.get("dependencyTypes") {
        out.insert("dependencyTypes".into(), types.clone());
    }
    out.insert("rule".into(), rule.clone());
    let found = find_rule_by_name(rules, js::str_of(rule, "name").unwrap_or_default());
    if js::has(dependency, "cycle") && found.is_some_and(|r| r.to.circular == Some(true)) {
        out.insert("type".into(), json!("cycle"));
        out.insert("cycle".into(), dependency["cycle"].clone());
    }
    if js::has(module, "instability")
        && js::has(dependency, "instability")
        && found.is_some_and(|r| r.to.more_unstable.is_some())
    {
        out.insert("type".into(), json!("instability"));
        out.insert(
            "metrics".into(),
            json!({ "from": { "instability": module["instability"] }, "to": { "instability": dependency["instability"] } }),
        );
    }
    Value::Object(out)
}

fn module_violations(rule: &Value, module: &Value, rules: Option<&DependencyRules>) -> Vec<Value> {
    let source = js::text(module, "source");
    let name = js::str_of(rule, "name").unwrap_or_default();
    let reachable = find_rule_by_name(rules, name).is_some_and(|r| r.to.reachable == Some(true));
    if js::truthy(module.get("reaches")) && reachable {
        return js::array(module, "reaches")
            .iter()
            .filter(|r| js::str_of(r, "asDefinedInRule") == Some(name))
            .flat_map(|r| js::array(r, "modules").iter())
            .map(|m| {
                json!({ "type": "reachability", "from": source, "to": js::text(m, "source"), "rule": rule, "via": m.get("via").cloned().unwrap_or(Value::Null) })
            })
            .collect();
    }
    vec![json!({ "type": "module", "from": source, "to": source, "rule": rule })]
}

/// `summarizeModules`: dependency and module violations, sorted, duplicates removed.
pub fn summarize_modules(modules: &[Value], rules: Option<&DependencyRules>) -> Vec<Value> {
    let mut violations: Vec<Value> = Vec::new();
    for module in modules {
        for dependency in js::array(module, "dependencies") {
            if dependency.get("valid") == Some(&Value::Bool(false)) {
                for rule in js::array(dependency, "rules") {
                    violations.push(dependency_violation(rule, module, dependency, rules));
                }
            }
        }
    }
    for module in modules
        .iter()
        .filter(|m| m.get("valid") == Some(&Value::Bool(false)))
    {
        for rule in js::array(module, "rules") {
            violations.extend(module_violations(rule, module, rules));
        }
    }
    violations.sort_by(compare_violations);
    let mut unique: Vec<Value> = Vec::with_capacity(violations.len());
    for violation in violations {
        if !unique.iter().any(|u| is_same_violation(&violation, u)) {
            unique.push(violation);
        }
    }
    unique
}

/// `summarizeFolders`.
pub fn summarize_folders(folders: &[Value], rules: Option<&DependencyRules>) -> Vec<Value> {
    let mut out = Vec::new();
    for folder in folders {
        for dependency in js::array(folder, "dependencies") {
            if js::truthy(dependency.get("valid")) {
                continue;
            }
            for rule in js::array(dependency, "rules") {
                let found = find_rule_by_name(rules, js::str_of(rule, "name").unwrap_or_default());
                let kind = if found.is_some_and(|r| r.to.more_unstable.is_some()) {
                    "instability"
                } else if found.is_some_and(|r| r.to.circular.is_some()) {
                    "cycle"
                } else {
                    "folder"
                };
                let mut v = json!({ "type": kind, "from": js::text(folder, "name"), "to": js::text(dependency, "name"), "rule": rule });
                match kind {
                    "instability" => js::set(
                        &mut v,
                        "metrics",
                        json!({ "from": { "instability": folder.get("instability").cloned().unwrap_or(Value::Null) },
                                "to": { "instability": dependency.get("instability").cloned().unwrap_or(Value::Null) } }),
                    ),
                    "cycle" => js::set(
                        &mut v,
                        "cycle",
                        dependency.get("cycle").cloned().unwrap_or(Value::Null),
                    ),
                    _ => {}
                }
                out.push(v);
            }
        }
    }
    out
}

/// `getViolationStats`: counts per severity.
pub fn violation_stats(violations: &[Value]) -> Map<String, Value> {
    let mut counts = [0u64; 4];
    for v in violations {
        let index = match v.get("rule").and_then(|r| js::str_of(r, "severity")) {
            Some("error") => 0,
            Some("warn") => 1,
            Some("info") => 2,
            Some("ignore") => 3,
            _ => continue,
        };
        counts[index] += 1;
    }
    let mut out = Map::new();
    for (key, count) in ["error", "warn", "info", "ignore"].iter().zip(counts) {
        out.insert((*key).to_owned(), json!(count));
    }
    out
}

/// `addRuleSetUsed`: the normalised rule set, `allowed` rules without their fixed name.
pub fn rule_set_used(rules: &DependencyRules) -> Map<String, Value> {
    let mut out = Map::new();
    let list = |rules: &[Rule]| {
        Value::Array(
            rules
                .iter()
                .filter_map(|r| serde_json::to_value(r).ok())
                .collect(),
        )
    };
    if !rules.forbidden.is_empty() {
        out.insert("forbidden".into(), list(&rules.forbidden));
    }
    if !rules.allowed.is_empty() {
        let mut allowed = list(&rules.allowed);
        if let Value::Array(items) = &mut allowed {
            for item in items {
                if let Value::Object(map) = item {
                    map.remove("name");
                }
            }
        }
        out.insert("allowed".into(), allowed);
    }
    if let Some(severity) = rules.allowed_severity {
        out.insert("allowedSeverity".into(), json!(severity.as_str()));
    }
    if !rules.required.is_empty() {
        out.insert("required".into(), list(&rules.required));
    }
    out
}

/// dependency-cruiser's `SHAREABLE_OPTIONS`: the keys `optionsUsed` may carry.
pub const SHAREABLE_OPTIONS: &[&str] = &[
    "babelConfig",
    "baseDir",
    "cache",
    "collapse",
    "combinedDependencies",
    "detectJSDocImports",
    "detectProcessBuiltinModuleCalls",
    "doNotFollow",
    "enhancedResolveOptions",
    "exclude",
    "exoticallyRequired",
    "exoticRequireStrings",
    "experimentalStats",
    "externalModuleResolutionStrategy",
    "focus",
    "focusDepth",
    "includeOnly",
    "knownViolations",
    "maxDepth",
    "metrics",
    "moduleSystems",
    "outputTo",
    "outputType",
    "parser",
    "prefix",
    "preserveSymlinks",
    "reaches",
    "reporterOptions",
    "rulesFile",
    "skipAnalysisNotInRules",
    "suffix",
    "tsConfig",
    "tsPreCompilationDeps",
    "webpackConfig",
];

/// `summarizeOptions`: the shareable options, `includeOnly` flattened to its path, plus `args`.
pub fn options_used(options: &Map<String, Value>, args: &[String]) -> Map<String, Value> {
    let mut out = Map::new();
    for key in SHAREABLE_OPTIONS {
        let Some(value) = options.get(*key) else {
            continue;
        };
        if value == &json!(0) {
            continue;
        }
        let empty_object = value.as_object().is_some_and(Map::is_empty);
        if (*key == "doNotFollow" || *key == "exclude") && empty_object {
            continue;
        }
        if *key == "knownViolations" && value.as_array().is_some_and(Vec::is_empty) {
            continue;
        }
        let value = if *key == "includeOnly" {
            value.get("path").cloned().unwrap_or_else(|| value.clone())
        } else {
            value.clone()
        };
        out.insert((*key).to_owned(), value);
    }
    out.insert("args".into(), json!(args.join(" ")));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(value: Value) -> DependencyRules {
        let map = match value {
            Value::Object(map) => map,
            _ => serde_json::Map::new(),
        };
        rb_config::normalize::rule_set(&map).unwrap_or_default()
    }

    #[test]
    fn violations_from_dependencies_and_modules() {
        let set = rules(json!({ "forbidden": [
            { "name": "no-circular", "severity": "error", "from": {}, "to": { "circular": true } },
            { "name": "no-orphans", "severity": "warn", "from": { "orphan": true }, "to": {} },
            { "name": "reach", "severity": "info", "from": {}, "to": { "path": "b", "reachable": true } }
        ] }));
        let error = json!({ "name": "no-circular", "severity": "error" });
        let modules = vec![
            json!({ "source": "a", "valid": true, "dependencies": [
                { "module": "./b", "resolved": "b", "dependencyTypes": ["local"], "valid": false, "rules": [error],
                  "cycle": [{ "name": "b" }, { "name": "a" }] }
            ] }),
            json!({ "source": "b", "valid": true, "dependencies": [
                { "module": "./a", "resolved": "a", "dependencyTypes": ["local"], "valid": false, "rules": [error],
                  "cycle": [{ "name": "a" }, { "name": "b" }] }
            ] }),
            json!({ "source": "o", "valid": false, "rules": [{ "name": "no-orphans", "severity": "warn" }], "dependencies": [] }),
            json!({ "source": "r", "valid": false, "rules": [{ "name": "reach", "severity": "info" }], "dependencies": [],
                    "reaches": [{ "asDefinedInRule": "reach", "modules": [{ "source": "b", "via": [{ "name": "b" }] }] }] }),
        ];
        let violations = summarize_modules(&modules, Some(&set));
        let kinds: Vec<&str> = violations
            .iter()
            .filter_map(|v| v["type"].as_str())
            .collect();
        assert_eq!(
            kinds,
            ["cycle", "module", "reachability"],
            "the second cycle is the same violation"
        );
        assert_eq!(violations[0]["unresolvedTo"], "./b");
        assert_eq!(violations[2]["to"], "b");
        let stats = violation_stats(&violations);
        assert_eq!(stats["error"], 1);
        assert_eq!(stats["warn"], 1);
        assert_eq!(stats["info"], 1);
        assert_eq!(stats["ignore"], 0);
        let without_rules = summarize_modules(&modules, None);
        assert_eq!(
            without_rules[0]["type"], "dependency",
            "fmt re-summarises without a rule set"
        );
    }

    #[test]
    fn reaches_of_a_non_reachable_rule_leave_a_module_violation() {
        let set = rules(json!({ "forbidden": [
            { "name": "no-orphans", "severity": "ignore", "from": { "orphan": true }, "to": {} },
            { "name": "reach", "from": {}, "to": { "path": "b", "reachable": true } }
        ] }));
        let modules = vec![json!({
            "source": "o", "valid": false, "dependencies": [],
            "rules": [{ "name": "no-orphans", "severity": "ignore" }],
            "reaches": [{ "asDefinedInRule": "reach", "modules": [{ "source": "b", "via": [] }] }]
        })];
        let violations = summarize_modules(&modules, Some(&set));
        assert_eq!(
            violations,
            [
                json!({ "type": "module", "from": "o", "to": "o", "rule": { "name": "no-orphans", "severity": "ignore" } })
            ]
        );
        let stats = violation_stats(&violations);
        assert_eq!(stats["ignore"], 1);
        assert_eq!(stats["error"], 0);
    }

    #[test]
    fn instability_violations() {
        let set = rules(
            json!({ "forbidden": [{ "name": "sdp", "from": {}, "to": { "moreUnstable": true } }] }),
        );
        let modules = vec![
            json!({ "source": "a", "instability": 0.1, "valid": true, "dependencies": [
            { "resolved": "b", "instability": 0.9, "valid": false, "rules": [{ "name": "sdp", "severity": "warn" }] }
        ] }),
        ];
        let v = summarize_modules(&modules, Some(&set));
        assert_eq!(v[0]["type"], "instability");
        assert_eq!(v[0]["metrics"]["to"]["instability"], 0.9);
        // All three are needed: the module's instability, the dependency's, and a moreUnstable
        // rule.
        let sdp = json!([{ "name": "sdp", "severity": "warn" }]);
        let plain =
            rules(json!({ "forbidden": [{ "name": "sdp", "from": {}, "to": { "path": "b" } }] }));
        let both = vec![
            json!({ "source": "a", "instability": 0.1, "valid": true, "dependencies": [
            { "resolved": "b", "instability": 0.9, "valid": false, "rules": sdp }
        ] }),
        ];
        assert_eq!(
            summarize_modules(&both, Some(&plain))[0]["type"],
            "dependency"
        );
        let module_only = vec![
            json!({ "source": "a", "instability": 0.1, "valid": true, "dependencies": [
            { "resolved": "b", "valid": false, "rules": sdp }
        ] }),
        ];
        assert_eq!(
            summarize_modules(&module_only, Some(&set))[0]["type"],
            "dependency"
        );
        let dependency_only = vec![json!({ "source": "a", "valid": true, "dependencies": [
            { "resolved": "b", "instability": 0.9, "valid": false, "rules": sdp }
        ] })];
        assert_eq!(
            summarize_modules(&dependency_only, Some(&set))[0]["type"],
            "dependency"
        );
    }

    #[test]
    fn folder_violations() {
        let set = rules(json!({ "forbidden": [
            { "name": "c", "scope": "folder", "from": {}, "to": { "circular": true } },
            { "name": "u", "scope": "folder", "from": {}, "to": { "moreUnstable": true } },
            { "name": "f", "scope": "folder", "from": {}, "to": { "path": "x" } }
        ] }));
        let folders = vec![json!({ "name": "a", "instability": 0.1, "dependencies": [
            { "name": "b", "valid": false, "cycle": [{ "name": "b" }, { "name": "a" }], "instability": 0.5, "rules": [{ "name": "c" }, { "name": "u" }, { "name": "f" }] },
            { "name": "z", "valid": true, "rules": [] }
        ] })];
        let v = summarize_folders(&folders, Some(&set));
        let kinds: Vec<&str> = v.iter().filter_map(|x| x["type"].as_str()).collect();
        assert_eq!(kinds, ["cycle", "instability", "folder"]);
        assert_eq!(v[1]["metrics"]["from"]["instability"], 0.1);
        assert_eq!(v[0]["cycle"], json!([{ "name": "b" }, { "name": "a" }]));
        assert!(!js::has(&v[2], "cycle"));
    }

    #[test]
    fn sameness() {
        let r = json!({ "name": "x" });
        let v = |from: &str, to: &str| json!({ "rule": r, "from": from, "to": to });
        assert!(is_same_violation(&v("a", "b"), &v("a", "b")));
        assert!(!is_same_violation(&v("a", "b"), &v("a", "c")));
        let mut other = v("a", "b");
        other["rule"]["name"] = json!("y");
        assert!(!is_same_violation(&v("a", "b"), &other));
        let mut c1 = v("a", "b");
        c1["cycle"] = json!([{ "name": "a" }, { "name": "b" }]);
        let mut c2 = v("b", "a");
        c2["cycle"] = json!([{ "name": "b" }, { "name": "a" }]);
        assert!(is_same_violation(&c1, &c2));
        let mut w1 = v("a", "b");
        w1["via"] = json!([{ "name": "q" }]);
        let mut w2 = v("a", "b");
        w2["via"] = json!([{ "name": "r" }]);
        assert!(!is_same_violation(&w1, &w2));
        // A cycle or via on one side only is not compared: from and to decide.
        assert!(is_same_violation(&c1, &v("a", "b")));
        assert!(is_same_violation(&v("a", "b"), &c1));
        assert!(is_same_violation(&w1, &v("a", "b")));
        assert!(is_same_violation(&v("a", "b"), &w1));
    }

    #[test]
    fn rule_set_and_options_used() {
        let set = rules(
            json!({ "allowed": [{ "from": {}, "to": {} }], "required": [{ "name": "q", "module": {}, "to": {} }], "forbidden": [{ "name": "f" }] }),
        );
        let used = rule_set_used(&set);
        assert!(used["allowed"][0].get("name").is_none());
        assert_eq!(used["allowedSeverity"], "warn");
        assert_eq!(used["required"][0]["name"], "q");
        assert_eq!(used["forbidden"][0]["name"], "f");
        let options = json!({ "maxDepth": 0, "doNotFollow": {}, "exclude": { "path": "x" }, "knownViolations": [],
                              "includeOnly": { "path": "^src" }, "tsPreCompilationDeps": true, "progress": {} });
        let used = options_used(
            options.as_object().unwrap_or(&Map::new()),
            &["src".into(), "lib".into()],
        );
        assert_eq!(
            Value::Object(used),
            json!({ "exclude": { "path": "x" }, "includeOnly": "^src", "tsPreCompilationDeps": true, "args": "src lib" })
        );
        // Only doNotFollow and exclude drop an empty object, only knownViolations an empty array.
        let kept = json!({ "reporterOptions": {}, "moduleSystems": [], "knownViolations": [{ "from": "a" }] });
        let used = options_used(kept.as_object().unwrap_or(&Map::new()), &[]);
        assert_eq!(
            Value::Object(used),
            json!({ "knownViolations": [{ "from": "a" }], "moduleSystems": [], "reporterOptions": {}, "args": "" })
        );
        assert!(find_rule_by_name(None, "f").is_none());
    }
}
