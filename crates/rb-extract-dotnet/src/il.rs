//! Method bodies: the IL instructions whose operands name another member or type.
//!
//! - Specification: ECMA-335 II.25.4 (method header formats), III (the instruction set and each
//!   opcode's operand)
//! - Plan: [Wave 2, Step 2](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#22-step-2-the-metadata-il-and-pdb-readers-2a)
//!   (`il.rs`: a body walker over the fat and tiny header formats yielding the operand tokens of
//!   `call`, `callvirt`, `newobj`, `ldfld`, `stfld`, `ldtoken`, `box`, `castclass`, `isinst`, and
//!   the `ldstr` operand before a literal reflection load)
//! - Decision: [ADR-0011](../../../docs/adr/0011-read-dotnet-assemblies-not-source.md) (`body`,
//!   `call`, `typeof` and `dynamic` edges)
//!
//! Every opcode's operand width is known, so the walker never loses its place; an unknown opcode
//! or an operand running past the body is an error naming the offset. Only the instructions whose
//! operand is a metadata token are yielded.

use crate::bytes::{Read, Reader, malformed};

/// What an instruction's token operand is used for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Use {
    /// `call`, `callvirt`, `jmp`, `ldftn`, `ldvirtftn`: a method is invoked or referenced.
    Call,
    /// `newobj`: a constructor is invoked.
    New,
    /// `ldfld`, `ldflda`, `stfld`, `ldsfld`, `ldsflda`, `stsfld`: a field is accessed.
    Field,
    /// `castclass`, `isinst`, `box`, `unbox`, `newarr`, `initobj`, `sizeof` and the other type
    /// operands.
    Type,
    /// `ldtoken`: a type, method or field handle is loaded (`typeof`).
    Token,
    /// `ldstr`: a string literal from the `#US` heap.
    String,
}

/// One instruction with a token operand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Instruction {
    /// Offset of the opcode from the start of the IL code.
    pub offset: u32,
    /// The opcode: one byte, or `0xFE00 | second` for a two-byte opcode.
    pub opcode: u16,
    /// What the token is used for.
    pub use_: Use,
    /// The metadata token: table in the top byte, row below.
    pub token: u32,
}

/// A parsed method body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Body {
    /// The `StandAloneSig` token of the local variable signature, or 0.
    pub locals: u32,
    /// The instructions with token operands, in order.
    pub instructions: Vec<Instruction>,
}

/// How many operand bytes follow a one-byte opcode, and what a token operand is used for.
/// `None` for an opcode the instruction set does not define. `switch` (0x45) is handled apart.
fn one_byte(opcode: u8) -> Option<(usize, Option<Use>)> {
    Some(match opcode {
        0x00..=0x0D
        | 0x14..=0x1E
        | 0x25
        | 0x26
        | 0x2A
        | 0x46..=0x6E
        | 0x76
        | 0x7A
        | 0x82..=0x8B
        | 0x8E
        | 0x90..=0xA2
        | 0xB3..=0xBA
        | 0xC3
        | 0xD1..=0xDC
        | 0xDF
        | 0xE0 => (0, None),
        0x0E..=0x13 | 0x1F | 0x2B..=0x37 | 0xDE => (1, None),
        // 0x29 is calli, whose operand is a stand-alone signature, not a member.
        0x20 | 0x22 | 0x29 | 0x38..=0x44 | 0xDD => (4, None),
        0x21 | 0x23 => (8, None),
        0x27 | 0x28 | 0x6F => (4, Some(Use::Call)),
        0x73 => (4, Some(Use::New)),
        0x72 => (4, Some(Use::String)),
        0x7B..=0x80 => (4, Some(Use::Field)),
        0x70
        | 0x71
        | 0x74
        | 0x75
        | 0x79
        | 0x81
        | 0x8C
        | 0x8D
        | 0x8F
        | 0xA3..=0xA5
        | 0xC2
        | 0xC6 => (4, Some(Use::Type)),
        0xD0 => (4, Some(Use::Token)),
        _ => return None,
    })
}

/// As [`one_byte`], for the second byte of a two-byte (`0xFE`-prefixed) opcode.
fn two_byte(opcode: u8) -> Option<(usize, Option<Use>)> {
    Some(match opcode {
        0x00..=0x05 | 0x0F | 0x11 | 0x13 | 0x14 | 0x17 | 0x18 | 0x1A | 0x1D | 0x1E => (0, None),
        0x12 | 0x19 => (1, None),
        0x09..=0x0E => (2, None),
        0x06 | 0x07 => (4, Some(Use::Call)),
        0x15 | 0x16 | 0x1C => (4, Some(Use::Type)),
        _ => return None,
    })
}

/// Parses the method body at the start of `data` (II.25.4.1 to II.25.4.3).
///
/// # Errors
/// When the header is not tiny or fat, the code runs past `data`, or an opcode is unknown.
pub fn parse_body(data: &[u8]) -> Read<Body> {
    let mut r = Reader::new(data, 0, "method body header");
    let first = r.u8()?;
    let (code_size, locals, code_start) = match first & 0x03 {
        0x02 => (usize::from(first >> 2), 0, 1),
        0x03 => {
            r.seek(0);
            let flags = r.u16()?;
            let header_size = usize::from(flags >> 12) * 4;
            r.skip(2)?; // max stack
            let code_size = r.u32()? as usize;
            let locals = r.u32()?;
            if header_size < 12 {
                return malformed("fat method header", 0);
            }
            (code_size, locals, header_size)
        }
        _ => return malformed("method body header", 0),
    };
    let Some(code) = code_start
        .checked_add(code_size)
        .and_then(|end| data.get(code_start..end))
    else {
        return malformed("method body (code exceeds the image)", code_start);
    };
    Ok(Body {
        locals,
        instructions: walk(code)?,
    })
}

/// The instructions with token operands in a stretch of IL code.
///
/// # Errors
/// When an opcode is unknown or an operand runs past the code.
pub fn walk(code: &[u8]) -> Read<Vec<Instruction>> {
    let mut r = Reader::new(code, 0, "IL code");
    let mut found = Vec::new();
    while r.position() < code.len() {
        let offset = r.position();
        let opcode = r.u8()?;
        let mut code = u16::from(opcode);
        let decoded = match opcode {
            0xFE => {
                let second = r.u8()?;
                code = 0xFE00 | u16::from(second);
                two_byte(second)
            }
            0x45 => {
                let targets = r.u32()? as usize;
                let Some(length) = targets.checked_mul(4) else {
                    return malformed("IL switch", offset);
                };
                r.skip(length)?;
                continue;
            }
            other => one_byte(other),
        };
        let Some((width, use_)) = decoded else {
            return malformed("IL opcode", offset);
        };
        match (use_, width) {
            (Some(use_), 4) => found.push(Instruction {
                offset: u32::try_from(offset).unwrap_or(u32::MAX),
                opcode: code,
                use_,
                token: r.u32()?,
            }),
            _ => r.skip(width)?,
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn token(table: u8, row: u32) -> [u8; 4] {
        ((u32::from(table) << 24) | row).to_le_bytes()
    }

    #[test]
    fn a_tiny_body_yields_its_token_operands_in_order() {
        // ldarg.0; ldfld 0x04000002; call 0x0A000005; ldc.i4.s 7; ret
        let mut code = vec![0x02, 0x7B];
        code.extend(token(0x04, 2));
        code.push(0x28);
        code.extend(token(0x0A, 5));
        code.extend([0x1F, 7, 0x2A]);
        let mut body = vec![u8::try_from(code.len() << 2).unwrap_or(0) | 0x02];
        body.extend(&code);
        let parsed = parse_body(&body);
        assert_eq!(
            parsed.map(|b| (b.locals, b.instructions)),
            Ok((
                0,
                vec![
                    Instruction {
                        offset: 1,
                        opcode: 0x7B,
                        use_: Use::Field,
                        token: 0x0400_0002
                    },
                    Instruction {
                        offset: 6,
                        opcode: 0x28,
                        use_: Use::Call,
                        token: 0x0A00_0005
                    },
                ]
            ))
        );
    }

    #[test]
    fn a_fat_body_skips_switches_and_two_byte_opcodes() {
        // newobj; switch(2 targets); ldstr; ldtoken; constrained. T; callvirt; initobj T; ldloc 1; ret
        let mut code = vec![0x73];
        code.extend(token(0x0A, 1));
        code.push(0x45);
        code.extend(2u32.to_le_bytes());
        code.extend([0; 8]);
        code.push(0x72);
        code.extend(token(0x70, 9));
        code.push(0xD0);
        code.extend(token(0x02, 3));
        code.extend([0xFE, 0x16]);
        code.extend(token(0x1B, 1));
        code.push(0x6F);
        code.extend(token(0x06, 4));
        code.extend([0xFE, 0x15]);
        code.extend(token(0x01, 8));
        code.extend([0xFE, 0x0C, 1, 0, 0x2A]);
        let mut body = Vec::new();
        body.extend((0x3003u16).to_le_bytes()); // fat, header 3 dwords
        body.extend(8u16.to_le_bytes());
        body.extend(u32::try_from(code.len()).unwrap_or(0).to_le_bytes());
        body.extend(0x1100_0002u32.to_le_bytes());
        body.extend(&code);
        let parsed = parse_body(&body);
        let uses: Vec<Use> = parsed
            .as_ref()
            .map(|b| b.instructions.iter().map(|i| i.use_).collect())
            .unwrap_or_default();
        assert_eq!(
            uses,
            [
                Use::New,
                Use::String,
                Use::Token,
                Use::Type,
                Use::Call,
                Use::Type
            ]
        );
        let opcodes: Vec<u16> = parsed
            .as_ref()
            .map(|b| b.instructions.iter().map(|i| i.opcode).collect())
            .unwrap_or_default();
        assert_eq!(opcodes, [0x73, 0x72, 0xD0, 0xFE16, 0x6F, 0xFE15]);
        assert_eq!(parsed.map(|b| b.locals), Ok(0x1100_0002));
    }

    #[test]
    fn every_defined_opcode_has_a_width_and_undefined_ones_do_not() {
        for opcode in 0..=0xFFu8 {
            let defined = one_byte(opcode).is_some();
            let expected = !matches!(
                opcode,
                0x24 | 0x45 | 0x77 | 0x78 | 0xA6..=0xB2 | 0xBB..=0xC1 | 0xC4 | 0xC5 | 0xC7..=0xCF
                    | 0xE1..=0xFF
            );
            assert_eq!(defined, expected, "one-byte {opcode:#04x}");
        }
        for opcode in 0..=0xFFu8 {
            let defined = two_byte(opcode).is_some();
            let expected = matches!(opcode, 0x00..=0x07 | 0x09..=0x0F | 0x11..=0x1A | 0x1C..=0x1E);
            assert_eq!(defined, expected, "two-byte {opcode:#04x}");
        }
    }

    #[test]
    fn malformed_bodies_are_errors() {
        assert!(parse_body(&[]).is_err());
        assert!(parse_body(&[0x01]).is_err(), "neither tiny nor fat");
        assert!(
            parse_body(&[0x0A, 0x00]).is_err(),
            "code runs past the data"
        );
        assert!(parse_body(&[0x06, 0x24]).is_err(), "0x24 is undefined");
        assert!(
            parse_body(&[0x0A, 0x28, 0x00]).is_err(),
            "operand cut short"
        );
        assert!(
            walk(&[0x45, 0xFF, 0xFF, 0xFF, 0xFF]).is_err(),
            "switch past the end"
        );
        let mut fat = vec![0x03, 0x10, 0, 0];
        fat.extend(0u32.to_le_bytes());
        fat.extend(0u32.to_le_bytes());
        assert!(
            parse_body(&fat).is_err(),
            "a fat header smaller than 12 bytes"
        );
    }

    proptest! {
        #[test]
        fn any_code_walks_or_fails_cleanly(code in proptest::collection::vec(any::<u8>(), 0..128)) {
            if let Ok(found) = walk(&code) {
                for pair in found.windows(2) {
                    prop_assert!(pair[0].offset < pair[1].offset);
                }
            }
            let _ = parse_body(&code);
        }
    }
}
