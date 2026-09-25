//! Signature blobs: the types of fields, methods, properties, locals and generic instantiations.
//!
//! - Specification: ECMA-335 II.23.2 (blobs and signatures), II.23.1.16 (element types)
//! - Plan: [Wave 2, Step 2](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#22-step-2-the-metadata-il-and-pdb-readers-2a)
//!   (`sig.rs`: field, method, property and `TypeSpec` blobs yielding type chains with generic
//!   arguments)
//! - Decision: [ADR-0011](../../../docs/adr/0011-read-dotnet-assemblies-not-source.md) (the
//!   `signature`, `field` and `generic-argument` edges come from these)
//!
//! A signature names types by `TypeDefOrRef` coded index; [`TypeSig`] keeps the decoded
//! `(table, row)` so the caller resolves names against the assembly it came from. Nesting is
//! bounded by [`MAX_DEPTH`], so a crafted blob that nests without end is an error, not a stack
//! overflow.

use crate::bytes::{Read, Reader, malformed};
use crate::metadata::tables::{Coded, TableId};

/// The deepest type nesting a signature may have before it is rejected as malformed.
pub const MAX_DEPTH: u32 = 64;

/// The largest array rank a signature may declare (ECMA-335 II.14.1 and the runtime's limit).
pub const MAX_RANK: u32 = 32;

/// A `TypeDefOrRef` or `TypeSpec` row a signature names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Token {
    /// The table: `TypeDef`, `TypeRef` or `TypeSpec`.
    pub table: TableId,
    /// The 1-based row.
    pub row: u32,
}

/// One type in a signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeSig {
    /// A built-in type, by its `System` name (`System.Int32`, `System.Void`).
    Primitive(&'static str),
    /// A class or value type named by token.
    Named {
        /// The type's row.
        token: Token,
        /// `VALUETYPE` rather than `CLASS`.
        value_type: bool,
    },
    /// A generic instantiation: `List<int>`.
    GenericInst {
        /// The generic type definition.
        base: Box<TypeSig>,
        /// The type arguments, in order.
        args: Vec<TypeSig>,
    },
    /// A single-dimensional, zero-based array: `T[]`.
    SzArray(Box<TypeSig>),
    /// A general array: `T[,]`.
    Array {
        /// The element type.
        element: Box<TypeSig>,
        /// The rank.
        rank: u32,
    },
    /// An unmanaged pointer: `T*`.
    Ptr(Box<TypeSig>),
    /// A managed reference: `ref T`.
    ByRef(Box<TypeSig>),
    /// A type's generic parameter by position: `!0`.
    Var(u32),
    /// A method's generic parameter by position: `!!0`.
    MVar(u32),
    /// A function pointer.
    FnPtr(Box<MethodSig>),
    /// A custom modifier around a type (`modreq(IsExternalInit)`, `modopt(IsConst)`).
    Modified {
        /// `modreq` rather than `modopt`.
        required: bool,
        /// The modifier type.
        modifier: Token,
        /// The modified type.
        inner: Box<TypeSig>,
    },
    /// A pinned local.
    Pinned(Box<TypeSig>),
}

impl TypeSig {
    /// Every type token the signature names, outermost first, with whether it is a generic
    /// argument (for the `generic-argument` edge kind).
    pub fn tokens(&self) -> Vec<(Token, bool)> {
        let mut out = Vec::new();
        self.collect(false, &mut out);
        out
    }

    fn collect(&self, argument: bool, out: &mut Vec<(Token, bool)>) {
        match self {
            Self::Named { token, .. } => out.push((*token, argument)),
            Self::GenericInst { base, args } => {
                base.collect(argument, out);
                for arg in args {
                    arg.collect(true, out);
                }
            }
            Self::SzArray(inner)
            | Self::Ptr(inner)
            | Self::ByRef(inner)
            | Self::Pinned(inner)
            | Self::Array { element: inner, .. }
            | Self::Modified { inner, .. } => inner.collect(argument, out),
            Self::FnPtr(method) => {
                method.ret.collect(argument, out);
                for param in &method.params {
                    param.collect(argument, out);
                }
            }
            Self::Primitive(_) | Self::Var(_) | Self::MVar(_) => {}
        }
    }

    /// Every token the signature names, custom modifiers included, in no particular order.
    pub fn all_tokens(&self) -> Vec<Token> {
        let mut out = Vec::new();
        self.collect_all(&mut out);
        out
    }

    fn collect_all(&self, out: &mut Vec<Token>) {
        match self {
            Self::Named { token, .. } => out.push(*token),
            Self::GenericInst { base, args } => {
                base.collect_all(out);
                for arg in args {
                    arg.collect_all(out);
                }
            }
            Self::Modified {
                modifier, inner, ..
            } => {
                out.push(*modifier);
                inner.collect_all(out);
            }
            Self::SzArray(inner)
            | Self::Ptr(inner)
            | Self::ByRef(inner)
            | Self::Pinned(inner)
            | Self::Array { element: inner, .. } => inner.collect_all(out),
            Self::FnPtr(method) => {
                method.ret.collect_all(out);
                for param in &method.params {
                    param.collect_all(out);
                }
            }
            Self::Primitive(_) | Self::Var(_) | Self::MVar(_) => {}
        }
    }

    /// Whether the outermost modifier chain carries a `modreq` naming `token`.
    pub fn has_modreq(&self, test: &dyn Fn(Token) -> bool) -> bool {
        match self {
            Self::Modified {
                required,
                modifier,
                inner,
            } => (*required && test(*modifier)) || inner.has_modreq(test),
            _ => false,
        }
    }

    /// The type with its custom modifiers and pinning removed.
    pub fn unmodified(&self) -> &Self {
        match self {
            Self::Modified { inner, .. } | Self::Pinned(inner) => inner.unmodified(),
            other => other,
        }
    }
}

/// A method, property or function-pointer signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodSig {
    /// An instance method (`HASTHIS`).
    pub has_this: bool,
    /// The number of the method's own generic parameters.
    pub generic_params: u32,
    /// The return type (the property type for a property signature).
    pub ret: TypeSig,
    /// The parameter types, in order.
    pub params: Vec<TypeSig>,
}

const HASTHIS: u8 = 0x20;
const GENERIC: u8 = 0x10;
const FIELD: u8 = 0x06;
const LOCAL_SIG: u8 = 0x07;
const PROPERTY: u8 = 0x08;
const GENERIC_INST: u8 = 0x0A;

/// The `System` name of a primitive element type.
fn primitive(element: u8) -> Option<&'static str> {
    Some(match element {
        0x01 => "System.Void",
        0x02 => "System.Boolean",
        0x03 => "System.Char",
        0x04 => "System.SByte",
        0x05 => "System.Byte",
        0x06 => "System.Int16",
        0x07 => "System.UInt16",
        0x08 => "System.Int32",
        0x09 => "System.UInt32",
        0x0A => "System.Int64",
        0x0B => "System.UInt64",
        0x0C => "System.Single",
        0x0D => "System.Double",
        0x0E => "System.String",
        0x16 => "System.TypedReference",
        0x18 => "System.IntPtr",
        0x19 => "System.UIntPtr",
        0x1C => "System.Object",
        _ => return None,
    })
}

struct Decoder<'a> {
    r: Reader<'a>,
}

impl Decoder<'_> {
    fn token(&mut self) -> Read<Token> {
        let at = self.r.position();
        let coded = self.r.compressed_u32()?;
        match Coded::TypeDefOrRef.decode(coded) {
            Some((table, row)) if row > 0 => Ok(Token { table, row }),
            _ => malformed("signature type token", at),
        }
    }

    fn ty(&mut self, depth: u32) -> Read<TypeSig> {
        let at = self.r.position();
        if depth > MAX_DEPTH {
            return malformed("signature (nested too deeply)", at);
        }
        let element = self.r.u8()?;
        if let Some(name) = primitive(element) {
            return Ok(TypeSig::Primitive(name));
        }
        let next = depth + 1;
        Ok(match element {
            0x0F => TypeSig::Ptr(Box::new(self.ty(next)?)),
            0x10 => TypeSig::ByRef(Box::new(self.ty(next)?)),
            0x11 | 0x12 => TypeSig::Named {
                token: self.token()?,
                value_type: element == 0x11,
            },
            0x13 => TypeSig::Var(self.r.compressed_u32()?),
            0x1E => TypeSig::MVar(self.r.compressed_u32()?),
            0x14 => {
                let element = self.ty(next)?;
                let rank_at = self.r.position();
                let rank = self.r.compressed_u32()?;
                if rank > MAX_RANK {
                    return malformed("signature array rank", rank_at);
                }
                let sizes = self.r.compressed_u32()?;
                for _ in 0..sizes {
                    self.r.compressed_u32()?;
                }
                let bounds = self.r.compressed_u32()?;
                for _ in 0..bounds {
                    self.r.compressed_i32()?;
                }
                TypeSig::Array {
                    element: Box::new(element),
                    rank,
                }
            }
            0x15 => {
                let base = self.ty(next)?;
                let count = self.r.compressed_u32()?;
                let mut args = Vec::with_capacity(count.min(16) as usize);
                for _ in 0..count {
                    args.push(self.ty(next)?);
                }
                TypeSig::GenericInst {
                    base: Box::new(base),
                    args,
                }
            }
            0x1B => TypeSig::FnPtr(Box::new(self.method(next)?)),
            0x1D => TypeSig::SzArray(Box::new(self.ty(next)?)),
            0x1F | 0x20 => {
                let modifier = self.token()?;
                TypeSig::Modified {
                    required: element == 0x1F,
                    modifier,
                    inner: Box::new(self.ty(next)?),
                }
            }
            0x45 => TypeSig::Pinned(Box::new(self.ty(next)?)),
            _ => return malformed("signature element type", at),
        })
    }

    /// A parameter or return type: custom modifiers, then `BYREF`/`TYPEDBYREF`/`VOID`/a type,
    /// all of which [`Self::ty`] reads. A vararg `SENTINEL` (0x41) before a parameter is skipped.
    fn param(&mut self, depth: u32) -> Read<TypeSig> {
        let at = self.r.position();
        if self.r.u8()? != 0x41 {
            self.r.seek(at);
        }
        self.ty(depth)
    }

    fn method(&mut self, depth: u32) -> Read<MethodSig> {
        let flags = self.r.u8()?;
        let generic_params = if flags & GENERIC != 0 {
            self.r.compressed_u32()?
        } else {
            0
        };
        let count = self.r.compressed_u32()?;
        let ret = self.param(depth)?;
        let mut params = Vec::with_capacity(count.min(32) as usize);
        for _ in 0..count {
            params.push(self.param(depth)?);
        }
        Ok(MethodSig {
            has_this: flags & HASTHIS != 0,
            generic_params,
            ret,
            params,
        })
    }
}

fn decoder<'a>(blob: &'a [u8], what: &'static str) -> Decoder<'a> {
    Decoder {
        r: Reader::new(blob, 0, what),
    }
}

/// A `MethodDef` or `MemberRef` method signature (II.23.2.1, II.23.2.2).
///
/// # Errors
/// When the blob is truncated or names an unknown element type.
pub fn method_sig(blob: &[u8]) -> Read<MethodSig> {
    decoder(blob, "method signature").method(0)
}

/// A field signature (II.23.2.4): its type.
///
/// # Errors
/// When the blob does not start with `FIELD` or is malformed.
pub fn field_sig(blob: &[u8]) -> Read<TypeSig> {
    let mut d = decoder(blob, "field signature");
    if d.r.u8()? & 0x0F != FIELD {
        return malformed("field signature", 0);
    }
    d.param(0)
}

/// A property signature (II.23.2.5): the property type as `ret`, indexer parameters as `params`.
///
/// # Errors
/// When the blob does not start with `PROPERTY` or is malformed.
pub fn property_sig(blob: &[u8]) -> Read<MethodSig> {
    let mut d = decoder(blob, "property signature");
    let flags = d.r.u8()?;
    if flags & 0x0F != PROPERTY {
        return malformed("property signature", 0);
    }
    let count = d.r.compressed_u32()?;
    let ret = d.param(0)?;
    let mut params = Vec::with_capacity(count.min(32) as usize);
    for _ in 0..count {
        params.push(d.param(0)?);
    }
    Ok(MethodSig {
        has_this: flags & HASTHIS != 0,
        generic_params: 0,
        ret,
        params,
    })
}

/// A `TypeSpec` blob (II.23.2.14): one type.
///
/// # Errors
/// When the blob is malformed.
pub fn type_spec(blob: &[u8]) -> Read<TypeSig> {
    decoder(blob, "type spec").ty(0)
}

/// A `StandAloneSig` local-variables blob (II.23.2.6): the local types.
///
/// # Errors
/// When the blob does not start with `LOCAL_SIG` or is malformed.
pub fn local_var_sig(blob: &[u8]) -> Read<Vec<TypeSig>> {
    let mut d = decoder(blob, "local variable signature");
    if d.r.u8()? != LOCAL_SIG {
        return malformed("local variable signature", 0);
    }
    let count = d.r.compressed_u32()?;
    let mut locals = Vec::with_capacity(count.min(64) as usize);
    for _ in 0..count {
        locals.push(d.param(0)?);
    }
    Ok(locals)
}

/// A `MethodSpec` instantiation blob (II.23.2.15): the method's type arguments.
///
/// # Errors
/// When the blob does not start with `GENERICINST` or is malformed.
pub fn method_spec(blob: &[u8]) -> Read<Vec<TypeSig>> {
    let mut d = decoder(blob, "method spec");
    if d.r.u8()? != GENERIC_INST {
        return malformed("method spec", 0);
    }
    let count = d.r.compressed_u32()?;
    let mut args = Vec::with_capacity(count.min(16) as usize);
    for _ in 0..count {
        args.push(d.ty(0)?);
    }
    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::tables::id;
    use proptest::prelude::*;

    /// `TypeDefOrRef` coded token for a `TypeRef` row.
    fn type_ref(row: u8) -> u8 {
        (row << 2) | 1
    }

    #[test]
    fn decodes_a_generic_instance_method() {
        // instance, generic 1, 2 params; returns List<string>; (int, ref T[]).
        let blob = [
            HASTHIS | GENERIC,
            1,
            2,
            0x15,
            0x12,
            type_ref(3),
            1,
            0x0E,
            0x08,
            0x10,
            0x1D,
            0x1E,
            0,
        ];
        let sig = method_sig(&blob);
        let list = Token {
            table: id::TYPE_REF,
            row: 3,
        };
        assert_eq!(
            sig,
            Ok(MethodSig {
                has_this: true,
                generic_params: 1,
                ret: TypeSig::GenericInst {
                    base: Box::new(TypeSig::Named {
                        token: list,
                        value_type: false
                    }),
                    args: vec![TypeSig::Primitive("System.String")],
                },
                params: vec![
                    TypeSig::Primitive("System.Int32"),
                    TypeSig::ByRef(Box::new(TypeSig::SzArray(Box::new(TypeSig::MVar(0))))),
                ],
            })
        );
        assert_eq!(
            sig.map(|s| s.ret.tokens()),
            Ok(vec![(list, false)]),
            "a primitive argument names no token"
        );
    }

    #[test]
    fn generic_arguments_are_marked_as_such() {
        // Dictionary<Order, List<Line>> as a TypeSpec.
        let blob = [
            0x15,
            0x12,
            type_ref(1),
            2,
            0x12,
            type_ref(2),
            0x15,
            0x12,
            type_ref(3),
            1,
            0x11,
            type_ref(4),
        ];
        let tokens = type_spec(&blob).map(|t| t.tokens());
        let t = |row| Token {
            table: id::TYPE_REF,
            row,
        };
        assert_eq!(
            tokens,
            Ok(vec![
                (t(1), false),
                (t(2), true),
                (t(3), true),
                (t(4), true)
            ])
        );
    }

    #[test]
    fn fields_properties_locals_and_method_specs() {
        // A field of type modreq(IsVolatile) int.
        let field = [FIELD, 0x1F, type_ref(5), 0x08];
        let decoded = field_sig(&field);
        assert!(
            decoded
                .as_ref()
                .is_ok_and(|t| t.has_modreq(&|m| m.row == 5))
        );
        assert_eq!(
            decoded.as_ref().map(TypeSig::unmodified),
            Ok(&TypeSig::Primitive("System.Int32"))
        );
        assert!(field_sig(&[PROPERTY, 0x08]).is_err());
        // An instance property of type string with one int index parameter.
        let property = [PROPERTY | HASTHIS, 1, 0x0E, 0x08];
        assert_eq!(
            property_sig(&property).map(|p| (p.has_this, p.ret, p.params.len())),
            Ok((true, TypeSig::Primitive("System.String"), 1))
        );
        assert!(property_sig(&[FIELD, 0, 0x08]).is_err());
        // Two locals: a pinned byte pointer and an int[,] with bounds.
        let locals = [LOCAL_SIG, 2, 0x45, 0x0F, 0x05, 0x14, 0x08, 2, 1, 3, 1, 0];
        assert_eq!(
            local_var_sig(&locals).map(|l| l.len()),
            Ok(2),
            "{:?}",
            local_var_sig(&locals)
        );
        assert!(local_var_sig(&[FIELD]).is_err());
        assert_eq!(
            method_spec(&[GENERIC_INST, 1, 0x1C]),
            Ok(vec![TypeSig::Primitive("System.Object")])
        );
        assert!(method_spec(&[0, 1, 0x1C]).is_err());
    }

    #[test]
    fn function_pointers_varargs_and_value_types() {
        // static method returning void with a sentinel before an int parameter, taking a
        // function pointer that returns a value type.
        let blob = [0x05, 2, 0x01, 0x41, 0x08, 0x1B, 0, 0, 0x11, type_ref(7)];
        let sig = method_sig(&blob);
        assert_eq!(sig.as_ref().map(|s| s.has_this), Ok(false));
        let tokens: Vec<u32> = sig
            .map(|s| {
                s.params
                    .iter()
                    .flat_map(TypeSig::tokens)
                    .map(|t| t.0.row)
                    .collect()
            })
            .unwrap_or_default();
        assert_eq!(tokens, [7]);
    }

    #[test]
    fn malformed_blobs_are_errors_not_panics() {
        assert!(method_sig(&[]).is_err());
        assert!(method_sig(&[0, 1]).is_err(), "a parameter is missing");
        assert!(type_spec(&[0xFF]).is_err(), "unknown element type");
        assert!(type_spec(&[0x12, 0x00]).is_err(), "row 0 is no type");
        assert!(type_spec(&[0x12, 0x03]).is_err(), "tag 3 is unused");
        // Nesting beyond the limit: pointers all the way down.
        let deep = vec![0x0F; (MAX_DEPTH as usize) + 8];
        assert!(type_spec(&deep).is_err());
        let fine: Vec<u8> = std::iter::repeat_n(0x0F, 10).chain([0x08]).collect();
        assert!(type_spec(&fine).is_ok());
    }

    #[test]
    fn an_array_rank_above_the_limit_is_malformed() {
        // int32[<rank>] with no sizes and no bounds; rank 0x1FFF_FFFF would spell a 512 MB name.
        assert_eq!(
            type_spec(&[0x14, 0x08, 0xDF, 0xFF, 0xFF, 0xFF, 0, 0]).map_err(|e| e.what),
            Err("signature array rank")
        );
        assert_eq!(
            type_spec(&[0x14, 0x08, 33, 0, 0]).map_err(|e| e.what),
            Err("signature array rank")
        );
        assert!(matches!(
            type_spec(&[0x14, 0x08, 32, 0, 0]),
            Ok(TypeSig::Array { rank: 32, .. })
        ));
    }

    #[test]
    fn all_tokens_include_custom_modifiers() {
        // modopt(TypeRef 2) CLASS TypeSpec 1
        let tokens = type_spec(&[0x20, type_ref(2), 0x12, (1 << 2) | 2]).map(|t| t.all_tokens());
        assert_eq!(
            tokens,
            Ok(vec![
                Token {
                    table: id::TYPE_REF,
                    row: 2
                },
                Token {
                    table: id::TYPE_SPEC,
                    row: 1
                }
            ])
        );
    }

    proptest! {
        #[test]
        fn any_blob_decodes_or_fails_cleanly(blob in proptest::collection::vec(any::<u8>(), 0..64)) {
            let _ = method_sig(&blob);
            let _ = field_sig(&blob);
            let _ = property_sig(&blob);
            let _ = type_spec(&blob);
            let _ = local_var_sig(&blob);
            let _ = method_spec(&blob);
        }
    }
}
