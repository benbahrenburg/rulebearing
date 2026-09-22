//! The PE container: headers, sections, the CLI header and the debug directory.
//!
//! - Specification: ECMA-335 II.25 (file format extensions to PE); the PE/COFF specification for
//!   the debug directory (CodeView `RSDS` and embedded portable PDB entries)
//! - Plan: [Wave 0, Step 9](../../../docs/plans/pending/0000-wave-0-spike.md#step-9-spike-b-rb-extract-dotnet-0d)
//!   (`pe.rs`)
//! - Architecture: [Extractors](../../../docs/architecture.md#extractors) (the .NET reader)

use crate::bytes::{Read, Reader, malformed};

/// One section: where an RVA range sits in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Section {
    /// First RVA of the section.
    pub virtual_address: u32,
    /// Size in memory.
    pub virtual_size: u32,
    /// Offset of the raw data in the file.
    pub raw_offset: u32,
    /// Size of the raw data in the file.
    pub raw_size: u32,
}

/// Where the debugging information for an assembly is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DebugInfo {
    /// A CodeView entry naming a PDB file.
    CodeView {
        /// The PDB path the compiler recorded.
        path: String,
        /// Whether the entry declares a portable PDB (minor version `0x504D`).
        portable: bool,
    },
    /// A portable PDB embedded in the assembly, deflate-compressed.
    Embedded {
        /// The decompressed PDB.
        pdb: Vec<u8>,
    },
}

/// A parsed PE image, holding what the metadata reader needs.
#[derive(Debug, Clone)]
pub struct PeImage<'a> {
    data: &'a [u8],
    sections: Vec<Section>,
    /// The metadata root, as a slice of the file.
    pub metadata: &'a [u8],
    /// Debug directory entries that were understood, in file order.
    pub debug: Vec<DebugInfo>,
}

const CLI_DIRECTORY: usize = 14;
const DEBUG_DIRECTORY: usize = 6;
const DEBUG_TYPE_CODEVIEW: u32 = 2;
const DEBUG_TYPE_EMBEDDED_PORTABLE_PDB: u32 = 17;
/// Upper bound on a decompressed embedded PDB, so a crafted size cannot exhaust memory.
const MAX_EMBEDDED_PDB: usize = 256 * 1024 * 1024;

impl<'a> PeImage<'a> {
    /// Parses a PE file that carries CLI metadata.
    ///
    /// # Errors
    /// When the file is not a PE image, has no CLI header, or any structure is truncated.
    pub fn parse(data: &'a [u8]) -> Read<Self> {
        let mut r = Reader::new(data, 0, "DOS header");
        if r.bytes(2)? != b"MZ" {
            return malformed("DOS header", 0);
        }
        r.seek(0x3C);
        let pe_offset = r.u32()? as usize;
        let mut r = Reader::new(data, pe_offset, "PE header");
        if r.bytes(4)? != b"PE\0\0" {
            return malformed("PE header", pe_offset);
        }
        r.skip(2)?; // machine
        let section_count = usize::from(r.u16()?);
        r.skip(12)?; // time stamp, symbol table, symbol count
        let optional_size = usize::from(r.u16()?);
        r.skip(2)?; // characteristics
        let optional_start = r.position();
        let mut o = Reader::new(data, optional_start, "optional header");
        let directories_at = match o.u16()? {
            0x10B => 96,
            0x20B => 112,
            _ => return malformed("optional header", optional_start),
        };
        o.seek(optional_start + directories_at - 4);
        let directory_count = o.u32()? as usize;
        let directory = |index: usize| -> Read<(u32, u32)> {
            if index >= directory_count {
                return Ok((0, 0));
            }
            let mut d = Reader::new(
                data,
                optional_start + directories_at + index * 8,
                "data directory",
            );
            Ok((d.u32()?, d.u32()?))
        };

        let mut s = Reader::new(data, optional_start + optional_size, "section table");
        let mut sections = Vec::with_capacity(section_count.min(96));
        for _ in 0..section_count {
            s.skip(8)?; // name
            let virtual_size = s.u32()?;
            let virtual_address = s.u32()?;
            let raw_size = s.u32()?;
            let raw_offset = s.u32()?;
            s.skip(16)?;
            sections.push(Section {
                virtual_address,
                virtual_size,
                raw_offset,
                raw_size,
            });
        }

        let mut image = Self {
            data,
            sections,
            metadata: &[],
            debug: Vec::new(),
        };
        let (cli_rva, cli_size) = directory(CLI_DIRECTORY)?;
        if cli_rva == 0 || cli_size < 72 {
            return malformed("CLI header (not a .NET assembly)", optional_start);
        }
        let cli = image.slice(cli_rva, 72, "CLI header")?;
        let mut c = Reader::new(cli, 8, "CLI header");
        let metadata_rva = c.u32()?;
        let metadata_size = c.u32()?;
        image.metadata = image.slice(metadata_rva, metadata_size, "metadata root")?;

        let (debug_rva, debug_size) = directory(DEBUG_DIRECTORY)?;
        if debug_rva != 0 && debug_size >= 28 {
            image.debug = image.debug_entries(debug_rva, debug_size)?;
        }
        Ok(image)
    }

    /// Maps an RVA to a file offset through the section table.
    pub fn offset_of(&self, rva: u32) -> Option<usize> {
        self.sections.iter().find_map(|s| {
            let within = rva.checked_sub(s.virtual_address)?;
            (within < s.virtual_size.max(s.raw_size) && within < s.raw_size)
                .then(|| s.raw_offset as usize + within as usize)
        })
    }

    fn slice(&self, rva: u32, size: u32, what: &'static str) -> Read<&'a [u8]> {
        let Some(offset) = self.offset_of(rva) else {
            return malformed(what, rva as usize);
        };
        let end = offset.checked_add(size as usize);
        match end.and_then(|end| self.data.get(offset..end)) {
            Some(slice) => Ok(slice),
            None => malformed(what, offset),
        }
    }

    fn debug_entries(&self, rva: u32, size: u32) -> Read<Vec<DebugInfo>> {
        let directory = self.slice(rva, size, "debug directory")?;
        let mut entries = Vec::new();
        for chunk in directory.chunks_exact(28) {
            let mut e = Reader::new(chunk, 12, "debug directory entry");
            let kind = e.u32()?;
            let size = e.u32()? as usize;
            e.skip(4)?; // address of raw data
            let pointer = e.u32()? as usize;
            let Some(raw) = pointer
                .checked_add(size)
                .and_then(|end| self.data.get(pointer..end))
            else {
                continue;
            };
            match kind {
                DEBUG_TYPE_CODEVIEW => {
                    let minor = u16::from_le_bytes([chunk[6], chunk[7]]);
                    let mut v = Reader::new(raw, 0, "CodeView entry");
                    if v.bytes(4)? == b"RSDS" {
                        v.skip(20)?; // GUID and age
                        let path = v.c_string()?.to_owned();
                        entries.push(DebugInfo::CodeView {
                            path,
                            portable: minor == 0x504D,
                        });
                    }
                }
                DEBUG_TYPE_EMBEDDED_PORTABLE_PDB => {
                    let mut v = Reader::new(raw, 0, "embedded PDB");
                    if v.bytes(4)? != b"MPDB" {
                        return malformed("embedded PDB", pointer);
                    }
                    let expected = v.u32()? as usize;
                    if expected > MAX_EMBEDDED_PDB {
                        return malformed("embedded PDB (declared size too large)", pointer);
                    }
                    let compressed = raw.get(8..).unwrap_or_default();
                    let pdb =
                        miniz_oxide::inflate::decompress_to_vec_with_limit(compressed, expected)
                            .or_else(|_| malformed("embedded PDB (deflate)", pointer))?;
                    entries.push(DebugInfo::Embedded { pdb });
                }
                _ => {}
            }
        }
        Ok(entries)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A minimal PE32 image with one section holding a CLI header and a 4-byte "metadata" blob,
    /// and optionally a debug directory. Offsets are fixed so the tests can reason about them.
    pub(crate) fn tiny_pe(metadata: &[u8], debug: Option<&[u8]>) -> Vec<u8> {
        let mut f = vec![0u8; 0x400];
        f[0..2].copy_from_slice(b"MZ");
        f[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        f[0x80..0x84].copy_from_slice(b"PE\0\0");
        f[0x86..0x88].copy_from_slice(&1u16.to_le_bytes()); // one section
        f[0x94..0x96].copy_from_slice(&224u16.to_le_bytes()); // optional header size
        let opt = 0x98;
        f[opt..opt + 2].copy_from_slice(&0x10Bu16.to_le_bytes());
        f[opt + 92..opt + 96].copy_from_slice(&16u32.to_le_bytes()); // directory count
        // CLI directory: RVA 0x2000, size 72
        f[opt + 96 + 14 * 8..opt + 96 + 14 * 8 + 4].copy_from_slice(&0x2000u32.to_le_bytes());
        f[opt + 96 + 14 * 8 + 4..opt + 96 + 14 * 8 + 8].copy_from_slice(&72u32.to_le_bytes());
        // Section table right after the optional header.
        let sec = opt + 224;
        f[sec + 8..sec + 12].copy_from_slice(&0x1000u32.to_le_bytes()); // virtual size
        f[sec + 12..sec + 16].copy_from_slice(&0x2000u32.to_le_bytes()); // virtual address
        f[sec + 16..sec + 20].copy_from_slice(&0x200u32.to_le_bytes()); // raw size
        f[sec + 20..sec + 24].copy_from_slice(&0x200u32.to_le_bytes()); // raw offset
        // CLI header at file 0x200: metadata at RVA 0x2048.
        f[0x208..0x20C].copy_from_slice(&0x2048u32.to_le_bytes());
        let size = u32::try_from(metadata.len()).unwrap_or(0);
        f[0x20C..0x210].copy_from_slice(&size.to_le_bytes());
        f[0x248..0x248 + metadata.len()].copy_from_slice(metadata);
        if let Some(entry) = debug {
            // Debug directory at RVA 0x2100 (file 0x300), one 28-byte entry, raw data at 0x340.
            f[opt + 96 + 6 * 8..opt + 96 + 6 * 8 + 4].copy_from_slice(&0x2100u32.to_le_bytes());
            f[opt + 96 + 6 * 8 + 4..opt + 96 + 6 * 8 + 8].copy_from_slice(&28u32.to_le_bytes());
            f[0x300..0x300 + 28].copy_from_slice(&entry[..28]);
            f[0x340..0x340 + entry.len() - 28].copy_from_slice(&entry[28..]);
        }
        f
    }

    fn codeview_entry(path: &str, portable: bool) -> Vec<u8> {
        let mut raw = b"RSDS".to_vec();
        raw.extend([0u8; 20]);
        raw.extend(path.as_bytes());
        raw.push(0);
        let mut e = vec![0u8; 28];
        if portable {
            e[6..8].copy_from_slice(&0x504Du16.to_le_bytes());
        }
        e[12..16].copy_from_slice(&DEBUG_TYPE_CODEVIEW.to_le_bytes());
        e[16..20].copy_from_slice(&u32::try_from(raw.len()).unwrap_or(0).to_le_bytes());
        e[24..28].copy_from_slice(&0x340u32.to_le_bytes());
        e.extend(raw);
        e
    }

    #[test]
    fn parses_the_cli_header_and_metadata_slice() {
        let file = tiny_pe(b"BSJB", None);
        let image = PeImage::parse(&file);
        let image = image.as_ref().ok();
        assert_eq!(image.map(|i| i.metadata), Some(&b"BSJB"[..]));
        assert_eq!(image.map(|i| i.debug.len()), Some(0));
        assert_eq!(image.and_then(|i| i.offset_of(0x2010)), Some(0x210));
        assert_eq!(image.and_then(|i| i.offset_of(0x5000)), None);
    }

    #[test]
    fn reads_a_codeview_pdb_path() {
        let file = tiny_pe(b"BSJB", Some(&codeview_entry("/_/obj/A.pdb", true)));
        let debug = PeImage::parse(&file).map(|i| i.debug).unwrap_or_default();
        assert_eq!(
            debug,
            [DebugInfo::CodeView {
                path: "/_/obj/A.pdb".to_owned(),
                portable: true
            }]
        );
        let file = tiny_pe(b"BSJB", Some(&codeview_entry("C:\\A.pdb", false)));
        let debug = PeImage::parse(&file).map(|i| i.debug).unwrap_or_default();
        assert!(matches!(
            &debug[..],
            [DebugInfo::CodeView {
                portable: false,
                ..
            }]
        ));
    }

    #[test]
    fn reads_an_embedded_portable_pdb() {
        let pdb = b"BSJB embedded".to_vec();
        let compressed = miniz_oxide::deflate::compress_to_vec(&pdb, 6);
        let mut raw = b"MPDB".to_vec();
        raw.extend(u32::try_from(pdb.len()).unwrap_or(0).to_le_bytes());
        raw.extend(&compressed);
        let mut e = vec![0u8; 28];
        e[12..16].copy_from_slice(&DEBUG_TYPE_EMBEDDED_PORTABLE_PDB.to_le_bytes());
        e[16..20].copy_from_slice(&u32::try_from(raw.len()).unwrap_or(0).to_le_bytes());
        e[24..28].copy_from_slice(&0x340u32.to_le_bytes());
        e.extend(raw);
        let file = tiny_pe(b"BSJB", Some(&e));
        let debug = PeImage::parse(&file).map(|i| i.debug).unwrap_or_default();
        assert_eq!(debug, [DebugInfo::Embedded { pdb }]);
    }

    #[test]
    fn rejects_what_is_not_a_dotnet_pe() {
        assert!(PeImage::parse(b"").is_err());
        assert!(PeImage::parse(b"ZM").is_err());
        let mut file = tiny_pe(b"BSJB", None);
        file[0x80] = b'X';
        assert!(PeImage::parse(&file).is_err());
        let mut file = tiny_pe(b"BSJB", None);
        file[0x98] = 0; // bad optional header magic
        assert!(PeImage::parse(&file).is_err());
        let mut file = tiny_pe(b"BSJB", None);
        file[0x98 + 96 + 14 * 8..0x98 + 96 + 14 * 8 + 8].fill(0); // no CLI directory
        let error = PeImage::parse(&file)
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default();
        assert!(error.contains("not a .NET assembly"), "{error}");
    }

    #[test]
    fn every_truncation_is_an_error_not_a_panic() {
        let file = tiny_pe(b"BSJB", Some(&codeview_entry("a.pdb", true)));
        for length in 0..file.len() {
            let _ = PeImage::parse(&file[..length]);
        }
    }
}
