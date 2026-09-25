//! Attribute-argument tests over an attribute whose arguments the .NET extractor could not
//! decode (`argumentsUnknown`): the test is refused, never answered true or false.
//!
//! - Plan: [Wave 2, Step 5](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#25-step-5-the-element-rule-engine-and-the-capability-table-2c)
//! - Decision: [ADR-0014](../../../docs/adr/0014-no-invented-cross-language-edges.md) (no silent
//!   false for a question the document cannot answer)
//! - Requirement: [FR-RULE-03](../../../docs/prd.md#fr-rule-03)

use rb_config::elements::parse_elements;
use rb_model::{
    AttributeElement, CodeLayer, GraphDocument, Language, Location, NamedArgument, TypeElement,
};
use rb_rules::elements::{Architecture, ElementError, Outcome, evaluate};
use serde_json::{Value, json};

fn at() -> Location {
    Location::in_file(Language::Dotnet, Some("A.cs".into()))
}

fn attribute(kind: &str, arguments: &[&str], unknown: bool) -> AttributeElement {
    AttributeElement {
        target: "N.A".into(),
        attribute_type: kind.into(),
        arguments: arguments.iter().map(|a| (*a).to_owned()).collect(),
        named_arguments: if unknown {
            Vec::new()
        } else {
            vec![NamedArgument {
                name: "Level".into(),
                value: "2".into(),
            }]
        },
        arguments_unknown: unknown,
        location: at(),
    }
}

/// `N.A` carries `N.Marker` (arguments unknown) and `N.Other(1, Level = 2)`.
fn document() -> GraphDocument {
    GraphDocument {
        code: Some(CodeLayer {
            types: vec![
                TypeElement::new("N.A", "A", "class", at()),
                TypeElement::new("N.Marker", "Marker", "attribute", at()),
                TypeElement::new("N.Other", "Other", "attribute", at()),
            ],
            attributes: vec![
                attribute("N.Marker", &[], true),
                attribute("N.Other", &["1"], false),
            ],
            ..CodeLayer::default()
        }),
        ..GraphDocument::default()
    }
}

fn element(should: &Value) -> Result<Outcome, ElementError> {
    let rule = json!({ "name": "r", "select": { "kind": "class", "where": { "haveFullName": "N.A" } }, "should": should });
    let rules = parse_elements(&json!([rule])).map_err(|e| ElementError::Pattern {
        rule: "r".into(),
        pattern: e.to_string(),
    })?;
    evaluate(&Architecture::new(&document()), &rules[0])
}

#[test]
fn an_undecodable_attribute_refuses_argument_tests_naming_it() {
    for should in [
        json!({ "haveAttributeWithArguments": { "attribute": "N.Marker", "arguments": [1] } }),
        json!({ "haveAnyAttributesWithArguments": [1] }),
        json!({ "haveAnyAttributesWithNamedArguments": { "Level": 2 } }),
        json!({ "not": { "haveAttributeWithNamedArguments": { "attribute": "N.Marker", "arguments": { "Level": 2 } } } }),
    ] {
        let refused = element(&should);
        assert!(
            matches!(&refused, Err(ElementError::UndecodableArguments { rule, attribute, target, .. })
                if rule == "r" && attribute == "N.Marker" && target == "N.A"),
            "{should}: {refused:?}"
        );
        let message = refused.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(message.contains("N.Marker") && message.contains("could not be decoded"));
    }
}

#[test]
fn a_decoded_attribute_on_the_same_type_is_still_answered() {
    let outcome = element(
        &json!({ "haveAttributeWithArguments": { "attribute": "N.Other", "arguments": [1] } }),
    );
    assert!(
        outcome
            .as_ref()
            .is_ok_and(|o| o.results.iter().all(|r| r.passed)),
        "{outcome:?}"
    );
    let types = element(&json!({ "haveAnyAttributes": ["N.Marker"] }));
    assert!(
        types
            .as_ref()
            .is_ok_and(|o| o.results.iter().all(|r| r.passed)),
        "the attribute's type is known even when its arguments are not: {types:?}"
    );
}
