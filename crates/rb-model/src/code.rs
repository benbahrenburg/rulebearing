//! The additive code layer: types, members, attributes and calls, the elements ArchUnitNET's
//! rules read.
//!
//! - Architecture: [The graph document](../../../docs/architecture.md#the-graph-document) (row
//!   "top level `code`")
//! - Decision: [ADR-0004](../../../docs/adr/0004-graph-document-is-cruise-result-superset.md)
//! - Source: [design § The five stages](../../../docs/artifacts/design.md#the-five-stages), stage 3
//! - Plan: [Wave 0, Step 3](../../../docs/plans/pending/0000-wave-0-spike.md#step-3-rb-model-graph-document-schema-violation-id-0a)
//!   freezes the shape; [Wave 2](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md)
//!   fills it
//! - Requirement: [FR-EXT-DN-03](../../../docs/prd.md#fr-ext-dn-03)
//!
//! Every element carries `language`, `file`, `line` and `column`, so an element rule's finding is
//! as locatable as a dependency rule's. The properties are the ones the element predicates read;
//! a language that has no such property leaves it absent rather than `false`
//! ([ADR-0014](../../../docs/adr/0014-no-invented-cross-language-edges.md)).
//!
//! The element structs flatten their [`Location`], and serde cannot combine `flatten` with
//! `deny_unknown_fields`, so unlike the module layer these accept unknown keys on input. They are
//! written only by Rulebearing's own extractors, never read from another tool.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::vocab::{Attribution, Language};

/// `code`: the four element lists.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodeLayer {
    /// Classes, interfaces, enums, structs, records, delegates, type aliases, functions.
    #[serde(default)]
    pub types: Vec<TypeElement>,
    /// Methods, constructors, properties, fields, events and accessors.
    #[serde(default)]
    pub members: Vec<MemberElement>,
    /// Attributes and decorators applied to a type or member.
    #[serde(default)]
    pub attributes: Vec<AttributeElement>,
    /// Call edges between members.
    #[serde(default)]
    pub calls: Vec<CallElement>,
}

/// Where an element was declared.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Location {
    /// The language the element came from.
    pub language: Language,
    /// Repository-relative path, absent when attribution failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// 1-based line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// 1-based column.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
}

/// One type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TypeElement {
    /// Fully qualified name (`CleanArchitecture.Domain.TodoItem`, `src/a.ts#Widget`).
    pub full_name: String,
    /// Simple name.
    pub name: String,
    /// The namespace or module path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    /// `class`, `interface`, `enum`, `struct`, `record`, `delegate`, `type-alias`, `function`.
    pub kind: String,
    /// Where it was declared.
    #[serde(flatten)]
    pub location: Location,
    /// How the file was found (.NET).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attribution: Option<Attribution>,
    /// The assembly (.NET) or package that contains it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assembly: Option<String>,
    /// `public`, `internal`, `protected`, `private`, `protected-internal`, `private-protected`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visibility: Option<String>,
    /// Sealed or final.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sealed: Option<bool>,
    /// Abstract.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#abstract: Option<bool>,
    /// Static.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#static: Option<bool>,
    /// A record type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record: Option<bool>,
    /// Nested inside another type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nested: Option<bool>,
    /// Generic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generic: Option<bool>,
    /// The base type's full name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_type: Option<String>,
    /// Implemented interfaces' full names.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub interfaces: Vec<String>,
}

/// One member of a type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MemberElement {
    /// The declaring type's full name.
    pub declaring_type: String,
    /// Simple name.
    pub name: String,
    /// `method`, `constructor`, `property`, `field`, `event`, `getter`, `setter`.
    pub kind: String,
    /// Where it was declared.
    #[serde(flatten)]
    pub location: Location,
    /// Visibility, as for [`TypeElement::visibility`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visibility: Option<String>,
    /// Static.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#static: Option<bool>,
    /// Virtual or overridable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#virtual: Option<bool>,
    /// Abstract.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#abstract: Option<bool>,
    /// Read-only (a field) or get-only (a property).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub readonly: Option<bool>,
    /// The return or field type's full name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub return_type: Option<String>,
    /// Parameter types' full names, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parameter_types: Vec<String>,
}

/// An attribute or decorator applied to a type or member.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AttributeElement {
    /// The full name of the type or `Type.member` it is applied to.
    pub target: String,
    /// The attribute type's full name.
    pub attribute_type: String,
    /// Constructor and named arguments, as literal text.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arguments: Vec<String>,
    /// Where it was applied.
    #[serde(flatten)]
    pub location: Location,
}

/// A call from one member to another.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CallElement {
    /// The calling member, `Type.member`.
    pub from: String,
    /// The called member, `Type.member`.
    pub to: String,
    /// Where the call is.
    #[serde(flatten)]
    pub location: Location,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn location_flattens_into_the_element() {
        let element = CallElement {
            from: "A.Run".to_owned(),
            to: "B.Save".to_owned(),
            location: Location {
                language: Language::Dotnet,
                file: Some("src/A.cs".to_owned()),
                line: Some(12),
                column: None,
            },
        };
        let json = serde_json::to_string(&element).unwrap_or_default();
        assert_eq!(
            json,
            r#"{"from":"A.Run","to":"B.Save","language":"dotnet","file":"src/A.cs","line":12}"#
        );
        let back: Option<CallElement> = serde_json::from_str(&json).ok();
        assert_eq!(back, Some(element));
    }

    #[test]
    fn absent_properties_are_omitted_not_false() {
        let element = TypeElement {
            full_name: "src/a.ts#Widget".to_owned(),
            name: "Widget".to_owned(),
            namespace: None,
            kind: "class".to_owned(),
            location: Location {
                language: Language::Typescript,
                file: Some("src/a.ts".to_owned()),
                line: Some(3),
                column: Some(1),
            },
            attribution: None,
            assembly: None,
            visibility: None,
            sealed: None,
            r#abstract: None,
            r#static: None,
            record: None,
            nested: None,
            generic: None,
            base_type: None,
            interfaces: vec![],
        };
        let json = serde_json::to_string(&element).unwrap_or_default();
        assert_eq!(
            json,
            r#"{"fullName":"src/a.ts#Widget","name":"Widget","kind":"class","language":"typescript","file":"src/a.ts","line":3,"column":1}"#
        );
    }
}
