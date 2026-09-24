//! The capability table: which language can answer each element predicate, and how.
//!
//! - Plan: [Wave 2, Step 5](../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#25-step-5-the-element-rule-engine-and-the-capability-table-2c)
//!   and § 1.4.3 ("cross-language behaviour comes from two data tables")
//! - Decisions: [ADR-0014](../../../../docs/adr/0014-no-invented-cross-language-edges.md) (a key a
//!   language cannot answer is an error, never a silent false),
//!   [ADR-0010](../../../../docs/adr/0010-crate-layout-and-extractor-boundary.md) rule 3 (the
//!   engine has no `match language`: this table is data the engine reads)
//! - Requirement: [FR-RULE-03](../../../../docs/prd.md#fr-rule-03) ("capability table")
//!
//! Every concept has a row for every language: `Answerable`, `Mapped` with the text `docs` and
//! the schema descriptions print, or `Unanswerable` with the reason. A rule that uses an
//! unanswerable key over objects of that language fails validation, naming the key, the language
//! and the rule; `select.language` scopes a rule to the languages that can answer it.

use rb_config::elements::{Concept, ElementRule, Expr, Objects, Operand, Selector};
use rb_model::Language;

use super::{Architecture, ElementError};

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

/// How `language` answers `concept`.
pub fn capability(concept: Concept, language: Language) -> Capability {
    match language {
        Language::Dotnet => Answerable,
        Language::Typescript | Language::Javascript => typescript(concept),
        Language::Python => python(concept),
    }
}

/// Every test in an expression, nested selectors included.
fn tests<'a>(expr: &'a Expr, out: &mut Vec<(&'a rb_config::elements::Test, Option<&'a Selector>)>) {
    match expr {
        Expr::All(items) | Expr::Any(items) => {
            for item in items {
                tests(item, out);
            }
        }
        Expr::Not(inner) => tests(inner, out),
        Expr::Test(test) => {
            let nested = match &test.operand {
                Operand::Objects(Objects::Selector(s))
                | Operand::Attribute {
                    attribute: Some(Objects::Selector(s)),
                    ..
                } => Some(s.as_ref()),
                _ => None,
            };
            out.push((test, nested));
            if let Some(selector) = nested
                && let Some(where_) = &selector.where_
            {
                tests(where_, out);
            }
        }
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

/// Refuses a rule that uses a key some language of its selection cannot answer.
///
/// # Errors
/// [`ElementError::Unanswerable`] naming the rule, the key and the language.
pub fn validate(architecture: &Architecture<'_>, rule: &ElementRule) -> Result<(), ElementError> {
    let languages = languages_of(architecture, &rule.select);
    let mut used = Vec::new();
    if let Some(where_) = &rule.select.where_ {
        tests(where_, &mut used);
    }
    tests(&rule.should, &mut used);
    for (test, _) in used {
        for language in &languages {
            if let Unanswerable(why) = capability(test.concept, *language) {
                return Err(ElementError::Unanswerable {
                    rule: rule.name.clone(),
                    key: test.key.clone(),
                    language: language.as_str().to_owned(),
                    why: why.to_owned(),
                });
            }
        }
    }
    Ok(())
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
