//! The `#~` table stream: row counts, row sizes, and typed access to the columns the reader uses.
//!
//! - Specification: ECMA-335 II.22 (every table's columns), II.24.2.6 (the stream layout and the
//!   width of heap, table and coded indices); portable PDB tables 0x30 to 0x37
//! - Plan: [Wave 0, Step 9](../../../../docs/plans/pending/0000-wave-0-spike.md#step-9-spike-b-rb-extract-dotnet-0d)
//!   (`metadata/tables.rs`: the row readers, "other tables are sized and skipped")
//!
//! Every table's column list is declared below, including the tables the spike never reads,
//! because a table's position in the stream depends on the size of every table before it.

use crate::bytes::{Read, Reader, malformed};

/// A table number (II.22).
pub type TableId = u8;

/// Table numbers used by name.
pub mod id {
    use super::TableId;
    /// `Module`.
    pub const MODULE: TableId = 0x00;
    /// `TypeRef`.
    pub const TYPE_REF: TableId = 0x01;
    /// `TypeDef`.
    pub const TYPE_DEF: TableId = 0x02;
    /// `Field`.
    pub const FIELD: TableId = 0x04;
    /// `MethodPtr`, present only in unoptimised (`#-`) metadata.
    pub const METHOD_PTR: TableId = 0x05;
    /// `MethodDef`.
    pub const METHOD_DEF: TableId = 0x06;
    /// `Param`.
    pub const PARAM: TableId = 0x08;
    /// `InterfaceImpl`.
    pub const INTERFACE_IMPL: TableId = 0x09;
    /// `MemberRef`.
    pub const MEMBER_REF: TableId = 0x0A;
    /// `CustomAttribute`.
    pub const CUSTOM_ATTRIBUTE: TableId = 0x0C;
    /// `StandAloneSig`.
    pub const STAND_ALONE_SIG: TableId = 0x11;
    /// `EventMap`.
    pub const EVENT_MAP: TableId = 0x12;
    /// `Event`.
    pub const EVENT: TableId = 0x14;
    /// `PropertyMap`.
    pub const PROPERTY_MAP: TableId = 0x15;
    /// `Property`.
    pub const PROPERTY: TableId = 0x17;
    /// `MethodSemantics`.
    pub const METHOD_SEMANTICS: TableId = 0x18;
    /// `ModuleRef`.
    pub const MODULE_REF: TableId = 0x1A;
    /// `TypeSpec`.
    pub const TYPE_SPEC: TableId = 0x1B;
    /// `Assembly`.
    pub const ASSEMBLY: TableId = 0x20;
    /// `AssemblyRef`.
    pub const ASSEMBLY_REF: TableId = 0x23;
    /// `NestedClass`.
    pub const NESTED_CLASS: TableId = 0x29;
    /// `GenericParam`.
    pub const GENERIC_PARAM: TableId = 0x2A;
    /// `MethodSpec`.
    pub const METHOD_SPEC: TableId = 0x2B;
    /// Portable PDB `Document`.
    pub const DOCUMENT: TableId = 0x30;
    /// Portable PDB `MethodDebugInformation`.
    pub const METHOD_DEBUG_INFORMATION: TableId = 0x31;
    /// Portable PDB `CustomDebugInformation`.
    pub const CUSTOM_DEBUG_INFORMATION: TableId = 0x37;
}

/// A coded index kind (II.24.2.6): the tables it can point at, in tag order. `None` is a tag
/// the specification leaves unused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Coded {
    /// `TypeDefOrRef`.
    TypeDefOrRef,
    /// `HasConstant`.
    HasConstant,
    /// `HasCustomAttribute`.
    HasCustomAttribute,
    /// `HasFieldMarshal`.
    HasFieldMarshal,
    /// `HasDeclSecurity`.
    HasDeclSecurity,
    /// `MemberRefParent`.
    MemberRefParent,
    /// `HasSemantics`.
    HasSemantics,
    /// `MethodDefOrRef`.
    MethodDefOrRef,
    /// `MemberForwarded`.
    MemberForwarded,
    /// `Implementation`.
    Implementation,
    /// `CustomAttributeType`.
    CustomAttributeType,
    /// `ResolutionScope`.
    ResolutionScope,
    /// `TypeOrMethodDef`.
    TypeOrMethodDef,
    /// Portable PDB `HasCustomDebugInformation`.
    HasCustomDebugInformation,
}

impl Coded {
    /// The tables a tag selects, in tag order.
    pub fn tables(self) -> &'static [Option<TableId>] {
        match self {
            Self::TypeDefOrRef => &[Some(0x02), Some(0x01), Some(0x1B)],
            Self::HasConstant => &[Some(0x04), Some(0x08), Some(0x17)],
            Self::HasCustomAttribute => &[
                Some(0x06),
                Some(0x04),
                Some(0x01),
                Some(0x02),
                Some(0x08),
                Some(0x09),
                Some(0x0A),
                Some(0x00),
                Some(0x0E),
                Some(0x17),
                Some(0x14),
                Some(0x11),
                Some(0x1A),
                Some(0x1B),
                Some(0x20),
                Some(0x23),
                Some(0x26),
                Some(0x27),
                Some(0x28),
                Some(0x2A),
                Some(0x2C),
                Some(0x2B),
            ],
            Self::HasFieldMarshal => &[Some(0x04), Some(0x08)],
            Self::HasDeclSecurity => &[Some(0x02), Some(0x06), Some(0x20)],
            Self::MemberRefParent => &[Some(0x02), Some(0x01), Some(0x1A), Some(0x06), Some(0x1B)],
            Self::HasSemantics => &[Some(0x14), Some(0x17)],
            Self::MethodDefOrRef => &[Some(0x06), Some(0x0A)],
            Self::MemberForwarded => &[Some(0x04), Some(0x06)],
            Self::Implementation => &[Some(0x26), Some(0x23), Some(0x27)],
            Self::CustomAttributeType => &[None, None, Some(0x06), Some(0x0A), None],
            Self::ResolutionScope => &[Some(0x00), Some(0x1A), Some(0x23), Some(0x01)],
            Self::TypeOrMethodDef => &[Some(0x02), Some(0x06)],
            Self::HasCustomDebugInformation => &[
                Some(0x06),
                Some(0x04),
                Some(0x01),
                Some(0x02),
                Some(0x08),
                Some(0x09),
                Some(0x0A),
                Some(0x00),
                Some(0x0E),
                Some(0x17),
                Some(0x14),
                Some(0x11),
                Some(0x1A),
                Some(0x1B),
                Some(0x20),
                Some(0x23),
                Some(0x26),
                Some(0x27),
                Some(0x28),
                Some(0x2A),
                Some(0x2C),
                Some(0x2B),
                Some(0x30),
                Some(0x32),
                Some(0x33),
                Some(0x34),
                Some(0x35),
            ],
        }
    }

    /// Bits used by the tag.
    pub fn tag_bits(self) -> u32 {
        let count = u32::try_from(self.tables().len()).unwrap_or(u32::MAX);
        u32::BITS - (count - 1).leading_zeros()
    }

    /// Splits a coded value into the table it names and the 1-based row; `None` for an unused tag.
    pub fn decode(self, value: u32) -> Option<(TableId, u32)> {
        let bits = self.tag_bits();
        let tag = (value & ((1 << bits) - 1)) as usize;
        let table = (*self.tables().get(tag)?)?;
        Some((table, value >> bits))
    }
}

/// A column's storage kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Column {
    /// One byte followed by a padding byte (the `Constant.Type` column).
    Byte2,
    /// Two bytes.
    U16,
    /// Four bytes.
    U32,
    /// An index into `#Strings`.
    Str,
    /// An index into `#GUID`.
    Guid,
    /// An index into `#Blob`.
    Blob,
    /// A simple index into a table.
    Table(TableId),
    /// A coded index.
    Coded(Coded),
}

use Coded as C;
use Column::{Blob, Byte2, Guid, Str, Table as T, U16, U32};

/// The columns of every table this reader knows, indexed by table id.
#[expect(
    clippy::match_same_arms,
    reason = "one arm per ECMA-335 table in table order, so each row can be checked against II.22"
)]
pub fn columns(table: TableId) -> Option<&'static [Column]> {
    let columns: &'static [Column] = match table {
        0x00 => &[U16, Str, Guid, Guid, Guid],
        0x01 => &[Column::Coded(C::ResolutionScope), Str, Str],
        0x02 => &[
            U32,
            Str,
            Str,
            Column::Coded(C::TypeDefOrRef),
            T(0x04),
            T(0x06),
        ],
        0x03 => &[T(0x04)],
        0x04 => &[U16, Str, Blob],
        0x05 => &[T(0x06)],
        0x06 => &[U32, U16, U16, Str, Blob, T(0x08)],
        0x07 => &[T(0x08)],
        0x08 => &[U16, U16, Str],
        0x09 => &[T(0x02), Column::Coded(C::TypeDefOrRef)],
        0x0A => &[Column::Coded(C::MemberRefParent), Str, Blob],
        0x0B => &[Byte2, Column::Coded(C::HasConstant), Blob],
        0x0C => &[
            Column::Coded(C::HasCustomAttribute),
            Column::Coded(C::CustomAttributeType),
            Blob,
        ],
        0x0D => &[Column::Coded(C::HasFieldMarshal), Blob],
        0x0E => &[U16, Column::Coded(C::HasDeclSecurity), Blob],
        0x0F => &[U16, U32, T(0x02)],
        0x10 => &[U32, T(0x04)],
        0x11 => &[Blob],
        0x12 => &[T(0x02), T(0x14)],
        0x13 => &[T(0x14)],
        0x14 => &[U16, Str, Column::Coded(C::TypeDefOrRef)],
        0x15 => &[T(0x02), T(0x17)],
        0x16 => &[T(0x17)],
        0x17 => &[U16, Str, Blob],
        0x18 => &[U16, T(0x06), Column::Coded(C::HasSemantics)],
        0x19 => &[
            T(0x02),
            Column::Coded(C::MethodDefOrRef),
            Column::Coded(C::MethodDefOrRef),
        ],
        0x1A => &[Str],
        0x1B => &[Blob],
        0x1C => &[U16, Column::Coded(C::MemberForwarded), Str, T(0x1A)],
        0x1D => &[U32, T(0x04)],
        0x1E => &[U32, U32],
        0x1F => &[U32],
        0x20 => &[U32, U16, U16, U16, U16, U32, Blob, Str, Str],
        0x21 => &[U32],
        0x22 => &[U32, U32, U32],
        0x23 => &[U16, U16, U16, U16, U32, Blob, Str, Str, Blob],
        0x24 => &[U32, T(0x23)],
        0x25 => &[U32, U32, U32, T(0x23)],
        0x26 => &[U32, Str, Blob],
        0x27 => &[U32, U32, Str, Str, Column::Coded(C::Implementation)],
        0x28 => &[U32, U32, Str, Column::Coded(C::Implementation)],
        0x29 => &[T(0x02), T(0x02)],
        0x2A => &[U16, U16, Column::Coded(C::TypeOrMethodDef), Str],
        0x2B => &[Column::Coded(C::MethodDefOrRef), Blob],
        0x2C => &[T(0x2A), Column::Coded(C::TypeDefOrRef)],
        0x30 => &[Blob, Guid, Blob, Guid],
        0x31 => &[T(0x30), Blob],
        0x32 => &[T(0x06), T(0x35), T(0x33), T(0x34), U32, U32],
        0x33 => &[U16, U16, Str],
        0x34 => &[Str, Blob],
        0x35 => &[T(0x35), Blob],
        0x36 => &[T(0x06), T(0x06)],
        0x37 => &[Column::Coded(C::HasCustomDebugInformation), Guid, Blob],
        _ => return None,
    };
    Some(columns)
}

/// The table stream: row counts and the byte layout of every present table.
#[derive(Debug, Clone)]
pub struct Tables<'a> {
    data: &'a [u8],
    heap_sizes: u8,
    rows: [u32; 64],
    external: [u32; 64],
    offsets: [usize; 64],
    row_sizes: [usize; 64],
}

impl<'a> Tables<'a> {
    /// Parses the stream header and computes every table's position.
    ///
    /// # Errors
    /// When a present table is unknown or the stream is shorter than its tables.
    pub fn parse(data: &'a [u8], external: Option<&[u32; 64]>) -> Read<Self> {
        let mut r = Reader::new(data, 0, "table stream header");
        r.skip(6)?; // reserved, major, minor
        let heap_sizes = r.u8()?;
        r.skip(1)?;
        let valid = r.u64()?;
        r.skip(8)?; // sorted
        let mut rows = [0u32; 64];
        for (table, count) in rows.iter_mut().enumerate() {
            if valid & (1u64 << table) != 0 {
                *count = r.u32()?;
            }
        }
        if heap_sizes & 0x40 != 0 {
            r.skip(4)?; // extra data
        }
        let mut tables = Self {
            data,
            heap_sizes,
            rows,
            external: external.copied().unwrap_or([0; 64]),
            offsets: [0; 64],
            row_sizes: [0; 64],
        };
        let mut offset = r.position();
        for table in 0..64u8 {
            if tables.rows[usize::from(table)] == 0 {
                continue;
            }
            let Some(columns) = columns(table) else {
                return malformed("table stream (unknown table)", offset);
            };
            let size: usize = columns.iter().map(|c| tables.width(*c)).sum();
            let index = usize::from(table);
            tables.offsets[index] = offset;
            tables.row_sizes[index] = size;
            let length = size.checked_mul(tables.rows[index] as usize);
            offset = match length.and_then(|l| offset.checked_add(l)) {
                Some(end) if end <= data.len() => end,
                _ => return malformed("table stream (tables exceed the stream)", offset),
            };
        }
        Ok(tables)
    }

    /// Rows in `table`, counting a table stored in another metadata root (a PDB's references).
    pub fn rows(&self, table: TableId) -> u32 {
        let index = usize::from(table) & 63;
        self.rows[index].max(self.external[index])
    }

    fn width(&self, column: Column) -> usize {
        let wide = |big: bool| if big { 4 } else { 2 };
        match column {
            Byte2 | U16 => 2,
            U32 => 4,
            Str => wide(self.heap_sizes & 0x01 != 0),
            Guid => wide(self.heap_sizes & 0x02 != 0),
            Blob => wide(self.heap_sizes & 0x04 != 0),
            T(table) => wide(self.rows(table) >= 1 << 16),
            Column::Coded(coded) => {
                let limit = 1u32 << (16 - coded.tag_bits());
                let largest = coded
                    .tables()
                    .iter()
                    .flatten()
                    .map(|t| self.rows(*t))
                    .max()
                    .unwrap_or(0);
                wide(largest >= limit)
            }
        }
    }

    /// The value of column `column` of 1-based row `row` of `table`.
    ///
    /// # Errors
    /// When the table is absent, the row is out of range, or the column does not exist.
    pub fn cell(&self, table: TableId, row: u32, column: usize) -> Read<u32> {
        let index = usize::from(table) & 63;
        let Some(columns) = columns(table) else {
            return malformed("table (unknown)", 0);
        };
        if row == 0 || row > self.rows[index] || column >= columns.len() {
            return malformed("table row", self.offsets[index]);
        }
        let skip: usize = columns[..column].iter().map(|c| self.width(*c)).sum();
        // row >= 1 and row <= rows were checked above, and parse() bounded every table's extent.
        let at = self.offsets[index] + (row as usize - 1) * self.row_sizes[index] + skip;
        let mut r = Reader::new(self.data, at, "table cell");
        match columns[column] {
            Byte2 | U16 => r.u16().map(u32::from),
            U32 => r.u32(),
            other => r.index(self.width(other) == 4),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A table stream holding `tables` as (id, rows of raw cells) with 2-byte indices.
    pub(crate) fn table_stream(tables: &[(TableId, Vec<Vec<u32>>)], heap_sizes: u8) -> Vec<u8> {
        let mut data = vec![0, 0, 0, 0, 2, 0, heap_sizes, 1];
        let valid = tables.iter().fold(0u64, |v, (t, _)| v | (1u64 << t));
        data.extend(valid.to_le_bytes());
        data.extend(0u64.to_le_bytes());
        let mut sorted: Vec<_> = tables.to_vec();
        sorted.sort_by_key(|(t, _)| *t);
        for (_, rows) in &sorted {
            data.extend(u32::try_from(rows.len()).unwrap_or(0).to_le_bytes());
        }
        let probe = Tables {
            data: &[],
            heap_sizes,
            rows: {
                let mut r = [0u32; 64];
                for (t, rows) in &sorted {
                    r[usize::from(*t)] = u32::try_from(rows.len()).unwrap_or(0);
                }
                r
            },
            external: [0; 64],
            offsets: [0; 64],
            row_sizes: [0; 64],
        };
        for (table, rows) in &sorted {
            let columns = columns(*table).unwrap_or_default();
            for row in rows {
                for (value, column) in row.iter().zip(columns) {
                    match probe.width(*column) {
                        4 => data.extend(value.to_le_bytes()),
                        _ => data.extend(u16::try_from(*value).unwrap_or(0).to_le_bytes()),
                    }
                }
            }
        }
        data
    }

    #[test]
    fn every_table_up_to_0x37_has_columns_and_the_gaps_do_not() {
        for table in 0..=0x2C {
            assert!(columns(table).is_some(), "{table:#x}");
        }
        for table in 0x30..=0x37 {
            assert!(columns(table).is_some(), "{table:#x}");
        }
        for table in [0x2D, 0x2E, 0x2F, 0x38, 0x3F] {
            assert!(columns(table).is_none(), "{table:#x}");
        }
    }

    #[test]
    fn coded_indices_use_the_specified_tag_widths() {
        assert_eq!(Coded::TypeDefOrRef.tag_bits(), 2);
        assert_eq!(Coded::HasCustomAttribute.tag_bits(), 5);
        assert_eq!(Coded::CustomAttributeType.tag_bits(), 3);
        assert_eq!(Coded::MemberRefParent.tag_bits(), 3);
        assert_eq!(Coded::MethodDefOrRef.tag_bits(), 1);
        assert_eq!(Coded::HasCustomDebugInformation.tag_bits(), 5);
        assert_eq!(
            Coded::TypeDefOrRef.decode((7 << 2) | 1),
            Some((id::TYPE_REF, 7))
        );
        assert_eq!(
            Coded::CustomAttributeType.decode((3 << 3) | 3),
            Some((id::MEMBER_REF, 3))
        );
        assert_eq!(Coded::CustomAttributeType.decode(1), None);
        assert_eq!(
            Coded::HasCustomAttribute.decode((2 << 5) | 3),
            Some((id::TYPE_DEF, 2))
        );
        assert_eq!(Coded::HasCustomAttribute.decode(31), None);
    }

    #[test]
    fn reads_cells_and_sizes_indices_by_row_count() {
        // Two TypeRefs and one TypeDef.
        let stream = table_stream(
            &[
                (id::TYPE_REF, vec![vec![6, 1, 2], vec![6, 3, 4]]),
                (
                    id::TYPE_DEF,
                    vec![vec![0x0010_0001, 5, 6, (2 << 2) | 1, 1, 1]],
                ),
            ],
            0,
        );
        let tables = Tables::parse(&stream, None);
        let tables = tables.as_ref().ok();
        assert_eq!(tables.map(|t| t.rows(id::TYPE_REF)), Some(2));
        assert_eq!(
            tables.and_then(|t| t.cell(id::TYPE_REF, 2, 1).ok()),
            Some(3)
        );
        assert_eq!(
            tables.and_then(|t| t.cell(id::TYPE_DEF, 1, 0).ok()),
            Some(0x0010_0001)
        );
        assert_eq!(
            tables.and_then(|t| t.cell(id::TYPE_DEF, 1, 3).ok()),
            Some(9)
        );
        assert!(tables.is_some_and(|t| t.cell(id::TYPE_DEF, 2, 0).is_err()));
        assert!(tables.is_some_and(|t| t.cell(id::TYPE_DEF, 0, 0).is_err()));
        assert!(tables.is_some_and(|t| t.cell(id::TYPE_DEF, 1, 6).is_err()));
        assert!(tables.is_some_and(|t| t.cell(0x2E, 1, 0).is_err()));
    }

    #[test]
    fn wide_heaps_and_external_rows_widen_columns() {
        let stream = table_stream(&[(id::FIELD, vec![vec![1, 70_000, 2]])], 0x01);
        let tables = Tables::parse(&stream, None);
        assert_eq!(
            tables
                .as_ref()
                .ok()
                .and_then(|t| t.cell(id::FIELD, 1, 1).ok()),
            Some(70_000)
        );
        assert_eq!(
            tables.ok().and_then(|t| t.cell(id::FIELD, 1, 2).ok()),
            Some(2)
        );
        // A PDB whose MethodDebugInformation references a Document table and a large MethodDef.
        let mut external = [0u32; 64];
        external[usize::from(id::METHOD_DEF)] = 100_000;
        let stream = table_stream(&[(id::DOCUMENT, vec![vec![1, 1, 1, 1]])], 0);
        let tables = Tables::parse(&stream, Some(&external));
        assert_eq!(
            tables.as_ref().ok().map(|t| t.rows(id::METHOD_DEF)),
            Some(100_000)
        );
        assert_eq!(tables.ok().map(|t| t.width(T(id::METHOD_DEF))), Some(4));
    }

    #[test]
    fn rejects_unknown_tables_and_short_streams() {
        let mut stream = table_stream(&[(id::FIELD, vec![vec![1, 1, 1]])], 0);
        assert!(Tables::parse(&stream[..stream.len() - 1], None).is_err());
        stream[8] |= 1 << 5; // mark table 0x2D present... in the high byte region
        let mut unknown = table_stream(&[], 0);
        unknown[8 + 5] = 0x20; // bit 0x2D
        unknown.extend(1u32.to_le_bytes());
        assert!(Tables::parse(&unknown, None).is_err());
        let with_extra = {
            let mut s = table_stream(&[], 0x40);
            s.extend([0, 0, 0, 0]);
            s
        };
        assert!(Tables::parse(&with_extra, None).is_ok());
        assert!(Tables::parse(&[0; 10], None).is_err());
    }
}
