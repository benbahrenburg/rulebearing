//! ECMA-335 metadata: the metadata root, its streams, the heaps and the table stream.
//!
//! - Specification: ECMA-335 II.24 (metadata physical layout), II.22 (tables); the portable PDB
//!   format (tables 0x30 to 0x37 and the `#Pdb` stream)
//! - Plan: [Wave 0, Step 9](../../../../docs/plans/pending/0000-wave-0-spike.md#step-9-spike-b-rb-extract-dotnet-0d)
//!   (`metadata/streams.rs`, `metadata/tables.rs`)
//! - Architecture: [Extractors](../../../../docs/architecture.md#extractors) (the .NET reader)
//!
//! The same code reads an assembly's metadata and a portable PDB's, because a portable PDB is an
//! ECMA-335 metadata root with extra tables; the only difference is that the PDB's references to
//! type-system tables are sized by row counts recorded in its `#Pdb` stream.

pub mod tables;

use crate::bytes::{Read, Reader, align4, malformed};
use tables::{TableId, Tables};

/// One stream of the metadata root.
#[derive(Debug, Clone, Copy)]
struct Stream<'a> {
    name: &'a str,
    data: &'a [u8],
}

/// A parsed metadata root.
#[derive(Debug, Clone)]
pub struct Metadata<'a> {
    /// The runtime version string, for example `v4.0.30319`.
    pub version: &'a str,
    strings: &'a [u8],
    user_strings: &'a [u8],
    blobs: &'a [u8],
    guids: &'a [u8],
    /// The `#Pdb` stream, present in a portable PDB.
    pub pdb_stream: Option<&'a [u8]>,
    /// The table stream.
    pub tables: Tables<'a>,
}

/// The fixed part of a portable PDB's `#Pdb` stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdbStream {
    /// The PDB id: the GUID and stamp that tie the PDB to its assembly.
    pub id: [u8; 20],
    /// Row counts of the type-system tables the PDB references, indexed by table id.
    pub type_system_rows: [u32; 64],
}

/// Reads the `#Pdb` stream's header.
///
/// # Errors
/// When the stream is truncated.
pub fn parse_pdb_stream(data: &[u8]) -> Read<PdbStream> {
    let mut r = Reader::new(data, 0, "#Pdb stream");
    let mut id = [0u8; 20];
    id.copy_from_slice(r.bytes(20)?);
    r.skip(4)?; // entry point
    let referenced = r.u64()?;
    let mut type_system_rows = [0u32; 64];
    for (table, rows) in type_system_rows.iter_mut().enumerate() {
        if referenced & (1u64 << table) != 0 {
            *rows = r.u32()?;
        }
    }
    Ok(PdbStream {
        id,
        type_system_rows,
    })
}

impl<'a> Metadata<'a> {
    /// Parses a metadata root. `external_rows` gives the row counts of tables stored elsewhere,
    /// which a portable PDB needs for its references to the assembly's tables.
    ///
    /// # Errors
    /// When the root, a stream header or the table stream is malformed.
    pub fn parse(data: &'a [u8], external_rows: Option<&[u32; 64]>) -> Read<Self> {
        let mut r = Reader::new(data, 0, "metadata root");
        if r.bytes(4)? != b"BSJB" {
            return malformed("metadata root signature", 0);
        }
        r.skip(8)?; // major, minor, reserved
        let length = r.u32()? as usize;
        let version_bytes = r.bytes(length)?;
        let version = std::str::from_utf8(version_bytes)
            .or_else(|_| malformed("metadata version string", 16))?
            .trim_end_matches('\0');
        r.skip(2)?; // flags
        let count = usize::from(r.u16()?);
        let mut streams = Vec::with_capacity(count.min(16));
        for _ in 0..count {
            let offset = r.u32()? as usize;
            let size = r.u32()? as usize;
            let name_start = r.position();
            let name = r.c_string()?;
            let Some(next) = align4(r.position()) else {
                return malformed("stream header", name_start);
            };
            r.seek(next);
            let Some(stream) = offset
                .checked_add(size)
                .and_then(|end| data.get(offset..end))
            else {
                return malformed("stream extent", name_start);
            };
            streams.push(Stream { name, data: stream });
        }
        let find = |name: &str| streams.iter().find(|s| s.name == name).map(|s| s.data);
        let Some(table_stream) = find("#~").or_else(|| find("#-")) else {
            return malformed("metadata (no table stream)", 0);
        };
        let pdb_stream = find("#Pdb");
        let own_external;
        let external = match (external_rows, pdb_stream) {
            (Some(rows), _) => Some(rows),
            (None, Some(pdb)) => {
                own_external = parse_pdb_stream(pdb)?.type_system_rows;
                Some(&own_external)
            }
            (None, None) => None,
        };
        let tables = Tables::parse(table_stream, external)?;
        Ok(Self {
            version,
            strings: find("#Strings").unwrap_or_default(),
            user_strings: find("#US").unwrap_or_default(),
            blobs: find("#Blob").unwrap_or_default(),
            guids: find("#GUID").unwrap_or_default(),
            pdb_stream,
            tables,
        })
    }

    /// A string from the `#Strings` heap.
    ///
    /// # Errors
    /// When the index is out of range or the string is not NUL-terminated UTF-8.
    pub fn string(&self, index: u32) -> Read<&'a str> {
        Reader::new(self.strings, index as usize, "#Strings heap").c_string()
    }

    /// A string literal from the `#US` heap (II.24.2.4): UTF-16LE with a trailing flag byte.
    ///
    /// # Errors
    /// When the index or the length is out of range.
    pub fn user_string(&self, index: u32) -> Read<String> {
        let mut r = Reader::new(self.user_strings, index as usize, "#US heap");
        let length = r.compressed_u32()? as usize;
        let bytes = r.bytes(length)?;
        let units: Vec<u16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect();
        Ok(String::from_utf16_lossy(&units))
    }

    /// A blob from the `#Blob` heap, without its length prefix.
    ///
    /// # Errors
    /// When the index or the length is out of range.
    pub fn blob(&self, index: u32) -> Read<&'a [u8]> {
        let mut r = Reader::new(self.blobs, index as usize, "#Blob heap");
        let length = r.compressed_u32()? as usize;
        r.bytes(length)
    }

    /// A GUID from the `#GUID` heap; index 0 is the nil GUID.
    ///
    /// # Errors
    /// When the index is out of range.
    pub fn guid(&self, index: u32) -> Read<[u8; 16]> {
        let mut guid = [0u8; 16];
        if index == 0 {
            return Ok(guid);
        }
        let Some(start) = (index as usize)
            .checked_sub(1)
            .and_then(|i| i.checked_mul(16))
        else {
            return malformed("#GUID heap", 0);
        };
        guid.copy_from_slice(Reader::new(self.guids, start, "#GUID heap").bytes(16)?);
        Ok(guid)
    }

    /// The number of rows in a table.
    pub fn rows(&self, table: TableId) -> u32 {
        self.tables.rows(table)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Builds a metadata root from streams, for tests.
    pub(crate) fn root(streams: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let mut header = b"BSJB".to_vec();
        header.extend([1, 0, 1, 0, 0, 0, 0, 0]);
        let version = b"v4.0.30319\0\0";
        header.extend(u32::try_from(version.len()).unwrap_or(0).to_le_bytes());
        header.extend(version);
        header.extend([0, 0]);
        header.extend(u16::try_from(streams.len()).unwrap_or(0).to_le_bytes());
        let names: Vec<Vec<u8>> = streams
            .iter()
            .map(|(name, _)| {
                let mut n = name.as_bytes().to_vec();
                n.push(0);
                while n.len() % 4 != 0 {
                    n.push(0);
                }
                n
            })
            .collect();
        let headers_len: usize = names.iter().map(|n| 8 + n.len()).sum();
        let mut offset = header.len() + headers_len;
        let mut body: Vec<u8> = Vec::new();
        for ((_, data), name) in streams.iter().zip(&names) {
            header.extend(u32::try_from(offset).unwrap_or(0).to_le_bytes());
            header.extend(u32::try_from(data.len()).unwrap_or(0).to_le_bytes());
            header.extend(name);
            body.extend(data);
            offset += data.len();
        }
        header.extend(body);
        header
    }

    #[test]
    fn reads_heaps_and_rejects_bad_roots() {
        let tilde = tables::tests::table_stream(&[], 0);
        let data = root(&[
            ("#~", tilde),
            ("#Strings", b"\0Hello\0".to_vec()),
            ("#Blob", vec![0, 3, 1, 2, 3]),
            ("#GUID", (1u8..=16).collect()),
            ("#US", vec![0, 5, b'h', 0, b'i', 0, 0]),
        ]);
        let metadata = Metadata::parse(&data, None);
        let metadata = metadata.as_ref().ok();
        assert_eq!(metadata.map(|m| m.version), Some("v4.0.30319"));
        assert_eq!(metadata.and_then(|m| m.string(1).ok()), Some("Hello"));
        assert_eq!(metadata.and_then(|m| m.blob(1).ok()), Some(&[1, 2, 3][..]));
        assert_eq!(metadata.and_then(|m| m.guid(0).ok()), Some([0; 16]));
        assert_eq!(
            metadata.and_then(|m| m.guid(1).ok()).map(|g| g[15]),
            Some(16)
        );
        assert!(metadata.is_some_and(|m| m.guid(2).is_err()));
        assert!(metadata.is_some_and(|m| m.string(99).is_err()));
        assert_eq!(
            metadata.and_then(|m| m.user_string(1).ok()).as_deref(),
            Some("hi")
        );
        assert!(metadata.is_some_and(|m| m.user_string(40).is_err()));
        assert!(metadata.is_some_and(|m| m.pdb_stream.is_none()));
        assert!(Metadata::parse(b"XXXX", None).is_err());
        assert!(Metadata::parse(&root(&[("#Strings", vec![0])]), None).is_err());
        for length in 0..data.len() {
            let _ = Metadata::parse(&data[..length], None);
        }
    }

    #[test]
    fn reads_the_pdb_stream() {
        let mut pdb = vec![7u8; 20];
        pdb.extend(0u32.to_le_bytes());
        pdb.extend(((1u64 << 2) | (1u64 << 6)).to_le_bytes());
        pdb.extend(5u32.to_le_bytes());
        pdb.extend(9u32.to_le_bytes());
        let stream = parse_pdb_stream(&pdb);
        assert_eq!(stream.as_ref().map(|s| s.type_system_rows[2]), Ok(5));
        assert_eq!(stream.as_ref().map(|s| s.type_system_rows[6]), Ok(9));
        assert_eq!(stream.map(|s| s.id[0]), Ok(7));
        assert!(parse_pdb_stream(&pdb[..30]).is_err());
    }
}
