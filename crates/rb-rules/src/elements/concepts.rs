//! What each element predicate and condition means, as `ArchUnitNET` 0.13.4 defines it.
//!
//! - Plan: [Wave 2, Step 5](../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#25-step-5-the-element-rule-engine-and-the-capability-table-2c)
//!   ("semantics to get right, each proven by a ported test")
//! - Specification: `ArchUnitNET` 0.13.4 `ObjectPredicatesDefinition`, `TypePredicatesDefinition`,
//!   `MemberPredicatesDefinition`, `MethodMemberPredicatesDefinition`,
//!   `PropertyMemberPredicatesDefinition` and `Domain/Extensions/NamingExtensions.cs`; proven by
//!   conformance gate 2
//!
//! Where `ArchUnitNET` defines a negation that is not the logical negation of its positive
//! (`DoNotHaveAttributeWithArguments` means none of the arguments appears on any instance, not "no
//! instance carries all of them"; `DoNotImplementAnyInterfaces` holds for an empty list), the
//! negation here follows it: [`test()`] asks each concept for its own negative. Name comparisons
//! follow `NamingExtensions`: `HaveName`, `HaveFullName` and the assembly-qualified and full-name
//! prefix, suffix and substring tests ignore case, the simple-name prefix, suffix and substring
//! tests do not.

use std::collections::BTreeSet;

use rb_config::capability::{ModuleNamespace, module_namespace};
use rb_config::elements::{Concept, Operand, Test};
use rb_model::AttributeElement;

use super::{ElementError, Evaluator, Object, operand_keys};

fn eq_ignore_case(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b) || a.to_lowercase() == b.to_lowercase()
}

fn starts_ignore_case(text: &str, prefix: &str) -> bool {
    text.to_lowercase().starts_with(&prefix.to_lowercase())
}

fn ends_ignore_case(text: &str, suffix: &str) -> bool {
    text.to_lowercase().ends_with(&suffix.to_lowercase())
}

fn contains_ignore_case(text: &str, part: &str) -> bool {
    text.to_lowercase().contains(&part.to_lowercase())
}

/// The object's namespaces: a type's own, a member's declaring type's, and a module's as the
/// capability table maps it per language ([`module_namespace`]): its path, or its
/// `namespaces[]`, any of which a namespace test may hold for.
fn namespaces<'a>(e: &Evaluator<'_, 'a>, object: &Object<'a>) -> Vec<&'a str> {
    match object {
        Object::Type(t) => vec![t.namespace.as_deref().unwrap_or_default()],
        Object::Member(m) => vec![
            e.architecture()
                .types
                .get(m.declaring_type.as_str())
                .and_then(|t| t.namespace.as_deref())
                .unwrap_or_default(),
        ],
        Object::Module(m) => match module_namespace(m.language) {
            ModuleNamespace::Path => vec![m.source.as_str()],
            ModuleNamespace::Namespaces => {
                m.namespaces.iter().flatten().map(String::as_str).collect()
            }
        },
    }
}

/// The declaring type of a member, or the type itself.
fn owning_type<'a>(
    e: &Evaluator<'_, 'a>,
    object: &Object<'a>,
) -> Option<&'a rb_model::TypeElement> {
    match object {
        Object::Type(t) => Some(t),
        Object::Member(m) => e
            .architecture()
            .types
            .get(m.declaring_type.as_str())
            .copied(),
        Object::Module(_) => None,
    }
}

fn assembly_names<'a>(
    e: &Evaluator<'_, 'a>,
    object: &Object<'a>,
) -> (Option<&'a str>, Option<&'a str>) {
    owning_type(e, object).map_or((None, None), |t| {
        (t.assembly_full_name.as_deref(), t.assembly.as_deref())
    })
}

fn assembly_qualified_name(e: &Evaluator<'_, '_>, object: &Object<'_>) -> Option<String> {
    match object {
        Object::Type(t) => t.assembly_qualified_name.clone(),
        Object::Member(m) => {
            let assembly = owning_type(e, object).and_then(|t| t.assembly_full_name.clone())?;
            Some(format!(
                "{}, {assembly}",
                m.full_name.as_deref().unwrap_or(&m.name)
            ))
        }
        Object::Module(_) => None,
    }
}

fn visibility<'a>(object: &Object<'a>) -> Option<&'a str> {
    match object {
        Object::Type(t) => t.visibility.as_deref(),
        Object::Member(m) => m.visibility.as_deref(),
        Object::Module(_) => None,
    }
}

/// What an object depends on: the full names a type or member depends on, the sources a
/// module's imports resolve to.
fn dependencies<'a>(object: &Object<'a>) -> Vec<&'a str> {
    match object {
        Object::Type(t) => t.dependencies.iter().map(|d| d.target.as_str()).collect(),
        Object::Member(m) => m.dependencies.iter().map(|d| d.target.as_str()).collect(),
        Object::Module(m) => m.dependencies.iter().map(|d| d.resolved.as_str()).collect(),
    }
}

/// The dependencies `onlyDependOn` judges, as `ArchUnitNET` judges only those on the
/// architecture's own types: a type's or member's on types the code defines, a module's on
/// modules of the run through an edge that is neither a core module nor unresolved.
fn own_dependencies<'a>(e: &Evaluator<'_, 'a>, object: &Object<'a>) -> Vec<&'a str> {
    let architecture = e.architecture();
    match object {
        Object::Module(m) => m
            .dependencies
            .iter()
            .filter(|d| {
                !d.core_module
                    && !d.could_not_resolve
                    && architecture.module_sources.contains(d.resolved.as_str())
            })
            .map(|d| d.resolved.as_str())
            .collect(),
        _ => dependencies(object)
            .into_iter()
            .filter(|target| {
                architecture
                    .types
                    .get(target)
                    .is_some_and(|t| t.referenced != Some(true))
            })
            .collect(),
    }
}

/// What a type is assignable to: itself, its base chain and its interfaces
/// (`TypeExtensions.GetAssignableTypes`).
fn assignable<'a>(object: &Object<'a>) -> BTreeSet<&'a str> {
    match object {
        Object::Type(t) => std::iter::once(t.full_name.as_str())
            .chain(t.base_types.iter().map(String::as_str))
            .chain(t.base_type.iter().map(String::as_str))
            .chain(t.interfaces.iter().map(String::as_str))
            .collect(),
        _ => BTreeSet::new(),
    }
}

/// Every argument value of every instance of the attributes `filter` accepts: positional values
/// and named values (`GetAllAttributeArgumentValues`).
fn argument_values<'a>(
    e: &Evaluator<'_, 'a>,
    object: &Object<'a>,
    filter: &dyn Fn(&str) -> bool,
    key: &str,
) -> Result<Vec<Vec<&'a str>>, ElementError> {
    Ok(decoded_attributes(e, object, filter, key)?
        .into_iter()
        .map(|a| {
            a.arguments
                .iter()
                .map(String::as_str)
                .chain(a.named_arguments.iter().map(|n| n.value.as_str()))
                .collect()
        })
        .collect())
}

/// The instances of the attributes `filter` accepts on `object`, refusing the test when one of
/// them has arguments the extractor could not decode (`argumentsUnknown`).
fn decoded_attributes<'a>(
    e: &Evaluator<'_, 'a>,
    object: &Object<'a>,
    filter: &dyn Fn(&str) -> bool,
    key: &str,
) -> Result<Vec<&'a AttributeElement>, ElementError> {
    let instances: Vec<&'a AttributeElement> = e
        .architecture()
        .attributes_of
        .get(object.key())
        .into_iter()
        .flatten()
        .copied()
        .filter(|a| filter(&a.attribute_type))
        .collect();
    if let Some(unknown) = instances.iter().find(|a| a.arguments_unknown) {
        return Err(ElementError::UndecodableArguments {
            rule: e.rule().to_owned(),
            key: key.to_owned(),
            attribute: unknown.attribute_type.clone(),
            target: unknown.target.clone(),
        });
    }
    Ok(instances)
}

/// Every `(name, value)` named argument of every instance of the attributes `filter` accepts.
fn named_values<'a>(
    e: &Evaluator<'_, 'a>,
    object: &Object<'a>,
    filter: &dyn Fn(&str) -> bool,
    key: &str,
) -> Result<Vec<Vec<(&'a str, &'a str)>>, ElementError> {
    Ok(decoded_attributes(e, object, filter, key)?
        .into_iter()
        .map(|a| {
            a.named_arguments
                .iter()
                .map(|n| (n.name.as_str(), n.value.as_str()))
                .collect()
        })
        .collect())
}

fn attribute_types<'a>(e: &Evaluator<'_, 'a>, object: &Object<'a>) -> BTreeSet<&'a str> {
    e.architecture()
        .attributes_of
        .get(object.key())
        .into_iter()
        .flatten()
        .map(|a| a.attribute_type.as_str())
        .collect()
}

/// `(holds, holds when negated)` for an attribute-argument concept, per `ArchUnitNET`: the
/// positive asks for one instance carrying every value, the negative for no value on any instance.
fn attribute_arguments<T: PartialEq>(instances: &[Vec<T>], wanted: &[T]) -> (bool, bool) {
    let positive = instances
        .iter()
        .any(|values| wanted.iter().all(|w| values.contains(w)));
    let negative = !wanted
        .iter()
        .any(|w| instances.iter().any(|values| values.contains(w)));
    (positive, negative)
}

fn names_of(operand: &Operand) -> &[String] {
    match operand {
        Operand::Names(names) => names,
        _ => &[],
    }
}

fn pattern_matches(e: &Evaluator<'_, '_>, pattern: &str, text: &str) -> Result<bool, ElementError> {
    if crate::patterns::get(pattern).is_none() {
        return Err(e.pattern_error(pattern));
    }
    Ok(crate::patterns::test(pattern, text))
}

/// The generic definition of a signature type name: ``Ns.Box`1<Ns.A>`` is ``Ns.Box`1``.
fn generic_definition(name: &str) -> &str {
    name.split('<').next().unwrap_or(name)
}

/// Whether `object` satisfies `test`, its negation applied.
///
/// # Errors
/// [`ElementError`] for an unknown object name or a bad pattern.
#[expect(
    clippy::too_many_lines,
    reason = "one arm per ArchUnitNET concept, kept in one table"
)]
pub fn test<'a>(
    e: &Evaluator<'_, 'a>,
    object: &Object<'a>,
    test: &Test,
) -> Result<bool, ElementError> {
    let key = object.key();
    let name = object.name();
    let any_name = |f: &dyn Fn(&str) -> bool| names_of(&test.operand).iter().any(|n| f(n));
    let member = match object {
        Object::Member(m) => Some(*m),
        _ => None,
    };
    let ty = match object {
        Object::Type(t) => Some(*t),
        _ => None,
    };
    let flag = |value: Option<bool>| value.unwrap_or(false);
    // `(positive, negative)`: most concepts negate plainly; a few define their own negative.
    let (positive, negative): (bool, Option<bool>) = match test.concept {
        Concept::Identity => (operand_keys(e, &test.operand)?.contains(key), None),
        // `ExistsCondition`: every selected object exists, so `exist` passes it and
        // `notExist` fails it; the empty selection is judged in `evaluate`.
        Concept::Exist => (true, None),
        Concept::Public => (visibility(object) == Some("public"), None),
        Concept::Private => (visibility(object) == Some("private"), None),
        Concept::Protected => (visibility(object) == Some("protected"), None),
        Concept::Internal => (visibility(object) == Some("internal"), None),
        Concept::ProtectedInternal => (visibility(object) == Some("protected-internal"), None),
        Concept::PrivateProtected => (visibility(object) == Some("private-protected"), None),
        Concept::HaveName => (any_name(&|n| eq_ignore_case(name, n)), None),
        Concept::HaveNameStartingWith => (any_name(&|n| name.starts_with(n)), None),
        Concept::HaveNameEndingWith => (any_name(&|n| name.ends_with(n)), None),
        Concept::HaveNameContaining => (any_name(&|n| name.contains(n)), None),
        Concept::HaveFullName => (any_name(&|n| eq_ignore_case(key, n)), None),
        Concept::HaveFullNameStartingWith => (any_name(&|n| starts_ignore_case(key, n)), None),
        Concept::HaveFullNameEndingWith => (any_name(&|n| ends_ignore_case(key, n)), None),
        Concept::HaveFullNameContaining => (any_name(&|n| contains_ignore_case(key, n)), None),
        Concept::HaveNameMatching
        | Concept::HaveFullNameMatching
        | Concept::HaveAssemblyQualifiedNameMatching
        | Concept::ResideInNamespaceMatching
        | Concept::ResideInAssemblyMatching => {
            let Operand::Pattern(pattern) = &test.operand else {
                return Ok(false);
            };
            let subjects: Vec<String> = match test.concept {
                Concept::HaveNameMatching => vec![name.to_owned()],
                Concept::HaveFullNameMatching => vec![key.to_owned()],
                Concept::HaveAssemblyQualifiedNameMatching => {
                    assembly_qualified_name(e, object).into_iter().collect()
                }
                Concept::ResideInNamespaceMatching => namespaces(e, object)
                    .into_iter()
                    .map(str::to_owned)
                    .collect(),
                _ => assembly_names(e, object)
                    .0
                    .into_iter()
                    .map(str::to_owned)
                    .collect(),
            };
            let mut hit = false;
            for subject in &subjects {
                if pattern_matches(e, pattern, subject)? {
                    hit = true;
                    break;
                }
            }
            (hit, None)
        }
        Concept::HaveAssemblyQualifiedName
        | Concept::HaveAssemblyQualifiedNameStartingWith
        | Concept::HaveAssemblyQualifiedNameEndingWith
        | Concept::HaveAssemblyQualifiedNameContaining => {
            let aqn = assembly_qualified_name(e, object).unwrap_or_default();
            let f: fn(&str, &str) -> bool = match test.concept {
                Concept::HaveAssemblyQualifiedName => eq_ignore_case,
                Concept::HaveAssemblyQualifiedNameStartingWith => starts_ignore_case,
                Concept::HaveAssemblyQualifiedNameEndingWith => ends_ignore_case,
                _ => contains_ignore_case,
            };
            (any_name(&|n| f(&aqn, n)), None)
        }
        Concept::ResideInNamespace => (
            namespaces(e, object)
                .iter()
                .any(|ns| any_name(&|n| eq_ignore_case(ns, n))),
            None,
        ),
        Concept::ResideInAssembly => {
            let (full, simple) = assembly_names(e, object);
            (
                any_name(&|n| {
                    full.is_some_and(|f| eq_ignore_case(f, n))
                        || simple.is_some_and(|s| eq_ignore_case(s, n))
                }),
                None,
            )
        }
        Concept::DependOnAny => {
            let wanted = operand_keys(e, &test.operand)?;
            (
                dependencies(object).iter().any(|d| wanted.contains(*d)),
                None,
            )
        }
        Concept::OnlyDependOn => {
            let allowed = operand_keys(e, &test.operand)?;
            (
                own_dependencies(e, object)
                    .iter()
                    .all(|d| allowed.contains(*d)),
                None,
            )
        }
        Concept::CallAny => {
            let wanted = operand_keys(e, &test.operand)?;
            let calls = &e.architecture().calls_from;
            let called: Vec<&str> = match object {
                Object::Type(t) => e
                    .architecture()
                    .members_of
                    .get(t.full_name.as_str())
                    .into_iter()
                    .flatten()
                    .filter_map(|m| m.full_name.as_deref())
                    .flat_map(|f| calls.get(f).into_iter().flatten().copied())
                    .collect(),
                _ => calls.get(key).into_iter().flatten().copied().collect(),
            };
            (called.iter().any(|c| wanted.contains(*c)), None)
        }
        Concept::HaveAnyAttributes => {
            let wanted = operand_keys(e, &test.operand)?;
            (
                attribute_types(e, object)
                    .iter()
                    .any(|a| wanted.contains(*a)),
                None,
            )
        }
        Concept::OnlyHaveAttributes => {
            let allowed = operand_keys(e, &test.operand)?;
            (
                attribute_types(e, object)
                    .iter()
                    .all(|a| allowed.contains(*a)),
                None,
            )
        }
        Concept::HaveAttributeWithArguments | Concept::HaveAnyAttributesWithArguments => {
            let Operand::Attribute {
                attribute,
                positional,
                ..
            } = &test.operand
            else {
                return Ok(false);
            };
            let attribute_keys = attribute.as_ref().map(|a| e.resolve(a)).transpose()?;
            let filter = |t: &str| attribute_keys.as_ref().is_none_or(|k| k.contains(t));
            let instances = argument_values(e, object, &filter, &test.key)?;
            let wanted: Vec<&str> = positional.iter().map(String::as_str).collect();
            let (p, n) = attribute_arguments(&instances, &wanted);
            (p, Some(n))
        }
        Concept::HaveAttributeWithNamedArguments | Concept::HaveAnyAttributesWithNamedArguments => {
            let Operand::Attribute {
                attribute, named, ..
            } = &test.operand
            else {
                return Ok(false);
            };
            let attribute_keys = attribute.as_ref().map(|a| e.resolve(a)).transpose()?;
            let filter = |t: &str| attribute_keys.as_ref().is_none_or(|k| k.contains(t));
            let instances = named_values(e, object, &filter, &test.key)?;
            let wanted: Vec<(&str, &str)> = named
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();
            let (p, n) = attribute_arguments(&instances, &wanted);
            (p, Some(n))
        }
        Concept::AssignableTo => {
            let wanted = operand_keys(e, &test.operand)?;
            (assignable(object).iter().any(|a| wanted.contains(*a)), None)
        }
        Concept::ImplementInterface => {
            // A named interface outside the code is not an error here: ArchUnitNET's
            // ImplementInterface(Type) answers false, and its negation true.
            let wanted: super::Keys = match &test.operand {
                Operand::Objects(rb_config::elements::Objects::Names(names)) => {
                    std::rc::Rc::new(names.iter().cloned().collect())
                }
                other => operand_keys(e, other)?,
            };
            (
                ty.is_some_and(|t| t.interfaces.iter().any(|i| wanted.contains(i))),
                None,
            )
        }
        Concept::ImplementAnyInterfaces => {
            let wanted = operand_keys(e, &test.operand)?;
            let hit = ty.is_some_and(|t| t.interfaces.iter().any(|i| wanted.contains(i)));
            (!wanted.is_empty() && hit, Some(wanted.is_empty() || !hit))
        }
        Concept::Enums => (ty.is_some_and(|t| t.kind == "enum"), None),
        Concept::Structs => (ty.is_some_and(|t| t.kind == "struct"), None),
        Concept::ValueTypes => (
            ty.is_some_and(|t| matches!(t.kind.as_str(), "enum" | "struct")),
            None,
        ),
        Concept::Nested => (ty.is_some_and(|t| flag(t.nested)), None),
        Concept::NestedIn => {
            let outer = operand_keys(e, &test.operand)?;
            (
                outer.iter().any(|o| key.starts_with(&format!("{o}+"))),
                None,
            )
        }
        Concept::HaveMemberWithName
        | Concept::HaveFieldMemberWithName
        | Concept::HaveMethodMemberWithName
        | Concept::HavePropertyMemberWithName => {
            let wanted = names_of(&test.operand);
            let kind_ok = |m: &rb_model::MemberElement| match test.concept {
                Concept::HaveFieldMemberWithName => m.kind == "field",
                Concept::HaveMethodMemberWithName => m.kind == "method" || m.kind == "constructor",
                Concept::HavePropertyMemberWithName => m.kind == "property",
                _ => true,
            };
            let members = ty.and_then(|t| e.architecture().members_of.get(t.full_name.as_str()));
            (
                members
                    .into_iter()
                    .flatten()
                    .any(|m| kind_ok(m) && wanted.iter().any(|w| eq_ignore_case(&m.name, w))),
                None,
            )
        }
        Concept::Abstract => (
            ty.map_or_else(
                || member.is_some_and(|m| flag(m.r#abstract)),
                |t| flag(t.r#abstract),
            ),
            None,
        ),
        Concept::Sealed => (ty.is_some_and(|t| flag(t.sealed)), None),
        Concept::Record => (ty.is_some_and(|t| flag(t.record)), None),
        Concept::Generic => (ty.is_some_and(|t| flag(t.generic)), None),
        Concept::Static => (
            ty.map_or_else(
                || member.is_some_and(|m| flag(m.r#static)),
                |t| flag(t.r#static),
            ),
            None,
        ),
        Concept::Immutable => (
            match (ty, member) {
                (_, Some(m)) => member_immutable(m),
                (Some(t), None) => t.immutable.unwrap_or_else(|| {
                    e.architecture()
                        .members_of
                        .get(t.full_name.as_str())
                        .into_iter()
                        .flatten()
                        // Static state is not the instance's: ArchUnitNET reads instance
                        // fields and properties only.
                        .filter(|m| {
                            matches!(m.kind.as_str(), "field" | "property") && !flag(m.r#static)
                        })
                        .all(|m| member_immutable(m))
                }),
                _ => false,
            },
            None,
        ),
        Concept::ReadOnly => (
            member.is_some_and(|m| match m.kind.as_str() {
                "field" => flag(m.readonly),
                "property" => m.setter.is_none() && m.init_setter.is_none(),
                _ => false,
            }),
            None,
        ),
        Concept::DeclaredIn => {
            let types = operand_keys(e, &test.operand)?;
            (
                member.is_some_and(|m| types.contains(&m.declaring_type)),
                None,
            )
        }
        Concept::Constructor => (member.is_some_and(|m| m.kind == "constructor"), None),
        Concept::Virtual => (member.is_some_and(|m| flag(m.r#virtual)), None),
        Concept::HaveReturnType => {
            let wanted: super::Keys = match &test.operand {
                Operand::Objects(rb_config::elements::Objects::Names(names)) => {
                    std::rc::Rc::new(names.iter().cloned().collect())
                }
                other => operand_keys(e, other)?,
            };
            (
                member
                    .and_then(|m| m.return_type.as_deref())
                    .is_some_and(|r| wanted.contains(r) || wanted.contains(generic_definition(r))),
                None,
            )
        }
        Concept::HaveDependencyInMethodBodyTo => {
            let wanted = operand_keys(e, &test.operand)?;
            (
                member.is_some_and(|m| {
                    m.dependencies.iter().any(|d| {
                        d.kind == "body"
                            && d.form.as_deref() == Some("body-type")
                            && wanted.contains(&d.target)
                    })
                }),
                None,
            )
        }
        Concept::CalledBy => {
            let types = operand_keys(e, &test.operand)?;
            let callers = e.architecture().callers_of.get(key);
            (
                callers.into_iter().flatten().any(|c| types.contains(*c)),
                None,
            )
        }
        Concept::HaveGetter => (member.is_some_and(|m| m.getter.is_some()), None),
        Concept::HaveSetter => (
            member.is_some_and(|m| m.setter.is_some() || m.init_setter.is_some()),
            None,
        ),
        Concept::HaveInitOnlySetter => (member.is_some_and(|m| m.init_setter.is_some()), None),
        Concept::HavePublicGetter => (getter_is(member, "public"), None),
        Concept::HaveProtectedGetter => (getter_is(member, "protected"), None),
        Concept::HaveInternalGetter => (getter_is(member, "internal"), None),
        Concept::HaveProtectedInternalGetter => (getter_is(member, "protected-internal"), None),
        Concept::HavePrivateGetter => (getter_is(member, "private"), None),
        Concept::HavePrivateProtectedGetter => (getter_is(member, "private-protected"), None),
        Concept::HavePublicSetter => (setter_is(member, "public"), None),
        Concept::HaveProtectedSetter => (setter_is(member, "protected"), None),
        Concept::HaveInternalSetter => (setter_is(member, "internal"), None),
        Concept::HaveProtectedInternalSetter => (setter_is(member, "protected-internal"), None),
        Concept::HavePrivateSetter => (setter_is(member, "private"), None),
        Concept::HavePrivateProtectedSetter => (setter_is(member, "private-protected"), None),
        Concept::AdhereToPlantUmlDiagram => {
            let Operand::Diagram(path) = &test.operand else {
                return Ok(false);
            };
            (crate::plantuml::adheres(e, object, path)?, None)
        }
    };
    Ok(if test.negated {
        negative.unwrap_or(!positive)
    } else {
        positive
    })
}

/// `Writability.IsImmutable()`: a method always, a field when read-only, a property with no
/// setter or only an `init` setter.
fn member_immutable(m: &rb_model::MemberElement) -> bool {
    match m.kind.as_str() {
        "field" => m.readonly.unwrap_or(false),
        "property" => m.setter.is_none(),
        _ => true,
    }
}

fn getter_is(member: Option<&rb_model::MemberElement>, visibility: &str) -> bool {
    member
        .and_then(|m| m.getter.as_ref())
        .is_some_and(|g| g.visibility == visibility)
}

fn setter_is(member: Option<&rb_model::MemberElement>, visibility: &str) -> bool {
    member
        .and_then(|m| m.setter.as_ref().or(m.init_setter.as_ref()))
        .is_some_and(|s| s.visibility == visibility)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::Architecture;

    #[test]
    fn name_comparisons_follow_naming_extensions() {
        assert!(eq_ignore_case("Order", "order"));
        assert!(starts_ignore_case("Ns.Order", "ns."));
        assert!(ends_ignore_case("Ns.Order", "ORDER"));
        assert!(contains_ignore_case("Ns.Order", "s.o"));
        assert!(!eq_ignore_case("Order", "Orders"));
        // Beyond ASCII, case still folds: `NamingExtensions` compares with `ToLower`.
        assert!(eq_ignore_case("Écrire", "écrire"));
        assert!(!eq_ignore_case("Écrire", "ecrire"));
        assert_eq!(generic_definition("Ns.Box`1<Ns.A>"), "Ns.Box`1");
        assert_eq!(generic_definition("Ns.A"), "Ns.A");
    }

    #[test]
    fn attribute_argument_negation_is_archunitnets_not_logical() {
        let instances = vec![vec!["a", "b"], vec!["c"]];
        assert_eq!(attribute_arguments(&instances, &["a", "b"]), (true, false));
        // No instance carries both a and c, yet a appears: neither the positive nor the negative.
        assert_eq!(attribute_arguments(&instances, &["a", "c"]), (false, false));
        assert_eq!(attribute_arguments(&instances, &["z"]), (false, true));
        assert_eq!(attribute_arguments::<&str>(&[], &[]), (false, true));
        assert_eq!(attribute_arguments(&instances, &[]), (true, true));
    }

    #[test]
    fn writability_decides_immutability() {
        let location = rb_model::Location::in_file(rb_model::Language::Dotnet, None);
        let mut field = rb_model::MemberElement::new("T", "f", "field", location.clone());
        assert!(!member_immutable(&field));
        field.readonly = Some(true);
        assert!(member_immutable(&field));
        let mut property = rb_model::MemberElement::new("T", "p", "property", location.clone());
        assert!(member_immutable(&property));
        property.init_setter = Some(rb_model::Accessor {
            visibility: "public".into(),
        });
        assert!(
            member_immutable(&property),
            "an init setter keeps it immutable"
        );
        property.setter = Some(rb_model::Accessor {
            visibility: "private".into(),
        });
        assert!(!member_immutable(&property));
        assert!(setter_is(Some(&property), "private"));
        assert!(!getter_is(Some(&property), "public"));
        let method = rb_model::MemberElement::new("T", "m()", "method", location);
        assert!(member_immutable(&method));
    }

    /// `S.T` declares a field `x`, a method `M` that calls `S.U::Callee()` and has two body
    /// dependencies on `S.U` (one a call, one a local's type), a constructor `.ctor` and a
    /// property `P`; `S.U` declares `Callee`.
    fn members_document() -> rb_model::GraphDocument {
        let at = || rb_model::Location::in_file(rb_model::Language::Dotnet, None);
        let member = |ty: &str, name: &str, kind: &str, full: &str| {
            let mut m = rb_model::MemberElement::new(ty, name, kind, at());
            m.full_name = Some(full.to_owned());
            m
        };
        let body = |target: &str, form: &str| rb_model::ElementDependency {
            target: target.to_owned(),
            kind: "body".to_owned(),
            member: None,
            line: None,
            form: Some(form.to_owned()),
        };
        let mut method = member("S.T", "M", "method", "S.T::M()");
        method.dependencies = vec![body("S.U", "call")];
        let mut local = member("S.T", "L", "method", "S.T::L()");
        local.dependencies = vec![body("S.V", "body-type")];
        rb_model::GraphDocument {
            code: Some(rb_model::CodeLayer {
                types: vec![
                    rb_model::TypeElement::new("S.T", "T", "class", at()),
                    rb_model::TypeElement::new("S.U", "U", "class", at()),
                    rb_model::TypeElement::new("S.V", "V", "class", at()),
                ],
                members: vec![
                    member("S.T", "x", "field", "S.T::x"),
                    method,
                    local,
                    member("S.T", ".ctor", "constructor", "S.T::.ctor()"),
                    member("S.T", "P", "property", "S.T::P"),
                    member("S.U", "Callee", "method", "S.U::Callee()"),
                ],
                calls: vec![rb_model::CallElement {
                    from: "S.T::M()".to_owned(),
                    to: "S.U::Callee()".to_owned(),
                    location: at(),
                }],
                ..rb_model::CodeLayer::default()
            }),
            ..rb_model::GraphDocument::default()
        }
    }

    fn check(
        e: &Evaluator<'_, '_>,
        object: &Object<'_>,
        concept: Concept,
        operand: Operand,
    ) -> Result<bool, ElementError> {
        test(
            e,
            object,
            &Test {
                key: "k".to_owned(),
                concept,
                negated: false,
                operand,
            },
        )
    }

    fn names(names: &[&str]) -> Operand {
        Operand::Names(names.iter().map(|n| (*n).to_owned()).collect())
    }

    fn objects(names: &[&str]) -> Operand {
        Operand::Objects(rb_config::elements::Objects::Names(
            names.iter().map(|n| (*n).to_owned()).collect(),
        ))
    }

    #[test]
    fn member_name_concepts_match_only_their_own_kind() -> Result<(), ElementError> {
        let document = members_document();
        let architecture = Architecture::new(&document);
        let e = Evaluator::new(&architecture, "r");
        let t = Object::Type(architecture.types["S.T"]);
        let table = [
            (Concept::HaveMemberWithName, "x", true),
            (Concept::HaveMemberWithName, "P", true),
            (Concept::HaveFieldMemberWithName, "x", true),
            (Concept::HaveFieldMemberWithName, "M", false),
            (Concept::HaveFieldMemberWithName, "P", false),
            (Concept::HaveMethodMemberWithName, "M", true),
            (Concept::HaveMethodMemberWithName, ".ctor", true),
            (Concept::HaveMethodMemberWithName, "x", false),
            (Concept::HaveMethodMemberWithName, "P", false),
            (Concept::HavePropertyMemberWithName, "P", true),
            (Concept::HavePropertyMemberWithName, "x", false),
            (Concept::HavePropertyMemberWithName, "M", false),
        ];
        for (concept, name, want) in table {
            assert_eq!(
                check(&e, &t, concept, names(&[name]))?,
                want,
                "{concept:?} {name}"
            );
        }
        Ok(())
    }

    #[test]
    fn call_any_on_a_type_counts_its_members_calls() -> Result<(), ElementError> {
        let document = members_document();
        let architecture = Architecture::new(&document);
        let e = Evaluator::new(&architecture, "r");
        let callee = || objects(&["S.U::Callee()"]);
        let t = Object::Type(architecture.types["S.T"]);
        assert!(check(&e, &t, Concept::CallAny, callee())?);
        let u = Object::Type(architecture.types["S.U"]);
        assert!(!check(&e, &u, Concept::CallAny, callee())?);
        let m = Object::Member(architecture.members_of["S.T"][1]);
        assert!(check(&e, &m, Concept::CallAny, callee())?);
        Ok(())
    }

    #[test]
    fn a_method_body_dependency_counts_only_as_a_body_type() -> Result<(), ElementError> {
        let document = members_document();
        let architecture = Architecture::new(&document);
        let e = Evaluator::new(&architecture, "r");
        let concept = Concept::HaveDependencyInMethodBodyTo;
        let calling = Object::Member(architecture.members_of["S.T"][1]);
        assert!(
            !check(&e, &calling, concept, objects(&["S.U"]))?,
            "a call is not a body-type dependency"
        );
        let local = Object::Member(architecture.members_of["S.T"][2]);
        assert!(check(&e, &local, concept, objects(&["S.V"]))?);
        assert!(!check(&e, &local, concept, objects(&["S.U"]))?);
        Ok(())
    }
}
