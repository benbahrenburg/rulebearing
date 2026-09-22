//! The portable PDB: which source document and line each method, and each body-less type, came from.
//!
//! - Specification: the portable PDB format (the `#Pdb` stream, the `Document` and
//!   `MethodDebugInformation` tables, the sequence-points blob, and the `TypeDefinitionDocuments`
//!   custom debug information)
//! - Decision: [ADR-0011](../../../docs/adr/0011-read-dotnet-assemblies-not-source.md) (attribution
//!   is read from the PDB, never from C# source)
//! - Plan: [Wave 0, Step 9](../../../docs/plans/pending/0000-wave-0-spike.md#step-9-spike-b-rb-extract-dotnet-0d)
//!   (`pdb.rs`)

use std::collections::BTreeMap;

use crate::bytes::{Read, Reader, malformed};
use crate::metadata::Metadata;
use crate::metadata::tables::{Coded, id};

/// The `CustomDebugInformation` kind Roslyn writes for a type with no method bodies, listing the
/// documents that declare it, in the `#GUID` heap's byte order
/// (`932E74BC-DBA9-4478-8D46-0F32A7BAB3D3`).
const TYPE_DEFINITION_DOCUMENTS: [u8; 16] = [
    0xBC, 0x74, 0x2E, 0x93, 0xA9, 0xDB, 0x78, 0x44, 0x8D, 0x46, 0x0F, 0x32, 0xA7, 0xBA, 0xB3, 0xD3,
];

/// The first visible sequence point of a method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FirstPoint {
    /// 1-based row of the `Document` table.
    pub document: u32,
    /// 1-based start line.
    pub line: u32,
    /// 1-based start column.
    pub column: u32,
}

/// Why a file is not a readable portable PDB.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PdbError {
    /// A Windows (MSF) PDB, which has no portable document table.
    NotPortable,
    /// A portable PDB whose structure is broken.
    Malformed(crate::bytes::ReadError),
}

impl From<crate::bytes::ReadError> for PdbError {
    fn from(error: crate::bytes::ReadError) -> Self {
        Self::Malformed(error)
    }
}

/// A parsed portable PDB.
#[derive(Debug, Clone)]
pub struct PortablePdb<'a> {
    metadata: Metadata<'a>,
}

impl<'a> PortablePdb<'a> {
    /// Parses a PDB file.
    ///
    /// # Errors
    /// [`PdbError::NotPortable`] for a Windows PDB (anything without the `BSJB` signature), and
    /// [`PdbError::Malformed`] for a portable PDB that cannot be read.
    pub fn parse(data: &'a [u8]) -> Result<Self, PdbError> {
        if !data.starts_with(b"BSJB") {
            return Err(PdbError::NotPortable);
        }
        let metadata = Metadata::parse(data, None)?;
        if metadata.pdb_stream.is_none() {
            return Err(PdbError::Malformed(crate::bytes::ReadError {
                what: "portable PDB (no #Pdb stream)",
                offset: 0,
            }));
        }
        Ok(Self { metadata })
    }

    /// Every document's path, indexed by row minus one.
    ///
    /// # Errors
    /// When a document name blob is malformed.
    pub fn documents(&self) -> Read<Vec<String>> {
        let count = self.metadata.rows(id::DOCUMENT);
        (1..=count).map(|row| self.document_name(row)).collect()
    }

    fn document_name(&self, row: u32) -> Read<String> {
        let blob = self
            .metadata
            .blob(self.metadata.tables.cell(id::DOCUMENT, row, 0)?)?;
        let mut r = Reader::new(blob, 0, "document name");
        let separator = r.u8()?;
        let mut name = String::new();
        let mut first = true;
        while r.position() < blob.len() {
            let part = r.compressed_u32()?;
            if !first && separator != 0 {
                name.push(char::from(separator));
            }
            first = false;
            if part != 0 {
                let bytes = self.metadata.blob(part)?;
                let text =
                    std::str::from_utf8(bytes).or_else(|_| malformed("document name part", 0))?;
                name.push_str(text);
            }
        }
        Ok(name)
    }

    /// The first visible sequence point of the method with `MethodDef` row `method`, if it has one.
    ///
    /// # Errors
    /// When the method's sequence-points blob is malformed.
    pub fn first_point(&self, method: u32) -> Read<Option<FirstPoint>> {
        if method == 0 || method > self.metadata.rows(id::METHOD_DEBUG_INFORMATION) {
            return Ok(None);
        }
        let tables = &self.metadata.tables;
        let document = tables.cell(id::METHOD_DEBUG_INFORMATION, method, 0)?;
        let points = tables.cell(id::METHOD_DEBUG_INFORMATION, method, 1)?;
        if points == 0 {
            return Ok(None);
        }
        first_visible_point(self.metadata.blob(points)?, document)
    }

    /// For each `TypeDef` row that carries `TypeDefinitionDocuments`, the documents declaring it.
    ///
    /// # Errors
    /// When the custom debug information table or a value blob is malformed.
    pub fn type_definition_documents(&self) -> Read<BTreeMap<u32, Vec<u32>>> {
        let mut found = BTreeMap::new();
        let tables = &self.metadata.tables;
        for row in 1..=self.metadata.rows(id::CUSTOM_DEBUG_INFORMATION) {
            let parent = tables.cell(id::CUSTOM_DEBUG_INFORMATION, row, 0)?;
            let Some((id::TYPE_DEF, type_row)) = Coded::HasCustomDebugInformation.decode(parent)
            else {
                continue;
            };
            let kind = self
                .metadata
                .guid(tables.cell(id::CUSTOM_DEBUG_INFORMATION, row, 1)?)?;
            if kind != TYPE_DEFINITION_DOCUMENTS {
                continue;
            }
            let value = self
                .metadata
                .blob(tables.cell(id::CUSTOM_DEBUG_INFORMATION, row, 2)?)?;
            let mut r = Reader::new(value, 0, "TypeDefinitionDocuments");
            let mut documents = Vec::new();
            while r.position() < value.len() {
                documents.push(r.compressed_u32()?);
            }
            found.insert(type_row, documents);
        }
        Ok(found)
    }
}

/// Decodes a sequence-points blob far enough to find the first visible point.
///
/// # Errors
/// When the blob ends early.
pub fn first_visible_point(blob: &[u8], document: u32) -> Read<Option<FirstPoint>> {
    let mut r = Reader::new(blob, 0, "sequence points");
    r.compressed_u32()?; // local signature
    let mut document = if document == 0 {
        r.compressed_u32()?
    } else {
        document
    };
    let mut first_record = true;
    while r.position() < blob.len() {
        let il_delta = r.compressed_u32()?;
        if il_delta == 0 && !first_record {
            document = r.compressed_u32()?; // a document record
            continue;
        }
        first_record = false;
        let line_delta = r.compressed_u32()?;
        let column_delta = if line_delta == 0 {
            i64::from(r.compressed_u32()?)
        } else {
            i64::from(r.compressed_i32()?)
        };
        if line_delta == 0 && column_delta == 0 {
            continue; // hidden
        }
        let line = r.compressed_u32()?;
        let column = r.compressed_u32()?;
        return Ok(Some(FirstPoint {
            document,
            line,
            column,
        }));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::tables::tests::table_stream;
    use crate::metadata::tests::root;

    #[test]
    fn a_windows_pdb_is_reported_by_name() {
        assert!(matches!(
            PortablePdb::parse(b"Microsoft C/C++ MSF 7.00\r\n"),
            Err(PdbError::NotPortable)
        ));
        assert!(matches!(
            PortablePdb::parse(b"BSJB"),
            Err(PdbError::Malformed(_))
        ));
    }

    #[test]
    fn finds_the_first_visible_point_past_hidden_ones() {
        // local sig 0; record 1: IL 0, hidden (0,0); record 2: IL +4, lines 0, columns 5, line 12, column 9.
        let blob = [0, 0, 0, 0, 4, 0, 5, 12, 9];
        assert_eq!(
            first_visible_point(&blob, 3),
            Ok(Some(FirstPoint {
                document: 3,
                line: 12,
                column: 9
            }))
        );
        // Initial document from the blob (the column is nil), a hidden point, a document record
        // switching to 7, then IL +2, one line, a signed column delta of 3, line 10, column 5.
        let blob = [0, 2, 0, 0, 0, 0, 7, 2, 1, 6, 10, 5];
        assert_eq!(
            first_visible_point(&blob, 0),
            Ok(Some(FirstPoint {
                document: 7,
                line: 10,
                column: 5
            }))
        );
        assert_eq!(first_visible_point(&[0, 0, 0, 0], 1), Ok(None));
        assert!(first_visible_point(&[0, 0, 1], 1).is_err());
    }

    /// A PDB with one document, one method with a point, and `TypeDefinitionDocuments` on `TypeDef` 4.
    fn pdb() -> Vec<u8> {
        let mut pdb_stream = vec![0u8; 20];
        pdb_stream.extend(0u32.to_le_bytes());
        pdb_stream.extend(((1u64 << id::TYPE_DEF) | (1u64 << id::METHOD_DEF)).to_le_bytes());
        pdb_stream.extend(4u32.to_le_bytes());
        pdb_stream.extend(2u32.to_le_bytes());
        // #Blob: [0]=empty, [1]="src" (len 3), [5]="A.cs", [10] name blob '/', 1, 5,
        //        [14] sequence points, [21] TypeDefinitionDocuments value.
        let mut blob = vec![0u8];
        blob.extend([3, b's', b'r', b'c']);
        blob.extend([4, b'A', b'.', b'c', b's']);
        blob.extend([3, b'/', 1, 5]);
        blob.extend([6, 0, 0, 3, 0, 8, 2]);
        blob.extend([1, 1]);
        let mut guid = TYPE_DEFINITION_DOCUMENTS.to_vec();
        guid.extend([0u8; 16]);
        let tilde = table_stream(
            &[
                (id::DOCUMENT, vec![vec![10, 0, 0, 0]]),
                (id::METHOD_DEBUG_INFORMATION, vec![vec![0, 0], vec![1, 14]]),
                (
                    id::CUSTOM_DEBUG_INFORMATION,
                    vec![vec![(4 << 5) | 3, 1, 21], vec![(1 << 5) | 3, 2, 21]],
                ),
            ],
            0,
        );
        root(&[
            ("#Pdb", pdb_stream),
            ("#~", tilde),
            ("#Blob", blob),
            ("#GUID", guid),
        ])
    }

    #[test]
    fn reads_documents_points_and_type_documents() {
        let data = pdb();
        let pdb = PortablePdb::parse(&data);
        let pdb = pdb.as_ref().ok();
        assert_eq!(
            pdb.and_then(|p| p.documents().ok()),
            Some(vec!["src/A.cs".to_owned()])
        );
        assert_eq!(pdb.and_then(|p| p.first_point(1).ok()), Some(None));
        assert_eq!(
            pdb.and_then(|p| p.first_point(2).ok()),
            Some(Some(FirstPoint {
                document: 1,
                line: 8,
                column: 2
            }))
        );
        assert_eq!(pdb.and_then(|p| p.first_point(3).ok()), Some(None));
        assert_eq!(pdb.and_then(|p| p.first_point(0).ok()), Some(None));
        let types = pdb
            .and_then(|p| p.type_definition_documents().ok())
            .unwrap_or_default();
        assert_eq!(types.get(&4), Some(&vec![1]));
        assert_eq!(types.len(), 1, "the row with another GUID is ignored");
        for length in 0..data.len() {
            if let Ok(p) = PortablePdb::parse(&data[..length]) {
                let _ = p.documents();
                let _ = p.first_point(2);
                let _ = p.type_definition_documents();
            }
        }
    }
}
