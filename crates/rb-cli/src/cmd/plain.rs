//! `explain --plain`: one deterministic English sentence per rule shape.
//!
//! - Source: [design § The agentic engineering hat](../../../../docs/artifacts/design.md#the-agentic-engineering-hat-turn-two)
//!   ("Files under `apps/<x>/` may not import files under `apps/<y>/` unless x = y")
//! - Plan: [Wave 1, Step 14](../../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-14-rules---json-explain-explain---plain-test-can-import-1e)
//!   (templates in `rb-cli/src/plain/`, here one module; one test per shape)
//! - Requirement: [FR-CLI-01](../../../../docs/prd.md#fr-cli-01)
//!
//! Patterns are shown as paths: the leading `^` and trailing `$` dropped, `\.` unescaped, and a
//! capturing group named `<x>` (or `<y>` on the target side). A pattern that does not read as a
//! path is shown as written, in backticks.

use std::fmt::Write as _;

use rb_config::{Family, Rule};
use rb_model::options::Patterns;

/// A pattern as a reader sees it.
pub fn humanize(pattern: &str, variable: &str) -> String {
    let body = pattern.strip_prefix('^').unwrap_or(pattern);
    let body = body.strip_suffix('$').unwrap_or(body);
    let mut out = String::new();
    let mut depth = 0;
    let mut replaced = false;
    let mut chars = body.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(next) = chars.next()
                    && depth == 0
                {
                    out.push(next);
                }
            }
            '(' if !replaced && chars.peek() != Some(&'?') => {
                depth += 1;
                replaced = true;
                out.push('<');
                out.push_str(variable);
                out.push('>');
            }
            '(' if depth > 0 => depth += 1,
            ')' if depth > 0 => depth -= 1,
            _ if depth > 0 => {}
            _ => out.push(c),
        }
    }
    format!("`{out}`")
}

fn joined(patterns: Option<&Patterns>) -> Option<String> {
    patterns.map(Patterns::joined).filter(|p| !p.is_empty())
}

/// A noun phrase for the modules a side selects: plural (`files matching x`) or singular.
fn phrase(path: Option<String>, not: Option<String>, variable: &str, plural: bool) -> String {
    let noun = if plural { "files" } else { "file" };
    let mut s = match path {
        Some(p) => format!("{noun} matching {}", humanize(&p, variable)),
        None => noun.to_owned(),
    };
    if let Some(n) = not {
        let _ = write!(s, " (except {})", humanize(&n, variable));
    }
    s
}

fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
}

fn is_fence(rule: &Rule) -> bool {
    let from = joined(rule.from.path.as_ref()).unwrap_or_default();
    let not = joined(rule.to.path_not.as_ref()).unwrap_or_default();
    from.contains('(') && not.contains("$1")
}

fn qualifiers(rule: &Rule) -> String {
    let mut parts = Vec::new();
    if let Some(types) = &rule.to.dependency_types {
        parts.push(format!(
            "through a {} dependency",
            types
                .iter()
                .map(|t| t.as_str())
                .collect::<Vec<_>>()
                .join(" or ")
        ));
    }
    if let Some(types) = &rule.to.dependency_types_not {
        parts.push(format!(
            "ignoring {} dependencies",
            types
                .iter()
                .map(|t| t.as_str())
                .collect::<Vec<_>>()
                .join(" and ")
        ));
    }
    if rule.to.dynamic == Some(true) {
        parts.push("with a dynamic import".into());
    }
    if rule.to.dynamic == Some(false) {
        parts.push("with a static import".into());
    }
    if rule.to.more_than_one_dependency_type == Some(true) {
        parts.push("when the target is listed in more than one dependencies section".into());
    }
    if rule.to.ancestor == Some(true) {
        parts.push("in a folder above their own".into());
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!(", {}", parts.join(", "))
    }
}

/// The modules the rule's `from` selects.
fn from_side(rule: &Rule, plural: bool) -> String {
    phrase(
        joined(rule.from.path.as_ref()),
        joined(rule.from.path_not.as_ref()),
        "x",
        plural,
    )
}

/// The modules the rule's `to` selects.
fn to_side(rule: &Rule, plural: bool) -> String {
    phrase(
        joined(rule.to.path.as_ref()),
        joined(rule.to.path_not.as_ref()),
        "y",
        plural,
    )
}

/// The modules the rule's `module` selects.
fn module_side(rule: &Rule, plural: bool) -> String {
    let m = rule.module.as_ref();
    phrase(
        joined(m.and_then(|m| m.path.as_ref())),
        joined(m.and_then(|m| m.path_not.as_ref())),
        "x",
        plural,
    )
}

/// The sentence for one rule.
pub fn sentence(family: Family, rule: &Rule) -> String {
    let from = |plural| from_side(rule, plural);
    let to = |plural| to_side(rule, plural);
    let module = |plural| module_side(rule, plural);
    if family == Family::Allowed {
        return format!(
            "Imports from {} to {} are allowed; an import no allowed entry covers is a violation.",
            from(true),
            to(true)
        );
    }
    if family == Family::Required {
        let verb = if rule.to.reachable == Some(true) {
            "reach"
        } else {
            "import"
        };
        return format!("Every {} must {verb} a {}.", module(false), to(false));
    }
    if let Some(m) = &rule.module {
        let count = |n: Option<u64>, word: &str| n.map(|n| format!("{word} {n}"));
        let limit = [
            count(m.number_of_dependents_less_than, "fewer than"),
            count(m.number_of_dependents_more_than, "more than"),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" and ");
        return format!(
            "No {} may have {limit} dependents among {}.",
            module(false),
            from(true)
        );
    }
    if rule.from.orphan == Some(true) {
        return format!(
            "Every {} must import another module or be imported by one.",
            from(false)
        );
    }
    match rule.to.reachable {
        Some(true) => {
            return format!(
                "{} may not reach {}, directly or through other modules.",
                capitalized(&from(true)),
                to(true)
            );
        }
        Some(false) => {
            return format!(
                "Every {} must be reachable from a {}.",
                to(false),
                from(false)
            );
        }
        None => {}
    }
    if rule.to.circular == Some(true) {
        return format!(
            "{} may not be part of an import cycle{}.",
            capitalized(&from(true)),
            qualifiers(rule)
        );
    }
    if rule.to.could_not_resolve == Some(true) {
        return format!(
            "{} may not import anything that does not resolve.",
            capitalized(&from(true))
        );
    }
    if is_fence(rule) {
        let target = joined(rule.to.path.as_ref())
            .map_or_else(|| "`anything`".into(), |p| humanize(&p, "y"));
        return format!(
            "Files under {} may not import files under {target} unless x = y{}.",
            humanize(&joined(rule.from.path.as_ref()).unwrap_or_default(), "x"),
            qualifiers(rule)
        );
    }
    format!(
        "{} may not import {}{}.",
        capitalized(&from(true)),
        to(true),
        qualifiers(rule)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rule(value: serde_json::Value) -> Rule {
        serde_json::from_value(value).unwrap_or_default()
    }

    #[test]
    fn one_sentence_per_shape() {
        let cases = [
            (
                Family::Forbidden,
                json!({ "from": { "path": "^apps/([^/]+)/" }, "to": { "path": "^apps/([^/]+)/", "pathNot": "^apps/$1/" } }),
                "Files under `apps/<x>/` may not import files under `apps/<y>/` unless x = y.",
            ),
            (
                Family::Forbidden,
                json!({ "from": {}, "to": { "circular": true, "dependencyTypesNot": ["type-only"] } }),
                "Files may not be part of an import cycle, ignoring type-only dependencies.",
            ),
            (
                Family::Forbidden,
                json!({ "from": { "orphan": true, "pathNot": "\\.d\\.ts$" }, "to": {} }),
                "Every file (except `.d.ts`) must import another module or be imported by one.",
            ),
            (
                Family::Forbidden,
                json!({ "from": { "path": "^src/" }, "to": { "couldNotResolve": true } }),
                "Files matching `src/` may not import anything that does not resolve.",
            ),
            (
                Family::Forbidden,
                json!({ "from": { "path": "^src/Domain/" }, "to": { "path": "^src/(Web|Infrastructure)/" } }),
                "Files matching `src/Domain/` may not import files matching `src/<y>/`.",
            ),
            (
                Family::Forbidden,
                json!({ "from": { "path": "^test/" }, "to": { "path": "^src/internal", "reachable": true } }),
                "Files matching `test/` may not reach files matching `src/internal`, directly or through other modules.",
            ),
            (
                Family::Forbidden,
                json!({ "from": { "path": "^src/main\\.ts$" }, "to": { "path": "^src/", "reachable": false } }),
                "Every file matching `src/` must be reachable from a file matching `src/main.ts`.",
            ),
            (
                Family::Forbidden,
                json!({ "from": {}, "module": { "path": "^lib/", "numberOfDependentsLessThan": 2 } }),
                "No file matching `lib/` may have fewer than 2 dependents among files.",
            ),
            (
                Family::Required,
                json!({ "module": { "path": "^src/pages/" }, "to": { "path": "^src/auth\\.ts$", "reachable": true } }),
                "Every file matching `src/pages/` must reach a file matching `src/auth.ts`.",
            ),
            (
                Family::Allowed,
                json!({ "from": { "path": "^src/" }, "to": { "path": "^(src|node_modules)/" } }),
                "Imports from files matching `src/` to files matching `<y>/` are allowed; an import no allowed entry covers is a violation.",
            ),
            (
                Family::Forbidden,
                json!({ "from": {}, "to": { "dependencyTypes": ["npm-no-pkg"], "dynamic": true } }),
                "Files may not import files, through a npm-no-pkg dependency, with a dynamic import.",
            ),
        ];
        for (family, value, expected) in cases {
            assert_eq!(sentence(family, &rule(value.clone())), expected, "{value}");
        }
    }

    #[test]
    fn deterministic_and_humanized() {
        let r = rule(
            json!({ "from": { "path": "^a/" }, "to": { "path": "^b/", "moreThanOneDependencyType": true, "ancestor": true, "dynamic": false } }),
        );
        assert_eq!(
            sentence(Family::Forbidden, &r),
            sentence(Family::Forbidden, &r)
        );
        assert!(sentence(Family::Forbidden, &r).contains("static import"));
        assert_eq!(humanize("^(?:a|b)/x$", "x"), "`(?:a|b)/x`");
        assert_eq!(humanize("^src/((a)b)/c", "x"), "`src/<x>/c`");
        let more = rule(
            json!({ "from": { "path": "^x" }, "module": { "numberOfDependentsMoreThan": 5 } }),
        );
        assert!(sentence(Family::Forbidden, &more).contains("more than 5"));
        let required = rule(json!({ "module": { "path": "^m" }, "to": { "path": "^t" } }));
        assert!(sentence(Family::Required, &required).contains("must import"));
        assert_eq!(capitalized(""), "");
    }
}
