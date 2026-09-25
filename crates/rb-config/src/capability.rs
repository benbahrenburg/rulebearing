//! The capability table: which language can answer each element predicate, and how.
//!
//! - Plan: [Wave 2, Step 5](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#25-step-5-the-element-rule-engine-and-the-capability-table-2c)
//!   and § 1.4.3 ("cross-language behaviour comes from two data tables")
//! - Decisions: [ADR-0014](../../../docs/adr/0014-no-invented-cross-language-edges.md) (a key a
//!   language cannot answer is an error, never a silent false),
//!   [ADR-0010](../../../docs/adr/0010-crate-layout-and-extractor-boundary.md) rule 3 (the
//!   engine has no `match language`: this table is data the engine reads)
//! - Requirement: [FR-RULE-03](../../../docs/prd.md#fr-rule-03) ("capability table")
//!
//! Every concept has a row for every language: `Answerable`, `Mapped` with the text `docs` and
//! the schema descriptions print, or `Unanswerable` with the reason. It lives beside the
//! vocabulary it describes, so the configuration schema and the engine read the same rows;
//! `rb-rules` validates a rule against it.
//!
//! A second table, [`applicability`], says for every concept and every `select.kind` whether the
//! concept means anything for the objects of that kind: `beSealed` for a type, not for a method;
//! `beVirtual` for a method or property, not for a type or a field; `adhereToPlantUmlDiagram`
//! for types and functions, whose namespaces a diagram's components name. A test whose concept
//! does not apply to the kind it is applied to (the rule's `select.kind`, or a nested selector's
//! own kind) is exit 3 naming the rule, the key and the kind, never a fixed answer.
//!
//! `kind: module` selects the modules of the module layer. A module has a name (its file name),
//! a full name (its source), dependencies (its resolved imports; `onlyDependOn` judges the ones
//! on modules of the run that are neither core modules nor unresolved) and a namespace, as
//! [`module_namespace`] reads it per language, the mapping the capability table's
//! `resideInNamespace` rows state: a TypeScript or JavaScript module's path, a Python module's
//! dotted name, the namespaces a .NET file declares (any of them). It has no visibility,
//! attributes, assembly, calls or members of its own, so every other concept does not apply.
//!
//! A third table, [`records`], serves the cross-language keys of dependency rules
//! ([design § Dependency rules](../../../docs/artifacts/design.md#dependency-rules-the-whole-of-dependency-cruiser-1820),
//! [Wave 2, Step 8](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#28-step-8-cross-language-rule-additions-per-language-dependencytypes-license-moreunstable-2d)):
//! which module and edge properties each language's extractor writes. `namespace(Not)` reads a
//! module's `namespaces[]`, `project(Not)` its `project`, `assembly(Not)` the assembly of the
//! code-layer types in its file, `dependencyKind(Not)` the edge's `dependencyKind`.
//!
//! | Property | .NET | TypeScript, JavaScript | Python |
//! | --- | --- | --- | --- |
//! | `namespaces` | the namespaces the file declares | not recorded | the dotted module name |
//! | `project` | the `.csproj` (or loaded assembly) path | not recorded | the top-level package |
//! | assembly | the assembly of the file's types | not recorded | not recorded (no assemblies) |
//! | `dependencyKind` | `inherits`, `implements`, `attribute`, ... | `import` | `import` |
//!
//! A rule whose `from` or `to` narrows by a property that a language it can select does not
//! record is exit 3 naming the rule, the side, the key and the language, unless that side's
//! `language` key leaves the language out: the key would otherwise be false for every module of
//! that language, a silent answer ADR-0014 forbids. A module of a recording language that lacks
//! the property (a core or external module, which no extractor analysed) matches neither the key
//! nor its `Not` form.

use rb_model::Language;

use crate::elements::{Concept, Kind};

/// How a language answers a concept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    /// As `ArchUnitNET` defines it.
    Answerable,
    /// Through the stated mapping.
    Mapped(&'static str),
    /// Not at all, for the stated reason.
    Unanswerable(&'static str),
}

use Capability::{Answerable, Mapped, Unanswerable};

const NO_ASSEMBLIES: Capability = Unanswerable("the language has no assemblies");
const NO_VALUE_TYPES: Capability = Unanswerable("the language has no value types");
const NO_SEALED: Capability = Unanswerable("the language cannot forbid subclassing");
const NO_SUCH_ACCESS: Capability = Unanswerable("the language has no such access level");

/// How a TypeScript or JavaScript module answers a concept.
fn typescript(concept: Concept) -> Capability {
    match concept {
        Concept::Public | Concept::Internal => Mapped(
            "reads `export`: an exported declaration is public, any other internal to its module",
        ),
        Concept::Private | Concept::Protected => Mapped(
            "reads the `private` and `protected` modifiers and `#private` names on class members",
        ),
        Concept::ProtectedInternal | Concept::PrivateProtected => NO_SUCH_ACCESS,
        Concept::HaveAssemblyQualifiedName
        | Concept::HaveAssemblyQualifiedNameMatching
        | Concept::HaveAssemblyQualifiedNameStartingWith
        | Concept::HaveAssemblyQualifiedNameEndingWith
        | Concept::HaveAssemblyQualifiedNameContaining
        | Concept::ResideInAssembly
        | Concept::ResideInAssemblyMatching => NO_ASSEMBLIES,
        Concept::ResideInNamespace | Concept::ResideInNamespaceMatching => {
            Mapped("the namespace is the module's path")
        }
        Concept::HaveAnyAttributes | Concept::OnlyHaveAttributes => Mapped("reads decorators"),
        Concept::HaveAttributeWithArguments
        | Concept::HaveAttributeWithNamedArguments
        | Concept::HaveAnyAttributesWithArguments
        | Concept::HaveAnyAttributesWithNamedArguments => Mapped("reads decorator arguments"),
        Concept::AssignableTo | Concept::ImplementInterface | Concept::ImplementAnyInterfaces => {
            Mapped("reads `extends` and `implements`")
        }
        Concept::Structs | Concept::ValueTypes => NO_VALUE_TYPES,
        Concept::Sealed => NO_SEALED,
        Concept::Record => Unanswerable("the language has no record types"),
        Concept::Immutable | Concept::ReadOnly => {
            Mapped("reads `readonly` fields and get-only accessors")
        }
        Concept::Virtual => Unanswerable("every method can be overridden"),
        Concept::HaveReturnType => Mapped("reads the return type annotation"),
        Concept::HaveDependencyInMethodBodyTo => Mapped("reads `new` expressions in method bodies"),
        Concept::HaveInitOnlySetter => Unanswerable("the language has no init-only setters"),
        Concept::HavePublicGetter
        | Concept::HaveProtectedGetter
        | Concept::HavePrivateGetter
        | Concept::HavePublicSetter
        | Concept::HaveProtectedSetter
        | Concept::HavePrivateSetter
        | Concept::HaveGetter
        | Concept::HaveSetter => Mapped("reads `get` and `set` accessors"),
        Concept::HaveInternalGetter
        | Concept::HaveProtectedInternalGetter
        | Concept::HavePrivateProtectedGetter
        | Concept::HaveInternalSetter
        | Concept::HaveProtectedInternalSetter
        | Concept::HavePrivateProtectedSetter => {
            Unanswerable("the language has no such access level")
        }
        _ => Answerable,
    }
}

/// How a Python module answers a concept.
fn python(concept: Concept) -> Capability {
    match concept {
        Concept::Public | Concept::Private => {
            Mapped("reads the leading underscore: `_name` is private, any other name public")
        }
        Concept::Protected
        | Concept::Internal
        | Concept::ProtectedInternal
        | Concept::PrivateProtected => {
            Unanswerable("the language has only the leading-underscore convention")
        }
        Concept::HaveAssemblyQualifiedName
        | Concept::HaveAssemblyQualifiedNameMatching
        | Concept::HaveAssemblyQualifiedNameStartingWith
        | Concept::HaveAssemblyQualifiedNameEndingWith
        | Concept::HaveAssemblyQualifiedNameContaining
        | Concept::ResideInAssembly
        | Concept::ResideInAssemblyMatching => NO_ASSEMBLIES,
        Concept::ResideInNamespace | Concept::ResideInNamespaceMatching => {
            Mapped("the namespace is the dotted module path")
        }
        Concept::HaveAnyAttributes | Concept::OnlyHaveAttributes => Mapped("reads decorators"),
        Concept::HaveAttributeWithArguments
        | Concept::HaveAttributeWithNamedArguments
        | Concept::HaveAnyAttributesWithArguments
        | Concept::HaveAnyAttributesWithNamedArguments => Mapped("reads decorator arguments"),
        Concept::AssignableTo | Concept::ImplementInterface | Concept::ImplementAnyInterfaces => {
            Mapped("reads base classes")
        }
        Concept::Enums => Unanswerable("enumerations are ordinary classes"),
        Concept::Structs | Concept::ValueTypes => NO_VALUE_TYPES,
        Concept::Sealed => NO_SEALED,
        Concept::Record => Unanswerable("the language has no record types"),
        Concept::Abstract => Mapped("an `ABC` base or an `@abstractmethod`"),
        Concept::Immutable => Mapped("`@dataclass(frozen=True)`"),
        Concept::Generic => Unanswerable("generic parameters are not recorded"),
        Concept::Static => Mapped("`@staticmethod` and `@classmethod`"),
        Concept::ReadOnly => Mapped("a `@property` without a setter"),
        Concept::Constructor => Mapped("`__init__`"),
        Concept::Virtual => Unanswerable("every method can be overridden"),
        Concept::HaveReturnType => Unanswerable("return annotations are not recorded"),
        Concept::HaveDependencyInMethodBodyTo | Concept::CallAny | Concept::CalledBy => {
            Unanswerable("method bodies are not analysed")
        }
        Concept::HaveGetter | Concept::HaveSetter => Mapped("`@property` and `@name.setter`"),
        Concept::HaveInitOnlySetter
        | Concept::HavePublicGetter
        | Concept::HaveProtectedGetter
        | Concept::HaveInternalGetter
        | Concept::HaveProtectedInternalGetter
        | Concept::HavePrivateGetter
        | Concept::HavePrivateProtectedGetter
        | Concept::HavePublicSetter
        | Concept::HaveProtectedSetter
        | Concept::HaveInternalSetter
        | Concept::HaveProtectedInternalSetter
        | Concept::HavePrivateSetter
        | Concept::HavePrivateProtectedSetter => {
            Unanswerable("accessors have no access level of their own")
        }
        _ => Answerable,
    }
}

/// What a slice groups in a language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SliceUnit {
    /// Its types by namespace, joined by their dependencies, as `ArchUnitNET`'s slices are.
    Types,
    /// Its modules, by path or dotted name, joined by their imports: the design's path pattern
    /// for TypeScript and dotted module pattern for Python, where an import is the dependency.
    Modules,
}

/// What a slice groups in `language`.
pub fn slice_unit(language: Language) -> SliceUnit {
    match language {
        Language::Dotnet => SliceUnit::Types,
        Language::Typescript | Language::Javascript | Language::Python => SliceUnit::Modules,
    }
}

/// How `language` answers `concept`.
pub fn capability(concept: Concept, language: Language) -> Capability {
    match language {
        Language::Dotnet => Answerable,
        Language::Typescript | Language::Javascript => typescript(concept),
        Language::Python => python(concept),
    }
}

/// Whether a concept means anything for the objects a `select.kind` selects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Applicability {
    /// It does.
    Applies,
    /// It does not, for the stated reason.
    DoesNotApply(&'static str),
}

use Applicability::{Applies, DoesNotApply};

/// The kinds that select types: `ArchUnitNET`'s `Types()`, `Classes()`, `Interfaces()`,
/// `Attributes()`.
const fn is_type_kind(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::Type | Kind::Class | Kind::Interface | Kind::Attribute
    )
}

/// The reason a concept of a type's shape does not apply to `kind`.
const fn not_a_type(kind: Kind) -> Applicability {
    match kind {
        Kind::Module => DoesNotApply("a module is not a type"),
        Kind::Function => DoesNotApply("a function is not a type"),
        _ => DoesNotApply("a member is not a type"),
    }
}

/// Whether `concept` means anything for the objects `kind` selects. Total: every concept has a
/// cell for every kind.
#[expect(
    clippy::too_many_lines,
    reason = "one arm per group of concepts, kept in one table as the capability table is"
)]
pub const fn applicability(concept: Concept, kind: Kind) -> Applicability {
    let types = is_type_kind(kind);
    let member = kind.is_member();
    match concept {
        // Every object has an identity, a name, a full name, a namespace and dependencies.
        Concept::Identity
        | Concept::Exist
        | Concept::HaveName
        | Concept::HaveNameMatching
        | Concept::HaveNameStartingWith
        | Concept::HaveNameEndingWith
        | Concept::HaveNameContaining
        | Concept::HaveFullName
        | Concept::HaveFullNameMatching
        | Concept::HaveFullNameStartingWith
        | Concept::HaveFullNameEndingWith
        | Concept::HaveFullNameContaining
        | Concept::ResideInNamespace
        | Concept::ResideInNamespaceMatching
        | Concept::DependOnAny
        | Concept::OnlyDependOn => Applies,
        // Every declaration has these; a module is a file, not a declaration.
        Concept::Public
        | Concept::Private
        | Concept::Protected
        | Concept::Internal
        | Concept::ProtectedInternal
        | Concept::PrivateProtected => match kind {
            Kind::Module => DoesNotApply("a module has no visibility of its own"),
            _ => Applies,
        },
        Concept::HaveAssemblyQualifiedName
        | Concept::HaveAssemblyQualifiedNameMatching
        | Concept::HaveAssemblyQualifiedNameStartingWith
        | Concept::HaveAssemblyQualifiedNameEndingWith
        | Concept::HaveAssemblyQualifiedNameContaining
        | Concept::ResideInAssembly
        | Concept::ResideInAssemblyMatching => match kind {
            Kind::Module => DoesNotApply("a module is not in an assembly; its types are"),
            _ => Applies,
        },
        Concept::HaveAnyAttributes
        | Concept::OnlyHaveAttributes
        | Concept::HaveAttributeWithArguments
        | Concept::HaveAttributeWithNamedArguments
        | Concept::HaveAnyAttributesWithArguments
        | Concept::HaveAnyAttributesWithNamedArguments => match kind {
            Kind::Module => DoesNotApply("a module carries no attributes; its declarations do"),
            _ => Applies,
        },
        Concept::CallAny => match kind {
            Kind::Module => DoesNotApply("a module makes no calls; its members do"),
            _ => Applies,
        },
        // The shape of a type.
        Concept::AssignableTo
        | Concept::ImplementInterface
        | Concept::ImplementAnyInterfaces
        | Concept::Enums
        | Concept::Structs
        | Concept::ValueTypes
        | Concept::Nested
        | Concept::NestedIn
        | Concept::HaveMemberWithName
        | Concept::HaveFieldMemberWithName
        | Concept::HaveMethodMemberWithName
        | Concept::HavePropertyMemberWithName
        | Concept::Generic => {
            if types {
                Applies
            } else {
                not_a_type(kind)
            }
        }
        // Classes only: an interface is never sealed and never a record.
        Concept::Sealed | Concept::Record => match kind {
            Kind::Interface => DoesNotApply("an interface is neither sealed nor a record"),
            _ if types => Applies,
            _ => not_a_type(kind),
        },
        // Types and the members that can be abstract.
        Concept::Abstract => match kind {
            Kind::Field => DoesNotApply("a field cannot be abstract"),
            _ if types || member => Applies,
            _ => not_a_type(kind),
        },
        Concept::Static | Concept::Immutable => {
            if types || member {
                Applies
            } else {
                not_a_type(kind)
            }
        }
        // Members.
        Concept::DeclaredIn => {
            if member {
                Applies
            } else {
                DoesNotApply("only a member is declared in a type")
            }
        }
        Concept::ReadOnly => match kind {
            Kind::Member | Kind::Field | Kind::Property => Applies,
            Kind::Method => DoesNotApply("a method is never read-only; a field or property is"),
            _ => DoesNotApply("only a field or property is read-only"),
        },
        Concept::Virtual => match kind {
            Kind::Member | Kind::Method | Kind::Property => Applies,
            Kind::Field => DoesNotApply("a field cannot be virtual"),
            _ => DoesNotApply("only a method or property is virtual"),
        },
        Concept::Constructor
        | Concept::HaveReturnType
        | Concept::HaveDependencyInMethodBodyTo
        | Concept::CalledBy => match kind {
            Kind::Member | Kind::Method => Applies,
            _ => DoesNotApply("only a method has this"),
        },
        Concept::HaveGetter
        | Concept::HaveSetter
        | Concept::HaveInitOnlySetter
        | Concept::HavePublicGetter
        | Concept::HaveProtectedGetter
        | Concept::HaveInternalGetter
        | Concept::HaveProtectedInternalGetter
        | Concept::HavePrivateGetter
        | Concept::HavePrivateProtectedGetter
        | Concept::HavePublicSetter
        | Concept::HaveProtectedSetter
        | Concept::HaveInternalSetter
        | Concept::HaveProtectedInternalSetter
        | Concept::HavePrivateSetter
        | Concept::HavePrivateProtectedSetter => match kind {
            Kind::Member | Kind::Property => Applies,
            _ => DoesNotApply("only a property has accessors"),
        },
        // A diagram's components name the namespaces of types and functions.
        Concept::AdhereToPlantUmlDiagram => match kind {
            Kind::Function => Applies,
            _ if types => Applies,
            _ => DoesNotApply(
                "a diagram's components hold types; select `type`, `class`, `interface`, `attribute` or `function`",
            ),
        },
    }
}

/// What a module object's namespace is, per language: the mapping the capability table's
/// `resideInNamespace` rows state, applied to `kind: module`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleNamespace {
    /// The module's path (`source`), as a TypeScript type's namespace is its file.
    Path,
    /// The module's `namespaces[]`: a Python module's dotted name, the namespaces a .NET file
    /// declares; a test holds when any of them does.
    Namespaces,
}

/// What the namespace of a module in `language` is; a module without a language (a core or
/// external module) is known by its path.
pub const fn module_namespace(language: Option<Language>) -> ModuleNamespace {
    match language {
        Some(Language::Python | Language::Dotnet) => ModuleNamespace::Namespaces,
        Some(Language::Typescript | Language::Javascript) | None => ModuleNamespace::Path,
    }
}

/// A module or edge property a cross-language dependency key reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ModuleProperty {
    /// `namespaces[]`, read by `namespace` and `namespaceNot`.
    Namespaces,
    /// `project`, read by `project` and `projectNot`.
    Project,
    /// The assembly of the code-layer types in the module's file, read by `assembly` and
    /// `assemblyNot`.
    Assembly,
    /// The edge's `dependencyKind`, read by `dependencyKind` and `dependencyKindNot`.
    DependencyKind,
}

impl ModuleProperty {
    /// The property a cross-language key reads, by the key's configuration spelling; `None`
    /// for `language`, which every extractor records, and for any other key.
    pub fn of_key(key: &str) -> Option<Self> {
        match key {
            "namespace" | "namespaceNot" => Some(Self::Namespaces),
            "project" | "projectNot" => Some(Self::Project),
            "assembly" | "assemblyNot" => Some(Self::Assembly),
            "dependencyKind" | "dependencyKindNot" => Some(Self::DependencyKind),
            _ => None,
        }
    }
}

/// Whether a language's extractor records a property.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recorded {
    /// It does, with what it records.
    Yes(&'static str),
    /// It does not, for the stated reason.
    No(&'static str),
}

/// Whether the extractor of `language` records `property` on the modules (or, for
/// `dependencyKind`, the edges) it writes. Total: every language has a cell for every property.
pub const fn records(language: Language, property: ModuleProperty) -> Recorded {
    use ModuleProperty::{Assembly, DependencyKind, Namespaces, Project};
    use Recorded::{No, Yes};
    const NO_NAMESPACES: Recorded =
        No("a TypeScript or JavaScript module has no namespaces; narrow it by `path`");
    const NO_PROJECT: Recorded =
        No("a TypeScript or JavaScript module records no project; narrow it by `path`");
    match (language, property) {
        (Language::Dotnet, Namespaces) => Yes("the namespaces the file declares"),
        (Language::Dotnet, Project) => Yes("the `.csproj` (or loaded assembly) path"),
        (Language::Dotnet, Assembly) => Yes("the assembly of the file's types"),
        (Language::Dotnet, DependencyKind) => {
            Yes("`inherits`, `implements`, `attribute` and the other .NET kinds")
        }
        (Language::Python, Namespaces) => Yes("the dotted module name"),
        (Language::Python, Project) => Yes("the top-level package"),
        (Language::Python, Assembly) => No("the language has no assemblies"),
        (Language::Typescript | Language::Javascript, Namespaces) => NO_NAMESPACES,
        (Language::Typescript | Language::Javascript, Project) => NO_PROJECT,
        (Language::Typescript | Language::Javascript, Assembly) => {
            No("the language has no assemblies")
        }
        (Language::Python | Language::Typescript | Language::Javascript, DependencyKind) => {
            Yes("`import`")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_concept_has_a_row_for_every_language() {
        for concept in Concept::all() {
            for language in [
                Language::Dotnet,
                Language::Typescript,
                Language::Javascript,
                Language::Python,
            ] {
                // The table is total by construction; this pins that .NET answers everything and
                // that a mapping or a reason is never empty.
                match capability(concept, language) {
                    Answerable => {}
                    Mapped(text) | Unanswerable(text) => {
                        assert!(!text.is_empty(), "{concept:?} {language:?}");
                    }
                }
            }
            assert_eq!(
                capability(concept, Language::Dotnet),
                Answerable,
                "{concept:?}"
            );
        }
    }

    #[test]
    fn every_concept_has_a_cell_for_every_kind_and_the_kinds_are_right() {
        let applies = |concept: Concept| -> Vec<&'static str> {
            Kind::ALL
                .into_iter()
                .filter(|k| applicability(concept, *k) == Applies)
                .map(Kind::as_str)
                .collect()
        };
        for concept in Concept::all() {
            for kind in Kind::ALL {
                if let DoesNotApply(why) = applicability(concept, kind) {
                    assert!(!why.is_empty(), "{concept:?} {kind:?}");
                }
            }
        }
        let everything = Kind::ALL.map(Kind::as_str).to_vec();
        let code: Vec<&str> = everything
            .iter()
            .copied()
            .filter(|k| *k != "module")
            .collect();
        let types = ["type", "class", "interface", "attribute"];
        for concept in [
            Concept::Identity,
            Concept::Exist,
            Concept::HaveName,
            Concept::HaveFullNameMatching,
            Concept::ResideInNamespace,
            Concept::ResideInNamespaceMatching,
            Concept::DependOnAny,
            Concept::OnlyDependOn,
        ] {
            assert_eq!(applies(concept), everything, "{concept:?}");
        }
        for concept in [
            Concept::Public,
            Concept::PrivateProtected,
            Concept::HaveAssemblyQualifiedName,
            Concept::ResideInAssembly,
            Concept::HaveAnyAttributes,
            Concept::HaveAnyAttributesWithNamedArguments,
            Concept::CallAny,
        ] {
            assert_eq!(applies(concept), code, "{concept:?}");
        }
        for concept in [
            Concept::AssignableTo,
            Concept::Enums,
            Concept::Nested,
            Concept::HaveMemberWithName,
            Concept::Generic,
        ] {
            assert_eq!(applies(concept), types, "{concept:?}");
        }
        for concept in [Concept::Sealed, Concept::Record] {
            assert_eq!(
                applies(concept),
                ["type", "class", "attribute"],
                "{concept:?}"
            );
        }
    }

    fn kinds_of(concept: Concept) -> Vec<&'static str> {
        Kind::ALL
            .into_iter()
            .filter(|k| applicability(concept, *k) == Applies)
            .map(Kind::as_str)
            .collect()
    }

    #[test]
    fn member_concepts_apply_to_the_member_kinds_that_have_them() {
        let applies = kinds_of;
        assert_eq!(
            applies(Concept::Abstract),
            [
                "type",
                "class",
                "interface",
                "attribute",
                "member",
                "method",
                "property"
            ]
        );
        for concept in [Concept::Static, Concept::Immutable] {
            assert_eq!(
                applies(concept),
                [
                    "type",
                    "class",
                    "interface",
                    "attribute",
                    "member",
                    "field",
                    "method",
                    "property"
                ],
                "{concept:?}"
            );
        }
        assert_eq!(
            applies(Concept::DeclaredIn),
            ["member", "field", "method", "property"]
        );
        assert_eq!(applies(Concept::ReadOnly), ["member", "field", "property"]);
        assert_eq!(applies(Concept::Virtual), ["member", "method", "property"]);
        for concept in [
            Concept::Constructor,
            Concept::HaveReturnType,
            Concept::HaveDependencyInMethodBodyTo,
            Concept::CalledBy,
        ] {
            assert_eq!(applies(concept), ["member", "method"], "{concept:?}");
        }
        for concept in [
            Concept::HaveGetter,
            Concept::HaveInitOnlySetter,
            Concept::HavePrivateProtectedSetter,
        ] {
            assert_eq!(applies(concept), ["member", "property"], "{concept:?}");
        }
        assert_eq!(
            applies(Concept::AdhereToPlantUmlDiagram),
            ["type", "class", "interface", "attribute", "function"]
        );
        assert!(matches!(
            applicability(Concept::Sealed, Kind::Method),
            DoesNotApply(why) if why.contains("member")
        ));
        assert!(matches!(
            applicability(Concept::Sealed, Kind::Module),
            DoesNotApply(why) if why.contains("module")
        ));
        assert!(matches!(
            applicability(Concept::Enums, Kind::Function),
            DoesNotApply(why) if why.contains("function")
        ));
        assert!(matches!(
            applicability(Concept::Public, Kind::Module),
            DoesNotApply(why) if why.contains("visibility")
        ));
    }

    #[test]
    fn a_module_namespace_is_its_path_or_its_namespaces() {
        assert_eq!(
            module_namespace(Some(Language::Typescript)),
            ModuleNamespace::Path
        );
        assert_eq!(
            module_namespace(Some(Language::Javascript)),
            ModuleNamespace::Path
        );
        assert_eq!(module_namespace(None), ModuleNamespace::Path);
        assert_eq!(
            module_namespace(Some(Language::Python)),
            ModuleNamespace::Namespaces
        );
        assert_eq!(
            module_namespace(Some(Language::Dotnet)),
            ModuleNamespace::Namespaces
        );
    }

    #[test]
    fn what_each_extractor_records() {
        use ModuleProperty::{Assembly, DependencyKind, Namespaces, Project};
        let recorded = |language: Language| -> Vec<ModuleProperty> {
            [Namespaces, Project, Assembly, DependencyKind]
                .into_iter()
                .filter(|p| matches!(records(language, *p), Recorded::Yes(_)))
                .collect()
        };
        assert_eq!(
            recorded(Language::Dotnet),
            [Namespaces, Project, Assembly, DependencyKind]
        );
        assert_eq!(
            recorded(Language::Python),
            [Namespaces, Project, DependencyKind]
        );
        for language in [Language::Typescript, Language::Javascript] {
            assert_eq!(recorded(language), [DependencyKind], "{language:?}");
            assert!(matches!(
                records(language, Namespaces),
                Recorded::No(why) if why.contains("path")
            ));
        }
        for language in [
            Language::Dotnet,
            Language::Typescript,
            Language::Javascript,
            Language::Python,
        ] {
            for property in [Namespaces, Project, Assembly, DependencyKind] {
                let (Recorded::Yes(text) | Recorded::No(text)) = records(language, property);
                assert!(!text.is_empty(), "{language:?} {property:?}");
            }
        }
        let keys = [
            ("namespace", Some(Namespaces)),
            ("namespaceNot", Some(Namespaces)),
            ("project", Some(Project)),
            ("projectNot", Some(Project)),
            ("assembly", Some(Assembly)),
            ("assemblyNot", Some(Assembly)),
            ("dependencyKind", Some(DependencyKind)),
            ("dependencyKindNot", Some(DependencyKind)),
            ("language", None),
            ("path", None),
        ];
        for (key, property) in keys {
            assert_eq!(ModuleProperty::of_key(key), property, "{key}");
        }
    }

    #[test]
    fn slices_group_types_in_dotnet_and_modules_elsewhere() {
        assert_eq!(slice_unit(Language::Dotnet), SliceUnit::Types);
        for language in [Language::Typescript, Language::Javascript, Language::Python] {
            assert_eq!(slice_unit(language), SliceUnit::Modules, "{language:?}");
        }
    }

    #[test]
    fn the_design_mappings_hold() {
        assert!(
            matches!(capability(Concept::Public, Language::Typescript), Mapped(t) if t.contains("export"))
        );
        assert!(
            matches!(capability(Concept::Private, Language::Python), Mapped(t) if t.contains("underscore"))
        );
        assert!(
            matches!(capability(Concept::HaveAnyAttributes, Language::Python), Mapped(t) if t.contains("decorators"))
        );
        assert!(matches!(
            capability(Concept::Sealed, Language::Typescript),
            Unanswerable(_)
        ));
        assert!(matches!(
            capability(Concept::Sealed, Language::Python),
            Unanswerable(_)
        ));
        assert_eq!(
            capability(Concept::HaveName, Language::Javascript),
            Answerable
        );
    }
}
