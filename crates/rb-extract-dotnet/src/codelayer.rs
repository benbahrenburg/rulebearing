//! The .NET code layer: types, members, attributes and calls with every property the element
//! predicates read, and each type's and member's dependencies, as `ArchUnitNET` builds them.
//!
//! - Plan: [Wave 2, Step 3](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#23-step-3-attribution-edge-projection-the-net-code-layer-and-defaults-2a)
//!   (`codelayer.rs`), § 1.4.2 (the edge set), § 1.7 (`record` and `immutable` follow
//!   `ArchUnitNET` 0.13.4's own detection)
//! - Requirement: [FR-EXT-DN-03](../../../docs/prd.md#fr-ext-dn-03)
//! - Decisions: [ADR-0011](../../../docs/adr/0011-read-dotnet-assemblies-not-source.md),
//!   [ADR-0004](../../../docs/adr/0004-graph-document-is-cruise-result-superset.md)
//! - Specification: `ArchUnitNET` 0.13.4 `ArchBuilder.LoadTypesForModule` (which types),
//!   `TypeProcessor` phases 1 to 9 (which members and dependencies), `DomainResolver` (kinds),
//!   `MonoCecilTypeExtensions.IsRecord` (a class with a `<Clone>$` method)
//!
//! Which types: every `TypeDef` except `<Module>`, the compiler's embedded `Nullable*` and
//! `EmbeddedAttribute`, `Coverlet*`, types with a compiler-generated name (or nested in one), and
//! test-SDK generated code. Which members: fields except backing fields, properties, methods;
//! none with a compiler-generated name. Which dependencies, by `dependencyKind`:
//!
//! | `ArchUnitNET` dependency | Kind |
//! | --- | --- |
//! | `InheritsBaseClassDependency` | `inherits` |
//! | `ImplementsInterfaceDependency` (own and the loaded base chain's) | `implements` |
//! | `FieldTypeDependency` | `field` |
//! | `PropertyTypeDependency`, `MethodSignatureDependency` | `signature` |
//! | `MethodCallDependency`, `BodyTypeMemberDependency`, `CastTypeDependency`, `TypeCheckDependency`, `AccessFieldDependency` | `body` |
//! | `MetaDataDependency` (`ldtoken`), `TypeReferenceDependency` (`typeof` in an attribute) | `typeof` |
//! | `AttributeTypeDependency`, `AttributeMemberDependency` | `attribute` |
//! | `GenericArgumentTypeDependency`, `GenericArgumentMemberDependency` | `generic-argument` |
//!
//! Member dependencies roll up to their type, as phase 9 does. Names follow [`crate::names`].

use std::collections::BTreeMap;

use rb_model::{
    Accessor, AttributeElement, Attribution, CallElement, CodeLayer, DependencyKind,
    ElementDependency, Language, Location, MemberElement, NamedArgument, TypeElement,
};

use crate::attribute::TypeAttribution;
use crate::loader::{Attribute, Loaded, Method, Type};
use crate::metadata::tables::id;
use crate::names::{
    Generics, Resolved, Universe, assembly_qualified_name, is_compiler_generated_name,
};
use crate::pdb::{SequencePoint, point_at};
use crate::sig::{Token, TypeSig};

/// What a dependency points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A type, loaded or not.
    Type(Resolved),
    /// A generic parameter, by `ArchUnitNET`'s name (``Ns.Box`1+<T>``).
    GenericParameter(String),
}

/// A type as a signature uses it: the type and its generic arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ref {
    /// The type (a generic type's definition).
    pub target: Target,
    /// Its generic arguments.
    pub args: Vec<Ref>,
}

impl Ref {
    /// A reference with no generic arguments.
    pub fn of(target: Target) -> Self {
        Self {
            target,
            args: Vec::new(),
        }
    }

    /// The resolved type, unless this is a generic parameter.
    pub fn resolved(&self) -> Option<&Resolved> {
        match &self.target {
            Target::Type(r) => Some(r),
            Target::GenericParameter(_) => None,
        }
    }
}

/// One dependency found while building, before it is attached to its element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// The target and its generic arguments.
    pub target: Ref,
    /// The kind.
    pub kind: DependencyKind,
    /// The member reference that formed it.
    pub member: Option<String>,
    /// The source document, when a sequence point says.
    pub file: Option<String>,
    /// 1-based line.
    pub line: Option<u32>,
    /// 1-based column.
    pub column: Option<u32>,
    /// A literal reflection load.
    pub dynamic: bool,
    /// The called member's full name, for a call.
    pub call: Option<String>,
    /// How a body used the type (see [`rb_model::ElementDependency::form`]).
    pub form: Option<&'static str>,
}

/// One assembly's inputs.
#[derive(Debug)]
pub struct Source<'a> {
    /// The loaded assembly.
    pub loaded: &'a Loaded,
    /// Per `TypeDef` row minus one, the type's attribution.
    pub attribution: &'a [TypeAttribution],
    /// Sequence points per `MethodDef` row.
    pub points: BTreeMap<u32, Vec<SequencePoint>>,
    /// PDB documents, repository-relative, row minus one.
    pub documents: Vec<String>,
    /// `languages.dotnet.namespaces`: keep only types in these namespaces or below.
    pub namespaces: Option<&'a [String]>,
}

/// A type-level dependency, for the module layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeDependency {
    /// The source assembly's index.
    pub assembly: usize,
    /// The source type's full name.
    pub from_type: String,
    /// The source file: the sequence point's document, else the type's file.
    pub file: Option<String>,
    /// The target.
    pub target: Resolved,
    /// The kind.
    pub kind: DependencyKind,
    /// The member reference.
    pub member: Option<String>,
    /// 1-based line.
    pub line: Option<u32>,
    /// 1-based column.
    pub column: Option<u32>,
    /// A literal reflection load.
    pub dynamic: bool,
}

/// What the builder produced.
#[derive(Debug, Default)]
pub struct Built {
    /// The code layer, normalised.
    pub code: CodeLayer,
    /// Every type-level dependency, in discovery order.
    pub dependencies: Vec<TypeDependency>,
}

/// Attribute types `ArchUnitNET` never records.
const HIDDEN_ATTRIBUTES: &[&str] = &[
    "Microsoft.CodeAnalysis.EmbeddedAttribute",
    "System.Runtime.CompilerServices.NullableAttribute",
    "System.Runtime.CompilerServices.NullableContextAttribute",
];
const BACKING_FIELD: &str = "k__BackingField";
const IS_EXTERNAL_INIT: &str = "System.Runtime.CompilerServices.IsExternalInit";

/// Builds the code layer.
pub struct Builder<'u> {
    pub(crate) universe: &'u Universe<'u>,
    pub(crate) sources: &'u [Source<'u>],
}

/// `TypeAttributes` and `MethodAttributes` bits used here (ECMA-335 II.23.1.15, II.23.1.10).
mod flags {
    pub const TYPE_VISIBILITY: u32 = 0x7;
    pub const INTERFACE: u32 = 0x20;
    pub const ABSTRACT: u32 = 0x80;
    pub const SEALED: u32 = 0x100;
    pub const MEMBER_ACCESS: u16 = 0x7;
    pub const STATIC: u16 = 0x10;
    pub const FINAL: u16 = 0x20;
    pub const VIRTUAL: u16 = 0x40;
    pub const METHOD_ABSTRACT: u16 = 0x400;
    pub const INIT_ONLY: u16 = 0x20;
}

fn type_visibility(bits: u32) -> &'static str {
    match bits & flags::TYPE_VISIBILITY {
        1 | 2 => "public",
        3 => "private",
        4 => "protected",
        6 => "private-protected",
        7 => "protected-internal",
        _ => "internal",
    }
}

fn member_visibility(bits: u16) -> &'static str {
    match bits & flags::MEMBER_ACCESS {
        1 => "private",
        2 => "private-protected",
        4 => "protected",
        5 => "protected-internal",
        6 => "public",
        _ => "internal",
    }
}

/// How accessible a member visibility is, for picking a property's.
fn openness(visibility: &str) -> u8 {
    match visibility {
        "public" => 5,
        "protected-internal" => 4,
        "internal" => 3,
        "protected" => 2,
        "private-protected" => 1,
        _ => 0,
    }
}

impl<'u> Builder<'u> {
    /// A builder over `sources`, which must be in the universe's assembly order.
    pub fn new(universe: &'u Universe<'u>, sources: &'u [Source<'u>]) -> Self {
        Self { universe, sources }
    }

    pub(crate) fn generics_of<'t>(owner: &'t Type, method: Option<&'t Method>) -> Generics<'t> {
        Generics {
            type_: Some((owner.full_name.as_str(), owner.generic_params.as_slice())),
            method: method.map(|m| (m.name.as_str(), m.generic_params.as_slice())),
        }
    }

    /// The type a signature names, with its generic arguments.
    pub(crate) fn sig_ref(&self, asm: usize, sig: &TypeSig, generics: Generics<'_>) -> Option<Ref> {
        Some(match sig {
            TypeSig::Primitive(name) => Ref::of(Target::Type(Resolved::External {
                full_name: (*name).to_owned(),
                assembly: None,
            })),
            TypeSig::Named { token, .. } => return self.token_ref(asm, *token, generics),
            TypeSig::GenericInst { base, args } => {
                let mut base = self.sig_ref(asm, base, generics)?;
                base.args = args
                    .iter()
                    .filter_map(|a| self.sig_ref(asm, a, generics))
                    .collect();
                base
            }
            TypeSig::SzArray(inner)
            | TypeSig::Ptr(inner)
            | TypeSig::ByRef(inner)
            | TypeSig::Pinned(inner)
            | TypeSig::Array { element: inner, .. }
            | TypeSig::Modified { inner, .. } => return self.sig_ref(asm, inner, generics),
            TypeSig::Var(n) => Ref::of(Target::GenericParameter(generic(generics.type_, *n, "!"))),
            TypeSig::MVar(n) => {
                Ref::of(Target::GenericParameter(generic(generics.method, *n, "!!")))
            }
            TypeSig::FnPtr(_) => return None,
        })
    }

    /// The type a `TypeDef`, `TypeRef` or `TypeSpec` token names.
    pub(crate) fn token_ref(
        &self,
        asm: usize,
        token: Token,
        generics: Generics<'_>,
    ) -> Option<Ref> {
        if let Some(spec) = self.universe.spec(asm, token) {
            return self.sig_ref(asm, spec, generics);
        }
        if token.table == id::TYPE_SPEC {
            return None;
        }
        Some(Ref::of(Target::Type(self.universe.resolve(asm, token))))
    }

    pub(crate) fn target_name(&self, target: &Target) -> String {
        match target {
            Target::Type(r) => self.universe.full_name(r),
            Target::GenericParameter(name) => name.clone(),
        }
    }

    /// A type named by reflection text (`Ns.T, Assembly, Version=…`): a loaded definition when
    /// one has that full name, else external.
    pub(crate) fn named(&self, text: &str) -> Target {
        let mut parts = text.split(',').map(str::trim);
        let full_name = parts.next().unwrap_or_default();
        let full_name = full_name.split("[[").next().unwrap_or(full_name).to_owned();
        let assembly = parts.next().filter(|a| !a.is_empty()).map(str::to_owned);
        let loaded = (0..self.universe.assemblies.len()).find_map(|a| {
            let wanted = assembly.as_deref().is_none_or(|name| {
                self.universe.assemblies[a]
                    .identity
                    .name
                    .eq_ignore_ascii_case(name)
            });
            wanted
                .then(|| self.universe.def_named(a, &full_name))
                .flatten()
                .map(|row| Resolved::Def { assembly: a, row })
        });
        Target::Type(loaded.unwrap_or(Resolved::External {
            full_name,
            assembly,
        }))
    }

    pub(crate) fn attribute_name(&self, asm: usize, token: Token) -> String {
        self.universe.full_name(&self.universe.resolve(asm, token))
    }

    /// A dependency found in `method`, located by the sequence point covering `offset` (the
    /// method's first point when `offset` is `None`).
    pub(crate) fn found(
        &self,
        asm: usize,
        method: &Method,
        target: Ref,
        kind: DependencyKind,
        member: Option<String>,
        offset: Option<u32>,
    ) -> Found {
        let points = self
            .sources
            .get(asm)
            .and_then(|s| s.points.get(&method.row));
        let point = points.and_then(|p| match offset {
            Some(o) => point_at(p, o).or_else(|| p.first()),
            None => p.first(),
        });
        let file = point.and_then(|p| {
            self.sources
                .get(asm)
                .and_then(|s| s.documents.get((p.document as usize).checked_sub(1)?))
                .cloned()
        });
        Found {
            target,
            kind,
            member,
            file,
            line: point.map(|p| p.line),
            column: point.map(|p| p.column),
            dynamic: false,
            call: None,
            form: None,
        }
    }

    /// A method's Cecil full name, with `ArchUnitNET`'s `<T>` suffix per generic parameter.
    pub(crate) fn method_full_name(&self, asm: usize, owner: &Type, method: &Method) -> String {
        let generics = Self::generics_of(owner, Some(method));
        let params: Vec<String> = method
            .sig
            .params
            .iter()
            .map(|p| self.universe.sig_name(asm, p, generics, true))
            .collect();
        let mut name = format!(
            "{} {}::{}({})",
            self.universe.sig_name(asm, &method.sig.ret, generics, true),
            owner.full_name.replace('+', "/"),
            method.name,
            params.join(",")
        );
        for parameter in &method.generic_params {
            name.push('<');
            name.push_str(parameter);
            name.push('>');
        }
        name
    }

    fn is_generated(loaded: &Loaded, ty: &Type) -> bool {
        let mut current = Some(ty);
        let mut steps = 0;
        while let Some(t) = current {
            if is_compiler_generated_name(&t.name) {
                return true;
            }
            current = t.enclosing.and_then(|row| loaded.type_at(row));
            steps += 1;
            if steps > loaded.types.len() {
                break;
            }
        }
        false
    }

    fn kept(&self, source: &Source<'_>, ty: &Type) -> bool {
        let loaded = source.loaded;
        let hidden = HIDDEN_ATTRIBUTES.contains(&ty.full_name.as_str())
            || ty.full_name.starts_with("Coverlet");
        let in_namespace = source
            .namespaces
            .is_none_or(|list| list.iter().any(|n| ty.namespace.starts_with(n.as_str())));
        let test_sdk = ty.attributes.iter().any(|a| {
            self.attribute_name(loaded_index(self, loaded), a.type_)
                == "Microsoft.VisualStudio.TestPlatform.TestSDKAutoGeneratedCode"
        });
        !ty.is_module_type
            && !hidden
            && in_namespace
            && !test_sdk
            && !Self::is_generated(loaded, ty)
    }

    /// The base chain of a type, nearest first, as far as loaded definitions go (the first
    /// external base ends it).
    fn base_chain(&self, asm: usize, ty: &Type) -> Vec<(usize, Resolved)> {
        let mut chain = Vec::new();
        let mut current = (asm, ty.extends);
        while let (a, Some(token)) = current {
            if chain.len() > 64 {
                break;
            }
            let resolved = match self.universe.spec(a, token) {
                Some(spec) => match self
                    .sig_ref(a, spec, Generics::default())
                    .and_then(|r| r.resolved().cloned())
                {
                    Some(r) => r,
                    None => break,
                },
                None => self.universe.resolve(a, token),
            };
            let next = match &resolved {
                Resolved::Def { assembly, row } => self.universe.assemblies[*assembly]
                    .type_at(*row)
                    .map(|t| (*assembly, t.extends)),
                Resolved::External { .. } => None,
            };
            chain.push((a, resolved));
            match next {
                Some(n) => current = n,
                None => break,
            }
        }
        chain
    }

    fn kind(&self, asm: usize, ty: &Type) -> (&'static str, bool) {
        if ty.flags & flags::INTERFACE != 0 {
            return ("interface", false);
        }
        let chain = self.base_chain(asm, ty);
        let names: Vec<String> = chain
            .iter()
            .map(|(_, r)| self.universe.full_name(r))
            .collect();
        match names.first().map(String::as_str) {
            Some("System.Enum") => return ("enum", true),
            Some("System.ValueType") if ty.full_name != "System.Enum" => return ("struct", true),
            _ => {}
        }
        let attribute = names.iter().any(|n| n == "System.Attribute")
            || chain.last().is_some_and(|(_, r)| {
                matches!(r, Resolved::External { full_name, .. } if full_name.ends_with("Attribute"))
            });
        (if attribute { "attribute" } else { "class" }, false)
    }

    /// Adds `found` to `deps`, then its generic arguments recursively (phase 8), skipping
    /// generic parameters and compiler-generated targets.
    fn push(&self, deps: &mut Vec<Found>, found: &Found) {
        let args = found.target.args.clone();
        if self.visible(&found.target.target) {
            deps.push(found.clone());
        }
        for arg in args {
            if matches!(arg.target, Target::GenericParameter(_)) {
                continue;
            }
            self.push(
                deps,
                &Found {
                    target: arg,
                    kind: DependencyKind::GenericArgument,
                    member: None,
                    dynamic: false,
                    call: None,
                    form: None,
                    ..found.clone()
                },
            );
        }
    }

    fn visible(&self, target: &Target) -> bool {
        match target {
            Target::Type(Resolved::Def { assembly, row }) => self.universe.assemblies[*assembly]
                .type_at(*row)
                .is_some_and(|t| !Self::is_generated(self.universe.assemblies[*assembly], t)),
            Target::Type(Resolved::External { full_name, .. }) => !full_name
                .rsplit(['.', '+'])
                .next()
                .is_some_and(is_compiler_generated_name),
            Target::GenericParameter(_) => true,
        }
    }

    fn attribute_element(
        &self,
        asm: usize,
        target: &str,
        attribute: &Attribute,
        location: &Location,
    ) -> AttributeElement {
        AttributeElement {
            target: target.to_owned(),
            attribute_type: self.attribute_name(asm, attribute.type_),
            arguments: attribute.arguments.clone(),
            named_arguments: attribute
                .named
                .iter()
                .map(|(name, value)| NamedArgument {
                    name: name.clone(),
                    value: value.clone(),
                })
                .collect(),
            location: location.clone(),
        }
    }

    /// Attribute dependencies and elements for one element's attributes; `typeof` arguments go
    /// to `type_deps` (phase 4 adds them to the type).
    #[expect(
        clippy::too_many_arguments,
        reason = "the element, its attributes and the two dependency lists it feeds"
    )]
    fn attributes(
        &self,
        asm: usize,
        target: &str,
        attributes: &[Attribute],
        location: &Location,
        deps: &mut Vec<Found>,
        type_deps: &mut Vec<Found>,
        elements: &mut Vec<AttributeElement>,
    ) {
        for attribute in attributes {
            for text in &attribute.type_arguments {
                // AddAttributeArgumentReferenceDependencies skips compiler-generated types.
                self.push(
                    type_deps,
                    &Self::at(Ref::of(self.named(text)), DependencyKind::Typeof, location),
                );
            }
            let name = self.attribute_name(asm, attribute.type_);
            if HIDDEN_ATTRIBUTES.contains(&name.as_str()) {
                continue;
            }
            if let Some(r) = self.token_ref(asm, attribute.type_, Generics::default()) {
                self.push(deps, &Self::at(r, DependencyKind::Attribute, location));
            }
            elements.push(self.attribute_element(asm, target, attribute, location));
        }
    }

    /// A dependency located at an element.
    fn at(target: Ref, kind: DependencyKind, location: &Location) -> Found {
        Found {
            target,
            kind,
            member: None,
            file: location.file.clone(),
            line: location.line,
            column: location.column,
            dynamic: false,
            call: None,
            form: None,
        }
    }

    fn element_dependencies(&self, found: &[Found]) -> Vec<ElementDependency> {
        found
            .iter()
            .map(|f| ElementDependency {
                target: self.target_name(&f.target.target),
                kind: f.kind.as_str().to_owned(),
                member: f.member.clone(),
                line: f.line,
                form: f.form.map(str::to_owned),
            })
            .filter(|d| !d.target.is_empty())
            .collect()
    }

    /// Builds the code layer and the type-level dependencies of every source.
    pub fn build(&self) -> Built {
        let mut built = Built::default();
        for (asm, source) in self.sources.iter().enumerate() {
            for ty in &source.loaded.types {
                if self.kept(source, ty) {
                    self.build_type(asm, source, ty, &mut built);
                }
            }
        }
        built.code.normalise();
        built
    }

    /// A type the kept code references but no analysed assembly defines, as `ArchUnitNET`'s
    /// `DomainResolver` holds it: a stub with its definition's facts when an assembly from
    /// index `from` on (one found beside the analysed ones) defines it, else an
    /// `UnavailableType` with only its name. Neither has members or dependencies.
    pub fn referenced(&self, from: usize, full_name: &str) -> TypeElement {
        let location = Location::in_file(Language::Dotnet, None);
        let Some((asm, ty)) = self.universe.defined(from, full_name) else {
            let outer = full_name.split('+').next().unwrap_or(full_name);
            let (namespace, _) = outer.rsplit_once('.').unwrap_or(("", outer));
            let name = full_name.rsplit(['.', '+']).next().unwrap_or(full_name);
            let mut element = TypeElement::new(full_name, name, "unavailable", location);
            element.namespace = (!namespace.is_empty()).then(|| namespace.to_owned());
            element.referenced = Some(true);
            return element;
        };
        let loaded = self.universe.assemblies[asm];
        let (kind, value_type) = self.kind(asm, ty);
        let mut element = TypeElement::new(&ty.full_name, &ty.name, kind, location);
        element.namespace = (!ty.namespace.is_empty()).then(|| ty.namespace.clone());
        element.referenced = Some(true);
        element.assembly = Some(loaded.identity.name.clone());
        let display = loaded.identity.display();
        element.assembly_qualified_name = Some(assembly_qualified_name(&ty.full_name, &display));
        element.assembly_full_name = Some(display);
        element.visibility = Some(type_visibility(ty.flags).to_owned());
        element.r#abstract = Some(ty.flags & flags::ABSTRACT != 0 && kind != "interface");
        element.sealed = Some(ty.flags & flags::SEALED != 0);
        element.record = Some(
            matches!(kind, "class" | "attribute")
                && ty.methods.iter().any(|m| m.name == "<Clone>$"),
        );
        element.value_type = Some(value_type);
        element.nested = Some(ty.enclosing.is_some());
        element.generic = Some(!ty.generic_params.is_empty());
        element
    }

    #[expect(
        clippy::too_many_lines,
        reason = "the type's own properties, then each member family in ArchUnitNET's phase order"
    )]
    fn build_type(&self, asm: usize, source: &Source<'_>, ty: &Type, built: &mut Built) {
        let loaded = source.loaded;
        let attribution = source.attribution.get((ty.row as usize).wrapping_sub(1));
        let file = attribution.and_then(|a| a.file.clone());
        let location = Location {
            line: attribution.and_then(|a| a.line),
            ..Location::in_file(Language::Dotnet, file.clone())
        };
        let (kind, value_type) = self.kind(asm, ty);
        let mut element = TypeElement::new(&ty.full_name, &ty.name, kind, location.clone());
        element.namespace = (!ty.namespace.is_empty()).then(|| ty.namespace.clone());
        element.attribution = attribution
            .and_then(|a| a.attribution)
            .or(Some(Attribution::None));
        element.assembly = Some(loaded.identity.name.clone());
        let display = loaded.identity.display();
        element.assembly_qualified_name = Some(assembly_qualified_name(&ty.full_name, &display));
        element.assembly_full_name = Some(display);
        element.visibility = Some(type_visibility(ty.flags).to_owned());
        let abstract_ = ty.flags & flags::ABSTRACT != 0;
        let sealed = ty.flags & flags::SEALED != 0;
        element.r#abstract = Some(abstract_ && kind != "interface");
        element.sealed = Some(sealed);
        element.r#static = Some(abstract_ && sealed && matches!(kind, "class" | "attribute"));
        element.record = Some(
            matches!(kind, "class" | "attribute")
                && ty.methods.iter().any(|m| m.name == "<Clone>$"),
        );
        element.value_type = Some(value_type);
        element.nested = Some(ty.enclosing.is_some());
        element.nested_in = ty
            .enclosing
            .and_then(|r| loaded.type_at(r))
            .map(|t| t.full_name.clone());
        element.generic = Some(!ty.generic_params.is_empty());
        let chain = self.base_chain(asm, ty);
        element.base_type = chain.first().map(|(_, r)| self.universe.full_name(r));
        element.base_types = chain
            .iter()
            .map(|(_, r)| self.universe.full_name(r))
            .collect();

        let generics = Self::generics_of(ty, None);
        let mut type_deps: Vec<Found> = Vec::new();
        if let Some(base) = ty.extends.and_then(|t| self.token_ref(asm, t, generics)) {
            self.push(
                &mut type_deps,
                &Self::at(base, DependencyKind::Inherits, &location),
            );
        }
        // Own interfaces, then each loaded base type's (GetInterfacesImplementedByClass).
        let mut interface_refs: Vec<Ref> = ty
            .interfaces
            .iter()
            .filter_map(|t| self.token_ref(asm, *t, generics))
            .collect();
        for (_, base) in &chain {
            if let Resolved::Def { assembly, row } = base
                && let Some(base_type) = self.universe.assemblies[*assembly].type_at(*row)
            {
                let base_generics = Self::generics_of(base_type, None);
                interface_refs.extend(
                    base_type
                        .interfaces
                        .iter()
                        .filter_map(|t| self.token_ref(*assembly, *t, base_generics)),
                );
            }
        }
        for r in interface_refs {
            let name = self.target_name(&r.target);
            if !element.interfaces.contains(&name) {
                element.interfaces.push(name);
            }
            self.push(
                &mut type_deps,
                &Self::at(r, DependencyKind::Implements, &location),
            );
        }
        let mut attribute_elements = Vec::new();
        let mut attribute_deps = Vec::new();
        self.attributes(
            asm,
            &ty.full_name,
            &ty.attributes,
            &location,
            &mut attribute_deps,
            &mut type_deps,
            &mut attribute_elements,
        );
        type_deps.extend(attribute_deps);

        let declaring_cecil = ty.full_name.replace('+', "/");
        let mut members = Vec::new();
        let mut member_deps_all: Vec<Found> = Vec::new();
        let mut files: Vec<String> = Vec::new();
        for field in &ty.fields {
            if field.name.contains(BACKING_FIELD) || is_compiler_generated_name(&field.name) {
                continue;
            }
            let type_name = self.universe.sig_name(asm, &field.ty, generics, true);
            let mut member =
                MemberElement::new(&ty.full_name, &field.name, "field", location.clone());
            member.full_name = Some(format!("{type_name} {declaring_cecil}::{}", field.name));
            member.visibility = Some(member_visibility(field.flags).to_owned());
            member.r#static = Some(field.flags & flags::STATIC != 0);
            member.readonly = Some(field.flags & flags::INIT_ONLY != 0);
            member.return_type = Some(self.universe.sig_name(asm, &field.ty, generics, false));
            let mut deps = Vec::new();
            if let Some(r) = self.sig_ref(asm, &field.ty, generics) {
                self.push(&mut deps, &Self::at(r, DependencyKind::Field, &location));
            }
            let target = member.full_name.clone().unwrap_or_default();
            self.attributes(
                asm,
                &target,
                &field.attributes,
                &location,
                &mut deps,
                &mut type_deps,
                &mut attribute_elements,
            );
            member.dependencies = self.element_dependencies(&deps);
            member_deps_all.extend(deps);
            members.push(member);
        }
        let method_by_row: BTreeMap<u32, &Method> = ty.methods.iter().map(|m| (m.row, m)).collect();
        for property in &ty.properties {
            if is_compiler_generated_name(&property.name) {
                continue;
            }
            let type_name = self
                .universe
                .sig_name(asm, &property.sig.ret, generics, true);
            let params: Vec<String> = property
                .sig
                .params
                .iter()
                .map(|p| self.universe.sig_name(asm, p, generics, true))
                .collect();
            let mut member =
                MemberElement::new(&ty.full_name, &property.name, "property", location.clone());
            member.full_name = Some(format!(
                "{type_name} {declaring_cecil}::{}({})",
                property.name,
                params.join(",")
            ));
            let getter = property.getter.and_then(|r| method_by_row.get(&r));
            let setter = property.setter.and_then(|r| method_by_row.get(&r));
            let accessor = |m: &&Method| Accessor {
                visibility: member_visibility(m.flags).to_owned(),
            };
            member.getter = getter.map(accessor);
            let init = setter.is_some_and(|m| {
                m.sig
                    .ret
                    .has_modreq(&|t| self.attribute_name(asm, t) == IS_EXTERNAL_INIT)
            });
            if init {
                member.init_setter = setter.map(accessor);
            } else {
                member.setter = setter.map(accessor);
            }
            member.readonly = Some(setter.is_none());
            member.r#static = Some(
                getter
                    .or(setter)
                    .is_some_and(|m| m.flags & flags::STATIC != 0),
            );
            member.r#virtual = Some(
                getter
                    .or(setter)
                    .is_some_and(|m| m.flags & flags::VIRTUAL != 0),
            );
            member.visibility = [getter, setter]
                .into_iter()
                .flatten()
                .map(|m| member_visibility(m.flags))
                .max_by_key(|v| openness(v))
                .map(str::to_owned);
            member.return_type =
                Some(
                    self.universe
                        .sig_name(asm, &property.sig.ret, generics, false),
                );
            let mut deps = Vec::new();
            if let Some(r) = self.sig_ref(asm, &property.sig.ret, generics) {
                self.push(
                    &mut deps,
                    &Self::at(r, DependencyKind::Signature, &location),
                );
            }
            let target = member.full_name.clone().unwrap_or_default();
            self.attributes(
                asm,
                &target,
                &property.attributes,
                &location,
                &mut deps,
                &mut type_deps,
                &mut attribute_elements,
            );
            member.dependencies = self.element_dependencies(&deps);
            member_deps_all.extend(deps);
            members.push(member);
        }
        for method in &ty.methods {
            if is_compiler_generated_name(&method.name) {
                continue;
            }
            let full_name = self.method_full_name(asm, ty, method);
            let method_generics = Self::generics_of(ty, Some(method));
            let params: Vec<String> = method
                .sig
                .params
                .iter()
                .map(|p| self.universe.sig_name(asm, p, method_generics, true))
                .collect();
            let constructor = method.name == ".ctor" || method.name == ".cctor";
            // An iterator's or async method's statements are in its state machine's MoveNext.
            let first = source
                .points
                .get(&method.row)
                .and_then(|p| p.first())
                .or_else(|| {
                    let (machine, _) =
                        self.state_machine(asm, method, &mut std::collections::BTreeSet::new());
                    machine
                        .and_then(|(_, move_next)| source.points.get(&move_next.row))
                        .and_then(|p| p.first())
                });
            let method_location = Location {
                language: Language::Dotnet,
                file: first
                    .and_then(|p| source.documents.get((p.document as usize).checked_sub(1)?))
                    .cloned()
                    .or_else(|| file.clone()),
                line: first.map(|p| p.line),
                column: first.map(|p| p.column),
            };
            if let Some(doc) = &method_location.file
                && Some(doc) != file.as_ref()
                && !files.contains(doc)
            {
                files.push(doc.clone());
            }
            let mut member = MemberElement::new(
                &ty.full_name,
                format!("{}({})", method.name, params.join(",")),
                if constructor { "constructor" } else { "method" },
                method_location.clone(),
            );
            member.full_name = Some(full_name.clone());
            member.visibility = Some(member_visibility(method.flags).to_owned());
            member.r#static = Some(method.flags & flags::STATIC != 0);
            member.r#virtual =
                Some(method.flags & flags::VIRTUAL != 0 && method.flags & flags::FINAL == 0);
            member.r#abstract = Some(method.flags & flags::METHOD_ABSTRACT != 0);
            member.return_type =
                Some(
                    self.universe
                        .sig_name(asm, &method.sig.ret, method_generics, false),
                );
            member.parameter_types = method
                .sig
                .params
                .iter()
                .map(|p| self.universe.sig_name(asm, p, method_generics, false))
                .collect();
            let mut deps = Vec::new();
            let returns_void = method.sig.ret == TypeSig::Primitive("System.Void");
            let signature = (!returns_void)
                .then_some(&method.sig.ret)
                .into_iter()
                .chain(&method.sig.params);
            let mut seen = Vec::new();
            for sig in signature {
                if let Some(r) = self.sig_ref(asm, sig, method_generics)
                    && !seen.contains(&r)
                {
                    seen.push(r.clone());
                    self.push(
                        &mut deps,
                        &self.found(asm, method, r, DependencyKind::Signature, None, None),
                    );
                }
            }
            for (n, _) in method.generic_params.iter().enumerate() {
                let r = Ref::of(Target::GenericParameter(generic(
                    method_generics.method,
                    u32::try_from(n).unwrap_or(0),
                    "!!",
                )));
                self.push(
                    &mut deps,
                    &self.found(asm, method, r, DependencyKind::Signature, None, None),
                );
            }
            for found in self.body(asm, ty, method) {
                if let Some(call) = &found.call {
                    built.code.calls.push(CallElement {
                        from: full_name.clone(),
                        to: call.clone(),
                        location: Location {
                            language: Language::Dotnet,
                            file: found.file.clone().or_else(|| method_location.file.clone()),
                            line: found.line,
                            column: found.column,
                        },
                    });
                }
                self.push(&mut deps, &found);
            }
            self.attributes(
                asm,
                &full_name,
                &method.attributes,
                &method_location,
                &mut deps,
                &mut type_deps,
                &mut attribute_elements,
            );
            member.dependencies = self.element_dependencies(&deps);
            member_deps_all.extend(deps);
            members.push(member);
        }
        type_deps.extend(member_deps_all);
        element.files = files;
        element.dependencies = self.element_dependencies(&type_deps);
        for found in type_deps {
            let Target::Type(target) = found.target.target else {
                continue;
            };
            built.dependencies.push(TypeDependency {
                assembly: asm,
                from_type: ty.full_name.clone(),
                file: found.file.or_else(|| file.clone()),
                target,
                kind: found.kind,
                member: found.member,
                line: found.line,
                column: found.column,
                dynamic: found.dynamic,
            });
        }
        built.code.types.push(element);
        built.code.members.extend(members);
        built.code.attributes.extend(attribute_elements);
    }
}

/// The index of `loaded` in the builder's universe.
fn loaded_index(builder: &Builder<'_>, loaded: &Loaded) -> usize {
    builder
        .universe
        .assemblies
        .iter()
        .position(|a| std::ptr::eq(*a, loaded))
        .unwrap_or(0)
}

/// `ArchUnitNET`'s name for a generic parameter: `Owner+<T>`, or `!n` when the name is unknown.
fn generic(scope: Option<(&str, &[String])>, n: u32, prefix: &str) -> String {
    match scope.and_then(|(owner, names)| names.get(n as usize).map(|name| (owner, name))) {
        Some((owner, name)) => format!("{owner}+<{name}>"),
        None => format!("{prefix}{n}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visibilities_follow_the_flag_tables() {
        let types = [
            (0, "internal"),
            (1, "public"),
            (2, "public"),
            (3, "private"),
            (4, "protected"),
            (5, "internal"),
            (6, "private-protected"),
            (7, "protected-internal"),
        ];
        for (bits, expected) in types {
            assert_eq!(type_visibility(bits | 0x100), expected, "{bits}");
        }
        let members = [
            (0, "internal"),
            (1, "private"),
            (2, "private-protected"),
            (3, "internal"),
            (4, "protected"),
            (5, "protected-internal"),
            (6, "public"),
        ];
        for (bits, expected) in members {
            assert_eq!(member_visibility(bits | 0x40), expected, "{bits}");
        }
        let mut order: Vec<&str> = vec![
            "public",
            "private",
            "internal",
            "protected-internal",
            "protected",
            "private-protected",
        ];
        order.sort_by_key(|v| std::cmp::Reverse(openness(v)));
        assert_eq!(
            order,
            [
                "public",
                "protected-internal",
                "internal",
                "protected",
                "private-protected",
                "private"
            ]
        );
    }

    #[test]
    fn generic_parameter_names() {
        let names = ["T".to_owned()];
        assert_eq!(
            generic(Some(("Ns.Box`1", &names[..])), 0, "!"),
            "Ns.Box`1+<T>"
        );
        assert_eq!(generic(Some(("Ns.Box`1", &names[..])), 3, "!"), "!3");
        assert_eq!(generic(None, 1, "!!"), "!!1");
    }

    #[test]
    fn a_ref_knows_its_resolution() {
        let external = Ref::of(Target::Type(Resolved::External {
            full_name: "System.Int32".into(),
            assembly: None,
        }));
        assert!(external.resolved().is_some());
        assert!(external.args.is_empty());
        let parameter = Ref::of(Target::GenericParameter("A+<T>".into()));
        assert_eq!(parameter.resolved(), None);
    }
}
