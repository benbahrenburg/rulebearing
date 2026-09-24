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
}

impl Loaded {
    /// The type with 1-based row `row`.
    pub fn type_at(&self, row: u32) -> Option<&Type> {
        self.types.get((row as usize).checked_sub(1)?)
    }

    /// The declaring type row and method of `MethodDef` row `row`.
    pub fn method_at(&self, row: u32) -> Option<(&Type, &Method)> {
        self.types.iter().find_map(|t| {
            t.methods
                .binary_search_by_key(&row, |m| m.row)
                .ok()
                .map(|i| (t, &t.methods[i]))
        })
    }

    /// The declaring type and field of `Field` row `row`.
    pub fn field_at(&self, row: u32) -> Option<(&Type, &Field)> {
        self.types.iter().find_map(|t| {
            t.fields
                .binary_search_by_key(&row, |f| f.row)
                .ok()
                .map(|i| (t, &t.fields[i]))
        })
    }

    /// Reads an assembly file.
    ///
    /// # Errors
    /// When the PE image, the metadata, a signature or a method body is malformed.
    pub fn read(bytes: &[u8]) -> Read<Self> {
        let image = PeImage::parse(bytes)?;
        let metadata = Metadata::parse(image.metadata, None)?;
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
    let start = starts.get(index).copied().unwrap_or(total + 1);
    let end = starts.get(index + 1).copied().unwrap_or(total + 1);
    start.min(total + 1)..end.clamp(start.min(total + 1), total + 1)
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
        let type_specs = (1..=self.rows(id::TYPE_SPEC))
            .map(|row| type_spec(self.blob(id::TYPE_SPEC, row, 0)?))
            .collect::<Read<Vec<_>>>()?;
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
        let mut attributes = self.attributes(&member_refs)?;
        let mut types = self.types(&mut attributes)?;
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
        Ok(Loaded {
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
        })
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
            let (arguments, named) = decode_attribute(value, &params).unwrap_or_default();
            let type_arguments = params
                .iter()
                .zip(&arguments)
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
                .collect();
            found.entry((table, target)).or_default().push(Attribute {
                type_,
                arguments,
                named,
                type_arguments,
            });
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
    fn types(&self, attributes: &mut BTreeMap<(TableId, u32), Vec<Attribute>>) -> Read<Vec<Type>> {
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

        let mut types = Vec::with_capacity(count as usize);
        for row in 1..=count {
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
                namespace: self.string(id::TYPE_DEF, row, 2)?,
                full_name: String::new(),
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
        full_names(&mut types);
        Ok(types)
    }
}

/// Fills `full_name` and the outermost namespace by walking each nesting chain; a malformed
/// cycle stops at the type count.
fn full_names(types: &mut [Type]) {
    for index in 0..types.len() {
        let mut parts = vec![types[index].name.clone()];
        let mut namespace = types[index].namespace.clone();
        let mut current = types[index].enclosing;
        let mut steps = 0;
        while let Some(outer) =
            current.and_then(|r| (r as usize).checked_sub(1).and_then(|i| types.get(i)))
        {
            parts.push(outer.name.clone());
            namespace.clone_from(&outer.namespace);
            current = outer.enclosing;
            steps += 1;
            if steps > types.len() {
                break;
            }
        }
        parts.reverse();
        let joined = parts.join("+");
        types[index].full_name = if namespace.is_empty() {
            joined
        } else {
            format!("{namespace}.{joined}")
        };
        types[index].namespace = namespace;
    }
}

/// Decodes a custom attribute value blob (II.23.3) against the constructor's parameter types.
///
/// # Errors
/// When the prolog is wrong or a value runs past the blob.
pub fn decode_attribute(value: &[u8], params: &[TypeSig]) -> Read<AttributeArguments> {
    let mut r = Reader::new(value, 0, "custom attribute value");
    if value.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    if r.u16()? != 1 {
        return malformed("custom attribute prolog", 0);
    }
    let mut arguments = Vec::with_capacity(params.len());
    for param in params {
        arguments.push(fixed_arg(&mut r, param)?);
    }
    let mut named = Vec::new();
    if r.position() + 2 <= value.len() {
        let count = r.u16()?;
        for _ in 0..count {
            r.u8()?; // FIELD (0x53) or PROPERTY (0x54)
            let kind = field_or_prop_type(&mut r)?;
            let name = ser_string(&mut r)?.unwrap_or_default();
            named.push((name, elem_by_code(&mut r, &kind)?));
        }
    }
    Ok((arguments, named))
}

/// A `FieldOrPropType` (II.23.3): an element type code, with an enum or array's detail.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ArgType {
    Code(u8),
    Enum,
    Array(Box<ArgType>),
}

fn field_or_prop_type(r: &mut Reader<'_>) -> Read<ArgType> {
    Ok(match r.u8()? {
        0x1D => ArgType::Array(Box::new(field_or_prop_type(r)?)),
        0x55 => {
            ser_string(r)?; // the enum's type name
            ArgType::Enum
        }
        code => ArgType::Code(code),
    })
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

fn elem_by_code(r: &mut Reader<'_>, kind: &ArgType) -> Read<String> {
    Ok(match kind {
        ArgType::Enum => r.u32()?.to_string(),
        ArgType::Array(element) => {
            let count = r.u32()?;
            if count == u32::MAX {
                return Ok("null".to_owned());
            }
            let mut items = Vec::with_capacity(count.min(64) as usize);
            for _ in 0..count {
                items.push(elem_by_code(r, element)?);
            }
            format!("[{}]", items.join(", "))
        }
        ArgType::Code(code) => match code {
            0x02 => bool_text(r.u8()? != 0),
            0x03 => char::from_u32(u32::from(r.u16()?)).map_or_else(String::new, String::from),
            0x04 => i8::from_le_bytes([r.u8()?]).to_string(),
            0x05 => r.u8()?.to_string(),
            0x06 => i16::from_le_bytes(r.u16()?.to_le_bytes()).to_string(),
            0x07 => r.u16()?.to_string(),
            0x08 => i32::from_le_bytes(r.u32()?.to_le_bytes()).to_string(),
            0x09 => r.u32()?.to_string(),
            0x0A => i64::from_le_bytes(r.u64()?.to_le_bytes()).to_string(),
            0x0B => r.u64()?.to_string(),
            0x0C => f32::from_le_bytes(r.u32()?.to_le_bytes()).to_string(),
            0x0D => f64::from_le_bytes(r.u64()?.to_le_bytes()).to_string(),
            0x0E | 0x50 => ser_string(r)?.unwrap_or_else(|| "null".to_owned()),
            0x51 => {
                let boxed = field_or_prop_type(r)?;
                elem_by_code(r, &boxed)?
            }
            _ => return malformed("custom attribute element type", r.position()),
        },
    })
}

/// A fixed argument typed by a constructor parameter.
fn fixed_arg(r: &mut Reader<'_>, param: &TypeSig) -> Read<String> {
    let kind = match param.unmodified() {
        TypeSig::Primitive(name) => ArgType::Code(match *name {
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
            "System.Single" => 0x0C,
            "System.Double" => 0x0D,
            "System.String" => 0x0E,
            "System.Object" => 0x51,
            _ => return malformed("custom attribute parameter type", r.position()),
        }),
        // A value type parameter is an enum (the only value types an attribute takes); its
        // underlying type is not known here, and every enum C# emits by default is 32-bit.
        TypeSig::Named {
            value_type: true, ..
        } => ArgType::Enum,
        // A class parameter is System.Type (the only class an attribute takes besides string
        // and object, which are primitives here).
        TypeSig::Named {
            value_type: false, ..
        } => ArgType::Code(0x50),
        TypeSig::SzArray(element) => {
            let count = r.u32()?;
            if count == u32::MAX {
                return Ok("null".to_owned());
            }
            let mut items = Vec::with_capacity(count.min(64) as usize);
            for _ in 0..count {
                items.push(fixed_arg(r, element)?);
            }
            return Ok(format!("[{}]", items.join(", ")));
        }
        _ => return malformed("custom attribute parameter type", r.position()),
    };
    elem_by_code(r, &kind)
}

impl From<ReadError> for crate::attribute::AttributeError {
    fn from(source: ReadError) -> Self {
        Self::Malformed {
            path: std::path::PathBuf::new(),
            source,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

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
        let decoded = decode_attribute(&value, &params);
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
            let decoded = decode_attribute(&value, &[TypeSig::Primitive(name)]).map(|d| d.0);
            assert_eq!(decoded, Ok(vec![expected.to_owned()]), "{name}");
        }
        let mut null_array = vec![1, 0];
        null_array.extend(u32::MAX.to_le_bytes());
        assert_eq!(
            decode_attribute(
                &null_array,
                &[TypeSig::SzArray(Box::new(TypeSig::Primitive(
                    "System.Int32"
                )))]
            )
            .map(|d| d.0),
            Ok(vec!["null".to_owned()])
        );
        assert_eq!(decode_attribute(&[], &[]), Ok((vec![], vec![])));
        assert!(decode_attribute(&[2, 0], &[]).is_err(), "wrong prolog");
        assert!(
            decode_attribute(&[1, 0], &[TypeSig::Primitive("System.Int32")]).is_err(),
            "value missing"
        );
        assert!(
            decode_attribute(&[1, 0, 0], &[TypeSig::Primitive("System.Void")]).is_err(),
            "no attribute takes a void"
        );
        assert!(decode_attribute(&[1, 0, 0], &[TypeSig::Var(0)]).is_err());
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
            let _ = decode_attribute(&value, &params);
        }
    }
}
