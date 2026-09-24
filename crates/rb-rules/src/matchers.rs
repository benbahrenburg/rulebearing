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
//!
//! **Cross-language keys** ([Wave 2, Step 8](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#28-step-8-cross-language-rule-additions-per-language-dependencytypes-license-moreunstable-2d),
//! [FR-RULE-02](../../../docs/prd.md#fr-rule-02),
//! [design § Dependency rules](../../../docs/artifacts/design.md#dependency-rules-the-whole-of-dependency-cruiser-1820))
//! have no upstream. [`ModuleFacts`] indexes what they read about each module, from the document
//! alone: `language`, `namespaces[]` and `project` as the extractor wrote them, and the
//! assemblies of the types the code layer declares in the file. [`matches_cross_language`]
//! answers `language`, `namespace(Not)`, `project(Not)` and `assembly(Not)` for one side;
//! [`matches_dependency_kind`] answers `dependencyKind(Not)` over the edge. Every comparison is
//! a string the document carries; no language is special here
//! ([ADR-0010](../../../docs/adr/0010-crate-layout-and-extractor-boundary.md)).

use std::collections::{BTreeMap, BTreeSet};

use rb_config::Rule;
use rb_config::model::{CrossLanguageKeys, ToRestriction, ViaRestriction};
use rb_config::pattern::replace_group_placeholders;
use rb_model::options::Patterns;
use rb_model::{CodeLayer, DependencyType, Module};
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

/// What the cross-language keys read about one module.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Facts {
    /// `language`, as the document spells it.
    pub language: Option<String>,
    /// `namespaces[]`, absent when the module carries none.
    pub namespaces: Option<Vec<String>>,
    /// `project`.
    pub project: Option<String>,
    /// The distinct `assembly` of every code-layer type declared in the module's file, sorted;
    /// empty when the code layer places no type there.
    pub assemblies: Vec<String>,
}

/// [`Facts`] for every module of a document, by `source`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModuleFacts {
    by_source: BTreeMap<String, Facts>,
}

impl ModuleFacts {
    /// Indexes the modules, joining the code layer's types to the files that declare them (a
    /// partial type's `files[]` included).
    pub fn new(modules: &[Module], code: Option<&CodeLayer>) -> Self {
        let mut assemblies: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        for ty in code.map(|c| c.types.as_slice()).unwrap_or_default() {
            let Some(assembly) = ty.assembly.as_deref() else {
                continue;
            };
            for file in ty.location.file.iter().chain(&ty.files) {
                assemblies.entry(file).or_default().insert(assembly);
            }
        }
        let by_source = modules
            .iter()
            .map(|m| {
                let facts = Facts {
                    language: m.language.map(|l| l.as_str().to_owned()),
                    namespaces: m.namespaces.clone(),
                    project: m.project.clone(),
                    assemblies: assemblies
                        .get(m.source.as_str())
                        .map(|a| a.iter().map(|s| (*s).to_owned()).collect())
                        .unwrap_or_default(),
                };
                (m.source.clone(), facts)
            })
            .collect();
        Self { by_source }
    }

    /// Adds or replaces one module's facts.
    pub fn insert(&mut self, source: impl Into<String>, facts: Facts) {
        self.by_source.insert(source.into(), facts);
    }

    /// The facts of the module at `source`.
    pub fn get(&self, source: &str) -> Option<&Facts> {
        self.by_source.get(source)
    }
}

/// `key` against one optional string: the value must be present and match.
fn one_matches(p: Option<&Patterns>, value: Option<&str>) -> bool {
    pattern(p).is_none_or(|p| value.is_some_and(|v| patterns::test(&p, v)))
}

/// `keyNot` against one optional string: the value must be present and not match.
fn one_matches_not(p: Option<&Patterns>, value: Option<&str>) -> bool {
    pattern(p).is_none_or(|p| value.is_some_and(|v| !patterns::test(&p, v)))
}

/// `key` against a list: the list must be present and one entry match.
fn any_matches(p: Option<&Patterns>, values: Option<&[String]>) -> bool {
    pattern(p).is_none_or(|p| values.is_some_and(|v| v.iter().any(|x| patterns::test(&p, x))))
}

/// `keyNot` against a list: the list must be present and no entry match.
fn none_matches(p: Option<&Patterns>, values: Option<&[String]>) -> bool {
    pattern(p).is_none_or(|p| values.is_some_and(|v| !v.iter().any(|x| patterns::test(&p, x))))
}

/// `language`, `namespace(Not)`, `project(Not)` and `assembly(Not)` of one side against the
/// module's facts. A module with no facts, or without the property a key reads, matches neither
/// the key nor its `Not` form.
pub fn matches_cross_language(keys: &CrossLanguageKeys, facts: Option<&Facts>) -> bool {
    let language = facts.and_then(|f| f.language.as_deref());
    let namespaces = facts.and_then(|f| f.namespaces.as_deref());
    let project = facts.and_then(|f| f.project.as_deref());
    let assemblies = facts
        .map(|f| f.assemblies.as_slice())
        .filter(|a| !a.is_empty());
    keys.language.as_ref().is_none_or(|wanted| {
        language.is_some_and(|l| wanted.as_slice().iter().any(|w| w.as_str() == l))
    }) && any_matches(keys.namespace.as_ref(), namespaces)
        && none_matches(keys.namespace_not.as_ref(), namespaces)
        && one_matches(keys.project.as_ref(), project)
        && one_matches_not(keys.project_not.as_ref(), project)
        && any_matches(keys.assembly.as_ref(), assemblies)
        && none_matches(keys.assembly_not.as_ref(), assemblies)
}

/// `to.dependencyKind` and `to.dependencyKindNot` against the edge's `dependencyKind`; an edge
/// without one matches neither.
pub fn matches_dependency_kind(to: &ToRestriction, kind: Option<&str>) -> bool {
    to.dependency_kind.as_ref().is_none_or(|wanted| {
        kind.is_some_and(|k| wanted.as_slice().iter().any(|w| w.as_str() == k))
    }) && to.dependency_kind_not.as_ref().is_none_or(|unwanted| {
        kind.is_some_and(|k| !unwanted.as_slice().iter().any(|w| w.as_str() == k))
    })
}

/// The cross-language keys of `from` against the module.
pub fn matches_from_cross_language(rule: &Rule, module: &Value, facts: &ModuleFacts) -> bool {
    matches_cross_language(&rule.from.cross, facts.get(&js::text(module, "source")))
}

/// The cross-language keys of `to` against the dependency's target module and the edge.
pub fn matches_to_cross_language(rule: &Rule, dependency: &Value, facts: &ModuleFacts) -> bool {
    matches_dependency_kind(&rule.to, js::str_of(dependency, "dependencyKind"))
        && matches_cross_language(&rule.to.cross, facts.get(&js::text(dependency, "resolved")))
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
        assert!(!match_to_module_path_not(
            &r,
            &json!({ "source": "src/a/private.ts" }),
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
        assert!(
            !matches_to_is_more_unstable(&more, &m, &m),
            "equally unstable is not more unstable"
        );
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

    fn dotnet_facts() -> Facts {
        Facts {
            language: Some("dotnet".into()),
            namespaces: Some(vec!["App.Web".into(), "App.Web.Controllers".into()]),
            project: Some("src/Web/Web.csproj".into()),
            assemblies: vec!["App.Web".into()],
        }
    }

    fn keys(value: Value) -> CrossLanguageKeys {
        serde_json::from_value(value).unwrap_or_default()
    }

    #[test]
    fn each_cross_language_key_is_a_table() {
        let web = dotnet_facts();
        let bare = Facts::default();
        let empty_namespaces = Facts {
            namespaces: Some(Vec::new()),
            ..Facts::default()
        };
        // (keys, facts, expected): each key and its Not form, matching, not matching, and
        // against a module that does not carry the property.
        let cases = [
            (json!({}), Some(&bare), true),
            (json!({}), None, true),
            (json!({ "language": "dotnet" }), Some(&web), true),
            (
                json!({ "language": ["python", "dotnet"] }),
                Some(&web),
                true,
            ),
            (json!({ "language": "python" }), Some(&web), false),
            (json!({ "language": "dotnet" }), Some(&bare), false),
            (json!({ "language": "dotnet" }), None, false),
            (json!({ "namespace": "^App\\.Web$" }), Some(&web), true),
            (json!({ "namespace": "Controllers$" }), Some(&web), true),
            (json!({ "namespace": "^App\\.Domain" }), Some(&web), false),
            (json!({ "namespace": "." }), Some(&bare), false),
            (json!({ "namespace": "." }), Some(&empty_namespaces), false),
            (json!({ "namespaceNot": "^App\\.Domain" }), Some(&web), true),
            (json!({ "namespaceNot": "Controllers$" }), Some(&web), false),
            (json!({ "namespaceNot": "." }), Some(&bare), false),
            (
                json!({ "namespaceNot": "." }),
                Some(&empty_namespaces),
                true,
            ),
            (json!({ "project": "Web\\.csproj$" }), Some(&web), true),
            (json!({ "project": "Domain" }), Some(&web), false),
            (json!({ "project": "." }), Some(&bare), false),
            (json!({ "projectNot": "Domain" }), Some(&web), true),
            (json!({ "projectNot": "^src/Web/" }), Some(&web), false),
            (json!({ "projectNot": "." }), Some(&bare), false),
            (json!({ "assembly": "^App\\.Web$" }), Some(&web), true),
            (json!({ "assembly": "^Web$" }), Some(&web), false),
            (json!({ "assembly": "." }), Some(&bare), false),
            (json!({ "assemblyNot": "^App\\.Domain$" }), Some(&web), true),
            (json!({ "assemblyNot": "Web" }), Some(&web), false),
            (json!({ "assemblyNot": "." }), Some(&bare), false),
            (
                json!({ "language": "dotnet", "namespace": "Web", "project": "Web", "assembly": "Web" }),
                Some(&web),
                true,
            ),
            (
                json!({ "language": "dotnet", "namespace": "Web", "project": "Web", "assembly": "Domain" }),
                Some(&web),
                false,
            ),
        ];
        for (value, facts, expected) in cases {
            assert_eq!(
                matches_cross_language(&keys(value.clone()), facts),
                expected,
                "{value} against {facts:?}"
            );
        }
    }

    #[test]
    fn dependency_kind_is_a_table() {
        let to =
            |value: Value| -> ToRestriction { serde_json::from_value(value).unwrap_or_default() };
        let cases = [
            (json!({}), None, true),
            (
                json!({ "dependencyKind": "inherits" }),
                Some("inherits"),
                true,
            ),
            (
                json!({ "dependencyKind": ["inherits", "implements"] }),
                Some("implements"),
                true,
            ),
            (json!({ "dependencyKind": "inherits" }), Some("body"), false),
            (json!({ "dependencyKind": "inherits" }), None, false),
            (json!({ "dependencyKindNot": "body" }), Some("field"), true),
            (
                json!({ "dependencyKindNot": ["body", "field"] }),
                Some("field"),
                false,
            ),
            (json!({ "dependencyKindNot": "body" }), None, false),
            (
                json!({ "dependencyKind": "import", "dependencyKindNot": "call" }),
                Some("import"),
                true,
            ),
        ];
        for (value, kind, expected) in cases {
            assert_eq!(
                matches_dependency_kind(&to(value.clone()), kind),
                expected,
                "{value} against {kind:?}"
            );
        }
    }

    #[test]
    fn module_facts_join_the_code_layer() -> Result<(), serde_json::Error> {
        let modules: Vec<Module> = serde_json::from_value(json!([
            { "source": "src/Web/Home.cs", "language": "dotnet", "project": "src/Web/Web.csproj", "namespaces": ["App.Web"], "dependencies": [], "valid": true },
            { "source": "src/Web/Part.cs", "language": "dotnet", "dependencies": [], "valid": true },
            { "source": "web/a.ts", "language": "typescript", "dependencies": [], "valid": true },
            { "source": "System.Runtime", "dependencies": [], "valid": true }
        ]))?;
        let code: CodeLayer = serde_json::from_value(json!({ "types": [
            { "fullName": "App.Web.Home", "name": "Home", "kind": "class", "language": "dotnet", "file": "src/Web/Home.cs", "assembly": "App.Web", "files": ["src/Web/Part.cs"] },
            { "fullName": "App.Web.Other", "name": "Other", "kind": "class", "language": "dotnet", "file": "src/Web/Home.cs", "assembly": "App.Web.Views" },
            { "fullName": "System.Object", "name": "Object", "kind": "unavailable", "language": "dotnet", "referenced": true }
        ] }))?;
        let facts = ModuleFacts::new(&modules, Some(&code));
        assert_eq!(
            facts.get("src/Web/Home.cs"),
            Some(&Facts {
                language: Some("dotnet".into()),
                namespaces: Some(vec!["App.Web".into()]),
                project: Some("src/Web/Web.csproj".into()),
                assemblies: vec!["App.Web".into(), "App.Web.Views".into()],
            })
        );
        assert_eq!(
            facts.get("src/Web/Part.cs").map(|f| f.assemblies.clone()),
            Some(vec!["App.Web".to_owned()]),
            "a partial type's other file declares it too"
        );
        assert_eq!(
            facts.get("web/a.ts").and_then(|f| f.language.as_deref()),
            Some("typescript")
        );
        assert_eq!(facts.get("System.Runtime"), Some(&Facts::default()));
        assert_eq!(facts.get("nowhere"), None);
        let without_code = ModuleFacts::new(&modules, None);
        assert!(
            without_code
                .get("src/Web/Home.cs")
                .is_some_and(|f| f.assemblies.is_empty())
        );
        let mut added = ModuleFacts::default();
        added.insert("x", dotnet_facts());
        assert_eq!(added.get("x"), Some(&dotnet_facts()));
        Ok(())
    }

    #[test]
    fn each_side_reads_its_own_module() {
        let r = rule(json!({
            "from": { "language": "dotnet", "namespace": "^App\\.Web" },
            "to": { "assembly": "^App\\.Infrastructure$", "dependencyKind": ["inherits", "implements"] }
        }));
        let mut facts = ModuleFacts::default();
        facts.insert("Web/Home.cs", dotnet_facts());
        facts.insert(
            "Infra/Repo.cs",
            Facts {
                assemblies: vec!["App.Infrastructure".into()],
                ..Facts::default()
            },
        );
        let from = json!({ "source": "Web/Home.cs" });
        let edge = |to: &str, kind: &str| json!({ "resolved": to, "dependencyKind": kind });
        assert!(matches_from_cross_language(&r, &from, &facts));
        assert!(!matches_from_cross_language(
            &r,
            &json!({ "source": "Infra/Repo.cs" }),
            &facts
        ));
        assert!(matches_to_cross_language(
            &r,
            &edge("Infra/Repo.cs", "inherits"),
            &facts
        ));
        assert!(!matches_to_cross_language(
            &r,
            &edge("Infra/Repo.cs", "body"),
            &facts
        ));
        assert!(!matches_to_cross_language(
            &r,
            &edge("Web/Home.cs", "inherits"),
            &facts
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
