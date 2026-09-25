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
    /// Every base type up the chain, nearest first, as far as the loaded code declares them
    /// (`areAssignableTo` walks it).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub base_types: Vec<String>,
    /// The enclosing type's full name, for a nested type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nested_in: Option<String>,
    /// Immutable: every field read-only and no settable property (a frozen dataclass in Python).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub immutable: Option<bool>,
    /// A value type (.NET struct or enum).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_type: Option<bool>,
    /// The assembly-qualified name in reflection's spelling (.NET).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assembly_qualified_name: Option<String>,
    /// The containing assembly's display name, `Name, Version=1.0.0.0, Culture=neutral,
    /// PublicKeyToken=null` (.NET): what `resideInAssembly` compares.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assembly_full_name: Option<String>,
    /// A type the analysed code references but does not define, as `ArchUnitNET` holds it among
    /// `ReferencedTypes`: a selection leaves it out unless it sets `includeReferenced`, while
    /// names and the predicates nested in a relation condition see it. Its kind is
    /// `unavailable` when no assembly beside the analysed ones defines it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub referenced: Option<bool>,
    /// Every other file that declares part of the type (a C# partial type), sorted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
    /// The types this type depends on, sorted by target then kind.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<ElementDependency>,
}

/// One dependency of a type or member on a type: what `dependOnAny`, `onlyDependOn` and
/// `haveDependencyInMethodBodyTo` read.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ElementDependency {
    /// The target type's full name.
    pub target: String,
    /// What formed the dependency: `inherits`, `implements`, `field`, `signature`, `body`,
    /// `attribute`, `generic-argument`, `typeof`, `import`.
    pub kind: String,
    /// The member reference that formed it, when there is one (`Save`, `_context`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member: Option<String>,
    /// 1-based line of the reference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// For a `body` dependency, how the body used the type: `call`, `access` (a field),
    /// `cast`, `type-check` or `body-type` (a local, `box`, `newarr` and the like), the
    /// distinction `haveDependencyInMethodBodyTo` reads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub form: Option<String>,
}

/// An accessor of a property: whether it exists is `Some`, and its visibility.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Accessor {
    /// Visibility, as for [`TypeElement::visibility`].
    pub visibility: String,
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
    /// The member's full name in `ArchUnitNET`'s spelling (`Namespace.Type::Method(System.Int32)`
    /// for .NET, `module.Class.method` for Python).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full_name: Option<String>,
    /// The property's getter, when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub getter: Option<Accessor>,
    /// The property's setter, when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setter: Option<Accessor>,
    /// The property's `init` setter (C# 9), when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub init_setter: Option<Accessor>,
    /// The types this member depends on, sorted by target then kind.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<ElementDependency>,
}

/// An attribute or decorator applied to a type or member.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AttributeElement {
    /// The full name of the type or `Type.member` it is applied to.
    pub target: String,
    /// The attribute type's full name.
    pub attribute_type: String,
    /// Positional (constructor) arguments, as literal text: a string's value, a number, `true`,
    /// a type's full name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arguments: Vec<String>,
    /// Named arguments (fields and properties set in the attribute), in declaration order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub named_arguments: Vec<NamedArgument>,
    /// Additive (.NET): the attribute's value could not be decoded (a malformed blob, or an
    /// enum argument whose underlying type no loaded assembly says), so `arguments` and
    /// `namedArguments` are unknown rather than empty. A rule on this attribute's arguments is
    /// refused, never answered.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub arguments_unknown: bool,
    /// Where it was applied.
    #[serde(flatten)]
    pub location: Location,
}

/// One named argument of an attribute.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NamedArgument {
    /// The field or property name.
    pub name: String,
    /// The value, as literal text.
    pub value: String,
}

impl Location {
    /// A location in `file`, with no line or column yet.
    pub fn in_file(language: Language, file: Option<String>) -> Self {
        Self {
            language,
            file,
            line: None,
            column: None,
        }
    }
}

impl TypeElement {
    /// A type with its identity and location and every property absent, for an extractor to fill.
    pub fn new(
        full_name: impl Into<String>,
        name: impl Into<String>,
        kind: impl Into<String>,
        location: Location,
    ) -> Self {
        Self {
            full_name: full_name.into(),
            name: name.into(),
            namespace: None,
            kind: kind.into(),
            location,
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
            interfaces: Vec::new(),
            base_types: Vec::new(),
            nested_in: None,
            immutable: None,
            value_type: None,
            assembly_qualified_name: None,
            assembly_full_name: None,
            referenced: None,
            files: Vec::new(),
            dependencies: Vec::new(),
        }
    }
}

impl MemberElement {
    /// A member with its identity and location and every property absent.
    pub fn new(
        declaring_type: impl Into<String>,
        name: impl Into<String>,
        kind: impl Into<String>,
        location: Location,
    ) -> Self {
        Self {
            declaring_type: declaring_type.into(),
            name: name.into(),
            kind: kind.into(),
            location,
            visibility: None,
            r#static: None,
            r#virtual: None,
            r#abstract: None,
            readonly: None,
            return_type: None,
            parameter_types: Vec::new(),
            full_name: None,
            getter: None,
            setter: None,
            init_setter: None,
            dependencies: Vec::new(),
        }
    }
}

impl CodeLayer {
    /// Sorts every list into its output order, so two runs serialise byte for byte: types by full
    /// name, members by declaring type then full name, attributes by target, calls by endpoints.
    pub fn normalise(&mut self) {
        for ty in &mut self.types {
            ty.dependencies.sort();
            ty.dependencies.dedup();
            ty.files.sort();
            ty.files.dedup();
        }
        for member in &mut self.members {
            member.dependencies.sort();
            member.dependencies.dedup();
        }
        self.types.sort_by(|a, b| a.full_name.cmp(&b.full_name));
        self.members.sort_by(|a, b| {
            (&a.declaring_type, &a.full_name, &a.name, &a.kind).cmp(&(
                &b.declaring_type,
                &b.full_name,
                &b.name,
                &b.kind,
            ))
        });
        self.attributes.sort_by(|a, b| {
            (&a.target, &a.attribute_type, &a.arguments).cmp(&(
                &b.target,
                &b.attribute_type,
                &b.arguments,
            ))
        });
        self.calls.sort_by(|a, b| {
            (&a.from, &a.to, a.location.line).cmp(&(&b.from, &b.to, b.location.line))
        });
        self.calls.dedup();
    }

    /// Appends another extractor's elements.
    pub fn merge(&mut self, other: Self) {
        self.types.extend(other.types);
        self.members.extend(other.members);
        self.attributes.extend(other.attributes);
        self.calls.extend(other.calls);
    }
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
        let element = TypeElement::new(
            "src/a.ts#Widget",
            "Widget",
            "class",
            Location {
                language: Language::Typescript,
                file: Some("src/a.ts".to_owned()),
                line: Some(3),
                column: Some(1),
            },
        );
        let json = serde_json::to_string(&element).unwrap_or_default();
        assert_eq!(
            json,
            r#"{"fullName":"src/a.ts#Widget","name":"Widget","kind":"class","language":"typescript","file":"src/a.ts","line":3,"column":1}"#
        );
    }

    fn dep(target: &str, kind: &str) -> ElementDependency {
        ElementDependency {
            target: target.to_owned(),
            kind: kind.to_owned(),
            member: None,
            line: None,
            form: None,
        }
    }

    fn at(file: &str) -> Location {
        Location::in_file(Language::Dotnet, Some(file.to_owned()))
    }

    #[test]
    fn constructors_leave_every_property_absent() {
        let location = at("A.cs");
        assert_eq!(location.line, None);
        assert_eq!(location.column, None);
        let ty = TypeElement::new("N.A", "A", "class", location.clone());
        assert_eq!(
            (ty.full_name.as_str(), ty.name.as_str(), ty.kind.as_str()),
            ("N.A", "A", "class")
        );
        assert_eq!(ty.location, location);
        let json = serde_json::to_string(&ty).unwrap_or_default();
        assert_eq!(
            json,
            r#"{"fullName":"N.A","name":"A","kind":"class","language":"dotnet","file":"A.cs"}"#
        );
        let member = MemberElement::new("N.A", "Run", "method", at("A.cs"));
        assert_eq!(
            (
                member.declaring_type.as_str(),
                member.name.as_str(),
                member.kind.as_str()
            ),
            ("N.A", "Run", "method")
        );
        let json = serde_json::to_string(&member).unwrap_or_default();
        assert_eq!(
            json,
            r#"{"declaringType":"N.A","name":"Run","kind":"method","language":"dotnet","file":"A.cs"}"#
        );
    }

    #[test]
    fn normalise_sorts_and_dedups_every_list() {
        let mut b = TypeElement::new("N.B", "B", "class", at("B.cs"));
        b.dependencies = vec![dep("N.Z", "body"), dep("N.A", "field"), dep("N.Z", "body")];
        b.files = vec!["y.cs".into(), "x.cs".into(), "y.cs".into()];
        let a = TypeElement::new("N.A", "A", "class", at("A.cs"));
        let mut run = MemberElement::new("N.B", "Run", "method", at("B.cs"));
        run.dependencies = vec![dep("N.Y", "body"), dep("N.X", "body"), dep("N.X", "body")];
        let mut stop = MemberElement::new("N.A", "Stop", "method", at("A.cs"));
        stop.full_name = Some("N.A::Stop()".into());
        let attribute = |target: &str| AttributeElement {
            target: target.to_owned(),
            attribute_type: "Obsolete".to_owned(),
            arguments: vec![],
            named_arguments: vec![],
            arguments_unknown: false,
            location: at("A.cs"),
        };
        let call = |from: &str, line: u32| CallElement {
            from: from.to_owned(),
            to: "N.A.Stop".to_owned(),
            location: Location {
                line: Some(line),
                ..at("B.cs")
            },
        };
        let mut layer = CodeLayer {
            types: vec![b, a],
            members: vec![run, stop],
            attributes: vec![attribute("N.B"), attribute("N.A")],
            calls: vec![call("N.B.Run", 9), call("N.B.Run", 3), call("N.B.Run", 3)],
        };
        layer.normalise();
        let names: Vec<&str> = layer.types.iter().map(|t| t.full_name.as_str()).collect();
        assert_eq!(names, ["N.A", "N.B"]);
        let b = &layer.types[1];
        assert_eq!(
            b.dependencies,
            vec![dep("N.A", "field"), dep("N.Z", "body")]
        );
        assert_eq!(b.files, ["x.cs", "y.cs"]);
        let members: Vec<&str> = layer.members.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(members, ["Stop", "Run"]);
        assert_eq!(
            layer.members[1].dependencies,
            vec![dep("N.X", "body"), dep("N.Y", "body")]
        );
        let targets: Vec<&str> = layer.attributes.iter().map(|a| a.target.as_str()).collect();
        assert_eq!(targets, ["N.A", "N.B"]);
        let lines: Vec<Option<u32>> = layer.calls.iter().map(|c| c.location.line).collect();
        assert_eq!(lines, [Some(3), Some(9)]);
    }

    #[test]
    fn merge_appends_every_list() {
        let mut left = CodeLayer {
            types: vec![TypeElement::new("A", "A", "class", at("A.cs"))],
            ..CodeLayer::default()
        };
        let right = CodeLayer {
            types: vec![TypeElement::new("B", "B", "class", at("B.cs"))],
            members: vec![MemberElement::new("B", "m", "method", at("B.cs"))],
            attributes: vec![AttributeElement {
                target: "B".into(),
                attribute_type: "X".into(),
                arguments: vec!["1".into()],
                named_arguments: vec![NamedArgument {
                    name: "Name".into(),
                    value: "v".into(),
                }],
                arguments_unknown: false,
                location: at("B.cs"),
            }],
            calls: vec![CallElement {
                from: "B.m".into(),
                to: "A.n".into(),
                location: at("B.cs"),
            }],
        };
        left.merge(right);
        assert_eq!(left.types.len(), 2);
        assert_eq!(left.members.len(), 1);
        assert_eq!(left.attributes.len(), 1);
        assert_eq!(left.calls.len(), 1);
        let json = serde_json::to_string(&left.attributes[0]).unwrap_or_default();
        assert_eq!(
            json,
            r#"{"target":"B","attributeType":"X","arguments":["1"],"namedArguments":[{"name":"Name","value":"v"}],"language":"dotnet","file":"B.cs"}"#
        );
    }
}
