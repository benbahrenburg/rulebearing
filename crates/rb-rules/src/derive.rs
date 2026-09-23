//! The derivations the rules read: cycles, dependents, orphans, reachability and instability.
//! dependency-cruiser 18.2.0's `src/analyze/derive`, ported.
//!
//! - Plan: [Wave 1, Step 6](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-6-graph-analysis-1b)
//!   (`tarjan.rs`, `reach.rs`, `dependents.rs`, `orphan.rs` in the plan's layout)
//! - Coverage: [coverage § Rules](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#rules)
//!   (`to.circular`, `to.reachable`, `from.orphan`, `module.numberOfDependents*`),
//!   [coverage § Options](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#options)
//!   (`skipAnalysisNotInRules`, `forceDeriveDependents`)
//! - Requirement: [FR-RULE-08](../../../docs/prd.md#fr-rule-08)
//!
//! With `skipAnalysisNotInRules`, a derivation no rule reads is skipped, exactly where upstream
//! skips it; `forceDeriveDependents` derives `dependents[]` regardless.

use rb_config::Rule;
use rb_config::model::DependencyRules;
use serde_json::{Value, json};

use crate::graph::indexed::{DependencySet, IndexedGraph};
use crate::js;
use crate::matchers::{match_to_module_path, match_to_module_path_not, pattern};
use crate::patterns;

fn any_rule(rules: &DependencyRules, f: impl Fn(&Rule) -> bool) -> bool {
    rules.forbidden.iter().chain(&rules.allowed).any(f)
}

/// `hasCycleRule`.
pub fn has_cycle_rule(rules: &DependencyRules) -> bool {
    any_rule(rules, |r| r.to.circular.is_some())
}

/// `hasDependentsRule`.
pub fn has_dependents_rule(rules: &DependencyRules) -> bool {
    any_rule(rules, |r| {
        r.module.as_ref().is_some_and(|m| {
            m.number_of_dependents_less_than.is_some() || m.number_of_dependents_more_than.is_some()
        })
    })
}

/// `ruleSetHasLicenseRule`: whether extraction must read each npm package's licence
/// (upstream's `resolveLicenses`), because a `forbidden` or `allowed` rule restricts `to.license`
/// or `to.licenseNot`.
pub fn has_license_rule(rules: &DependencyRules) -> bool {
    any_rule(rules, |r| {
        r.to.license.is_some() || r.to.license_not.is_some()
    })
}

/// `ruleSetHasDeprecationRule`: whether extraction must mark deprecated npm packages (upstream's
/// `resolveDeprecations`), because a `forbidden` or `allowed` rule names the `deprecated`
/// dependency type in `to.dependencyTypes`.
pub fn has_deprecation_rule(rules: &DependencyRules) -> bool {
    any_rule(rules, |r| {
        r.to.dependency_types
            .as_ref()
            .is_some_and(|types| types.contains(&rb_model::DependencyType::Deprecated))
    })
}

/// `hasOrphanRule`.
pub fn has_orphan_rule(rules: &DependencyRules) -> bool {
    any_rule(rules, |r| r.from.orphan.is_some())
}

/// `detectAndAddCycles` over modules (`source`, `resolved`) or folders (`name`, `name`).
pub fn cycles(
    items: &mut [Value],
    attribute: &str,
    dependency_name: &str,
    skip: bool,
    rules: &DependencyRules,
) {
    let analyse = !skip || has_cycle_rule(rules);
    let graph = analyse.then(|| IndexedGraph::new(items, attribute));
    for item in items.iter_mut() {
        let from = js::text(item, attribute).into_owned();
        if let Some(Value::Array(dependencies)) = item.get_mut("dependencies") {
            for dependency in dependencies {
                js::set(dependency, "circular", Value::Bool(false));
                if let Some(graph) = &graph {
                    let to = js::text(dependency, dependency_name).into_owned();
                    let cycle = graph.cycle(&from, &to);
                    if !cycle.is_empty() {
                        js::set(dependency, "circular", Value::Bool(true));
                        js::set(dependency, "cycle", Value::Array(cycle));
                    }
                }
            }
        }
    }
}

/// What `addDependents` needs to decide whether to run.
#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag mirrors one independent dependency-cruiser option that addDependents reads"
)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DependentsWhen {
    /// `skipAnalysisNotInRules`.
    pub skip: bool,
    /// Whether metrics are computed.
    pub metrics: bool,
    /// Whether a `reaches` filter is set.
    pub reaches: bool,
    /// Whether a `focus` filter is set.
    pub focus: bool,
    /// `forceDeriveDependents`.
    pub force: bool,
}

/// `addDependents`.
pub fn dependents(modules: &mut [Value], when: DependentsWhen, rules: &DependencyRules) {
    if !when.skip
        || when.force
        || when.metrics
        || when.reaches
        || when.focus
        || has_dependents_rule(rules)
    {
        let set = DependencySet::new(modules);
        for module in modules.iter_mut() {
            let dependents = set.dependents(module);
            js::set(
                module,
                "dependents",
                Value::Array(dependents.into_iter().map(Value::String).collect()),
            );
        }
    }
}

/// `deriveOrphans`.
pub fn orphans(modules: &mut [Value], skip: bool, rules: &DependencyRules) {
    if skip && !has_orphan_rule(rules) {
        return;
    }
    let set = DependencySet::new(modules);
    for module in modules.iter_mut() {
        let orphan = if !js::array(module, "dependencies").is_empty() {
            false
        } else if js::has(module, "dependents") {
            js::array(module, "dependents").is_empty()
        } else {
            !set.has_dependents(module)
        };
        js::set(module, "orphan", Value::Bool(orphan));
    }
}

fn reachable_rules(rules: &DependencyRules) -> Vec<&Rule> {
    rules
        .forbidden
        .iter()
        .chain(&rules.allowed)
        .chain(&rules.required)
        .filter(|r| r.to.reachable.is_some())
        .collect()
}

/// The selecting side of a reachability rule: `from`, or `module` for a required rule
/// (`pRule.from ?? pRule.module`). With neither, both branches give no patterns.
fn selecting(rule: &Rule) -> (Option<String>, Option<String>) {
    if rule.from == rb_config::model::FromRestriction::default() {
        let m = rule.module.as_ref();
        (
            pattern(m.and_then(|m| m.path.as_ref())),
            pattern(m.and_then(|m| m.path_not.as_ref())),
        )
    } else {
        (
            pattern(rule.from.path.as_ref()),
            pattern(rule.from.path_not.as_ref()),
        )
    }
}

/// `isModuleInRuleFrom`.
fn in_rule_from(rule: &Rule, module: &Value) -> bool {
    let (path, path_not) = selecting(rule);
    let source = js::text(module, "source");
    path.is_none_or(|p| patterns::test(&p, &source))
        && path_not.is_none_or(|p| !patterns::test(&p, &source))
}

/// `extractGroups(pRule.from ?? pRule.module, source)`.
fn groups_for(rule: &Rule, from_source: &str) -> Vec<String> {
    let (path, _) = selecting(rule);
    path.map_or_else(Vec::new, |p| patterns::groups(&p, from_source))
}

/// `isModuleInRuleTo`.
fn in_rule_to(rule: &Rule, to: &Value, from: Option<&Value>) -> bool {
    let groups = from.map_or_else(Vec::new, |f| groups_for(rule, &js::text(f, "source")));
    match_to_module_path(rule, to, &groups) && match_to_module_path_not(rule, to, &groups)
}

fn has_capturing_groups(rule: &Rule) -> bool {
    let has = |p: Option<String>| {
        p.is_some_and(|p| {
            p.as_bytes()
                .windows(2)
                .any(|w| w[0] == b'$' && w[1].is_ascii_digit())
        })
    };
    has(pattern(rule.to.path.as_ref())) || has(pattern(rule.to.path_not.as_ref()))
}

fn should_add_reaches(rule: &Rule, module: &Value) -> bool {
    (rule.to.reachable == Some(true) || rule.name() == "not-in-allowed")
        && in_rule_from(rule, module)
}

fn should_add_reachable(rule: &Rule, module: &Value, graph: &[Value]) -> bool {
    if !(rule.to.reachable == Some(false)
        || rule.name() == "not-in-allowed"
        || rule.module.is_some())
    {
        return false;
    }
    if has_capturing_groups(rule) {
        graph
            .iter()
            .filter(|f| in_rule_from(rule, f))
            .any(|f| in_rule_to(rule, module, Some(f)))
    } else {
        in_rule_to(rule, module, None)
    }
}

fn merge_reaches(module: &mut Value, rule: &Rule, to_source: &str, path: &[Value]) {
    let entry = json!({ "source": to_source, "via": path });
    if let Some(Value::Array(reaches)) = module.get_mut("reaches")
        && let Some(existing) = reaches
            .iter_mut()
            .find(|r| js::str_of(r, "asDefinedInRule") == Some(rule.name()))
    {
        if let Some(Value::Array(modules)) = existing.get_mut("modules") {
            modules.push(entry);
        } else {
            js::set(existing, "modules", json!([entry]));
        }
        return;
    }
    let record = json!({ "asDefinedInRule": rule.name(), "modules": [entry] });
    match module.get_mut("reaches") {
        Some(Value::Array(reaches)) => reaches.push(record),
        _ => js::set(module, "reaches", json!([record])),
    }
}

fn merge_reachable(module: &mut Value, rule: &Rule, reachable: bool, from: &str) {
    if let Some(Value::Array(records)) = module.get_mut("reachable")
        && let Some(existing) = records
            .iter_mut()
            .find(|r| js::str_of(r, "asDefinedInRule") == Some(rule.name()))
    {
        let value = js::truthy(existing.get("value")) || reachable;
        js::set(existing, "value", Value::Bool(value));
        return;
    }
    let record = json!({ "value": reachable, "asDefinedInRule": rule.name(), "matchedFrom": from });
    match module.get_mut("reachable") {
        Some(Value::Array(records)) => records.push(record),
        _ => js::set(module, "reachable", json!([record])),
    }
}

/// `deriveReachables`: `reaches[]` for rules that ask what a module reaches, `reachable[]` for
/// rules that ask whether it is reached.
pub fn reachables(modules: &mut [Value], rules: &DependencyRules) {
    let rules = reachable_rules(rules);
    if rules.is_empty() {
        return;
    }
    let graph = IndexedGraph::new(modules, "source");
    for rule in rules {
        // Only `source` is read from the other modules, so the snapshot carries only that.
        let snapshot: Vec<Value> = modules
            .iter()
            .map(|m| json!({ "source": js::text(m, "source") }))
            .collect();
        let from_modules: Vec<&Value> = snapshot.iter().filter(|m| in_rule_from(rule, m)).collect();
        for (index, module) in modules.iter_mut().enumerate() {
            if should_add_reaches(rule, module) {
                let source = js::text(module, "source").into_owned();
                for to in &snapshot {
                    let to_source = js::text(to, "source");
                    if source != to_source && in_rule_to(rule, to, Some(&snapshot[index])) {
                        let path = graph.path(&source, &to_source);
                        if !path.is_empty() {
                            merge_reaches(module, rule, &to_source, &path);
                        }
                    }
                }
            }
            if should_add_reachable(rule, module, &snapshot) {
                let source = js::text(module, "source").into_owned();
                let mut found = false;
                for from in &from_modules {
                    let from_source = js::text(from, "source");
                    if !found && source != from_source && in_rule_to(rule, module, Some(from)) {
                        let path = graph.path(&from_source, &source);
                        found = !path.is_empty();
                        merge_reachable(module, rule, found, &from_source);
                    }
                }
            }
        }
    }
}

/// `metricsAreCalculable`.
pub fn metrics_are_calculable(module: &Value) -> bool {
    !js::truthy(module.get("coreModule"))
        && !js::truthy(module.get("couldNotResolve"))
        && !js::truthy(module.get("matchesDoNotFollow"))
}

/// `calculateInstability`: efferent over total, 0 when there are none.
pub fn instability(efferent: usize, afferent: usize) -> f64 {
    let total = efferent + afferent;
    if total == 0 {
        0.0
    } else {
        #[expect(
            clippy::cast_precision_loss,
            reason = "coupling counts are far below 2^52"
        )]
        let (e, t) = (efferent as f64, total as f64);
        e / t
    }
}

/// `deriveModulesMetrics`: `instability` on each module and, denormalised, on each dependency.
pub fn module_metrics(modules: &mut [Value]) {
    for module in modules.iter_mut() {
        if metrics_are_calculable(module) {
            let value = instability(
                js::array(module, "dependencies").len(),
                js::array(module, "dependents").len(),
            );
            js::set(module, "instability", json!(value));
        }
    }
    let by_source: std::collections::HashMap<String, f64> = modules
        .iter()
        .map(|m| {
            (
                js::text(m, "source").into_owned(),
                js::number(m, "instability").unwrap_or(0.0),
            )
        })
        .collect();
    for module in modules.iter_mut() {
        if let Some(Value::Array(dependencies)) = module.get_mut("dependencies") {
            for dependency in dependencies {
                let value = by_source
                    .get(js::text(dependency, "resolved").as_ref())
                    .copied()
                    .unwrap_or(0.0);
                js::set(dependency, "instability", json!(value));
            }
        }
    }
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

    fn graph() -> Vec<Value> {
        let edge = |to: &str| json!({ "resolved": to, "dependencyTypes": ["local"] });
        vec![
            json!({ "source": "src/a.ts", "dependencies": [edge("src/b.ts")] }),
            json!({ "source": "src/b.ts", "dependencies": [edge("src/a.ts"), edge("lib/c.ts")] }),
            json!({ "source": "lib/c.ts", "dependencies": [] }),
            json!({ "source": "lonely.ts", "dependencies": [] }),
            json!({ "source": "fs", "dependencies": [], "coreModule": true }),
        ]
    }

    #[test]
    fn licence_and_deprecation_rules_are_found_in_forbidden_and_allowed() {
        let none =
            rules(json!({ "forbidden": [{ "from": {}, "to": { "dependencyTypes": ["npm"] } }] }));
        assert!(!has_license_rule(&none));
        assert!(!has_deprecation_rule(&none));
        for to in [json!({ "license": "GPL" }), json!({ "licenseNot": "MIT" })] {
            let forbidden = rules(json!({ "forbidden": [{ "from": {}, "to": to.clone() }] }));
            assert!(has_license_rule(&forbidden), "{to}");
            assert!(!has_deprecation_rule(&forbidden), "{to}");
            let allowed = rules(json!({ "allowed": [{ "from": {}, "to": to.clone() }] }));
            assert!(has_license_rule(&allowed), "{to}");
        }
        let deprecated = json!({ "dependencyTypes": ["npm", "deprecated"] });
        let forbidden = rules(json!({ "forbidden": [{ "from": {}, "to": deprecated.clone() }] }));
        assert!(has_deprecation_rule(&forbidden));
        assert!(!has_license_rule(&forbidden));
        let allowed = rules(json!({ "allowed": [{ "from": {}, "to": deprecated }] }));
        assert!(has_deprecation_rule(&allowed));
    }

    #[test]
    fn cycles_are_marked_or_skipped() {
        let mut modules = graph();
        cycles(&mut modules, "source", "resolved", false, &rules(json!({})));
        assert_eq!(modules[0]["dependencies"][0]["circular"], true);
        assert_eq!(
            modules[0]["dependencies"][0]["cycle"]
                .as_array()
                .map(Vec::len),
            Some(2)
        );
        assert_eq!(modules[1]["dependencies"][1]["circular"], false);
        let mut skipped = graph();
        cycles(&mut skipped, "source", "resolved", true, &rules(json!({})));
        assert_eq!(skipped[0]["dependencies"][0]["circular"], false);
        let with_rule = rules(json!({ "forbidden": [{ "from": {}, "to": { "circular": true } }] }));
        assert!(has_cycle_rule(&with_rule));
        let mut kept = graph();
        cycles(&mut kept, "source", "resolved", true, &with_rule);
        assert_eq!(kept[0]["dependencies"][0]["circular"], true);
    }

    #[test]
    fn dependents_and_orphans() {
        let none = rules(json!({}));
        let mut modules = graph();
        dependents(&mut modules, DependentsWhen::default(), &none);
        assert_eq!(modules[2]["dependents"], json!(["src/b.ts"]));
        orphans(&mut modules, false, &none);
        assert_eq!(modules[3]["orphan"], true);
        assert_eq!(modules[2]["orphan"], false);
        assert_eq!(modules[0]["orphan"], false);

        let mut skipped = graph();
        dependents(
            &mut skipped,
            DependentsWhen {
                skip: true,
                ..DependentsWhen::default()
            },
            &none,
        );
        assert!(!js::has(&skipped[0], "dependents"));
        for when in [
            DependentsWhen {
                skip: true,
                force: true,
                ..DependentsWhen::default()
            },
            DependentsWhen {
                skip: true,
                metrics: true,
                ..DependentsWhen::default()
            },
            DependentsWhen {
                skip: true,
                reaches: true,
                ..DependentsWhen::default()
            },
            DependentsWhen {
                skip: true,
                focus: true,
                ..DependentsWhen::default()
            },
        ] {
            let mut m = graph();
            dependents(&mut m, when, &none);
            assert!(js::has(&m[0], "dependents"), "{when:?}");
        }
        let with_rule = rules(
            json!({ "forbidden": [{ "from": {}, "module": { "numberOfDependentsLessThan": 1 } }] }),
        );
        assert!(has_dependents_rule(&with_rule));
        let mut m = graph();
        dependents(
            &mut m,
            DependentsWhen {
                skip: true,
                ..DependentsWhen::default()
            },
            &with_rule,
        );
        assert!(js::has(&m[0], "dependents"));

        orphans(&mut skipped, true, &none);
        assert!(!js::has(&skipped[3], "orphan"));
        orphans(&mut skipped, false, &none);
        assert_eq!(
            skipped[3]["orphan"], true,
            "without dependents it asks the dependency set"
        );
        assert_eq!(skipped[2]["orphan"], false);
        let orphan_rule = rules(json!({ "forbidden": [{ "from": { "orphan": true }, "to": {} }] }));
        assert!(has_orphan_rule(&orphan_rule));
    }

    #[test]
    fn reachability() {
        let set = rules(json!({ "forbidden": [
            { "name": "not-reached", "from": { "path": "^src/a" }, "to": { "path": "^lib/", "reachable": false } },
            { "name": "reaches", "from": { "path": "^src/a" }, "to": { "path": "^lib/", "reachable": true } }
        ] }));
        let mut modules = graph();
        reachables(&mut modules, &set);
        assert_eq!(
            modules[2]["reachable"][0],
            json!({ "value": true, "asDefinedInRule": "not-reached", "matchedFrom": "src/a.ts" })
        );
        let reaches = &modules[0]["reaches"][0];
        assert_eq!(reaches["asDefinedInRule"], "reaches");
        assert_eq!(reaches["modules"][0]["source"], "lib/c.ts");
        assert_eq!(
            reaches["modules"][0]["via"].as_array().map(Vec::len),
            Some(2)
        );
        let mut untouched = graph();
        reachables(&mut untouched, &rules(json!({})));
        assert_eq!(untouched, graph());
    }

    #[test]
    fn reachability_with_captures_and_required_modules() {
        let set = rules(json!({
            "forbidden": [{ "name": "own", "from": { "path": "^(src)/a" }, "to": { "path": "^$1/b", "reachable": false } }],
            "required": [{ "name": "req", "module": { "path": "^src/a" }, "to": { "path": "^lib/", "reachable": true } }]
        }));
        let mut modules = graph();
        reachables(&mut modules, &set);
        assert_eq!(modules[1]["reachable"][0]["value"], true);
        assert_eq!(modules[0]["reaches"][0]["asDefinedInRule"], "req");
        assert!(has_capturing_groups(&set.forbidden[0]));
        assert!(!has_capturing_groups(&set.required[0]));
    }

    #[test]
    fn only_the_selected_modules_get_reaches() {
        let set = rules(json!({
            "forbidden": [
                { "name": "reaches", "from": { "path": "^src/a" }, "to": { "path": "^lib/", "reachable": true } },
                { "name": "both", "from": { "path": "^src", "pathNot": "^src/b" }, "to": { "path": "^(src/b|lib/)", "reachable": true } }
            ],
            "required": [{ "name": "req", "module": { "path": "^src/a" }, "to": { "path": "^lib/", "reachable": true } }]
        }));
        let mut modules = graph();
        reachables(&mut modules, &set);
        let b = json!({ "name": "src/b.ts", "dependencyTypes": ["local"] });
        let c = json!({ "name": "lib/c.ts", "dependencyTypes": ["local"] });
        assert_eq!(
            modules[0]["reaches"],
            json!([
                { "asDefinedInRule": "reaches", "modules": [{ "source": "lib/c.ts", "via": [b, c] }] },
                { "asDefinedInRule": "both", "modules": [
                    { "source": "src/b.ts", "via": [b] },
                    { "source": "lib/c.ts", "via": [b, c] }
                ] },
                { "asDefinedInRule": "req", "modules": [{ "source": "lib/c.ts", "via": [b, c] }] }
            ])
        );
        assert!(
            !js::has(&modules[1], "reaches"),
            "src/b reaches lib/c, but no rule selects it"
        );
        // A required rule asks for reachable[] too, whatever its to.reachable says.
        assert_eq!(
            modules[2]["reachable"],
            json!([{ "value": true, "asDefinedInRule": "req", "matchedFrom": "src/a.ts" }])
        );
        assert!(!js::has(&modules[1], "reachable"));
    }

    #[test]
    fn reachable_records_merge_per_rule() {
        let modules = || {
            vec![
                json!({ "source": "island.ts", "dependencies": [] }),
                json!({ "source": "src/a.ts", "dependencies": [{ "resolved": "lib/c.ts" }] }),
                json!({ "source": "lib/c.ts", "dependencies": [] }),
            ]
        };
        let set = rules(json!({ "forbidden": [
            { "name": "r1", "from": { "path": "^(island|src/a)" }, "to": { "path": "^lib/", "reachable": false } },
            { "name": "r2", "from": { "path": "^src/a" }, "to": { "path": "^lib/", "reachable": false } }
        ] }));
        let mut m = modules();
        reachables(&mut m, &set);
        // island.ts is tried first and does not reach lib/c.ts; src/a.ts then does, which turns
        // the value true and keeps the first matchedFrom.
        assert_eq!(
            m[2]["reachable"],
            json!([
                { "value": true, "asDefinedInRule": "r1", "matchedFrom": "island.ts" },
                { "value": true, "asDefinedInRule": "r2", "matchedFrom": "src/a.ts" }
            ])
        );
        let own = rules(json!({ "forbidden": [
            { "name": "own", "from": { "path": "^lib/" }, "to": { "path": "^lib/", "reachable": false } }
        ] }));
        let mut m = modules();
        reachables(&mut m, &own);
        assert!(
            !js::has(&m[2], "reachable"),
            "a module is never asked whether it reaches itself"
        );
    }

    #[test]
    fn capturing_groups_are_a_dollar_and_a_digit() {
        let to =
            |path: &str| rules(json!({ "forbidden": [{ "from": {}, "to": { "path": path } }] }));
        assert!(has_capturing_groups(&to("^src/$1/").forbidden[0]));
        assert!(!has_capturing_groups(&to("^v2/").forbidden[0]));
        assert!(!has_capturing_groups(&to("x$|^$a").forbidden[0]));
        let not = rules(json!({ "forbidden": [{ "from": {}, "to": { "pathNot": "$2" } }] }));
        assert!(has_capturing_groups(&not.forbidden[0]));
    }

    #[test]
    fn instability_metrics() {
        let mut modules = graph();
        dependents(&mut modules, DependentsWhen::default(), &rules(json!({})));
        module_metrics(&mut modules);
        assert_eq!(modules[0]["instability"], json!(0.5));
        assert_eq!(modules[2]["instability"], json!(0.0));
        assert!(
            !js::has(&modules[4], "instability"),
            "core modules have no metric"
        );
        assert_eq!(modules[1]["dependencies"][1]["instability"], json!(0.0));
        assert_eq!(
            modules[0]["dependencies"][0]["instability"],
            json!(2.0 / 3.0)
        );
        assert!((instability(1, 3) - 0.25).abs() < f64::EPSILON);
        assert!(instability(0, 0).abs() < f64::EPSILON);
        assert!(!metrics_are_calculable(&json!({ "couldNotResolve": true })));
        assert!(!metrics_are_calculable(
            &json!({ "matchesDoNotFollow": true })
        ));
    }
}
