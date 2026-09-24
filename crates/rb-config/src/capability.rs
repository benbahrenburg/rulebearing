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

use rb_model::Language;

use crate::elements::Concept;

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
