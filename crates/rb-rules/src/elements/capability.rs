//! Validation against the capability table ([`rb_config::capability`]): which language can
//! answer each element predicate, and how.
//!
//! - Plan: [Wave 2, Step 5](../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#25-step-5-the-element-rule-engine-and-the-capability-table-2c)
//!   and § 1.4.3 ("cross-language behaviour comes from two data tables")
//! - Decisions: [ADR-0014](../../../../docs/adr/0014-no-invented-cross-language-edges.md) (a key a
//!   language cannot answer is an error, never a silent false),
//!   [ADR-0010](../../../../docs/adr/0010-crate-layout-and-extractor-boundary.md) rule 3 (the
//!   engine has no `match language`: this table is data the engine reads)
//! - Requirement: [FR-RULE-03](../../../../docs/prd.md#fr-rule-03) ("capability table")
//!
//! The table lives beside the vocabulary in `rb-config`, so the configuration schema prints its
//! mappings. A rule that uses an
//! unanswerable key over objects of that language fails validation, naming the key, the language
//! and the rule; `select.language` scopes a rule to the languages that can answer it. A key whose
//! concept does not apply to the kind of object it tests (the applicability table beside it) fails
//! the same way, naming the kind. Each test is checked against the objects it is applied to: the
//! rule's selection for `select.where` and `should`, a nested selector's own kind and languages
//! for that selector's `where`.

use rb_config::capability::{Applicability, applicability};
use rb_config::elements::{ElementRule, Expr, Kind, Objects, Operand, Selector};
use rb_model::Language;

use super::{Architecture, ElementError};

use Capability::Unanswerable;
pub use rb_config::capability::{Capability, capability};

/// The selector a test's operand nests, if any (`dependOnAnyTypesThat`, an attribute given by a
/// selector).
fn nested(operand: &Operand) -> Option<&Selector> {
    match operand {
        Operand::Objects(Objects::Selector(s))
        | Operand::Attribute {
            attribute: Some(Objects::Selector(s)),
            ..
        } => Some(s.as_ref()),
        _ => None,
    }
}

/// The languages a selector's objects can come from in this run.
fn languages_of(architecture: &Architecture<'_>, selector: &Selector) -> Vec<Language> {
    let mut found: Vec<Language> = architecture
        .of_kind(selector.kind)
        .iter()
        .filter_map(super::Object::language)
        .filter(|l| selector.languages.is_empty() || selector.languages.contains(l))
        .collect();
    found.sort();
    found.dedup();
    found
}

/// Checks every test of `expr` against the kind and the languages of the objects it is applied
/// to; a nested selector's own `where` is checked against its own kind and languages.
fn check(
    architecture: &Architecture<'_>,
    rule: &str,
    expr: &Expr,
    kind: Kind,
    languages: &[Language],
) -> Result<(), ElementError> {
    match expr {
        Expr::All(items) | Expr::Any(items) => items
            .iter()
            .try_for_each(|item| check(architecture, rule, item, kind, languages)),
        Expr::Not(inner) => check(architecture, rule, inner, kind, languages),
        Expr::Test(test) => {
            if let Applicability::DoesNotApply(why) = applicability(test.concept, kind) {
                return Err(ElementError::Inapplicable {
                    rule: rule.to_owned(),
                    key: test.key.clone(),
                    kind: kind.as_str().to_owned(),
                    why: why.to_owned(),
                });
            }
            for language in languages {
                if let Unanswerable(why) = capability(test.concept, *language) {
                    return Err(ElementError::Unanswerable {
                        rule: rule.to_owned(),
                        key: test.key.clone(),
                        language: language.as_str().to_owned(),
                        why: why.to_owned(),
                    });
                }
            }
            match nested(&test.operand) {
                Some(selector) => validate_selector(architecture, rule, selector),
                None => Ok(()),
            }
        }
    }
}

/// Checks a selector's `where` against its own kind and languages.
fn validate_selector(
    architecture: &Architecture<'_>,
    rule: &str,
    selector: &Selector,
) -> Result<(), ElementError> {
    match &selector.where_ {
        Some(where_) => check(
            architecture,
            rule,
            where_,
            selector.kind,
            &languages_of(architecture, selector),
        ),
        None => Ok(()),
    }
}

/// Refuses a rule that uses a key some language of its selection cannot answer, or a key that
/// means nothing for the kind of object it is applied to. `select.where` and `should` are
/// checked against `select`; a nested selector's `where` against that selector's own kind and
/// languages.
///
/// # Errors
/// [`ElementError::Inapplicable`] naming the rule, the key and the kind;
/// [`ElementError::Unanswerable`] naming the rule, the key and the language.
pub fn validate(architecture: &Architecture<'_>, rule: &ElementRule) -> Result<(), ElementError> {
    validate_selector(architecture, &rule.name, &rule.select)?;
    check(
        architecture,
        &rule.name,
        &rule.should,
        rule.select.kind,
        &languages_of(architecture, &rule.select),
    )
}

#[cfg(test)]
mod tests {
    use rb_config::elements::parse_elements;
    use rb_model::{CodeLayer, GraphDocument, Location, MemberElement, Module, TypeElement};
    use serde_json::json;

    use super::*;

    /// A .NET class with a method, a Python class, and a TypeScript module.
    fn document() -> GraphDocument {
        let dotnet = || Location::in_file(Language::Dotnet, Some("a.cs".to_owned()));
        let mut service = TypeElement::new("App.Service", "Service", "class", dotnet());
        service.namespace = Some("App".to_owned());
        let mut run = MemberElement::new("App.Service", "Run", "method", dotnet());
        run.full_name = Some("App.Service::Run()".to_owned());
        let python = TypeElement::new(
            "app.core.Order",
            "Order",
            "class",
            Location::in_file(Language::Python, Some("src/app/core.py".to_owned())),
        );
        let mut module = Module::new("src/a.ts");
        module.language = Some(Language::Typescript);
        GraphDocument {
            modules: vec![module],
            code: Some(CodeLayer {
                types: vec![service, python],
                members: vec![run],
                ..CodeLayer::default()
            }),
            ..GraphDocument::default()
        }
    }

    fn validated(rule: serde_json::Value) -> Result<(), ElementError> {
        let document = document();
        let architecture = Architecture::new(&document);
        let mut rule = rule;
        rule["name"] = json!("r");
        let rules = parse_elements(&json!([rule])).map_err(|e| ElementError::Pattern {
            rule: "r".into(),
            pattern: e.to_string(),
        })?;
        validate(&architecture, &rules[0])
    }

    fn inapplicable(key: &str, kind: &str) -> ElementError {
        let why = match applicability(
            rb_config::elements::VOCABULARY
                .iter()
                .find(|(n, ..)| key.to_lowercase().ends_with(&n.to_lowercase()) && !n.is_empty())
                .map_or(rb_config::elements::Concept::Exist, |(_, c, ..)| *c),
            Kind::ALL
                .into_iter()
                .find(|k| k.as_str() == kind)
                .unwrap_or(Kind::Type),
        ) {
            Applicability::DoesNotApply(why) => why.to_owned(),
            Applicability::Applies => String::new(),
        };
        ElementError::Inapplicable {
            rule: "r".into(),
            key: key.into(),
            kind: kind.into(),
            why,
        }
    }

    #[test]
    fn a_type_concept_on_members_and_a_member_concept_on_types_are_refused() {
        let dotnet = json!(["dotnet"]);
        assert_eq!(
            validated(json!({ "select": { "kind": "method", "language": dotnet },
                              "should": { "notBeSealed": true } })),
            Err(inapplicable("notBeSealed", "method"))
        );
        assert_eq!(
            validated(json!({ "select": { "kind": "class", "language": dotnet },
                              "should": { "notBeVirtual": true } })),
            Err(inapplicable("notBeVirtual", "class"))
        );
        assert_eq!(
            validated(
                json!({ "select": { "kind": "method", "where": { "areSealed": true } },
                              "should": { "exist": true } })
            ),
            Err(inapplicable("areSealed", "method")),
            "select.where is checked against the selected kind too"
        );
        assert_eq!(
            validated(json!({ "select": { "kind": "method", "language": dotnet },
                              "should": { "beVirtual": true } })),
            Ok(())
        );
        assert_eq!(
            validated(json!({ "select": { "kind": "class", "language": dotnet },
                              "should": { "any": [{ "beSealed": true }, { "not": { "beStatic": true } }] } })),
            Ok(())
        );
    }

    #[test]
    fn a_diagram_over_members_or_modules_is_refused() {
        for kind in ["method", "member", "property", "field", "module"] {
            assert_eq!(
                validated(json!({ "select": { "kind": kind },
                                  "should": { "adhereToPlantUmlDiagram": "d.puml" } })),
                Err(inapplicable("adhereToPlantUmlDiagram", kind)),
                "{kind}"
            );
        }
        assert_eq!(
            validated(
                json!({ "select": { "kind": "class", "language": ["dotnet"] },
                              "should": { "adhereToPlantUmlDiagram": "d.puml" } })
            ),
            Ok(())
        );
    }

    #[test]
    fn a_nested_selector_is_checked_against_its_own_kind_and_languages() {
        // The outer selection is .NET only; the nested one reaches Python, which cannot answer
        // `sealed`: the old check read the outer languages and let it through.
        assert_eq!(
            validated(json!({
                "select": { "kind": "class", "language": ["dotnet"] },
                "should": { "notDependOnAnyTypesThat": { "kind": "class", "where": { "areSealed": true } } }
            })),
            Err(ElementError::Unanswerable {
                rule: "r".into(),
                key: "areSealed".into(),
                language: "python".into(),
                why: "the language cannot forbid subclassing".into(),
            })
        );
        // The outer selection reaches Python; the nested one is .NET only, which can.
        assert_eq!(
            validated(json!({
                "select": { "kind": "class", "where": { "haveNameStartingWith": "O" } },
                "should": { "notDependOnAnyTypesThat": { "kind": "class", "language": ["dotnet"],
                                                         "where": { "areSealed": true } } }
            })),
            Ok(())
        );
        // The nested kind decides what its predicates may ask, not the outer one.
        assert_eq!(
            validated(json!({
                "select": { "kind": "class", "language": ["dotnet"] },
                "should": { "notDependOnAnyTypesThat": { "kind": "method", "where": { "areSealed": true } } }
            })),
            Err(inapplicable("areSealed", "method"))
        );
        assert_eq!(
            validated(json!({
                "select": { "kind": "method", "language": ["dotnet"] },
                "should": { "notDependOnAnyTypesThat": { "kind": "class", "language": ["dotnet"],
                                                         "where": { "areSealed": true } } }
            })),
            Ok(())
        );
        // A selector nested in select.where is checked the same way.
        assert_eq!(
            validated(json!({
                "select": { "kind": "class", "language": ["dotnet"],
                            "where": { "dependOnAnyTypesThat": { "kind": "field", "where": { "areVirtual": true } } } },
                "should": { "exist": true }
            })),
            Err(inapplicable("areVirtual", "field"))
        );
    }
}
