//! The reader's first end-to-end test: ArchUnitNET's `TestAssembly`, committed as a fixture.
//!
//! - Plan: [Wave 0, Step 6](../../../docs/plans/pending/0000-wave-0-spike.md#step-6-conformance-gate-2-skeleton-0b)
//!   item 4 and [Step 9](../../../docs/plans/pending/0000-wave-0-spike.md#step-9-spike-b-rb-extract-dotnet-0d)
//! - Decision: [ADR-0003](../../../docs/adr/0003-dotnet-extractor-fallback.md) (the figure),
//!   [ADR-0009](../../../docs/adr/0009-conformance-suites-as-specification.md) (the fixture)
//! - Fixture: `conformance/archunitnet/fixtures/TestAssembly.{dll,pdb}` (0.13.4, portable PDB)
//!
//! `TestAssembly` is a plain SDK project, so every type the compiler did not generate must be
//! attributed by the PDB: anything below 100% is a bug in the reader.

use std::path::{Path, PathBuf};

use rb_extract_dotnet::assembly::Assembly;
use rb_extract_dotnet::attribute::{PdbKind, attribute_assembly};
use rb_extract_dotnet::pdb::PortablePdb;
use rb_model::Attribution;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../conformance/archunitnet/fixtures")
        .join(name)
}

#[test]
fn test_assembly_parses_and_its_pdb_is_portable() -> Result<(), Box<dyn std::error::Error>> {
    let bytes = std::fs::read(fixture("TestAssembly.dll"))?;
    let assembly = Assembly::read(&bytes)?;
    assert_eq!(assembly.name, "TestAssembly");
    assert_eq!(
        assembly.target_framework.as_deref(),
        Some(".NETCoreApp,Version=v10.0")
    );
    assert!(assembly.types.len() > 40, "{} types", assembly.types.len());
    assert!(assembly.types[0].is_module_type);
    let pdb = std::fs::read(fixture("TestAssembly.pdb"))?;
    let pdb = PortablePdb::parse(&pdb).map_err(|e| format!("{e:?}"))?;
    let documents = pdb.documents()?;
    assert!(
        documents.iter().all(|d| d.starts_with("/_/TestAssembly/")),
        "{documents:?}"
    );
    Ok(())
}

#[test]
fn every_type_of_test_assembly_is_attributed_by_the_pdb() -> Result<(), Box<dyn std::error::Error>>
{
    let repository = fixture("");
    let result = attribute_assembly(&fixture("TestAssembly.dll"), &repository, &repository)?;
    let counts = result.counts;
    println!(
        "test-assembly: types={} excluded={} pdb={} inferred={} none={} raw={:.4} adjusted={:.4}",
        counts.types_total,
        counts.types_excluded,
        counts.pdb_attributed,
        counts.inferred,
        counts.none,
        counts.raw(),
        counts.adjusted()
    );
    assert_eq!(result.pdb, PdbKind::Portable);
    let unattributed: Vec<&str> = result
        .types
        .iter()
        .filter(|t| t.attribution == Some(Attribution::None))
        .map(|t| t.full_name.as_str())
        .collect();
    assert!(unattributed.is_empty(), "not attributed: {unattributed:?}");
    assert_eq!(counts.inferred, 0, "a plain SDK project needs no inference");
    assert!((counts.adjusted() - 1.0).abs() < f64::EPSILON);
    for ty in result
        .types
        .iter()
        .filter(|t| t.attribution == Some(Attribution::Pdb))
    {
        let file = ty.file.as_deref().unwrap_or_default();
        assert!(
            file.starts_with("TestAssembly/")
                && Path::new(file).extension().is_some_and(|e| e == "cs"),
            "{}: {file}",
            ty.full_name
        );
    }
    Ok(())
}

#[test]
fn truncated_assemblies_are_errors_never_panics() -> Result<(), Box<dyn std::error::Error>> {
    let bytes = std::fs::read(fixture("TestAssembly.dll"))?;
    let step = (bytes.len() / 512).max(1);
    for length in (0..bytes.len()).step_by(step) {
        let _ = Assembly::read(&bytes[..length]);
    }
    let pdb = std::fs::read(fixture("TestAssembly.pdb"))?;
    for length in (0..pdb.len()).step_by((pdb.len() / 512).max(1)) {
        if let Ok(parsed) = PortablePdb::parse(&pdb[..length]) {
            let _ = parsed.documents();
            let _ = parsed.type_definition_documents();
            for method in 0..64 {
                let _ = parsed.first_point(method);
            }
        }
    }
    Ok(())
}

#[test]
fn fuzz_regressions_are_errors_or_results_never_panics() -> Result<(), Box<dyn std::error::Error>> {
    // Every input a fuzz target or a review found a crash with is kept here and must stay
    // harmless (fuzz/README.md). nested-class-row-zero.bin: a NestedClass row naming the nil
    // row 0. nested-class-cycle.bin, type-ref-scope-cycle.bin: two rows nesting in each other
    // (quadratic walks). type-spec-names-itself.bin: TypeSpec 1 = CLASS TypeSpec 1 (a stack
    // overflow when named). pdb-stream-in-assembly.bin: a #Pdb stream claiming u32::MAX rows
    // (an overflow in the list ranges).
    let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fuzz-regressions");
    let mut inputs = 0;
    for entry in std::fs::read_dir(folder)? {
        let bytes = std::fs::read(entry?.path())?;
        let _ = Assembly::read(&bytes);
        if let Ok(loaded) = rb_extract_dotnet::loader::Loaded::read(&bytes) {
            let universe = rb_extract_dotnet::names::Universe::new(vec![&loaded]);
            for ty in &loaded.types {
                if let Some(base) = ty.extends {
                    let _ = universe.token_name(0, base, Default::default(), true);
                }
            }
            for spec in &loaded.type_specs {
                let _ = universe.sig_name(0, spec, Default::default(), false);
            }
        }
        if let Ok(pdb) = PortablePdb::parse(&bytes) {
            let _ = pdb.documents();
            let _ = pdb.type_definition_documents();
        }
        inputs += 1;
    }
    assert!(inputs >= 5);
    Ok(())
}
