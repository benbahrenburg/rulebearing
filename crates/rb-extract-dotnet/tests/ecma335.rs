//! The metadata and IL readers against what System.Reflection.Metadata reads from the same bytes.
//!
//! - Plan: [Wave 2, Step 2](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#22-step-2-the-metadata-il-and-pdb-readers-2a)
//!   ("`tests/ecma335.rs` reads the committed `TestAssembly.dll` and asserts row counts, a
//!   sample of decoded signatures and the IL operand stream of three known methods")
//! - Expected values: printed once by `conformance/archunitnet/tools/MetadataDump`
//!   (`dotnet run -c Release` with `../../fixtures/TestAssembly.dll` and the three method names)
//! - Requirement: [NFR-SEC-01](../../../docs/prd.md#nfr-sec-01) (malformed input is an error)

use std::path::{Path, PathBuf};

use rb_extract_dotnet::loader::Loaded;
use rb_extract_dotnet::sig::{TypeSig, method_sig};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../conformance/archunitnet/fixtures")
        .join(name)
}

fn load() -> Result<Loaded, Box<dyn std::error::Error>> {
    Ok(Loaded::read(&std::fs::read(fixture("TestAssembly.dll"))?)?)
}

#[test]
fn row_counts_agree_with_system_reflection_metadata() -> Result<(), Box<dyn std::error::Error>> {
    let loaded = load()?;
    assert_eq!(loaded.identity.name, "TestAssembly");
    assert_eq!(loaded.identity.version, [1, 0, 0, 0]);
    assert_eq!(loaded.type_refs.len(), 33);
    assert_eq!(loaded.types.len(), 46);
    assert_eq!(
        loaded.types.iter().map(|t| t.fields.len()).sum::<usize>(),
        26
    );
    assert_eq!(
        loaded.types.iter().map(|t| t.methods.len()).sum::<usize>(),
        95
    );
    assert_eq!(loaded.member_refs.len(), 39);
    assert_eq!(
        loaded
            .types
            .iter()
            .map(|t| t.properties.len())
            .sum::<usize>(),
        11
    );
    assert_eq!(loaded.type_specs.len(), 6);
    assert_eq!(loaded.assembly_refs.len(), 3);
    assert_eq!(
        loaded
            .types
            .iter()
            .map(|t| t.interfaces.len())
            .sum::<usize>(),
        9
    );
    assert!(
        loaded.types.iter().all(|t| t.enclosing.is_none()),
        "no NestedClass rows"
    );
    Ok(())
}

/// A method's token operands as `(offset, opcode, token)`.
type Operands = &'static [(u32, u16, u32)];

/// `(name, MethodDef row, signature blob, operands)` as `MetadataDump` printed them.
const METHODS: &[(&str, u32, &str, Operands)] = &[
    (
        "TestAssembly.Class1::.ctor",
        20,
        "2001010E",
        &[(0x1, 0x28, 0x0A00_0011), (0x8, 0x7D, 0x0400_0004)],
    ),
    (
        "TestAssembly.ClassCallingOtherMethod::CallingOther",
        27,
        "2001011210",
        &[
            (0x1, 0x6F, 0x0A00_0016),
            (0x6, 0x28, 0x0A00_0020),
            (0xD, 0x6F, 0x0600_0016),
        ],
    ),
    (
        "TestAssembly.PlantUml.Catalog.ProductCatalog::GonnaDoSomethingIllegalWithOrder",
        71,
        "200001",
        &[
            (0x0, 0x73, 0x0600_003F),
            (0x7, 0x7B, 0x0400_0013),
            (0xC, 0x6F, 0x0A00_0023),
            (0x16, 0x28, 0x0A00_0024),
            (0x1B, 0x6F, 0x0600_0039),
            (0x22, 0x28, 0x0A00_0025),
            (0x2D, 0xFE16, 0x1B00_0006),
            (0x33, 0x6F, 0x0A00_0026),
            (0x3B, 0x7B, 0x0400_0013),
            (0x40, 0x6F, 0x0600_003C),
        ],
    ),
];

fn hex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .filter_map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok())
        .collect()
}

#[test]
fn three_methods_decode_as_system_reflection_metadata_reads_them()
-> Result<(), Box<dyn std::error::Error>> {
    let loaded = load()?;
    for (name, row, signature, operands) in METHODS {
        let (owner, method) = loaded
            .method_at(*row)
            .ok_or(format!("{name}: no row {row}"))?;
        let expected_name = name.rsplit("::").next().unwrap_or_default();
        assert_eq!(format!("{}::{}", owner.full_name, method.name), *name);
        assert_eq!(method.name, expected_name);
        assert_eq!(method.sig, method_sig(&hex(signature))?, "{name}");
        let found: Vec<(u32, u16, u32)> = method
            .body
            .as_ref()
            .map(|b| {
                b.instructions
                    .iter()
                    .map(|i| (i.offset, i.opcode, i.token))
                    .collect()
            })
            .unwrap_or_default();
        assert_eq!(found, *operands, "{name}");
    }
    // CallingOther(Class1): instance, void, one class parameter.
    let (_, calling) = loaded.method_at(27).ok_or("row 27")?;
    assert!(calling.sig.has_this);
    assert_eq!(calling.sig.ret, TypeSig::Primitive("System.Void"));
    assert_eq!(calling.parameters, ["cls"]);
    Ok(())
}

#[test]
fn truncated_and_corrupted_assemblies_are_errors_not_panics()
-> Result<(), Box<dyn std::error::Error>> {
    let bytes = std::fs::read(fixture("TestAssembly.dll"))?;
    for length in [
        0,
        64,
        512,
        bytes.len() / 4,
        bytes.len() / 2,
        bytes.len() - 1,
    ] {
        let _ = Loaded::read(&bytes[..length]);
    }
    assert!(Loaded::read(&bytes[..bytes.len() / 2]).is_err());
    // Point every #Strings index past the heap by corrupting the metadata root's heap sizes:
    // flip bytes across the file and require an error or a clean read, never a panic.
    for step in (0..bytes.len()).step_by(97) {
        let mut corrupted = bytes.clone();
        corrupted[step] ^= 0xFF;
        let _ = Loaded::read(&corrupted);
    }
    Ok(())
}
