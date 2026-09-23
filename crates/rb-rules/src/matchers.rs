//! The restriction matchers: dependency-cruiser 18.2.0's `src/validate/matchers.mjs`, ported.
//!
//! - Specification: `test/validate/*.spec.mjs`, run unmodified by conformance gate 1 layer 2
//!   ([ADR-0009](../../../docs/adr/0009-conformance-suites-as-specification.md))
//! - Coverage: [coverage § Rules](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#rules)
//! - Decision: [ADR-0016](../../../docs/adr/0016-linear-time-regex-and-strict-compat.md)
//!   (captures substituted escaped)
//! - Plan: [Wave 1, Step 5](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-5-matchers-and-restriction-evaluation-1b)
//! - Requirement: [FR-RULE-01](../../../docs/prd.md#fr-rule-01)
//!
//! Each function keeps its upstream name in snake case and its upstream quirks. Two are worth
//! knowing: in `via` and `viaOnly`, a `pathNot` replaces the result of `path` rather than being
//! combined with it; and `ancestor` compares directories resolved against a working directory,
//! which here is a fixed virtual one deep enough that `../` never runs out of parents.

use rb_config::Rule;
use rb_config::model::ViaRestriction;
use rb_config::pattern::replace_group_placeholders;
use rb_model::DependencyType;
use rb_model::options::Patterns;
use serde_json::Value;

use crate::js;
use crate::patterns;

/// A pattern as dependency-cruiser tests it: joined, and absent when empty (falsy).
pub fn pattern(patterns: Option<&Patterns>) -> Option<String> {
    patterns.map(Patterns::joined).filter(|p| !p.is_empty())
}

/// dependency-cruiser's `DEPENDENCY_TYPE_DUPLICATES_THAT_MATTER`.
pub const DUPLICATES_THAT_MATTER: &[&str] = &[
    "core",
    "local",
    "localmodule",
    "npm",
    "npm-bundled",
    "npm-dev",
    "npm-no-pkg",
    "npm-optional",
    "npm-peer",
    "npm-unknown",
];

/// `propertyEquals(rule, dependency, property)`: when the rule names the property, the
/// dependency's value must be the same boolean.
pub fn property_equals(rule_value: Option<bool>, dependency: &Value, property: &str) -> bool {
    rule_value.is_none_or(|expected| dependency.get(property) == Some(&Value::Bool(expected)))
}

/// `propertyMatches(rule, dependency, ruleProperty, property)`.
pub fn property_matches(rule_value: Option<&Patterns>, dependency: &Value, property: &str) -> bool {
    pattern(rule_value).is_none_or(|p| {
        js::truthy(dependency.get(property)) && patterns::test(&p, &js::text(dependency, property))
    })
}

/// `propertyMatchesNot(rule, dependency, ruleProperty, property)`.
pub fn property_matches_not(
    rule_value: Option<&Patterns>,
    dependency: &Value,
    property: &str,
) -> bool {
    pattern(rule_value).is_none_or(|p| {
        js::truthy(dependency.get(property)) && !patterns::test(&p, &js::text(dependency, property))
    })
}

/// `matchesFromPath`.
pub fn matches_from_path(rule: &Rule, module: &Value) -> bool {
    pattern(rule.from.path.as_ref()).is_none_or(|p| patterns::test(&p, &js::text(module, "source")))
}

/// `matchesFromPathNot`.
pub fn matches_from_path_not(rule: &Rule, module: &Value) -> bool {
    pattern(rule.from.path_not.as_ref())
        .is_none_or(|p| !patterns::test(&p, &js::text(module, "source")))
}

/// `matchesModulePath`.
pub fn matches_module_path(rule: &Rule, module: &Value) -> bool {
    pattern(rule.module.as_ref().and_then(|m| m.path.as_ref()))
        .is_none_or(|p| patterns::test(&p, &js::text(module, "source")))
}

/// `matchesModulePathNot`.
pub fn matches_module_path_not(rule: &Rule, module: &Value) -> bool {
    pattern(rule.module.as_ref().and_then(|m| m.path_not.as_ref()))
        .is_none_or(|p| !patterns::test(&p, &js::text(module, "source")))
}

fn to_path(rule: &Rule, text: &str, groups: &[String]) -> bool {
    pattern(rule.to.path.as_ref())
        .is_none_or(|p| patterns::test(&replace_group_placeholders(&p, groups), text))
}

fn to_path_not(rule: &Rule, text: &str, groups: &[String]) -> bool {
    pattern(rule.to.path_not.as_ref())
        .is_none_or(|p| !patterns::test(&replace_group_placeholders(&p, groups), text))
}

/// `matchesToPath`: `to.path` against the dependency's `resolved`.
pub fn matches_to_path(rule: &Rule, dependency: &Value, groups: &[String]) -> bool {
    to_path(rule, &js::text(dependency, "resolved"), groups)
}

/// `matchToModulePath`: `to.path` against a module's `source`.
pub fn match_to_module_path(rule: &Rule, module: &Value, groups: &[String]) -> bool {
    to_path(rule, &js::text(module, "source"), groups)
}

/// `matchesToPathNot`.
pub fn matches_to_path_not(rule: &Rule, dependency: &Value, groups: &[String]) -> bool {
    to_path_not(rule, &js::text(dependency, "resolved"), groups)
}

/// `matchToModulePathNot`.
pub fn match_to_module_path_not(rule: &Rule, module: &Value, groups: &[String]) -> bool {
    to_path_not(rule, &js::text(module, "source"), groups)
}

/// `intersects(left, right)`.
fn intersects(left: &[&str], right: &[DependencyType]) -> bool {
    left.iter().any(|l| right.iter().any(|r| r.as_str() == *l))
}

/// `matchesToDependencyTypes`.
pub fn matches_to_dependency_types(rule: &Rule, dependency: &Value) -> bool {
    rule.to
        .dependency_types
        .as_ref()
        .is_none_or(|types| intersects(&js::strings(dependency, "dependencyTypes"), types))
}

/// `matchesToDependencyTypesNot`.
pub fn matches_to_dependency_types_not(rule: &Rule, dependency: &Value) -> bool {
    rule.to
        .dependency_types_not
        .as_ref()
        .is_none_or(|types| !intersects(&js::strings(dependency, "dependencyTypes"), types))
}

fn step_has_any(step: &Value, types: &[DependencyType]) -> bool {
    let step_types = js::strings(step, "dependencyTypes");
    types.iter().any(|t| step_types.contains(&t.as_str()))
}

/// A quantifier over the steps of a cycle: `Array.prototype.some` or `every`.
type Quantifier = fn(&[Value], &dyn Fn(&Value) -> bool) -> bool;

fn via_matches(
    via: &ViaRestriction,
    cycle: &[Value],
    groups: &[String],
    some: Quantifier,
    every: Quantifier,
) -> bool {
    let name_matches = |p: &str| {
        let pattern = replace_group_placeholders(p, groups);
        move |step: &Value| patterns::test(&pattern, &js::text(step, "name"))
    };
    let mut result = true;
    if let Some(p) = pattern(via.path.as_ref()) {
        result = some(cycle, &name_matches(&p));
    }
    if let Some(p) = pattern(via.path_not.as_ref()) {
        result = !every(cycle, &name_matches(&p));
    }
    if let Some(types) = &via.dependency_types {
        result = result && some(cycle, &|step| step_has_any(step, types));
    }
    if let Some(types) = &via.dependency_types_not {
        result = result && !every(cycle, &|step| step_has_any(step, types));
    }
    result
}

fn any(items: &[Value], f: &dyn Fn(&Value) -> bool) -> bool {
    items.iter().any(f)
}

fn all(items: &[Value], f: &dyn Fn(&Value) -> bool) -> bool {
    items.iter().all(f)
}

/// `matchesToVia`: some step of the cycle matches (`pathNot`: not every step matches).
pub fn matches_to_via(rule: &Rule, dependency: &Value, groups: &[String]) -> bool {
    match (&rule.to.via, dependency.get("cycle")) {
        (Some(via), Some(Value::Array(cycle))) => via_matches(via, cycle, groups, any, all),
        _ => true,
    }
}

/// `matchesToViaOnly`: every step matches (`pathNot`: no step matches).
pub fn matches_to_via_only(rule: &Rule, dependency: &Value, groups: &[String]) -> bool {
    match (&rule.to.via_only, dependency.get("cycle")) {
        (Some(via), Some(Value::Array(cycle))) => via_matches(via, cycle, groups, all, any),
        _ => true,
    }
}

/// `matchesToIsMoreUnstable`, where a missing instability compares false either way.
pub fn matches_to_is_more_unstable(rule: &Rule, module: &Value, dependency: &Value) -> bool {
    let Some(more_unstable) = rule.to.more_unstable else {
        return true;
    };
    match (
        js::number(module, "instability"),
        js::number(dependency, "instability"),
    ) {
        (Some(from), Some(to)) => {
            if more_unstable {
                from < to
            } else {
                from >= to
            }
        }
        _ => false,
    }
}

/// `matchesMoreThanOneDependencyType`.
pub fn matches_more_than_one_dependency_type(rule: &Rule, dependency: &Value) -> bool {
    rule.to
        .more_than_one_dependency_type
        .is_none_or(|expected| {
            let count = js::strings(dependency, "dependencyTypes")
                .iter()
                .filter(|t| DUPLICATES_THAT_MATTER.contains(t))
                .count();
            expected == (count > 1)
        })
}

/// The virtual working directory `ancestor` resolves against; deep enough that a `../` chain in
/// a real repository never reaches the root, as dependency-cruiser's `process.cwd()` would not.
const VIRTUAL_CWD: &str = "/r/u/l/e/b/e/a/r/i/n/g/w/o/r/k/i/n/g/d/i/r";

/// Node's `path.posix.resolve(cwd, p)` followed by `dirname`, with a trailing `/`.
fn resolved_directory(path: &str) -> String {
    let joined = if path.starts_with('/') {
        path.to_owned()
    } else {
        format!("{VIRTUAL_CWD}/{path}")
    };
    let mut parts: Vec<&str> = Vec::new();
    for part in joined.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.pop();
    let dir = format!("/{}", parts.join("/"));
    format!("{dir}/")
}

/// `matchesAncestor`: the dependency sits in a folder above the module's.
pub fn matches_ancestor(rule: &Rule, module: &Value, dependency: &Value) -> bool {
    let Some(ancestor) = rule.to.ancestor else {
        return true;
    };
    if js::truthy(dependency.get("coreModule")) || js::truthy(dependency.get("couldNotResolve")) {
        return false;
    }
    let module_dir = resolved_directory(&js::text(module, "source"));
    let dependency_dir = resolved_directory(&js::text(dependency, "resolved"));
    let is_ancestor =
        module_dir.starts_with(&dependency_dir) && module_dir.len() > dependency_dir.len();
    is_ancestor == ancestor
}

/// dependency-cruiser's `extractGroups(rule.from, source)`.
pub fn from_groups(rule: &Rule, source: &str) -> Vec<String> {
    pattern(rule.from.path.as_ref()).map_or_else(Vec::new, |p| patterns::groups(&p, source))
}

/// `extractGroups(rule.module, source)`.
pub fn module_groups(rule: &Rule, source: &str) -> Vec<String> {
    pattern(rule.module.as_ref().and_then(|m| m.path.as_ref()))
        .map_or_else(Vec::new, |p| patterns::groups(&p, source))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rule(value: Value) -> Rule {
        serde_json::from_value(value).unwrap_or_default()
    }

    #[test]
    fn ancestor_follows_the_upstream_spec() {
        let r = rule(json!({ "from": {}, "to": { "ancestor": true } }));
        let check = |from: &str, to: &str, extra: Value| {
            let mut dependency = json!({ "resolved": to });
            if let (Value::Object(d), Value::Object(e)) = (&mut dependency, extra) {
                d.extend(e);
            }
            matches_ancestor(&r, &json!({ "source": from }), &dependency)
        };
        assert!(check(
            "src/aap/noot/pinda.mjs",
            "src/aap/chimpansee.mjs",
            json!({})
        ));
        assert!(check("src/brs/thing.mjs", "../../outside.mjs", json!({})));
        assert!(check("intheroot.mjs", "../abovetheroot.mjs", json!({})));
        assert!(check("src/index.mjs", "intheroot.mjs", json!({})));
        assert!(!check("src/index.mjs", "fs", json!({ "coreModule": true })));
        assert!(!check(
            "src/index.mjs",
            "fs",
            json!({ "couldNotResolve": true })
        ));
        assert!(!check(
            "src/aap/chimpansee.ts",
            "src/aap/oerangutan.ts",
            json!({})
        ));
        assert!(!check(
            "src/aap/chimpansee.ts",
            "src/aap/noot/pinda.ts",
            json!({})
        ));
        assert!(!check("aap.ts", "src/aap/chimpansee.ts", json!({})));
        let not = rule(json!({ "from": {}, "to": { "ancestor": false } }));
        assert!(matches_ancestor(
            &not,
            &json!({ "source": "a.ts" }),
            &json!({ "resolved": "b/c.ts" })
        ));
        assert!(matches_ancestor(&rule(json!({})), &json!({}), &json!({})));
        assert_eq!(resolved_directory("/x/y.ts"), "/x/");
    }

    #[test]
    fn property_equality_needs_the_same_boolean() {
        assert!(property_equals(None, &json!({}), "circular"));
        assert!(property_equals(
            Some(true),
            &json!({ "circular": true }),
            "circular"
        ));
        assert!(
            !property_equals(Some(false), &json!({}), "circular"),
            "undefined !== false"
        );
        assert!(!property_equals(
            Some(true),
            &json!({ "circular": "true" }),
            "circular"
        ));
    }

    #[test]
    fn property_patterns_need_a_value() {
        let p = Patterns::One("^MIT$".into());
        assert!(property_matches(None, &json!({}), "license"));
        assert!(property_matches(
            Some(&p),
            &json!({ "license": "MIT" }),
            "license"
        ));
        assert!(!property_matches(Some(&p), &json!({}), "license"));
        assert!(!property_matches(
            Some(&p),
            &json!({ "license": "GPL" }),
            "license"
        ));
        assert!(property_matches_not(
            Some(&p),
            &json!({ "license": "GPL" }),
            "license"
        ));
        assert!(!property_matches_not(Some(&p), &json!({}), "license"));
        assert!(!property_matches_not(
            Some(&p),
            &json!({ "license": "MIT" }),
            "license"
        ));
        assert!(property_matches_not(None, &json!({}), "license"));
        assert_eq!(pattern(Some(&Patterns::Many(vec![]))), None);
    }

    #[test]
    fn paths_with_captures() {
        let r = rule(
            json!({ "from": { "path": "^src/([^/]+)/", "pathNot": "^src/x/" },
                             "to": { "path": "^src/$1/", "pathNot": "^src/$1/private" } }),
        );
        let groups = from_groups(&r, "src/a/b.ts");
        assert_eq!(groups, ["src/a/", "a"]);
        assert!(matches_from_path(&r, &json!({ "source": "src/a/b.ts" })));
        assert!(!matches_from_path(&r, &json!({ "source": "lib/a/b.ts" })));
        assert!(!matches_from_path_not(
            &r,
            &json!({ "source": "src/x/b.ts" })
        ));
        assert!(matches_from_path_not(
            &r,
            &json!({ "source": "src/a/b.ts" })
        ));
        assert!(matches_to_path(
            &r,
            &json!({ "resolved": "src/a/c.ts" }),
            &groups
        ));
        assert!(!matches_to_path(
            &r,
            &json!({ "resolved": "src/b/c.ts" }),
            &groups
        ));
        assert!(!matches_to_path_not(
            &r,
            &json!({ "resolved": "src/a/private.ts" }),
            &groups
        ));
        assert!(matches_to_path_not(
            &r,
            &json!({ "resolved": "src/a/c.ts" }),
            &groups
        ));
        assert!(match_to_module_path(
            &r,
            &json!({ "source": "src/a/c.ts" }),
            &groups
        ));
        assert!(match_to_module_path_not(
            &r,
            &json!({ "source": "src/a/c.ts" }),
            &groups
        ));
        assert!(
            !matches_from_path(&r, &json!({})),
            "a missing source tests as \"undefined\""
        );
    }

    #[test]
    fn module_paths() {
        let r = rule(json!({ "module": { "path": "^m", "pathNot": "^mx" } }));
        assert!(matches_module_path(&r, &json!({ "source": "m.ts" })));
        assert!(!matches_module_path(&r, &json!({ "source": "a.ts" })));
        assert!(!matches_module_path_not(&r, &json!({ "source": "mx.ts" })));
        assert!(matches_module_path_not(&r, &json!({ "source": "m.ts" })));
        assert_eq!(
            module_groups(&rule(json!({ "module": { "path": "^(m)" } })), "m"),
            ["m", "m"]
        );
        assert!(module_groups(&r, "m").is_empty());
        let none = rule(json!({}));
        assert!(matches_module_path(&none, &json!({})));
        assert!(matches_module_path_not(&none, &json!({})));
    }

    #[test]
    fn dependency_types() {
        let r = rule(
            json!({ "to": { "dependencyTypes": ["npm", "core"], "dependencyTypesNot": ["type-only"] } }),
        );
        assert!(matches_to_dependency_types(
            &r,
            &json!({ "dependencyTypes": ["npm"] })
        ));
        assert!(!matches_to_dependency_types(
            &r,
            &json!({ "dependencyTypes": ["local"] })
        ));
        assert!(!matches_to_dependency_types(
            &r,
            &json!({ "dependencyTypes": [] })
        ));
        assert!(matches_to_dependency_types_not(
            &r,
            &json!({ "dependencyTypes": ["npm"] })
        ));
        assert!(!matches_to_dependency_types_not(
            &r,
            &json!({ "dependencyTypes": ["local", "type-only"] })
        ));
        let none = rule(json!({}));
        assert!(matches_to_dependency_types(&none, &json!({})));
        assert!(matches_to_dependency_types_not(&none, &json!({})));
    }

    #[test]
    fn more_than_one_dependency_type_counts_only_the_ones_that_matter() {
        let yes = rule(json!({ "to": { "moreThanOneDependencyType": true } }));
        let no = rule(json!({ "to": { "moreThanOneDependencyType": false } }));
        let twice = json!({ "dependencyTypes": ["npm", "npm-dev"] });
        let aliased = json!({ "dependencyTypes": ["local", "aliased", "type-only"] });
        assert!(matches_more_than_one_dependency_type(&yes, &twice));
        assert!(!matches_more_than_one_dependency_type(&yes, &aliased));
        assert!(matches_more_than_one_dependency_type(&no, &aliased));
        assert!(!matches_more_than_one_dependency_type(&no, &twice));
        assert!(matches_more_than_one_dependency_type(
            &rule(json!({})),
            &twice
        ));
    }

    #[test]
    fn instability() {
        let more = rule(json!({ "to": { "moreUnstable": true } }));
        let less = rule(json!({ "to": { "moreUnstable": false } }));
        let m = json!({ "instability": 0.2 });
        let d = json!({ "instability": 0.8 });
        assert!(matches_to_is_more_unstable(&more, &m, &d));
        assert!(!matches_to_is_more_unstable(&more, &d, &m));
        assert!(matches_to_is_more_unstable(&less, &d, &m));
        assert!(matches_to_is_more_unstable(&less, &m, &m));
        assert!(!matches_to_is_more_unstable(&less, &m, &d));
        assert!(!matches_to_is_more_unstable(&more, &json!({}), &d));
        assert!(matches_to_is_more_unstable(
            &rule(json!({})),
            &json!({}),
            &json!({})
        ));
    }

    #[test]
    fn via_and_via_only() {
        let cycle = json!({ "cycle": [
            { "name": "a.ts", "dependencyTypes": ["import"] },
            { "name": "b.ts", "dependencyTypes": ["import", "type-only"] }
        ] });
        let via = |v: Value| rule(json!({ "to": { "via": v } }));
        let only = |v: Value| rule(json!({ "to": { "viaOnly": v } }));
        assert!(matches_to_via(&via(json!("^b")), &cycle, &[]));
        assert!(!matches_to_via(&via(json!("^c")), &cycle, &[]));
        assert!(matches_to_via(
            &via(json!({ "pathNot": "^a" })),
            &cycle,
            &[]
        ));
        assert!(!matches_to_via(
            &via(json!({ "pathNot": "ts$" })),
            &cycle,
            &[]
        ));
        assert!(matches_to_via(
            &via(json!({ "dependencyTypes": ["type-only"] })),
            &cycle,
            &[]
        ));
        assert!(!matches_to_via(
            &via(json!({ "dependencyTypes": ["require"] })),
            &cycle,
            &[]
        ));
        assert!(matches_to_via(
            &via(json!({ "dependencyTypesNot": ["type-only"] })),
            &cycle,
            &[]
        ));
        assert!(!matches_to_via(
            &via(json!({ "dependencyTypesNot": ["import"] })),
            &cycle,
            &[]
        ));
        assert!(
            matches_to_via(&via(json!("^c")), &json!({}), &[]),
            "no cycle, no restriction"
        );
        assert!(matches_to_via_only(&only(json!("ts$")), &cycle, &[]));
        assert!(!matches_to_via_only(&only(json!("^a")), &cycle, &[]));
        assert!(matches_to_via_only(
            &only(json!({ "pathNot": "^c" })),
            &cycle,
            &[]
        ));
        assert!(!matches_to_via_only(
            &only(json!({ "pathNot": "^a" })),
            &cycle,
            &[]
        ));
        assert!(matches_to_via_only(
            &only(json!({ "dependencyTypes": ["import"] })),
            &cycle,
            &[]
        ));
        assert!(!matches_to_via_only(
            &only(json!({ "dependencyTypes": ["type-only"] })),
            &cycle,
            &[]
        ));
        assert!(matches_to_via_only(
            &only(json!({ "dependencyTypesNot": ["require"] })),
            &cycle,
            &[]
        ));
        assert!(!matches_to_via_only(
            &only(json!({ "dependencyTypesNot": ["type-only"] })),
            &cycle,
            &[]
        ));
        assert!(matches_to_via_only(&only(json!("^a")), &json!({}), &[]));
        // pathNot replaces the result of path, as upstream does.
        assert!(matches_to_via(
            &via(json!({ "path": "^c", "pathNot": "^a" })),
            &cycle,
            &[]
        ));
        let captured = rule(json!({ "to": { "via": "^$1" } }));
        assert!(matches_to_via(&captured, &cycle, &["x".into(), "b".into()]));
    }
}
