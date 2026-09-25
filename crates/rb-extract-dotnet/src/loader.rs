//! One assembly as a typed, owned model: types with their fields, methods, properties, events,
//! interfaces, generic parameters and attributes, and the references that name other assemblies.
//!
//! - Specification: ECMA-335 II.22 (the tables below), II.23.3 (custom attribute values)
//! - Plan: [Wave 2, Step 2](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#22-step-2-the-metadata-il-and-pdb-readers-2a)
//!   and § 1.7 ("The exact ECMA-335 table set")
//! - Decision: [ADR-0011](../../../docs/adr/0011-read-dotnet-assemblies-not-source.md)
//!
//! The table set, and the predicate or edge that needs each (§ 1.7 fixes the first eight; a
//! further table is read only when a predicate needs it):
//!
//! | Table | Needed for |
//! | --- | --- |
//! | `TypeDef`, `TypeRef`, `TypeSpec` | every type and every type reference |
//! | `Field`, `MethodDef`, `Param` | `field` and `signature` edges; member predicates; parameter names in `member` |
//! | `MemberRef`, `MethodSpec` | `body` and `call` edges to members of other types |
//! | `InterfaceImpl` | `implements` edges; `implementInterface` |
//! | `CustomAttribute` | `attribute` edges; `haveAnyAttributes`, `haveAttributeWithArguments` |
//! | `StandAloneSig` | local variable types, `body` edges |
//! | `PropertyMap`, `Property`, `MethodSemantics` | getters, setters, init setters (`haveGetter`, `getterVisibility`) |
//! | `EventMap`, `Event` | events as members |
//! | `NestedClass` | `areNested`, `nestedIn`, the `+` in nested full names |
//! | `GenericParam` | generic parameter names in member full names |
//! | `GenericParamConstraint` | `signature` edges to a generic parameter's constraint types (`where T : Base`) |
//! | `Assembly`, `AssemblyRef` | `resideInAssembly`, assembly-qualified names, `package` and `framework` classification |
//!
//! Every read is bounds-checked through [`crate::bytes`]; a malformed row or blob is a
//! [`ReadError`] naming the structure, never a panic.

use std::collections::BTreeMap;

use crate::bytes::{Read, ReadError, Reader, malformed};
use crate::il::{Body, Use, parse_body};
use crate::metadata::Metadata;
use crate::metadata::tables::{Coded, TableId, id};
use crate::names::MAX_NESTING;
use crate::pe::{DebugInfo, PeImage};
use crate::sig::{
    MethodSig, Token, TypeSig, field_sig, local_var_sig, method_sig, method_spec, property_sig,
    type_spec,
};

/// An assembly's identity, for assembly-qualified names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    /// The simple name.
    pub name: String,
    /// `major.minor.build.revision`.
    pub version: [u16; 4],
    /// The culture, empty for neutral.
    pub culture: String,
    /// The public key token as lowercase hex, `None` when the assembly is not strong-named.
    pub public_key_token: Option<String>,
}

impl Identity {
    /// Reflection's display name: `Name, Version=1.0.0.0, Culture=neutral, PublicKeyToken=null`.
    pub fn display(&self) -> String {
        let [a, b, c, d] = self.version;
        format!(
            "{}, Version={a}.{b}.{c}.{d}, Culture={}, PublicKeyToken={}",
            self.name,
            if self.culture.is_empty() {
                "neutral"
            } else {
                &self.culture
            },
            self.public_key_token.as_deref().unwrap_or("null")
        )
    }
}

/// Where a `TypeRef` is defined.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// This module.
    Module,
    /// Another module of this assembly.
    ModuleRef,
    /// An assembly, by 0-based index into [`Loaded::assembly_refs`].
    Assembly(usize),
    /// Nested in another `TypeRef`, by 1-based row.
    Enclosing(u32),
    /// A malformed or absent scope.
    Unknown,
}

/// One `TypeRef` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeRefRow {
    /// Namespace (empty for a nested reference).
    pub namespace: String,
    /// Simple name.
    pub name: String,
    /// Where it is defined.
    pub scope: Scope,
}

/// The type or method a `MemberRef` belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Parent {
    /// A type: `TypeDef`, `TypeRef` or `TypeSpec`.
    Type(Token),
    /// A `MethodDef` (a vararg call site).
    Method(u32),
    /// A module reference or something unreadable.
    Other,
}

/// What a `MemberRef` names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemberRefKind {
    /// A method, with its signature.
    Method(MethodSig),
    /// A field, with its type.
    Field(TypeSig),
}

/// One `MemberRef` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberRefRow {
    /// The declaring type or method.
    pub parent: Parent,
    /// The member name (`.ctor` for a constructor).
    pub name: String,
    /// Method or field.
    pub kind: MemberRefKind,
}

/// A method a token names, after following `MethodSpec`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MethodTarget {
    /// A `MethodDef` row of this assembly.
    Def(u32),
    /// A `MemberRef` row.
    Ref(u32),
}

/// One `MethodSpec` row: a generic method instantiation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodSpecRow {
    /// The generic method.
    pub method: MethodTarget,
    /// Its type arguments.
    pub args: Vec<TypeSig>,
}

/// One `AssemblyRef` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssemblyRefRow {
    /// The referenced assembly's identity.
    pub identity: Identity,
}

/// A custom attribute, decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attribute {
    /// The attribute type.
    pub type_: Token,
    /// Positional arguments as literal text.
    pub arguments: Vec<String>,
    /// Named arguments, in order.
    pub named: Vec<(String, String)>,
    /// The positional arguments that are `System.Type` values (`typeof(X)`), as the type names
    /// the blob spells (`Ns.X, Assembly, Version=...`).
    pub type_arguments: Vec<String>,
    /// Set when the value blob could not be decoded here: it is malformed, or it holds an enum
    /// whose underlying type this assembly does not say (an enum of another assembly). The
    /// arguments above are then empty, not a decoded answer; the code layer retries with every
    /// loaded assembly and otherwise marks the attribute `argumentsUnknown`.
    pub undecoded: Option<Undecoded>,
}

/// A custom attribute value blob kept for a later decode, with the constructor's parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Undecoded {
    /// The value blob.
    pub value: Vec<u8>,
    /// The constructor's parameter types.
    pub params: Vec<TypeSig>,
}

impl Attribute {
    /// An attribute whose arguments are decoded from `value` against `params`, enums resolved
    /// by `underlying`; one that cannot be decoded keeps its blob in [`Attribute::undecoded`].
    pub fn decode(
        type_: Token,
        value: &[u8],
        params: &[TypeSig],
        underlying: &dyn Fn(EnumKey<'_>) -> Option<u8>,
    ) -> Self {
        match decode_attribute(value, params, underlying) {
            Ok((arguments, named)) => Self {
                type_,
                type_arguments: type_arguments(params, &arguments),
                arguments,
                named,
                undecoded: None,
            },
            Err(_) => Self {
                type_,
                arguments: Vec::new(),
                named: Vec::new(),
                type_arguments: Vec::new(),
                undecoded: Some(Undecoded {
                    value: value.to_vec(),
                    params: params.to_vec(),
                }),
            },
        }
    }
}

/// The positional arguments that are `System.Type` values: a class parameter that is not null.
fn type_arguments(params: &[TypeSig], arguments: &[String]) -> Vec<String> {
    params
        .iter()
        .zip(arguments)
        .filter(|(p, a)| {
            matches!(
                p.unmodified(),
                TypeSig::Named {
                    value_type: false,
                    ..
                }
            ) && *a != "null"
        })
        .map(|(_, a)| a.clone())
        .collect()
}

/// One field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    /// 1-based `Field` row.
    pub row: u32,
    /// Name.
    pub name: String,
    /// `FieldAttributes`.
    pub flags: u16,
    /// Field type.
    pub ty: TypeSig,
    /// Attributes applied.
    pub attributes: Vec<Attribute>,
}

/// One method.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Method {
    /// 1-based `MethodDef` row.
    pub row: u32,
    /// Name (`.ctor`, `.cctor`, `get_Name`).
    pub name: String,
    /// `MethodAttributes`.
    pub flags: u16,
    /// Signature.
    pub sig: MethodSig,
    /// Parameter names, in order.
    pub parameters: Vec<String>,
    /// Generic parameter names.
    pub generic_params: Vec<String>,
    /// The constraint types of its generic parameters (`where T : Base`), in table order.
    pub generic_constraints: Vec<Token>,
    /// The parsed body, when the method has one.
    pub body: Option<Body>,
    /// Local variable types.
    pub locals: Vec<TypeSig>,
    /// `ldstr` literals by instruction offset.
    pub strings: Vec<(u32, String)>,
    /// Attributes applied.
    pub attributes: Vec<Attribute>,
}

/// One property.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Property {
    /// Name.
    pub name: String,
    /// Signature: the property type is `ret`.
    pub sig: MethodSig,
    /// The getter's `MethodDef` row.
    pub getter: Option<u32>,
    /// The setter's `MethodDef` row.
    pub setter: Option<u32>,
    /// Attributes applied.
    pub attributes: Vec<Attribute>,
}

/// One event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    /// Name.
    pub name: String,
    /// The delegate type.
    pub ty: Option<Token>,
    /// The `add` accessor's row.
    pub adder: Option<u32>,
}

/// One type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Type {
    /// 1-based `TypeDef` row.
    pub row: u32,
    /// Namespace; for a nested type, the outermost type's.
    pub namespace: String,
    /// Simple name, generic arity included (``List`1``).
    pub name: String,
    /// `Namespace.Outer+Inner`.
    pub full_name: String,
    /// `TypeAttributes`.
    pub flags: u32,
    /// The base type.
    pub extends: Option<Token>,
    /// The enclosing type's row.
    pub enclosing: Option<u32>,
    /// Interfaces implemented directly.
    pub interfaces: Vec<Token>,
    /// Generic parameter names.
    pub generic_params: Vec<String>,
    /// The constraint types of its generic parameters (`where T : Base`), in table order.
    pub generic_constraints: Vec<Token>,
    /// Fields, in row order.
    pub fields: Vec<Field>,
    /// Methods, in row order.
    pub methods: Vec<Method>,
    /// Properties.
    pub properties: Vec<Property>,
    /// Events.
    pub events: Vec<Event>,
    /// Attributes applied.
    pub attributes: Vec<Attribute>,
    /// Carries `CompilerGeneratedAttribute`.
    pub compiler_generated: bool,
    /// `<Module>`.
    pub is_module_type: bool,
}

/// A loaded assembly.
#[derive(Debug, Clone)]
pub struct Loaded {
    /// The assembly's identity.
    pub identity: Identity,
    /// `TargetFrameworkAttribute`.
    pub target_framework: Option<String>,
    /// Every type, in row order.
    pub types: Vec<Type>,
    /// `TypeRef` rows, row minus one.
    pub type_refs: Vec<TypeRefRow>,
    /// `TypeSpec` rows, row minus one.
    pub type_specs: Vec<TypeSig>,
    /// `MemberRef` rows, row minus one.
    pub member_refs: Vec<MemberRefRow>,
    /// `MethodSpec` rows, row minus one.
    pub method_specs: Vec<MethodSpecRow>,
    /// `AssemblyRef` rows, row minus one.
    pub assembly_refs: Vec<AssemblyRefRow>,
    /// Attributes on the assembly itself.
    pub assembly_attributes: Vec<Attribute>,
    /// Debug directory entries.
    pub debug: Vec<DebugInfo>,
    /// `MethodDef` and `Field` rows to their declaring types, built by [`Loaded::reindex`].
    pub index: RowIndex,
}

/// `MethodDef` and `Field` rows to (type index, member index), sorted by row, so a lookup is a
/// binary search rather than a scan of every type.
#[derive(Debug, Clone, Default)]
pub struct RowIndex {
    methods: Vec<(u32, usize, usize)>,
    fields: Vec<(u32, usize, usize)>,
}

impl RowIndex {
    fn find(entries: &[(u32, usize, usize)], row: u32) -> Option<(usize, usize)> {
        let at = entries.partition_point(|(r, _, _)| *r < row);
        entries
            .get(at)
            .filter(|(r, _, _)| *r == row)
            .map(|(_, t, m)| (*t, *m))
    }
}

/// Sorts `(row, type, member)` entries by row, keeping the first type that claims a row (the
/// one a scan in type order finds).
fn sorted_rows(mut entries: Vec<(u32, usize, usize)>) -> Vec<(u32, usize, usize)> {
    entries.sort_unstable();
    entries.dedup_by_key(|(row, _, _)| *row);
    entries
}

impl Loaded {
    /// Rebuilds [`Loaded::index`] from [`Loaded::types`]; the loader calls it, and a caller that
    /// builds or edits the types by hand calls it after.
    pub fn reindex(&mut self) {
        let mut methods = Vec::new();
        let mut fields = Vec::new();
        for (t, ty) in self.types.iter().enumerate() {
            methods.extend(ty.methods.iter().enumerate().map(|(m, x)| (x.row, t, m)));
            fields.extend(ty.fields.iter().enumerate().map(|(f, x)| (x.row, t, f)));
        }
        self.index = RowIndex {
            methods: sorted_rows(methods),
            fields: sorted_rows(fields),
        };
    }

    /// The type with 1-based row `row`.
    pub fn type_at(&self, row: u32) -> Option<&Type> {
        self.types.get((row as usize).checked_sub(1)?)
    }

    /// The declaring type row and method of `MethodDef` row `row`.
    pub fn method_at(&self, row: u32) -> Option<(&Type, &Method)> {
        let (t, m) = RowIndex::find(&self.index.methods, row)?;
        let ty = self.types.get(t)?;
        Some((ty, ty.methods.get(m)?))
    }

    /// The declaring type and field of `Field` row `row`.
    pub fn field_at(&self, row: u32) -> Option<(&Type, &Field)> {
        let (t, f) = RowIndex::find(&self.index.fields, row)?;
        let ty = self.types.get(t)?;
        Some((ty, ty.fields.get(f)?))
    }

    /// Reads an assembly file.
    ///
    /// # Errors
    /// When the PE image, the metadata, a signature or a method body is malformed.
    pub fn read(bytes: &[u8]) -> Read<Self> {
        let image = PeImage::parse(bytes)?;
        let metadata = Metadata::parse_assembly(image.metadata)?;
        Builder {
            md: &metadata,
            image: &image,
        }
        .build(image.debug.clone())
    }
}

/// Generic parameters by owner (`TypeDef` or `MethodDef`), as (number, name).
type GenericParams = BTreeMap<(TableId, u32), Vec<(u32, String)>>;
/// Generic parameter constraint types by the parameter's owner (`TypeDef` or `MethodDef`).
type GenericConstraints = BTreeMap<(TableId, u32), Vec<Token>>;
/// A method body with its local types and `ldstr` literals.
type BodyParts = (Body, Vec<TypeSig>, Vec<(u32, String)>);
/// Positional and named attribute arguments.
pub type AttributeArguments = (Vec<String>, Vec<(String, String)>);

/// Ranges of a list column (`FieldList`, `MethodList`, `ParamList`, `PropertyList`): row `r`
/// owns `start(r)..start(r + 1)`, the last up to the table's end.
fn list_range(starts: &[u32], index: usize, total: u32) -> std::ops::Range<u32> {
    let past = total.saturating_add(1);
    let start = starts.get(index).copied().unwrap_or(past).min(past);
    let end = index
        .checked_add(1)
        .and_then(|next| starts.get(next))
        .copied()
        .unwrap_or(past);
    start..end.clamp(start, past)
}

/// C#'s rendering of an attribute argument value.
fn bool_text(value: bool) -> String {
    if value { "True" } else { "False" }.to_owned()
}

/// Lowercase hex.
fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// The public key token of a public key: the last eight bytes of its SHA-1, reversed (II.6.2.1.3).
pub fn public_key_token(key: &[u8]) -> String {
    use sha1::Digest as _;
    let digest = sha1::Sha1::digest(key);
    let mut token: Vec<u8> = digest.iter().rev().take(8).copied().collect();
    token.truncate(8);
    hex(&token)
}

struct Builder<'m, 'a> {
    md: &'m Metadata<'a>,
    image: &'m PeImage<'a>,
}

impl Builder<'_, '_> {
    fn cell(&self, table: TableId, row: u32, column: usize) -> Read<u32> {
        self.md.tables.cell(table, row, column)
    }

    fn string(&self, table: TableId, row: u32, column: usize) -> Read<String> {
        Ok(self.md.string(self.cell(table, row, column)?)?.to_owned())
    }

    fn blob(&self, table: TableId, row: u32, column: usize) -> Read<&[u8]> {
        self.md.blob(self.cell(table, row, column)?)
    }

    fn rows(&self, table: TableId) -> u32 {
        self.md.rows(table)
    }

    fn starts(&self, table: TableId, column: usize) -> Read<Vec<u32>> {
        (1..=self.rows(table))
            .map(|row| self.cell(table, row, column))
            .collect()
    }

    fn identity(&self, table: TableId, row: u32) -> Read<Identity> {
        // Assembly: HashAlgId, Major, Minor, Build, Revision, Flags, PublicKey, Name, Culture.
        // AssemblyRef: Major, Minor, Build, Revision, Flags, PublicKeyOrToken, Name, Culture, Hash.
        let base = usize::from(table == id::ASSEMBLY);
        let mut version = [0u16; 4];
        for (i, part) in version.iter_mut().enumerate() {
            *part = u16::try_from(self.cell(table, row, base + i)?).unwrap_or(0);
        }
        let flags = self.cell(table, row, base + 4)?;
        let key = self.blob(table, row, base + 5)?;
        let token = match (key.is_empty(), table == id::ASSEMBLY || flags & 1 != 0) {
            (true, _) => None,
            (false, true) => Some(public_key_token(key)),
            (false, false) => Some(hex(key)),
        };
        Ok(Identity {
            name: self.string(table, row, base + 6)?,
            version,
            culture: self.string(table, row, base + 7)?,
            public_key_token: token,
        })
    }

    fn token(coded: Coded, value: u32) -> Option<Token> {
        let (table, row) = coded.decode(value)?;
        (row > 0).then_some(Token { table, row })
    }

    fn method_target(token: u32) -> Option<MethodTarget> {
        let row = token & 0x00FF_FFFF;
        match (token >> 24) as u8 {
            id::METHOD_DEF => Some(MethodTarget::Def(row)),
            id::MEMBER_REF => Some(MethodTarget::Ref(row)),
            _ => None,
        }
    }

    fn build(&self, debug: Vec<DebugInfo>) -> Read<Loaded> {
        let identity = if self.rows(id::ASSEMBLY) > 0 {
            self.identity(id::ASSEMBLY, 1)?
        } else {
            let module = self.string(id::MODULE, 1, 1)?;
            Identity {
                name: module
                    .trim_end_matches(".dll")
                    .trim_end_matches(".exe")
                    .to_owned(),
                version: [0; 4],
                culture: String::new(),
                public_key_token: None,
            }
        };
        let assembly_refs = (1..=self.rows(id::ASSEMBLY_REF))
            .map(|row| {
                self.identity(id::ASSEMBLY_REF, row)
                    .map(|identity| AssemblyRefRow { identity })
            })
            .collect::<Read<Vec<_>>>()?;
        let type_refs = (1..=self.rows(id::TYPE_REF))
            .map(|row| self.type_ref(row))
            .collect::<Read<Vec<_>>>()?;
        let ref_names = type_ref_names(&type_refs)?;
        let type_specs = self.type_specs()?;
        let member_refs = (1..=self.rows(id::MEMBER_REF))
            .map(|row| self.member_ref(row))
            .collect::<Read<Vec<_>>>()?;
        let method_specs = (1..=self.rows(id::METHOD_SPEC))
            .map(|row| {
                let method =
                    Self::token(Coded::MethodDefOrRef, self.cell(id::METHOD_SPEC, row, 0)?)
                        .and_then(|t| Self::method_target((u32::from(t.table) << 24) | t.row));
                let Some(method) = method else {
                    return malformed("MethodSpec method", row as usize);
                };
                Ok(MethodSpecRow {
                    method,
                    args: method_spec(self.blob(id::METHOD_SPEC, row, 1)?)?,
                })
            })
            .collect::<Read<Vec<_>>>()?;
        let names = self.type_names()?;
        let enums = self.enum_codes(&names)?;
        let underlying = |key: EnumKey<'_>| -> Option<u8> {
            let name = match key {
                EnumKey::Token(Token {
                    table: id::TYPE_DEF,
                    row,
                }) => return enums.by_row.get(&row).copied(),
                EnumKey::Token(Token {
                    table: id::TYPE_REF,
                    row,
                }) => ref_names.get((row as usize).checked_sub(1)?)?.as_str(),
                EnumKey::Token(_) => return None,
                EnumKey::Name(text) => type_name_of(text),
            };
            enums
                .by_name
                .get(name)
                .copied()
                .or_else(|| well_known_enum(name))
        };
        let mut attributes = self.attributes(&member_refs, &underlying)?;
        let mut types = self.types(&mut attributes, names)?;
        let assembly_attributes = attributes.remove(&(id::ASSEMBLY, 1)).unwrap_or_default();
        let target_framework = assembly_attributes.iter().find_map(|a| {
            Self::type_full_name(a.type_, &type_refs, &types)
                .filter(|n| n == "System.Runtime.Versioning.TargetFrameworkAttribute")
                .and_then(|_| a.arguments.first().cloned())
        });
        for ty in &mut types {
            ty.compiler_generated = ty.attributes.iter().any(|a| {
                Self::type_full_name(a.type_, &type_refs, &[]).as_deref()
                    == Some("System.Runtime.CompilerServices.CompilerGeneratedAttribute")
            });
        }
        let mut loaded = Loaded {
            identity,
            target_framework,
            types,
            type_refs,
            type_specs,
            member_refs,
            method_specs,
            assembly_refs,
            assembly_attributes,
            debug,
            index: RowIndex::default(),
        };
        loaded.reindex();
        Ok(loaded)
    }

    /// Every `TypeSpec` row's signature. A `TypeSpec` may name only an earlier `TypeSpec`, so
    /// following one always ends, and the tokens one names with every `TypeSpec` unfolded may
    /// not exceed [`MAX_SPEC_TOKENS`], so naming one is bounded work (the compilers never nest a
    /// `TypeSpec` inside another at all).
    fn type_specs(&self) -> Read<Vec<TypeSig>> {
        let mut specs = Vec::new();
        let mut unfolded: Vec<usize> = Vec::new();
        for row in 1..=self.rows(id::TYPE_SPEC) {
            let spec = type_spec(self.blob(id::TYPE_SPEC, row, 0)?)?;
            let mut size = 1usize;
            for token in spec.all_tokens() {
                size = size.saturating_add(1);
                if token.table != id::TYPE_SPEC {
                    continue;
                }
                match (token.row as usize)
                    .checked_sub(1)
                    .filter(|_| token.row < row)
                    .and_then(|i| unfolded.get(i))
                {
                    Some(inner) => size = size.saturating_add(*inner),
                    None => {
                        return malformed(
                            "TypeSpec (names itself or a later TypeSpec)",
                            row as usize,
                        );
                    }
                }
            }
            if size > MAX_SPEC_TOKENS {
                return malformed("TypeSpec (unfolds too large)", row as usize);
            }
            unfolded.push(size);
            specs.push(spec);
        }
        Ok(specs)
    }

    /// Every `TypeDef`'s outermost namespace and full name, in row order.
    fn type_names(&self) -> Read<Vec<(String, String)>> {
        let mut enclosing = BTreeMap::new();
        for row in 1..=self.rows(id::NESTED_CLASS) {
            enclosing.insert(
                self.cell(id::NESTED_CLASS, row, 0)?,
                self.cell(id::NESTED_CLASS, row, 1)?,
            );
        }
        let rows = (1..=self.rows(id::TYPE_DEF))
            .map(|row| {
                Ok((
                    self.string(id::TYPE_DEF, row, 2)?,
                    self.string(id::TYPE_DEF, row, 1)?,
                    enclosing.get(&row).copied(),
                ))
            })
            .collect::<Read<Vec<_>>>()?;
        nested_names(
            &rows
                .iter()
                .map(|(namespace, name, outer)| (namespace.as_str(), name.as_str(), *outer))
                .collect::<Vec<_>>(),
        )
    }

    /// The underlying element type of every enum this assembly defines: the type of its
    /// `value__` field (ECMA-335 II.14.3), by `TypeDef` row and by full name.
    fn enum_codes(&self, names: &[(String, String)]) -> Read<LocalEnums> {
        let field_starts = self.starts(id::TYPE_DEF, 4)?;
        let field_total = self.rows(id::FIELD);
        let mut enums = LocalEnums::default();
        for (index, (_, full_name)) in names.iter().enumerate() {
            for f in list_range(&field_starts, index, field_total) {
                if self.string(id::FIELD, f, 1)? != "value__" {
                    continue;
                }
                if let TypeSig::Primitive(name) = field_sig(self.blob(id::FIELD, f, 2)?)?
                    && let Some(code) = enum_code_of(name)
                {
                    let row = u32::try_from(index + 1).unwrap_or(0);
                    enums.by_row.insert(row, code);
                    enums.by_name.insert(full_name.clone(), code);
                }
                break;
            }
        }
        Ok(enums)
    }

    /// A type name good enough to recognise the two framework attributes the loader itself needs.
    fn type_full_name(token: Token, refs: &[TypeRefRow], defs: &[Type]) -> Option<String> {
        let index = (token.row as usize).checked_sub(1)?;
        match token.table {
            id::TYPE_REF => refs
                .get(index)
                .map(|r| format!("{}.{}", r.namespace, r.name)),
            id::TYPE_DEF => defs.get(index).map(|t| t.full_name.clone()),
            _ => None,
        }
    }

    fn type_ref(&self, row: u32) -> Read<TypeRefRow> {
        let scope = match Coded::ResolutionScope.decode(self.cell(id::TYPE_REF, row, 0)?) {
            Some((id::MODULE, _)) => Scope::Module,
            Some((id::MODULE_REF, _)) => Scope::ModuleRef,
            Some((id::ASSEMBLY_REF, r)) if r > 0 => Scope::Assembly(r as usize - 1),
            Some((id::TYPE_REF, r)) if r > 0 => Scope::Enclosing(r),
            _ => Scope::Unknown,
        };
        Ok(TypeRefRow {
            name: self.string(id::TYPE_REF, row, 1)?,
            namespace: self.string(id::TYPE_REF, row, 2)?,
            scope,
        })
    }

    fn member_ref(&self, row: u32) -> Read<MemberRefRow> {
        let parent = match Coded::MemberRefParent.decode(self.cell(id::MEMBER_REF, row, 0)?) {
            Some((table @ (id::TYPE_DEF | id::TYPE_REF | id::TYPE_SPEC), r)) if r > 0 => {
                Parent::Type(Token { table, row: r })
            }
            Some((id::METHOD_DEF, r)) => Parent::Method(r),
            _ => Parent::Other,
        };
        let blob = self.blob(id::MEMBER_REF, row, 2)?;
        let kind = if blob.first().is_some_and(|b| b & 0x0F == 0x06) {
            MemberRefKind::Field(field_sig(blob)?)
        } else {
            MemberRefKind::Method(method_sig(blob)?)
        };
        Ok(MemberRefRow {
            parent,
            name: self.string(id::MEMBER_REF, row, 1)?,
            kind,
        })
    }

    /// Every custom attribute, keyed by the (table, row) it is applied to.
    fn attributes(
        &self,
        member_refs: &[MemberRefRow],
        underlying: &dyn Fn(EnumKey<'_>) -> Option<u8>,
    ) -> Read<BTreeMap<(TableId, u32), Vec<Attribute>>> {
        let mut found: BTreeMap<(TableId, u32), Vec<Attribute>> = BTreeMap::new();
        let method_starts = self.starts(id::TYPE_DEF, 5)?;
        for row in 1..=self.rows(id::CUSTOM_ATTRIBUTE) {
            let Some((table, target)) =
                Coded::HasCustomAttribute.decode(self.cell(id::CUSTOM_ATTRIBUTE, row, 0)?)
            else {
                continue;
            };
            let constructor = self.cell(id::CUSTOM_ATTRIBUTE, row, 1)?;
            let (type_, params) = match Coded::CustomAttributeType.decode(constructor) {
                Some((id::MEMBER_REF, r)) => {
                    let Some(member) = (r as usize).checked_sub(1).and_then(|i| member_refs.get(i))
                    else {
                        continue;
                    };
                    let (Parent::Type(type_), MemberRefKind::Method(sig)) =
                        (member.parent, &member.kind)
                    else {
                        continue;
                    };
                    (type_, sig.params.clone())
                }
                Some((id::METHOD_DEF, r)) => {
                    let owners = method_starts.partition_point(|start| *start <= r);
                    let Some(owner) = u32::try_from(owners).ok().filter(|o| *o > 0) else {
                        continue;
                    };
                    let sig = method_sig(self.blob(id::METHOD_DEF, r, 4)?)?;
                    (
                        Token {
                            table: id::TYPE_DEF,
                            row: owner,
                        },
                        sig.params,
                    )
                }
                _ => continue,
            };
            let value = self.blob(id::CUSTOM_ATTRIBUTE, row, 2)?;
            found
                .entry((table, target))
                .or_default()
                .push(Attribute::decode(type_, value, &params, underlying));
        }
        Ok(found)
    }

    fn generic_params(&self) -> Read<GenericParams> {
        let mut found: GenericParams = BTreeMap::new();
        for row in 1..=self.rows(id::GENERIC_PARAM) {
            let number = self.cell(id::GENERIC_PARAM, row, 0)?;
            if let Some(owner) =
                Coded::TypeOrMethodDef.decode(self.cell(id::GENERIC_PARAM, row, 2)?)
            {
                found
                    .entry(owner)
                    .or_default()
                    .push((number, self.string(id::GENERIC_PARAM, row, 3)?));
            }
        }
        for params in found.values_mut() {
            params.sort();
        }
        Ok(found)
    }

    /// Every `GenericParamConstraint` row, gathered under its generic parameter's owner.
    fn generic_constraints(&self) -> Read<GenericConstraints> {
        let mut found: GenericConstraints = BTreeMap::new();
        for row in 1..=self.rows(id::GENERIC_PARAM_CONSTRAINT) {
            let parameter = self.cell(id::GENERIC_PARAM_CONSTRAINT, row, 0)?;
            let Some(token) = Self::token(
                Coded::TypeDefOrRef,
                self.cell(id::GENERIC_PARAM_CONSTRAINT, row, 1)?,
            ) else {
                continue;
            };
            if let Some(owner) =
                Coded::TypeOrMethodDef.decode(self.cell(id::GENERIC_PARAM, parameter, 2)?)
            {
                found.entry(owner).or_default().push(token);
            }
        }
        Ok(found)
    }

    fn body(&self, rva: u32) -> Read<Option<BodyParts>> {
        if rva == 0 {
            return Ok(None);
        }
        let Some(data) = self.image.from_rva(rva) else {
            return malformed("method body RVA", rva as usize);
        };
        let body = parse_body(data)?;
        let locals = match body.locals {
            0 => Vec::new(),
            token if (token >> 24) as u8 == id::STAND_ALONE_SIG => {
                local_var_sig(self.blob(id::STAND_ALONE_SIG, token & 0x00FF_FFFF, 0)?)?
            }
            _ => Vec::new(),
        };
        let mut strings = Vec::new();
        for instruction in &body.instructions {
            if instruction.use_ == Use::String {
                strings.push((
                    instruction.offset,
                    self.md.user_string(instruction.token & 0x00FF_FFFF)?,
                ));
            }
        }
        Ok(Some((body, locals, strings)))
    }

    #[expect(
        clippy::too_many_lines,
        reason = "one pass over the type-owned tables, in table order"
    )]
    fn types(
        &self,
        attributes: &mut BTreeMap<(TableId, u32), Vec<Attribute>>,
        names: Vec<(String, String)>,
    ) -> Read<Vec<Type>> {
        let count = self.rows(id::TYPE_DEF);
        let field_starts = self.starts(id::TYPE_DEF, 4)?;
        let method_starts = self.starts(id::TYPE_DEF, 5)?;
        let param_starts = self.starts(id::METHOD_DEF, 5)?;
        let field_total = self.rows(id::FIELD);
        let method_total = self.rows(id::METHOD_DEF);
        let param_total = self.rows(id::PARAM);
        let mut generics = self.generic_params()?;
        let mut take_generics = |table: TableId, row: u32| -> Vec<String> {
            generics
                .remove(&(table, row))
                .unwrap_or_default()
                .into_iter()
                .map(|(_, name)| name)
                .collect()
        };
        let mut constraints = self.generic_constraints()?;
        let mut take_constraints = |table: TableId, row: u32| -> Vec<Token> {
            constraints.remove(&(table, row)).unwrap_or_default()
        };
        let mut enclosing = BTreeMap::new();
        for row in 1..=self.rows(id::NESTED_CLASS) {
            enclosing.insert(
                self.cell(id::NESTED_CLASS, row, 0)?,
                self.cell(id::NESTED_CLASS, row, 1)?,
            );
        }
        let mut interfaces: BTreeMap<u32, Vec<Token>> = BTreeMap::new();
        for row in 1..=self.rows(id::INTERFACE_IMPL) {
            let class = self.cell(id::INTERFACE_IMPL, row, 0)?;
            if let Some(token) =
                Self::token(Coded::TypeDefOrRef, self.cell(id::INTERFACE_IMPL, row, 1)?)
            {
                interfaces.entry(class).or_default().push(token);
            }
        }
        // Accessors: method row -> (semantics, association).
        let mut semantics: BTreeMap<(TableId, u32), Vec<(u16, u32)>> = BTreeMap::new();
        for row in 1..=self.rows(id::METHOD_SEMANTICS) {
            let kind = u16::try_from(self.cell(id::METHOD_SEMANTICS, row, 0)?).unwrap_or(0);
            let method = self.cell(id::METHOD_SEMANTICS, row, 1)?;
            if let Some(association) =
                Coded::HasSemantics.decode(self.cell(id::METHOD_SEMANTICS, row, 2)?)
            {
                semantics
                    .entry(association)
                    .or_default()
                    .push((kind, method));
            }
        }
        let accessor = |table: TableId, row: u32, bit: u16| {
            semantics
                .get(&(table, row))
                .and_then(|list| list.iter().find(|(k, _)| k & bit != 0))
                .map(|(_, m)| *m)
        };
        let owners = |map_table: TableId,
                      list_table: TableId|
         -> Read<BTreeMap<u32, std::ops::Range<u32>>> {
            let parents: Vec<u32> = (1..=self.rows(map_table))
                .map(|r| self.cell(map_table, r, 0))
                .collect::<Read<_>>()?;
            let starts = self.starts(map_table, 1)?;
            let total = self.rows(list_table);
            Ok(parents
                .iter()
                .enumerate()
                .map(|(i, parent)| (*parent, list_range(&starts, i, total)))
                .collect())
        };
        let property_owners = owners(id::PROPERTY_MAP, id::PROPERTY)?;
        let event_owners = owners(id::EVENT_MAP, id::EVENT)?;

        let mut types = Vec::with_capacity(names.len());
        for (row, (namespace, full_name)) in (1..=count).zip(names) {
            let index = (row - 1) as usize;
            let name = self.string(id::TYPE_DEF, row, 1)?;
            let mut fields = Vec::new();
            for f in list_range(&field_starts, index, field_total) {
                fields.push(Field {
                    row: f,
                    name: self.string(id::FIELD, f, 1)?,
                    flags: u16::try_from(self.cell(id::FIELD, f, 0)?).unwrap_or(0),
                    ty: field_sig(self.blob(id::FIELD, f, 2)?)?,
                    attributes: attributes.remove(&(id::FIELD, f)).unwrap_or_default(),
                });
            }
            let mut methods = Vec::new();
            for m in list_range(&method_starts, index, method_total) {
                let sig = method_sig(self.blob(id::METHOD_DEF, m, 4)?)?;
                let mut parameters: Vec<(u32, String)> = Vec::new();
                let mut method_attributes =
                    attributes.remove(&(id::METHOD_DEF, m)).unwrap_or_default();
                for p in list_range(&param_starts, (m - 1) as usize, param_total) {
                    let sequence = self.cell(id::PARAM, p, 1)?;
                    if sequence > 0 {
                        parameters.push((sequence, self.string(id::PARAM, p, 2)?));
                    }
                    // ArchUnitNET's GetAllMethodCustomAttributes: the method's own, its
                    // parameters' and its return value's (sequence 0).
                    method_attributes
                        .extend(attributes.remove(&(id::PARAM, p)).unwrap_or_default());
                }
                parameters.sort();
                let (body, locals, strings) =
                    self.body(self.cell(id::METHOD_DEF, m, 0)?)?.map_or_else(
                        || (None, Vec::new(), Vec::new()),
                        |(b, l, s)| (Some(b), l, s),
                    );
                methods.push(Method {
                    row: m,
                    name: self.string(id::METHOD_DEF, m, 3)?,
                    flags: u16::try_from(self.cell(id::METHOD_DEF, m, 2)?).unwrap_or(0),
                    sig,
                    parameters: parameters.into_iter().map(|(_, n)| n).collect(),
                    generic_params: take_generics(id::METHOD_DEF, m),
                    generic_constraints: take_constraints(id::METHOD_DEF, m),
                    body,
                    locals,
                    strings,
                    attributes: method_attributes,
                });
            }
            let mut properties = Vec::new();
            for p in property_owners.get(&row).cloned().unwrap_or(0..0) {
                properties.push(Property {
                    name: self.string(id::PROPERTY, p, 1)?,
                    sig: property_sig(self.blob(id::PROPERTY, p, 2)?)?,
                    getter: accessor(id::PROPERTY, p, 0x2),
                    setter: accessor(id::PROPERTY, p, 0x1),
                    attributes: attributes.remove(&(id::PROPERTY, p)).unwrap_or_default(),
                });
            }
            let mut events = Vec::new();
            for e in event_owners.get(&row).cloned().unwrap_or(0..0) {
                events.push(Event {
                    name: self.string(id::EVENT, e, 1)?,
                    ty: Self::token(Coded::TypeDefOrRef, self.cell(id::EVENT, e, 2)?),
                    adder: accessor(id::EVENT, e, 0x8),
                });
            }
            types.push(Type {
                row,
                is_module_type: row == 1 && name == "<Module>",
                namespace,
                full_name,
                name,
                flags: self.cell(id::TYPE_DEF, row, 0)?,
                extends: Self::token(Coded::TypeDefOrRef, self.cell(id::TYPE_DEF, row, 3)?),
                enclosing: enclosing.get(&row).copied(),
                interfaces: interfaces.remove(&row).unwrap_or_default(),
                generic_params: take_generics(id::TYPE_DEF, row),
                generic_constraints: take_constraints(id::TYPE_DEF, row),
                fields,
                methods,
                properties,
                events,
                attributes: attributes.remove(&(id::TYPE_DEF, row)).unwrap_or_default(),
                compiler_generated: false,
            });
        }
        Ok(types)
    }
}

/// Every type's outermost namespace and full name (`Namespace.Outer+Inner`) from
/// `(namespace, name, enclosing row)` rows, each chain walked once. An enclosing row that names
/// no type ends the chain.
///
/// # Errors
/// `NestedClass cycle` when a chain comes back to itself, and `NestedClass (nested too deeply)`
/// past [`MAX_NESTING`] levels.
pub(crate) fn nested_names(rows: &[(&str, &str, Option<u32>)]) -> Read<Vec<(String, String)>> {
    let outer_of = |i: usize| {
        rows.get(i)
            .and_then(|(_, _, outer)| (*outer)?.checked_sub(1))
            .map(|o| o as usize)
            .filter(|o| *o < rows.len())
    };
    // Per row: (namespace, full name, depth) once named.
    let mut named: Vec<Option<(String, String, usize)>> = vec![None; rows.len()];
    let mut walking = vec![false; rows.len()];
    for start in 0..rows.len() {
        let mut chain = Vec::new();
        let mut current = Some(start);
        while let Some(i) = current {
            if named[i].is_some() {
                break;
            }
            if walking[i] {
                return malformed("NestedClass cycle", i + 1);
            }
            walking[i] = true;
            chain.push(i);
            current = outer_of(i);
        }
        for &i in chain.iter().rev() {
            let (namespace, name, _) = rows[i];
            let entry = match outer_of(i).and_then(|o| named[o].as_ref()) {
                Some((outer_namespace, outer_full, depth)) => {
                    if *depth >= MAX_NESTING {
                        return malformed("NestedClass (nested too deeply)", i + 1);
                    }
                    (
                        outer_namespace.clone(),
                        format!("{outer_full}+{name}"),
                        depth + 1,
                    )
                }
                None if namespace.is_empty() => (String::new(), name.to_owned(), 0),
                None => (namespace.to_owned(), format!("{namespace}.{name}"), 0),
            };
            named[i] = Some(entry);
            walking[i] = false;
        }
    }
    Ok(named
        .into_iter()
        .map(|entry| entry.map(|(n, f, _)| (n, f)).unwrap_or_default())
        .collect())
}

/// Every `TypeRef`'s full name (`+` for nesting), in row order.
///
/// # Errors
/// `TypeRef nesting cycle` when a resolution scope chain comes back to itself, and
/// `TypeRef (nested too deeply)` past [`MAX_NESTING`] levels.
fn type_ref_names(refs: &[TypeRefRow]) -> Read<Vec<String>> {
    let outer_of = |i: usize| match refs.get(i).map(|r| r.scope) {
        Some(Scope::Enclosing(outer)) => {
            (outer as usize).checked_sub(1).filter(|o| *o < refs.len())
        }
        _ => None,
    };
    let mut named: Vec<Option<(String, usize)>> = vec![None; refs.len()];
    let mut walking = vec![false; refs.len()];
    for start in 0..refs.len() {
        let mut chain = Vec::new();
        let mut current = Some(start);
        while let Some(i) = current {
            if named[i].is_some() {
                break;
            }
            if walking[i] {
                return malformed("TypeRef nesting cycle", i + 1);
            }
            walking[i] = true;
            chain.push(i);
            current = outer_of(i);
        }
        for &i in chain.iter().rev() {
            let r = &refs[i];
            let entry = match outer_of(i).and_then(|o| named[o].as_ref()) {
                Some((outer, depth)) => {
                    if *depth >= MAX_NESTING {
                        return malformed("TypeRef (nested too deeply)", i + 1);
                    }
                    (format!("{outer}+{}", r.name), depth + 1)
                }
                None if r.namespace.is_empty() => (r.name.clone(), 0),
                None => (format!("{}.{}", r.namespace, r.name), 0),
            };
            named[i] = Some(entry);
            walking[i] = false;
        }
    }
    Ok(named
        .into_iter()
        .map(|entry| entry.map(|(n, _)| n).unwrap_or_default())
        .collect())
}

/// The enums an assembly defines, with their underlying element types.
#[derive(Debug, Default)]
struct LocalEnums {
    by_row: BTreeMap<u32, u8>,
    by_name: BTreeMap<String, u8>,
}

/// An enum an attribute value names: by token (a constructor parameter's type) or by the
/// serialized type name a named argument or a boxed value carries (`Ns.E, Assembly, ...`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnumKey<'a> {
    /// A `TypeDef` or `TypeRef` token.
    Token(Token),
    /// A serialized type name.
    Name(&'a str),
}

/// The full name part of a serialized type name: up to the first comma outside `[...]`.
pub fn type_name_of(text: &str) -> &str {
    let mut depth = 0usize;
    for (at, c) in text.char_indices() {
        match c {
            '[' => depth += 1,
            ']' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => return text[..at].trim(),
            _ => {}
        }
    }
    text.trim()
}

/// The element type code of an integral primitive, the types an enum may have underneath.
pub fn enum_code_of(primitive: &str) -> Option<u8> {
    Some(match primitive {
        "System.Boolean" => 0x02,
        "System.Char" => 0x03,
        "System.SByte" => 0x04,
        "System.Byte" => 0x05,
        "System.Int16" => 0x06,
        "System.UInt16" => 0x07,
        "System.Int32" => 0x08,
        "System.UInt32" => 0x09,
        "System.Int64" => 0x0A,
        "System.UInt64" => 0x0B,
        _ => return None,
    })
}

/// The underlying type of the framework enums attributes commonly take, which an assembly only
/// references. Any other enum of another assembly is resolved against the loaded assemblies,
/// or its attribute's arguments are unknown.
pub fn well_known_enum(full_name: &str) -> Option<u8> {
    const INT32: &[&str] = &[
        "System.AttributeTargets",
        "System.ComponentModel.DesignerSerializationVisibility",
        "System.ComponentModel.EditorBrowsableState",
        "System.Diagnostics.CodeAnalysis.DynamicallyAccessedMemberTypes",
        "System.Diagnostics.DebuggableAttribute+DebuggingModes",
        "System.Diagnostics.DebuggerBrowsableState",
        "System.Runtime.CompilerServices.CompilationRelaxations",
        "System.Runtime.CompilerServices.LoadHint",
        "System.Runtime.CompilerServices.MethodCodeType",
        "System.Runtime.CompilerServices.MethodImplOptions",
        "System.Runtime.InteropServices.CallingConvention",
        "System.Runtime.InteropServices.CharSet",
        "System.Runtime.InteropServices.ClassInterfaceType",
        "System.Runtime.InteropServices.ComInterfaceType",
        "System.Runtime.InteropServices.LayoutKind",
        "System.Runtime.InteropServices.UnmanagedType",
        "System.Runtime.Versioning.ResourceScope",
        "System.Security.Permissions.SecurityAction",
    ];
    match full_name {
        "System.Security.SecurityRuleSet" => Some(0x05),
        name if INT32.contains(&name) => Some(0x08),
        _ => None,
    }
}

/// The most type tokens a `TypeSpec` may name with every `TypeSpec` inside it unfolded.
const MAX_SPEC_TOKENS: usize = 4096;

/// How deeply a custom attribute value may nest arrays and boxed values before it is rejected.
const MAX_ATTRIBUTE_DEPTH: u32 = 16;

/// Decodes a custom attribute value blob (II.23.3) against the constructor's parameter types.
/// `underlying` names an enum's underlying element type (`0x08` for `System.Int32`); an enum it
/// does not know cannot be decoded, because its values' width is unknown.
///
/// # Errors
/// When the prolog is wrong, a value runs past the blob, arrays or boxed values nest deeper
/// than 16, or an enum's underlying type is unknown.
pub fn decode_attribute(
    value: &[u8],
    params: &[TypeSig],
    underlying: &dyn Fn(EnumKey<'_>) -> Option<u8>,
) -> Read<AttributeArguments> {
    let mut r = Reader::new(value, 0, "custom attribute value");
    if value.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    if r.u16()? != 1 {
        return malformed("custom attribute prolog", 0);
    }
    let mut d = ValueDecoder { r, underlying };
    let mut arguments = Vec::with_capacity(params.len());
    for param in params {
        arguments.push(d.fixed_arg(param, 0)?);
    }
    let mut named = Vec::new();
    if d.r.position() + 2 <= value.len() {
        let count = d.r.u16()?;
        for _ in 0..count {
            d.r.u8()?; // FIELD (0x53) or PROPERTY (0x54)
            let kind = d.field_or_prop_type(0)?;
            let name = ser_string(&mut d.r)?.unwrap_or_default();
            named.push((name, d.elem_by_code(&kind, 0)?));
        }
    }
    Ok((arguments, named))
}

/// A `FieldOrPropType` (II.23.3): an element type code, with an enum or array's detail.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ArgType {
    Code(u8),
    /// An enum, by its underlying element type code.
    Enum(u8),
    Array(Box<ArgType>),
}

struct ValueDecoder<'a, 'u> {
    r: Reader<'a>,
    underlying: &'u dyn Fn(EnumKey<'_>) -> Option<u8>,
}

impl ValueDecoder<'_, '_> {
    fn deep(&self, depth: u32) -> Read<()> {
        if depth > MAX_ATTRIBUTE_DEPTH {
            return malformed("custom attribute (nested too deeply)", self.r.position());
        }
        Ok(())
    }

    fn enum_of(&self, key: EnumKey<'_>) -> Read<ArgType> {
        match (self.underlying)(key) {
            Some(code) => Ok(ArgType::Enum(code)),
            None => malformed(
                "custom attribute enum (underlying type unknown)",
                self.r.position(),
            ),
        }
    }

    fn field_or_prop_type(&mut self, depth: u32) -> Read<ArgType> {
        self.deep(depth)?;
        Ok(match self.r.u8()? {
            0x1D => ArgType::Array(Box::new(self.field_or_prop_type(depth + 1)?)),
            0x55 => {
                let name = ser_string(&mut self.r)?.unwrap_or_default();
                self.enum_of(EnumKey::Name(&name))?
            }
            code => ArgType::Code(code),
        })
    }

    fn elem_by_code(&mut self, kind: &ArgType, depth: u32) -> Read<String> {
        self.deep(depth)?;
        Ok(match kind {
            ArgType::Enum(code) => return self.elem_by_code(&ArgType::Code(*code), depth + 1),
            ArgType::Array(element) => {
                let count = self.r.u32()?;
                if count == u32::MAX {
                    return Ok("null".to_owned());
                }
                let mut items = Vec::with_capacity(count.min(64) as usize);
                for _ in 0..count {
                    items.push(self.elem_by_code(element, depth + 1)?);
                }
                format!("[{}]", items.join(", "))
            }
            ArgType::Code(code) => match code {
                0x02 => bool_text(self.r.u8()? != 0),
                0x03 => {
                    char::from_u32(u32::from(self.r.u16()?)).map_or_else(String::new, String::from)
                }
                0x04 => i8::from_le_bytes([self.r.u8()?]).to_string(),
                0x05 => self.r.u8()?.to_string(),
                0x06 => i16::from_le_bytes(self.r.u16()?.to_le_bytes()).to_string(),
                0x07 => self.r.u16()?.to_string(),
                0x08 => i32::from_le_bytes(self.r.u32()?.to_le_bytes()).to_string(),
                0x09 => self.r.u32()?.to_string(),
                0x0A => i64::from_le_bytes(self.r.u64()?.to_le_bytes()).to_string(),
                0x0B => self.r.u64()?.to_string(),
                0x0C => f32::from_le_bytes(self.r.u32()?.to_le_bytes()).to_string(),
                0x0D => f64::from_le_bytes(self.r.u64()?.to_le_bytes()).to_string(),
                0x0E | 0x50 => ser_string(&mut self.r)?.unwrap_or_else(|| "null".to_owned()),
                0x51 => {
                    let boxed = self.field_or_prop_type(depth + 1)?;
                    self.elem_by_code(&boxed, depth + 1)?
                }
                _ => return malformed("custom attribute element type", self.r.position()),
            },
        })
    }

    /// A fixed argument typed by a constructor parameter.
    fn fixed_arg(&mut self, param: &TypeSig, depth: u32) -> Read<String> {
        self.deep(depth)?;
        let kind = match param.unmodified() {
            TypeSig::Primitive(name) => ArgType::Code(match *name {
                "System.String" => 0x0E,
                "System.Object" => 0x51,
                "System.Single" => 0x0C,
                "System.Double" => 0x0D,
                other => match enum_code_of(other) {
                    Some(code) => code,
                    None => {
                        return malformed("custom attribute parameter type", self.r.position());
                    }
                },
            }),
            // A value type parameter is an enum (the only value types an attribute takes),
            // decoded at its underlying type's width.
            TypeSig::Named {
                value_type: true,
                token,
            } => self.enum_of(EnumKey::Token(*token))?,
            // A class parameter is System.Type (the only class an attribute takes besides string
            // and object, which are primitives here).
            TypeSig::Named {
                value_type: false, ..
            } => ArgType::Code(0x50),
            TypeSig::SzArray(element) => {
                let count = self.r.u32()?;
                if count == u32::MAX {
                    return Ok("null".to_owned());
                }
                let mut items = Vec::with_capacity(count.min(64) as usize);
                for _ in 0..count {
                    items.push(self.fixed_arg(element, depth + 1)?);
                }
                return Ok(format!("[{}]", items.join(", ")));
            }
            _ => return malformed("custom attribute parameter type", self.r.position()),
        };
        self.elem_by_code(&kind, depth + 1)
    }
}

fn ser_string(r: &mut Reader<'_>) -> Read<Option<String>> {
    let at = r.position();
    let mut probe = r.clone();
    if probe.u8()? == 0xFF {
        r.skip(1)?;
        return Ok(None);
    }
    let length = r.compressed_u32()? as usize;
    let bytes = r.bytes(length)?;
    std::str::from_utf8(bytes)
        .map(|s| Some(s.to_owned()))
        .or_else(|_| malformed("custom attribute string", at))
}

impl From<ReadError> for crate::attribute::AttributeError {
    fn from(source: ReadError) -> Self {
        Self::Malformed {
            path: std::path::PathBuf::new(),
            source,
        }
    }
}

/// Helpers for tests elsewhere in the crate that build a model by hand.
#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    /// An assembly named `name` with nothing in it.
    pub(crate) fn empty(name: &str) -> Loaded {
        Loaded {
            identity: Identity {
                name: name.to_owned(),
                version: [0; 4],
                culture: String::new(),
                public_key_token: None,
            },
            target_framework: None,
            types: Vec::new(),
            type_refs: Vec::new(),
            type_specs: Vec::new(),
            member_refs: Vec::new(),
            method_specs: Vec::new(),
            assembly_refs: Vec::new(),
            assembly_attributes: Vec::new(),
            debug: Vec::new(),
            index: RowIndex::default(),
        }
    }

    /// A type at `row` named `namespace.name`, with nothing in it.
    pub(crate) fn ty(row: u32, namespace: &str, name: &str) -> Type {
        Type {
            row,
            namespace: namespace.to_owned(),
            name: name.to_owned(),
            full_name: format!("{namespace}.{name}"),
            flags: 0,
            extends: None,
            enclosing: None,
            interfaces: Vec::new(),
            generic_params: Vec::new(),
            generic_constraints: Vec::new(),
            fields: Vec::new(),
            methods: Vec::new(),
            properties: Vec::new(),
            events: Vec::new(),
            attributes: Vec::new(),
            compiler_generated: false,
            is_module_type: false,
        }
    }

    /// Strings or blobs laid out as a heap (index 0 is the empty entry), with each one's index.
    pub(crate) fn heap(items: &[&[u8]], blob: bool) -> (Vec<u8>, Vec<u32>) {
        let mut data = vec![0u8];
        let mut at = Vec::new();
        for item in items {
            at.push(u32::try_from(data.len()).unwrap_or(0));
            if blob {
                data.push(u8::try_from(item.len()).unwrap_or(0));
                data.extend(*item);
            } else {
                data.extend(*item);
                data.push(0);
            }
        }
        (data, at)
    }

    /// A whole assembly image from tables (2-byte indices), a `#Strings` and a `#Blob` heap, and
    /// any extra streams.
    pub(crate) fn assembly(
        tables: &[(TableId, Vec<Vec<u32>>)],
        strings: Vec<u8>,
        blobs: Vec<u8>,
        extra: Vec<(&str, Vec<u8>)>,
    ) -> Vec<u8> {
        let mut streams = vec![
            (
                "#~",
                crate::metadata::tables::tests::table_stream(tables, 0),
            ),
            ("#Strings", strings),
            ("#Blob", blobs),
        ];
        streams.extend(extra);
        crate::pe::tests::pe_with(&crate::metadata::tests::root(&streams))
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{assembly, heap};
    use super::*;
    use proptest::prelude::*;

    /// Decodes with every enum taken as `System.Int32`.
    fn decode(value: &[u8], params: &[TypeSig]) -> Read<AttributeArguments> {
        decode_attribute(value, params, &|_| Some(0x08))
    }

    #[test]
    fn identities_display_as_reflection_does() {
        let identity = Identity {
            name: "TestAssembly".into(),
            version: [1, 0, 0, 0],
            culture: String::new(),
            public_key_token: None,
        };
        assert_eq!(
            identity.display(),
            "TestAssembly, Version=1.0.0.0, Culture=neutral, PublicKeyToken=null"
        );
        let signed = Identity {
            culture: "de".into(),
            public_key_token: Some("b77a5c561934e089".into()),
            ..identity
        };
        assert_eq!(
            signed.display(),
            "TestAssembly, Version=1.0.0.0, Culture=de, PublicKeyToken=b77a5c561934e089"
        );
    }

    #[test]
    fn the_public_key_token_is_the_reversed_tail_of_the_sha1() {
        // The ECMA standard public key has the well-known token b77a5c561934e089.
        let ecma_key = [0u8, 0, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(public_key_token(&ecma_key), "b77a5c561934e089");
        assert_eq!(hex(&[0x0a, 0xff]), "0aff");
    }

    #[test]
    fn list_ranges_clamp_to_the_table() {
        let starts = [1, 1, 3, 9];
        assert_eq!(list_range(&starts, 0, 5), 1..1);
        assert_eq!(list_range(&starts, 1, 5), 1..3);
        assert_eq!(list_range(&starts, 2, 5), 3..6, "clamped to the table end");
        assert_eq!(
            list_range(&starts, 3, 5),
            6..6,
            "a start past the end owns nothing"
        );
        assert_eq!(list_range(&starts, 9, 5), 6..6);
    }

    #[test]
    fn decodes_positional_and_named_attribute_arguments() {
        // [X("abc", 7, true, typeof(Foo), E.B, new[] {1, 2}, Name = "n", Flag = 3)]
        let mut value = vec![1, 0];
        value.extend([3, b'a', b'b', b'c']);
        value.extend(7i32.to_le_bytes());
        value.push(1);
        value.extend([3, b'F', b'o', b'o']);
        value.extend(1u32.to_le_bytes());
        value.extend(2u32.to_le_bytes());
        value.extend(1i32.to_le_bytes());
        value.extend(2i32.to_le_bytes());
        value.extend(2u16.to_le_bytes());
        value.extend([0x54, 0x0E, 4, b'N', b'a', b'm', b'e', 1, b'n']);
        value.extend([0x53, 0x51, 4, b'F', b'l', b'a', b'g', 0x08]);
        value.extend(3i32.to_le_bytes());
        let t = Token { table: 1, row: 1 };
        let params = [
            TypeSig::Primitive("System.String"),
            TypeSig::Primitive("System.Int32"),
            TypeSig::Primitive("System.Boolean"),
            TypeSig::Named {
                token: t,
                value_type: false,
            },
            TypeSig::Named {
                token: t,
                value_type: true,
            },
            TypeSig::SzArray(Box::new(TypeSig::Primitive("System.Int32"))),
        ];
        let decoded = decode(&value, &params);
        assert_eq!(
            decoded,
            Ok((
                vec![
                    "abc".to_owned(),
                    "7".to_owned(),
                    "True".to_owned(),
                    "Foo".to_owned(),
                    "1".to_owned(),
                    "[1, 2]".to_owned()
                ],
                vec![
                    ("Name".to_owned(), "n".to_owned()),
                    ("Flag".to_owned(), "3".to_owned())
                ]
            ))
        );
    }

    #[test]
    fn decodes_every_primitive_and_nulls() {
        let cases: Vec<(&str, Vec<u8>, &str)> = vec![
            ("System.Char", vec![b'x', 0], "x"),
            ("System.SByte", vec![0xFF], "-1"),
            ("System.Byte", vec![200], "200"),
            ("System.Int16", (-2i16).to_le_bytes().to_vec(), "-2"),
            ("System.UInt16", 9u16.to_le_bytes().to_vec(), "9"),
            ("System.UInt32", 9u32.to_le_bytes().to_vec(), "9"),
            ("System.Int64", (-5i64).to_le_bytes().to_vec(), "-5"),
            ("System.UInt64", 5u64.to_le_bytes().to_vec(), "5"),
            ("System.Single", 1.5f32.to_le_bytes().to_vec(), "1.5"),
            ("System.Double", 2.5f64.to_le_bytes().to_vec(), "2.5"),
            ("System.String", vec![0xFF], "null"),
            ("System.Boolean", vec![0], "False"),
        ];
        for (name, bytes, expected) in cases {
            let mut value = vec![1, 0];
            value.extend(bytes);
            let decoded = decode(&value, &[TypeSig::Primitive(name)]).map(|d| d.0);
            assert_eq!(decoded, Ok(vec![expected.to_owned()]), "{name}");
        }
        let mut null_array = vec![1, 0];
        null_array.extend(u32::MAX.to_le_bytes());
        assert_eq!(
            decode(
                &null_array,
                &[TypeSig::SzArray(Box::new(TypeSig::Primitive(
                    "System.Int32"
                )))]
            )
            .map(|d| d.0),
            Ok(vec!["null".to_owned()])
        );
        assert_eq!(decode(&[], &[]), Ok((vec![], vec![])));
        assert!(decode(&[2, 0], &[]).is_err(), "wrong prolog");
        assert!(
            decode(&[1, 0], &[TypeSig::Primitive("System.Int32")]).is_err(),
            "value missing"
        );
        assert!(
            decode(&[1, 0, 0], &[TypeSig::Primitive("System.Void")]).is_err(),
            "no attribute takes a void"
        );
        assert!(decode(&[1, 0, 0], &[TypeSig::Var(0)]).is_err());
    }

    proptest! {
        #[test]
        fn any_attribute_blob_decodes_or_fails_cleanly(
            value in proptest::collection::vec(any::<u8>(), 0..48)
        ) {
            let params = [
                TypeSig::Primitive("System.String"),
                TypeSig::Primitive("System.Object"),
                TypeSig::SzArray(Box::new(TypeSig::Primitive("System.Int32"))),
            ];
            let _ = decode(&value, &params);
        }
    }

    fn what<T>(result: &Read<T>) -> Option<&'static str> {
        result.as_ref().err().map(|e| e.what)
    }

    /// A `Module` row naming the module `M.dll` (string index 1 of every heap below).
    fn module() -> (TableId, Vec<Vec<u32>>) {
        (id::MODULE, vec![vec![0, 1, 0, 0, 0]])
    }

    #[test]
    fn a_type_spec_naming_itself_or_a_later_one_is_malformed() {
        let (strings, _) = heap(&[b"M.dll"], false);
        for spec in [
            vec![0x12, 0x06],                // CLASS TypeSpec 1, in TypeSpec 1
            vec![0x15, 0x12, 0x06, 1, 0x08], // GenericInst<int> of TypeSpec 1
            vec![0x20, 0x0A, 0x08],          // modopt(TypeSpec 2) int
            vec![0x1D, 0x11, 0x0A],          // VALUETYPE TypeSpec 2 []
        ] {
            let (blobs, at) = heap(&[&spec], true);
            let image = assembly(
                &[module(), (id::TYPE_SPEC, vec![vec![at[0]]])],
                strings.clone(),
                blobs,
                vec![],
            );
            let loaded = Loaded::read(&image);
            assert_eq!(
                what(&loaded),
                Some("TypeSpec (names itself or a later TypeSpec)"),
                "{spec:?}"
            );
        }
        // TypeSpec k = GenericInst<TypeSpec k-1, TypeSpec k-1>: in order, but 2^k tokens unfolded.
        let mut specs: Vec<Vec<u8>> = vec![vec![0x08]];
        for row in 1u8..20 {
            let t = (row << 2) | 2;
            specs.push(vec![0x15, 0x12, t, 2, 0x12, t, 0x12, t]);
        }
        let refs: Vec<&[u8]> = specs.iter().map(Vec::as_slice).collect();
        let (blobs, at) = heap(&refs, true);
        let image = assembly(
            &[
                module(),
                (id::TYPE_SPEC, at.iter().map(|b| vec![*b]).collect()),
            ],
            strings.clone(),
            blobs,
            vec![],
        );
        assert_eq!(
            what(&Loaded::read(&image)),
            Some("TypeSpec (unfolds too large)")
        );
        // TypeSpec 2 naming TypeSpec 1 is fine.
        let (blobs, at) = heap(&[&[0x08], &[0x1D, 0x12, 0x06]], true);
        let image = assembly(
            &[module(), (id::TYPE_SPEC, vec![vec![at[0]], vec![at[1]]])],
            strings,
            blobs,
            vec![],
        );
        assert_eq!(Loaded::read(&image).map(|l| l.type_specs.len()), Ok(2));
    }

    #[test]
    fn a_nested_class_cycle_is_malformed_for_both_readers() {
        let (strings, _) = heap(&[b"M.dll", b"A", b"B"], false);
        let (blobs, _) = heap(&[], true);
        let typedef = |name: u32| vec![0, name, 0, 0, 1, 1];
        for nesting in [vec![vec![1, 2], vec![2, 1]], vec![vec![2, 2]]] {
            let image = assembly(
                &[
                    module(),
                    (id::TYPE_DEF, vec![typedef(7), typedef(9)]),
                    (id::NESTED_CLASS, nesting.clone()),
                ],
                strings.clone(),
                blobs.clone(),
                vec![],
            );
            assert_eq!(
                what(&Loaded::read(&image)),
                Some("NestedClass cycle"),
                "{nesting:?}"
            );
            assert_eq!(
                what(&crate::assembly::Assembly::read(&image)),
                Some("NestedClass cycle")
            );
        }
    }

    #[test]
    fn nested_names_are_linear_and_bounded() {
        assert_eq!(
            nested_names(&[
                ("Ns", "Outer", None),
                ("", "Inner", Some(1)),
                ("", "Deep", Some(2))
            ]),
            Ok(vec![
                ("Ns".to_owned(), "Ns.Outer".to_owned()),
                ("Ns".to_owned(), "Ns.Outer+Inner".to_owned()),
                ("Ns".to_owned(), "Ns.Outer+Inner+Deep".to_owned()),
            ])
        );
        assert_eq!(
            nested_names(&[
                ("", "Global", None),
                ("X", "Stray", Some(0)),
                ("Y", "Past", Some(9))
            ]),
            Ok(vec![
                (String::new(), "Global".to_owned()),
                ("X".to_owned(), "X.Stray".to_owned()),
                ("Y".to_owned(), "Y.Past".to_owned()),
            ]),
            "an enclosing row naming no type ends the chain"
        );
        let chain = |length: u32| -> Vec<(&str, &str, Option<u32>)> {
            (1..=length)
                .map(|row| ("N", "T", (row > 1).then_some(row - 1)))
                .collect()
        };
        assert!(nested_names(&chain(65)).is_ok());
        assert_eq!(
            what(&nested_names(&chain(66))),
            Some("NestedClass (nested too deeply)")
        );
        // A wide, shallow forest of 200,000 types is named in one pass.
        let wide: Vec<(&str, &str, Option<u32>)> = (1..=200_000u32)
            .map(|row| ("N", "T", (row > 1).then_some(1)))
            .collect();
        assert_eq!(nested_names(&wide).map(|n| n.len()), Ok(200_000));
    }

    #[test]
    fn a_type_ref_scope_cycle_is_malformed() {
        let (strings, _) = heap(&[b"M.dll", b"A", b"B"], false);
        let (blobs, _) = heap(&[], true);
        let enclosing = |row: u32| (row << 2) | 3;
        let image = assembly(
            &[
                module(),
                (
                    id::TYPE_REF,
                    vec![vec![enclosing(2), 7, 0], vec![enclosing(1), 9, 0]],
                ),
            ],
            strings,
            blobs,
            vec![],
        );
        assert_eq!(what(&Loaded::read(&image)), Some("TypeRef nesting cycle"));
        let deep: Vec<TypeRefRow> = (1..=70u32)
            .map(|row| TypeRefRow {
                namespace: String::new(),
                name: "T".into(),
                scope: if row == 1 {
                    Scope::Module
                } else {
                    Scope::Enclosing(row - 1)
                },
            })
            .collect();
        assert_eq!(
            what(&type_ref_names(&deep)),
            Some("TypeRef (nested too deeply)")
        );
        assert_eq!(
            type_ref_names(&deep[..3]),
            Ok(vec!["T".to_owned(), "T+T".to_owned(), "T+T+T".to_owned()])
        );
    }

    #[test]
    fn a_pdb_stream_in_an_assembly_does_not_size_its_tables() {
        // One TypeDef, and a #Pdb stream claiming u32::MAX Field and MethodDef rows: honoured,
        // the list ranges would run to u32::MAX + 1.
        let (strings, _) = heap(&[b"M.dll", b"A"], false);
        let (blobs, _) = heap(&[], true);
        let mut pdb = vec![0u8; 20];
        pdb.extend(0u32.to_le_bytes());
        pdb.extend(((1u64 << id::FIELD) | (1u64 << id::METHOD_DEF)).to_le_bytes());
        pdb.extend(u32::MAX.to_le_bytes());
        pdb.extend(u32::MAX.to_le_bytes());
        let image = assembly(
            &[module(), (id::TYPE_DEF, vec![vec![0, 7, 0, 0, 1, 1]])],
            strings,
            blobs,
            vec![("#Pdb", pdb)],
        );
        let loaded = Loaded::read(&image);
        assert_eq!(
            loaded.map(|l| l.types.iter().map(|t| t.methods.len()).sum::<usize>()),
            Ok(0)
        );
        assert!(crate::assembly::Assembly::read(&image).is_ok());
        assert_eq!(list_range(&[1], 0, u32::MAX), 1..u32::MAX);
        assert_eq!(list_range(&[5, 2], 0, u32::MAX), 5..5);
        assert_eq!(list_range(&[1], usize::MAX, 3), 4..4);
    }

    /// Types `<Module>`, `N.E` (an enum over `System.Byte`), `N.Attr` (whose constructor takes
    /// `N.E`) and `N.Target`, which carries `[Attr(E)7]`, `[Ext(Other.Unknown)7]` and
    /// `[Ext(System.AttributeTargets)4]`.
    fn enum_image() -> Vec<u8> {
        let (strings, s) = heap(
            &[
                b"M.dll",
                b"<Module>",
                b"E",
                b"N",
                b"value__",
                b"Attr",
                b".ctor",
                b"Target",
                b"Ext",
                b"Unknown",
                b"Other",
                b"AttributeTargets",
                b"System",
            ],
            false,
        );
        let (blobs, b) = heap(
            &[
                &[0x06, 0x05],                // field: uint8
                &[0x20, 1, 0x01, 0x11, 0x08], // .ctor(valuetype E)
                &[1, 0, 7, 0, 0],             // Attr: E 7 as a byte
                &[0x20, 1, 0x01, 0x11, 0x09], // .ctor(valuetype TypeRef 2)
                &[1, 0, 7, 0, 0, 0, 0, 0],    // Ext: 7
                &[0x20, 1, 0x01, 0x11, 0x0D], // .ctor(valuetype TypeRef 3)
                &[1, 0, 4, 0, 0, 0, 0, 0],    // Ext: 4
            ],
            true,
        );
        let typedef = |name: usize, ns: u32, fields: u32, methods: u32| {
            vec![0, s[name], ns, 0, fields, methods]
        };
        let module_scope = 1 << 2;
        assembly(
            &[
                module(),
                (
                    id::TYPE_REF,
                    vec![
                        vec![module_scope, s[8], s[3]],
                        vec![module_scope, s[9], s[10]],
                        vec![module_scope, s[11], s[12]],
                    ],
                ),
                (
                    id::TYPE_DEF,
                    vec![
                        typedef(1, 0, 1, 1),
                        typedef(2, s[3], 1, 1),
                        typedef(5, s[3], 2, 1),
                        typedef(7, s[3], 2, 2),
                    ],
                ),
                (id::FIELD, vec![vec![0, s[4], b[0]]]),
                (id::METHOD_DEF, vec![vec![0, 0, 0, s[6], b[1], 1]]),
                (
                    id::MEMBER_REF,
                    vec![
                        vec![(1 << 3) | 1, s[6], b[3]],
                        vec![(1 << 3) | 1, s[6], b[5]],
                    ],
                ),
                (
                    id::CUSTOM_ATTRIBUTE,
                    vec![
                        vec![(4 << 5) | 3, (1 << 3) | 2, b[2]],
                        vec![(4 << 5) | 3, (1 << 3) | 3, b[4]],
                        vec![(4 << 5) | 3, (2 << 3) | 3, b[6]],
                    ],
                ),
            ],
            strings,
            blobs,
            vec![],
        )
    }

    #[test]
    fn enum_arguments_decode_at_the_underlying_width_or_are_marked_undecoded() {
        let loaded = Loaded::read(&enum_image());
        let loaded = loaded.as_ref();
        let target = loaded.ok().and_then(|l| l.type_at(4));
        let attributes: Vec<(Vec<String>, bool)> = target
            .map(|t| {
                t.attributes
                    .iter()
                    .map(|a| (a.arguments.clone(), a.undecoded.is_some()))
                    .collect()
            })
            .unwrap_or_default();
        assert_eq!(
            attributes,
            vec![
                (vec!["7".to_owned()], false),
                (vec![], true),
                (vec!["4".to_owned()], false),
            ],
            "a local byte enum reads one byte; an unknown external enum is undecoded, not empty; \
             a framework enum is known"
        );
        let undecoded = target.and_then(|t| t.attributes[1].undecoded.clone());
        assert_eq!(
            undecoded.map(|u| u.value),
            Some(vec![1, 0, 7, 0, 0, 0, 0, 0]),
            "the blob is kept for the code layer's second try"
        );
        // The row index answers the member lookups.
        assert_eq!(
            loaded
                .ok()
                .and_then(|l| l.method_at(1))
                .map(|(t, m)| (t.full_name.as_str(), m.name.as_str())),
            Some(("N.Attr", ".ctor"))
        );
        assert_eq!(
            loaded
                .ok()
                .and_then(|l| l.field_at(1))
                .map(|(t, f)| (t.full_name.as_str(), f.name.as_str())),
            Some(("N.E", "value__"))
        );
        assert!(loaded.ok().and_then(|l| l.method_at(2)).is_none());
    }

    #[test]
    fn a_malformed_attribute_value_is_undecoded_not_empty() {
        let t = Token {
            table: id::TYPE_REF,
            row: 1,
        };
        let int = [TypeSig::Primitive("System.Int32")];
        let bad = Attribute::decode(t, &[1, 0, 7], &int, &|_| None);
        assert_eq!(bad.arguments, Vec::<String>::new());
        assert!(bad.undecoded.is_some());
        let good = Attribute::decode(t, &[1, 0, 7, 0, 0, 0], &int, &|_| None);
        assert_eq!(
            (good.arguments, good.undecoded),
            (vec!["7".to_owned()], None)
        );
    }

    #[test]
    fn enums_decode_at_their_underlying_width_and_sign() {
        let e = TypeSig::Named {
            token: Token {
                table: id::TYPE_DEF,
                row: 2,
            },
            value_type: true,
        };
        let with = |code: u8| move |_: EnumKey<'_>| Some(code);
        assert_eq!(
            decode_attribute(&[1, 0, 0xFF], std::slice::from_ref(&e), &with(0x04)).map(|d| d.0),
            Ok(vec!["-1".to_owned()])
        );
        assert_eq!(
            decode_attribute(
                &[1, 0, 0xFF, 0xFF, 0xFF, 0xFF],
                std::slice::from_ref(&e),
                &with(0x08)
            )
            .map(|d| d.0),
            Ok(vec!["-1".to_owned()])
        );
        assert_eq!(
            decode_attribute(
                &[1, 0, 9, 0, 0, 0, 0, 0, 0, 0],
                std::slice::from_ref(&e),
                &with(0x0B)
            )
            .map(|d| d.0),
            Ok(vec!["9".to_owned()])
        );
        assert_eq!(
            what(&decode_attribute(&[1, 0, 1, 0, 0, 0], &[e], &|_| None)),
            Some("custom attribute enum (underlying type unknown)")
        );
        // A named argument of an enum type names it by string: [Flag = (E)3].
        let mut value = vec![1, 0, 1, 0, 0x54, 0x55, 3, b'N', b'.', b'E', 4];
        value.extend(b"Flag");
        value.push(3);
        let named = decode_attribute(&value, &[], &|key| {
            (key == EnumKey::Name("N.E")).then_some(0x05)
        });
        assert_eq!(
            named.map(|d| d.1),
            Ok(vec![("Flag".to_owned(), "3".to_owned())])
        );
        assert_eq!(type_name_of("Ns.E, Asm, Version=1.0.0.0"), "Ns.E");
        assert_eq!(
            type_name_of("Ns.G`1[[Ns.A, Asm]], Other"),
            "Ns.G`1[[Ns.A, Asm]]"
        );
        assert_eq!(enum_code_of("System.String"), None);
        assert_eq!(well_known_enum("System.AttributeTargets"), Some(0x08));
        assert_eq!(
            well_known_enum("System.Security.SecurityRuleSet"),
            Some(0x05)
        );
        assert_eq!(well_known_enum("Other.Unknown"), None);
    }

    #[test]
    fn nested_attribute_values_are_bounded_not_a_stack_overflow() {
        // A boxed object whose type is a boxed object whose type is ... two million deep.
        let mut boxed = vec![1u8, 0];
        boxed.extend(std::iter::repeat_n(0x51u8, 2_000_000));
        assert_eq!(
            what(&decode(&boxed, &[TypeSig::Primitive("System.Object")])),
            Some("custom attribute (nested too deeply)")
        );
        // A named argument whose type is an array of arrays of ... a million deep.
        let mut arrays = vec![1u8, 0, 1, 0, 0x53];
        arrays.extend(std::iter::repeat_n(0x1Du8, 1_000_000));
        assert_eq!(
            what(&decode(&arrays, &[])),
            Some("custom attribute (nested too deeply)")
        );
        // Sixteen levels are fine: object -> object[] -> ... boxed.
        let mut fine = vec![1u8, 0, 0x51, 0x1D, 0x08];
        fine.extend(1u32.to_le_bytes());
        fine.extend(5i32.to_le_bytes());
        assert_eq!(
            decode(&fine, &[TypeSig::Primitive("System.Object")]).map(|d| d.0),
            Ok(vec!["[5]".to_owned()])
        );
    }

    #[test]
    fn the_row_index_keeps_the_first_type_that_claims_a_row() {
        let mut loaded = testing::empty("A");
        let method = |row: u32, name: &str| Method {
            row,
            name: name.to_owned(),
            flags: 0,
            sig: MethodSig {
                has_this: false,
                generic_params: 0,
                ret: TypeSig::Primitive("System.Void"),
                params: Vec::new(),
            },
            parameters: Vec::new(),
            generic_params: Vec::new(),
            generic_constraints: Vec::new(),
            body: None,
            locals: Vec::new(),
            strings: Vec::new(),
            attributes: Vec::new(),
        };
        let mut first = testing::ty(1, "N", "First");
        first.methods = vec![method(1, "a"), method(2, "b")];
        let mut second = testing::ty(2, "N", "Second");
        second.methods = vec![method(2, "overlap"), method(3, "c")];
        loaded.types = vec![first, second];
        assert!(
            loaded.method_at(1).is_none(),
            "an unindexed model finds nothing"
        );
        loaded.reindex();
        let name = |row| {
            loaded
                .method_at(row)
                .map(|(t, m)| (t.name.clone(), m.name.clone()))
        };
        assert_eq!(name(1), Some(("First".into(), "a".into())));
        assert_eq!(name(2), Some(("First".into(), "b".into())));
        assert_eq!(name(3), Some(("Second".into(), "c".into())));
        assert_eq!(name(4), None);
        assert_eq!(name(0), None);
    }
}
