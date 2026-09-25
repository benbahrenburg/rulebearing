//! The portable PDB reader against System.Reflection.Metadata's reading of the same file.
//!
//! - Plan: [Wave 2, Step 2](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#22-step-2-the-metadata-il-and-pdb-readers-2a)
//!   ("`tests/pdb.rs` asserts the document list and the sequence points of the same three methods")
//! - Expected values: `conformance/archunitnet/tools/MetadataDump`

use std::path::{Path, PathBuf};

use rb_extract_dotnet::pdb::PortablePdb;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../conformance/archunitnet/fixtures")
        .join(name)
}

/// A method's visible sequence points as `(IL offset, line, column)`.
type Points = &'static [(u32, u32, u32)];

/// `(MethodDef row, points)`, the visible points `MetadataDump` printed.
const POINTS: &[(u32, Points)] = &[
    (
        20,
        &[(0x0, 9, 9), (0x7, 10, 9), (0x8, 11, 13), (0xF, 12, 9)],
    ),
    (
        27,
        &[(0x0, 8, 9), (0x1, 9, 13), (0xD, 10, 13), (0x15, 11, 9)],
    ),
    (
        71,
        &[
            (0x0, 12, 9),
            (0x1, 13, 13),
            (0x7, 14, 13),
            (0x8, 14, 37),
            (0x16, 14, 22),
            (0x1E, 15, 13),
            (0x1F, 16, 17),
            (0x26, 17, 13),
            (0x27, 14, 34),
            (0x41, 19, 13),
            (0x4E, 20, 9),
        ],
    ),
];

#[test]
fn documents_and_sequence_points_agree() -> Result<(), Box<dyn std::error::Error>> {
    let bytes = std::fs::read(fixture("TestAssembly.pdb"))?;
    let pdb = PortablePdb::parse(&bytes).map_err(|e| format!("{e:?}"))?;
    let documents = pdb.documents()?;
    assert_eq!(documents.len(), 47);
    assert!(
        documents.contains(&"/_/TestAssembly/Class1.cs".to_owned()),
        "{documents:?}"
    );
    for (row, expected) in POINTS {
        let points = pdb.sequence_points(*row)?;
        let found: Vec<(u32, u32, u32)> = points
            .iter()
            .map(|p| (p.offset, p.line, p.column))
            .collect();
        assert_eq!(found, *expected, "method row {row}");
        let first = pdb.first_point(*row)?.map(|p| (p.line, p.column));
        assert_eq!(
            first,
            expected.first().map(|p| (p.1, p.2)),
            "first point of row {row}"
        );
    }
    assert!(pdb.sequence_points(0)?.is_empty());
    assert!(pdb.sequence_points(100_000)?.is_empty());
    let _ = pdb.source_link()?;
    Ok(())
}

#[test]
fn truncated_pdbs_are_errors_not_panics() -> Result<(), Box<dyn std::error::Error>> {
    let bytes = std::fs::read(fixture("TestAssembly.pdb"))?;
    for length in (0..bytes.len()).step_by(211) {
        if let Ok(pdb) = PortablePdb::parse(&bytes[..length]) {
            let _ = pdb.documents();
            let _ = pdb.sequence_points(71);
        }
    }
    Ok(())
}
