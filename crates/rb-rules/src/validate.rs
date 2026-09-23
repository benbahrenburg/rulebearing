//! Rule evaluation for one module, one dependency or one folder dependency: dependency-cruiser
//! 18.2.0's `src/validate`, ported.
//!
//! - Specification: `test/validate/*.spec.mjs`, run unmodified by conformance gate 1 layer 2
//!   ([ADR-0009](../../../docs/adr/0009-conformance-suites-as-specification.md))
//! - Coverage: [coverage § Rules](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#rules)
//!   (every Wave 1 row)
//! - Plan: [Wave 1, Step 5](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-5-matchers-and-restriction-evaluation-1b)
//! - Requirements: [FR-RULE-01](../../../docs/prd.md#fr-rule-01), [FR-RULE-08](../../../docs/prd.md#fr-rule-08)
//!
//! | Upstream module | Here |
//! | --- | --- |
//! | `rule-classifiers.mjs` | [`is_module_only_rule`], [`is_folder_scope`] |
//! | `match-dependency-rule.mjs` | [`dependency_match`] |
//! | `match-module-rule.mjs`, `match-module-rule-helpers.mjs` | [`module_match`] and the `matches_*_rule` functions |
//! | `match-folder-dependency-rule.mjs` | [`folder_match`] |
//! | `violates-required-rule.mjs` | [`violates_required_rule`] |
//! | `index.mjs` | [`validate_module`], [`validate_dependency`], [`validate_folder`] |

use rb_config::Rule;
use rb_config::model::DependencyRules;
use rb_model::Severity;
use serde_json::{Value, json};

use crate::js;
use crate::matchers::{
    from_groups, match_to_module_path, match_to_module_path_not, matches_ancestor,
    matches_from_path, matches_from_path_not, matches_module_path, matches_module_path_not,
    matches_more_than_one_dependency_type, matches_to_dependency_types,
    matches_to_dependency_types_not, matches_to_is_more_unstable, matches_to_path,
    matches_to_path_not, matches_to_via, matches_to_via_only, module_groups, pattern,
    property_equals, property_matches, property_matches_not,
};
use crate::patterns;

/// `isModuleOnlyRule`: an orphan, reachability or dependents rule.
pub fn is_module_only_rule(rule: &Rule) -> bool {
    rule.is_module_only()
}

/// `isFolderScope`.
pub fn is_folder_scope(rule: &Rule) -> bool {
    rule.is_folder_scope()
}

/// Which matcher a validation uses, as upstream's `pMatchModule` object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Matcher {
    /// `match-module-rule`.
    Module,
    /// `match-dependency-rule`.
    Dependency,
    /// `match-folder-dependency-rule`.
    Folder,
}

impl Matcher {
    /// `isInteresting`.
    pub fn is_interesting(self, rule: &Rule) -> bool {
        match self {
            Self::Module => is_module_only_rule(rule) && !is_folder_scope(rule),
            Self::Dependency => !is_module_only_rule(rule) && !is_folder_scope(rule),
            Self::Folder => is_folder_scope(rule) && !is_module_only_rule(rule),
        }
    }

    /// `match(from, to)(rule)`.
    pub fn matches(self, rule: &Rule, from: &Value, to: &Value) -> bool {
        match self {
            Self::Module => module_match(rule, from),
            Self::Dependency => dependency_match(rule, from, to),
            Self::Folder => folder_match(rule, from, to),
        }
    }
}

/// `match-dependency-rule`'s `match(from, to)(rule)`.
pub fn dependency_match(rule: &Rule, from: &Value, to: &Value) -> bool {
    let groups = from_groups(rule, &js::text(from, "source"));
    matches_from_path(rule, from)
        && matches_from_path_not(rule, from)
        && matches_to_path(rule, to, &groups)
        && matches_to_path_not(rule, to, &groups)
        && matches_to_dependency_types(rule, to)
        && matches_to_dependency_types_not(rule, to)
        && matches_more_than_one_dependency_type(rule, to)
        && property_equals(rule.to.pre_compilation_only, to, "preCompilationOnly")
        && property_equals(rule.to.could_not_resolve, to, "couldNotResolve")
        && property_equals(rule.to.circular, to, "circular")
        && property_equals(rule.to.dynamic, to, "dynamic")
        && property_equals(rule.to.exotically_required, to, "exoticallyRequired")
        && property_matches(rule.to.license.as_ref(), to, "license")
        && property_matches_not(rule.to.license_not.as_ref(), to, "license")
        && property_matches(rule.to.exotic_require.as_ref(), to, "exoticRequire")
        && property_matches_not(rule.to.exotic_require_not.as_ref(), to, "exoticRequire")
        && matches_to_via(rule, to, &groups)
        && matches_to_via_only(rule, to, &groups)
        && matches_to_is_more_unstable(rule, from, to)
        && matches_ancestor(rule, from, to)
}

/// `matchesOrphanRule`.
pub fn matches_orphan_rule(rule: &Rule, module: &Value) -> bool {
    rule.from.orphan.is_some_and(|orphan| {
        module.get("orphan") == Some(&Value::Bool(orphan))
            && matches_from_path(rule, module)
            && matches_from_path_not(rule, module)
    })
}

/// `matchesReachableRule`.
pub fn matches_reachable_rule(rule: &Rule, module: &Value) -> bool {
    let Some(reachable) = rule.to.reachable else {
        return false;
    };
    if !js::has(module, "reachable") {
        return false;
    }
    let record = js::array(module, "reachable").iter().find(|r| {
        js::str_of(r, "asDefinedInRule") == Some(rule.name())
            && r.get("value") == Some(&Value::Bool(reachable))
    });
    record.is_some_and(|record| {
        let groups = from_groups(rule, &js::text(record, "matchedFrom"));
        match_to_module_path(rule, module, &groups)
            && match_to_module_path_not(rule, module, &groups)
    })
}

/// `matchesReachesRule`.
pub fn matches_reaches_rule(rule: &Rule, module: &Value) -> bool {
    rule.to.reachable.is_some()
        && js::has(module, "reaches")
        && js::array(module, "reaches").iter().any(|reaches| {
            js::str_of(reaches, "asDefinedInRule") == Some(rule.name())
                && js::array(reaches, "modules").iter().any(|m| {
                    match_to_module_path(rule, m, &[]) && match_to_module_path_not(rule, m, &[])
                })
        })
}

/// `dependentsCountsMatch`: the dependents that match `from`, counted against the limits; a
/// limit of zero is no limit, as JavaScript's `!0` is.
fn dependents_counts_match(rule: &Rule, dependents: &[Value]) -> bool {
    let from_path = pattern(rule.from.path.as_ref());
    let from_path_not = pattern(rule.from.path_not.as_ref());
    let count = dependents
        .iter()
        .map(|d| d.as_str().unwrap_or("undefined"))
        .filter(|d| {
            from_path.as_ref().is_none_or(|p| patterns::test(p, d))
                && from_path_not.as_ref().is_none_or(|p| !patterns::test(p, d))
        })
        .count() as u64;
    let module = rule.module.as_ref();
    let less = module
        .and_then(|m| m.number_of_dependents_less_than)
        .filter(|n| *n != 0);
    let more = module
        .and_then(|m| m.number_of_dependents_more_than)
        .filter(|n| *n != 0);
    less.is_none_or(|n| count < n) && more.is_none_or(|n| count > n)
}

/// `matchesDependentsRule`, including upstream's operator precedence: a `numberOfDependentsMoreThan`
/// rule applies whether or not the module carries `dependents`.
pub fn matches_dependents_rule(rule: &Rule, module: &Value) -> bool {
    let limits = rule.module.as_ref();
    let less = limits.is_some_and(|m| m.number_of_dependents_less_than.is_some());
    let more = limits.is_some_and(|m| m.number_of_dependents_more_than.is_some());
    if (js::has(module, "dependents") && less) || more {
        return matches_module_path(rule, module)
            && matches_module_path_not(rule, module)
            && dependents_counts_match(rule, js::array(module, "dependents"));
    }
    false
}

/// `match-module-rule`'s `match(module)(rule)`.
pub fn module_match(rule: &Rule, module: &Value) -> bool {
    matches_orphan_rule(rule, module)
        || matches_reachable_rule(rule, module)
        || matches_reaches_rule(rule, module)
        || matches_dependents_rule(rule, module)
}

/// `match-folder-dependency-rule`'s `match(fromFolder, toFolder)(rule)`.
pub fn folder_match(rule: &Rule, from: &Value, to: &Value) -> bool {
    let from_name = js::text(from, "name");
    let to_name = js::text(to, "name");
    let groups = from_groups(rule, &from_name);
    let sub = |p: &str| rb_config::pattern::replace_group_placeholders(p, &groups);
    pattern(rule.from.path.as_ref()).is_none_or(|p| patterns::test(&p, &from_name))
        && pattern(rule.from.path_not.as_ref()).is_none_or(|p| !patterns::test(&p, &from_name))
        && pattern(rule.to.path.as_ref()).is_none_or(|p| patterns::test(&sub(&p), &to_name))
        && pattern(rule.to.path_not.as_ref()).is_none_or(|p| !patterns::test(&sub(&p), &to_name))
        && matches_to_is_more_unstable(rule, from, to)
        && property_equals(rule.to.circular, to, "circular")
}

/// `violatesRequiredRule`: a module the rule selects that neither is nor depends on (or, with
/// `to.reachable`, reaches) a module matching `to.path`.
pub fn violates_required_rule(rule: &Rule, module: &Value) -> bool {
    if !(matches_module_path(rule, module) && matches_module_path_not(rule, module)) {
        return false;
    }
    let reachable = rule.to.reachable == Some(true);
    let mut violates = false;
    if reachable {
        violates = !matches_reaches_rule(rule, module);
    }
    if violates || !reachable {
        let groups = module_groups(rule, &js::text(module, "source"));
        let matches_self = match_to_module_path(rule, module, &groups);
        violates = !matches_self
            && !js::array(module, "dependencies")
                .iter()
                .any(|d| matches_to_path(rule, d, &groups));
    }
    violates
}

/// The rank `compareSeverity` in `validate/index.mjs` sorts by. `ignore` and a missing severity
/// have none: upstream's subtraction gives `NaN`, which compares equal to everything.
fn rank(severity: Option<Severity>) -> Option<u8> {
    match severity {
        Some(Severity::Error) => Some(1),
        Some(Severity::Warn) => Some(2),
        Some(Severity::Info) => Some(3),
        _ => None,
    }
}

/// `compareSeverity(a, b) < 0`.
fn ranks_before(a: Option<Severity>, b: Option<Severity>) -> bool {
    matches!((rank(a), rank(b)), (Some(x), Some(y)) if x < y)
}

fn summary(severity: Option<Severity>, name: &str) -> Value {
    match severity {
        Some(severity) => json!({ "severity": severity.as_str(), "name": name }),
        None => json!({ "name": name }),
    }
}

/// `validateAgainstRules`: `{ valid: true }` or `{ valid: false, rules: [...] }`.
pub fn validate(rules: &DependencyRules, from: &Value, to: &Value, matcher: Matcher) -> Value {
    let mut found: Vec<(Option<Severity>, Value)> = Vec::new();
    if !rules.allowed.is_empty() {
        let interesting: Vec<&Rule> = rules
            .allowed
            .iter()
            .filter(|r| matcher.is_interesting(r))
            .collect();
        if !interesting.is_empty() && !interesting.iter().any(|r| matcher.matches(r, from, to)) {
            found.push((
                rules.allowed_severity,
                summary(rules.allowed_severity, "not-in-allowed"),
            ));
        }
    }
    for rule in rules
        .forbidden
        .iter()
        .filter(|r| matcher.is_interesting(r) && matcher.matches(r, from, to))
    {
        found.push((
            Some(rule.severity()),
            summary(Some(rule.severity()), rule.name()),
        ));
    }
    for rule in rules
        .required
        .iter()
        .filter(|r| matcher.is_interesting(r) && violates_required_rule(r, from))
    {
        found.push((
            Some(rule.severity()),
            summary(Some(rule.severity()), rule.name()),
        ));
    }
    js::sort(&mut found, |(a, _), (b, _)| ranks_before(*a, *b));
    if found.is_empty() {
        json!({ "valid": true })
    } else {
        json!({ "valid": false, "rules": found.into_iter().map(|(_, v)| v).collect::<Vec<_>>() })
    }
}

/// `validateModule`.
pub fn validate_module(rules: &DependencyRules, module: &Value) -> Value {
    validate(rules, module, &js::object(), Matcher::Module)
}

/// `validateDependency`.
pub fn validate_dependency(rules: &DependencyRules, from: &Value, to: &Value) -> Value {
    validate(rules, from, to, Matcher::Dependency)
}

/// `validateFolder`.
pub fn validate_folder(rules: &DependencyRules, from: &Value, to: &Value) -> Value {
    validate(rules, from, to, Matcher::Folder)
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

    fn rule(value: Value) -> Rule {
        serde_json::from_value(value).unwrap_or_default()
    }

    #[test]
    fn the_empty_rule_set_and_everything_allowed_are_valid() {
        let from = json!({ "source": "koos koets" });
        let to = json!({ "resolved": "robby van de kerkhof" });
        assert_eq!(
            validate_dependency(&rules(json!({})), &from, &to),
            json!({ "valid": true })
        );
        let all = rules(json!({ "allowed": [{ "from": {}, "to": {} }] }));
        assert_eq!(
            validate_dependency(&all, &from, &to),
            json!({ "valid": true })
        );
    }

    #[test]
    fn an_impossible_allowed_list_flags_everything() {
        let set = rules(
            json!({ "allowed": [{ "from": { "path": "only-this-one" }, "to": { "path": "only-that-one" } }] }),
        );
        let result = validate_dependency(
            &set,
            &json!({ "source": "koos koets" }),
            &json!({ "resolved": "robby" }),
        );
        assert_eq!(
            result,
            json!({ "valid": false, "rules": [{ "severity": "warn", "name": "not-in-allowed" }] })
        );
    }

    #[test]
    fn forbidden_rules_are_sorted_by_severity() {
        let set = rules(json!({ "forbidden": [
            { "name": "i", "severity": "info", "from": {}, "to": {} },
            { "name": "e", "severity": "error", "from": {}, "to": {} },
            { "name": "w", "severity": "warn", "from": {}, "to": {} }
        ] }));
        let result =
            validate_dependency(&set, &json!({ "source": "a" }), &json!({ "resolved": "b" }));
        let names: Vec<&str> = result["rules"]
            .as_array()
            .map(|r| r.iter().filter_map(|x| x["name"].as_str()).collect())
            .unwrap_or_default();
        assert_eq!(names, ["e", "w", "i"]);
        assert_eq!(rank(None), None);
        assert_eq!(rank(Some(Severity::Ignore)), None);
    }

    #[test]
    fn severities_sort_as_upstream_sorts_them() {
        // Each case ran through dependency-cruiser 18.2.0's own `validateDependency` on Node:
        // forbidden rules named by severity (e, w, i, g for ignore) and position, and, when the
        // flag is set, a failing `allowed` list without `allowedSeverity` ("n", first). An entry
        // without a rank compares equal to every other, so it can stay ahead of a worse one.
        let cases = [
            (false, "iwwieewgw", "e4 e5 w1 w2 w6 w8 i0 i3 g7"),
            (false, "iiwgggieeg", "w2 i0 i1 g3 g4 g5 e7 e8 i6 g9"),
            (false, "wgigggwe", "w0 g1 i2 g3 g4 g5 e7 w6"),
            (false, "wiieewege", "e3 e4 e6 e8 w0 w5 i1 i2 g7"),
            (false, "gegiigwi", "g0 e1 g2 i3 i4 g5 w6 i7"),
            (false, "iwg", "w1 i0 g2"),
            (false, "gw", "g0 w1"),
            (false, "ew", "e0 w1"),
            (false, "wigeiwii", "w0 i1 g2 e3 w5 i4 i6 i7"),
            (false, "iwegiig", "e2 w1 i0 g3 i4 i5 g6"),
            (false, "eeegegewg", "e0 e1 e2 g3 e4 g5 e6 w7 g8"),
            (true, "iig", "n i0 i1 g2"),
            (true, "eigiwigew", "n e0 w4 i1 g2 e7 w8 i3 i5 g6"),
            (false, "eiieigeig", "e0 e3 e6 i1 i2 i4 g5 i7 g8"),
            (false, "igee", "i0 g1 e2 e3"),
            (false, "wiegeeie", "e2 e4 e5 e7 w0 i1 g3 i6"),
            (false, "wiwge", "e4 w0 w2 i1 g3"),
            (false, "wwggee", "w0 w1 g2 g3 e4 e5"),
            (false, "iwgewi", "e3 w1 w4 i0 g2 i5"),
            (false, "gigeig", "g0 i1 g2 e3 i4 g5"),
            (false, "eegiiigeei", "e0 e1 g2 i3 i4 i5 g6 e7 e8 i9"),
            (false, "iiwegii", "e3 w2 i0 i1 g4 i5 i6"),
            (true, "iewegg", "n e1 e3 w2 i0 g4 g5"),
            (false, "ggewgwwgiw", "g0 g1 e2 w3 g4 w5 w6 g7 w9 i8"),
            (false, "ggw", "g0 g1 w2"),
            (true, "egewii", "n e0 g1 e2 w3 i4 i5"),
            (false, "wigiwe", "w0 i1 g2 e5 w4 i3"),
            (false, "geiiiw", "g0 e1 w5 i2 i3 i4"),
            (true, "wwwegiwegi", "n e3 e7 w0 w1 w2 g4 w6 i5 g8 i9"),
            (false, "giweg", "g0 e3 w2 i1 g4"),
            (false, "giieiiwiigg", "g0 e3 w6 i1 i2 i4 i5 i7 i8 g9 g10"),
            (false, "iewigiggeg", "e1 w2 i0 i3 g4 i5 g6 g7 e8 g9"),
            (true, "ei", "n e0 i1"),
            (
                false,
                "iiieiwegeiwiiggggew",
                "e3 e6 e8 e17 w5 w10 w18 i0 i1 i2 i4 g7 i9 i11 i12 g13 g14 g15 g16",
            ),
            (false, "giwewwg", "g0 e3 w2 w4 w5 i1 g6"),
            (false, "eeiegwiww", "e0 e1 e3 w7 w8 i2 g4 w5 i6"),
        ];
        for (allowed, severities, expected) in cases {
            let forbidden: Vec<Value> = severities
                .chars()
                .enumerate()
                .map(|(index, s)| {
                    let severity = match s {
                        'e' => "error",
                        'w' => "warn",
                        'i' => "info",
                        _ => "ignore",
                    };
                    json!({ "name": format!("{s}{index}"), "severity": severity, "from": {}, "to": {} })
                })
                .collect();
            // As the spec calls it: the rule set as built, not normalised (which drops `ignore`).
            let set = DependencyRules {
                forbidden: forbidden.into_iter().map(rule).collect(),
                allowed: if allowed {
                    vec![rule(json!({ "from": { "path": "^x" }, "to": {} }))]
                } else {
                    Vec::new()
                },
                ..DependencyRules::default()
            };
            let result =
                validate_dependency(&set, &json!({ "source": "a" }), &json!({ "resolved": "b" }));
            let names: Vec<&str> = js::array(&result, "rules")
                .iter()
                .map(|r| match js::str_of(r, "name") {
                    Some("not-in-allowed") => "n",
                    other => other.unwrap_or_default(),
                })
                .collect();
            assert_eq!(names.join(" "), expected, "{allowed} {severities}");
        }
    }

    #[test]
    fn module_only_rules_do_not_apply_to_dependencies() {
        let set = rules(json!({ "allowed": [{ "from": {}, "to": {} }],
                                "forbidden": [{ "name": "no-orphans", "from": { "orphan": true }, "to": {} }] }));
        assert_eq!(
            validate_module(&set, &json!({ "source": "koos koets" })),
            json!({ "valid": true })
        );
        let orphan = json!({ "source": "x", "orphan": true });
        assert_eq!(
            validate_module(&set, &orphan)["rules"][0]["name"],
            "no-orphans"
        );
    }

    #[test]
    fn dependency_matching_uses_captures() {
        let r = rule(
            json!({ "from": { "path": "^apps/([^/]+)/" }, "to": { "path": "^apps/([^/]+)/", "pathNot": "^apps/$1/" } }),
        );
        let from = json!({ "source": "apps/web/x.ts" });
        assert!(dependency_match(
            &r,
            &from,
            &json!({ "resolved": "apps/api/y.ts" })
        ));
        assert!(!dependency_match(
            &r,
            &from,
            &json!({ "resolved": "apps/web/y.ts" })
        ));
        let flags = rule(json!({ "to": { "circular": true, "dynamic": false } }));
        assert!(dependency_match(
            &flags,
            &from,
            &json!({ "circular": true, "dynamic": false })
        ));
        assert!(!dependency_match(
            &flags,
            &from,
            &json!({ "circular": true })
        ));
    }

    #[test]
    fn orphans_reachability_and_dependents() {
        let orphan = rule(json!({ "from": { "orphan": true, "pathNot": "^x" }, "to": {} }));
        assert!(matches_orphan_rule(
            &orphan,
            &json!({ "source": "a", "orphan": true })
        ));
        assert!(!matches_orphan_rule(
            &orphan,
            &json!({ "source": "x", "orphan": true })
        ));
        assert!(!matches_orphan_rule(
            &orphan,
            &json!({ "source": "a", "orphan": false })
        ));
        assert!(!matches_orphan_rule(
            &rule(json!({})),
            &json!({ "orphan": true })
        ));

        let unreachable = rule(
            json!({ "name": "u", "from": { "path": "^(src)/main" }, "to": { "path": "^$1/", "reachable": false } }),
        );
        let module = json!({ "source": "src/x.ts", "reachable": [{ "value": false, "asDefinedInRule": "u", "matchedFrom": "src/main.ts" }] });
        assert!(matches_reachable_rule(&unreachable, &module));
        assert!(!matches_reachable_rule(
            &unreachable,
            &json!({ "source": "src/x.ts" })
        ));
        assert!(!matches_reachable_rule(&rule(json!({})), &module));
        let other = json!({ "source": "lib/x.ts", "reachable": [{ "value": false, "asDefinedInRule": "u", "matchedFrom": "src/main.ts" }] });
        assert!(!matches_reachable_rule(&unreachable, &other));

        let reaches =
            rule(json!({ "name": "r", "from": {}, "to": { "path": "^b", "reachable": true } }));
        let reaching = json!({ "source": "a", "reaches": [{ "asDefinedInRule": "r", "modules": [{ "source": "b.ts", "via": [] }] }] });
        assert!(matches_reaches_rule(&reaches, &reaching));
        assert!(!matches_reaches_rule(&reaches, &json!({ "source": "a" })));
        assert!(!matches_reaches_rule(&rule(json!({ "to": {} })), &reaching));

        let fewer = rule(
            json!({ "from": { "path": "^src", "pathNot": "^src/test" }, "module": { "path": "^lib", "numberOfDependentsLessThan": 2 } }),
        );
        assert!(matches_dependents_rule(
            &fewer,
            &json!({ "source": "lib/a", "dependents": ["src/a"] })
        ));
        assert!(!matches_dependents_rule(
            &fewer,
            &json!({ "source": "lib/a", "dependents": ["src/a", "src/b"] })
        ));
        assert!(matches_dependents_rule(
            &fewer,
            &json!({ "source": "lib/a", "dependents": ["src/a", "src/test/b", "other"] })
        ));
        assert!(
            !matches_dependents_rule(&fewer, &json!({ "source": "lib/a" })),
            "no dependents, less-than rule"
        );
        assert!(!matches_dependents_rule(
            &fewer,
            &json!({ "source": "x", "dependents": [] })
        ));
        let more = rule(json!({ "from": {}, "module": { "numberOfDependentsMoreThan": 1 } }));
        assert!(matches_dependents_rule(
            &more,
            &json!({ "source": "x", "dependents": ["a", "b"] })
        ));
        assert!(
            !matches_dependents_rule(&more, &json!({ "source": "x" })),
            "precedence: applies, counts zero"
        );
        let zero = rule(json!({ "from": {}, "module": { "numberOfDependentsLessThan": 0 } }));
        assert!(
            matches_dependents_rule(&zero, &json!({ "source": "x", "dependents": ["a"] })),
            "zero is no limit"
        );
        assert!(!matches_dependents_rule(
            &rule(json!({})),
            &json!({ "dependents": [] })
        ));
        assert!(module_match(
            &orphan,
            &json!({ "source": "a", "orphan": true })
        ));
    }

    #[test]
    fn folders() {
        let r = rule(
            json!({ "scope": "folder", "from": { "path": "^(src)/", "pathNot": "^src/x" }, "to": { "path": "^$1/", "pathNot": "^src/y", "circular": true } }),
        );
        assert!(folder_match(
            &r,
            &json!({ "name": "src/a" }),
            &json!({ "name": "src/b", "circular": true })
        ));
        assert!(!folder_match(
            &r,
            &json!({ "name": "src/a" }),
            &json!({ "name": "src/b", "circular": false })
        ));
        assert!(!folder_match(
            &r,
            &json!({ "name": "src/x" }),
            &json!({ "name": "src/b", "circular": true })
        ));
        assert!(!folder_match(
            &r,
            &json!({ "name": "src/a" }),
            &json!({ "name": "src/y", "circular": true })
        ));
        assert!(!folder_match(
            &r,
            &json!({ "name": "lib" }),
            &json!({ "name": "src/b", "circular": true })
        ));
        assert!(!folder_match(
            &r,
            &json!({ "name": "src/a" }),
            &json!({ "name": "lib/b", "circular": true })
        ));
        let set = rules(
            json!({ "forbidden": [{ "name": "f", "scope": "folder", "from": {}, "to": { "circular": true } }] }),
        );
        assert_eq!(
            validate_folder(
                &set,
                &json!({ "name": "a" }),
                &json!({ "name": "b", "circular": true })
            )["valid"],
            false
        );
        assert_eq!(
            validate_dependency(
                &set,
                &json!({ "source": "a" }),
                &json!({ "circular": true })
            )["valid"],
            true
        );
        assert!(Matcher::Folder.is_interesting(&r));
        assert!(!Matcher::Dependency.is_interesting(&r));
    }

    #[test]
    fn required_rules() {
        let r = rule(
            json!({ "module": { "path": "^src/(.+)\\.ts$" }, "to": { "path": "^test/$1\\.spec\\.ts$" } }),
        );
        assert!(violates_required_rule(
            &r,
            &json!({ "source": "src/a.ts", "dependencies": [] })
        ));
        assert!(!violates_required_rule(
            &r,
            &json!({ "source": "src/a.ts", "dependencies": [{ "resolved": "test/a.spec.ts" }] })
        ));
        assert!(!violates_required_rule(
            &r,
            &json!({ "source": "lib/a.ts", "dependencies": [] })
        ));
        let own = rule(json!({ "module": { "path": "^src/" }, "to": { "path": "^src/" } }));
        assert!(
            !violates_required_rule(&own, &json!({ "source": "src/a.ts" })),
            "matches itself"
        );
        let reach = rule(
            json!({ "name": "reach", "module": { "path": "^src/" }, "to": { "path": "^lib/auth", "reachable": true } }),
        );
        let reaching = json!({ "source": "src/a.ts", "dependencies": [], "reaches": [{ "asDefinedInRule": "reach", "modules": [{ "source": "lib/auth.ts" }] }] });
        assert!(!violates_required_rule(&reach, &reaching));
        assert!(violates_required_rule(
            &reach,
            &json!({ "source": "src/a.ts", "dependencies": [] })
        ));
        let set = rules(
            json!({ "required": [{ "name": "req", "severity": "error", "module": { "path": "^src/" }, "to": { "path": "^lib/" } }] }),
        );
        assert_eq!(
            validate_module(&set, &json!({ "source": "src/a.ts", "dependencies": [] }))["rules"][0]
                ["name"],
            "req"
        );
    }

    #[test]
    fn folder_matchers_take_folder_rules_that_are_not_module_only() {
        let plain = rule(json!({ "from": {}, "to": { "circular": true } }));
        assert!(!Matcher::Folder.is_interesting(&plain));
        assert!(Matcher::Dependency.is_interesting(&plain));
        let folder_orphans =
            rule(json!({ "scope": "folder", "from": { "orphan": true }, "to": {} }));
        assert!(!Matcher::Folder.is_interesting(&folder_orphans));
        assert!(!Matcher::Module.is_interesting(&folder_orphans));
    }

    #[test]
    fn reachable_records_count_only_for_their_rule_and_value() {
        let unreachable =
            rule(json!({ "name": "u", "from": {}, "to": { "path": "^src/", "reachable": false } }));
        let record = |rule: &str, value: bool| json!({ "source": "src/x.ts", "reachable": [{ "value": value, "asDefinedInRule": rule, "matchedFrom": "src/main.ts" }] });
        assert!(matches_reachable_rule(&unreachable, &record("u", false)));
        assert!(!matches_reachable_rule(
            &unreachable,
            &record("other", false)
        ));
        assert!(!matches_reachable_rule(&unreachable, &record("u", true)));
        // Only the reachable rule matches; the other three helpers do not.
        assert!(module_match(&unreachable, &record("u", false)));
        assert!(!module_match(&unreachable, &record("u", true)));
    }

    #[test]
    fn reaches_need_the_rule_its_name_and_a_matching_module() {
        let reaches = rule(
            json!({ "name": "r", "from": {}, "to": { "path": "^b", "pathNot": "^b/private", "reachable": true } }),
        );
        let reaching = |target: &str| json!({ "source": "a", "reaches": [{ "asDefinedInRule": "r", "modules": [{ "source": target, "via": [] }] }] });
        assert!(matches_reaches_rule(&reaches, &reaching("b.ts")));
        assert!(module_match(&reaches, &reaching("b.ts")));
        assert!(!matches_reaches_rule(&reaches, &reaching("c.ts")));
        assert!(!matches_reaches_rule(&reaches, &reaching("b/private.ts")));
        let named_without_reachable =
            rule(json!({ "name": "r", "from": {}, "to": { "path": "^b" } }));
        assert!(!matches_reaches_rule(
            &named_without_reachable,
            &reaching("b.ts")
        ));
    }

    #[test]
    fn more_than_is_strict() {
        let more = rule(json!({ "from": {}, "module": { "numberOfDependentsMoreThan": 1 } }));
        assert!(!matches_dependents_rule(
            &more,
            &json!({ "source": "x", "dependents": ["a"] })
        ));
        assert!(matches_dependents_rule(
            &more,
            &json!({ "source": "x", "dependents": ["a", "b"] })
        ));
    }

    #[test]
    fn required_rules_do_not_judge_dependencies() {
        let set = rules(
            json!({ "required": [{ "name": "req", "severity": "error", "module": { "path": "^src/" }, "to": { "path": "^lib/" } }] }),
        );
        let from = json!({ "source": "src/a.ts", "dependencies": [] });
        assert_eq!(validate_module(&set, &from)["valid"], false);
        assert_eq!(
            validate_dependency(&set, &from, &json!({ "resolved": "x.ts" })),
            json!({ "valid": true })
        );
    }

    #[test]
    fn allowed_without_severity_names_only() {
        let mut set = rules(json!({ "allowed": [{ "from": { "path": "^x" }, "to": {} }] }));
        set.allowed_severity = None;
        let result =
            validate_dependency(&set, &json!({ "source": "a" }), &json!({ "resolved": "b" }));
        assert_eq!(result["rules"][0], json!({ "name": "not-in-allowed" }));
    }
}
