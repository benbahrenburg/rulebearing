//! An assembly's types, read from its metadata: names, nesting, method ranges, and whether the
//! compiler generated them.
//!
//! - Specification: ECMA-335 II.22.37 (`TypeDef`), II.22.32 (`NestedClass`), II.22.10
//!   (`CustomAttribute`), II.23.3 (custom attribute value blobs)
//! - Plan: [Wave 0, Step 9](../../../docs/plans/pending/0000-wave-0-spike.md#step-9-spike-b-rb-extract-dotnet-0d)
//!   and § 1.6 (the attribution denominator excludes `<Module>` and compiler-generated types)
//! - Decision: [ADR-0011](../../../docs/adr/0011-read-dotnet-assemblies-not-source.md)

use std::collections::BTreeMap;
use std::ops::Range;

use crate::bytes::{Read, Reader};
use crate::metadata::Metadata;
use crate::metadata::tables::{Coded, id};
use crate::pe::{DebugInfo, PeImage};

/// One `TypeDef` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeInfo {
    /// 1-based `TypeDef` row.
    pub row: u32,
    /// Namespace; for a nested type, the outermost type's namespace.
    pub namespace: String,
    /// Simple name, generic arity suffix included (``List`1``).
    pub name: String,
    /// `Namespace.Outer+Inner`, the form ArchUnitNET and reflection print.
    pub full_name: String,
    /// The enclosing type's row, for a nested type.
    pub enclosing: Option<u32>,
    /// `MethodDef` rows of the type's methods.
    pub methods: Range<u32>,
    /// The `MethodDef` row of the type's first instance constructor (`.ctor`), if it has one.
    pub first_constructor: Option<u32>,
    /// Whether the type carries `CompilerGeneratedAttribute`.
    pub compiler_generated: bool,
    /// Whether the type is `<Module>`, the pseudo-type holding global members.
    pub is_module_type: bool,
}

/// What the reader learns from one assembly.
#[derive(Debug, Clone)]
pub struct Assembly {
    /// The assembly name from the `Assembly` table (the module name without `.dll` otherwise).
    pub name: String,
    /// The `TargetFrameworkAttribute` value, for example `.NETCoreApp,Version=v10.0`.
    pub target_framework: Option<String>,
    /// Every type, in row order.
    pub types: Vec<TypeInfo>,
    /// Debug directory entries.
    pub debug: Vec<DebugInfo>,
}

const COMPILER_GENERATED: (&str, &str) = (
    "System.Runtime.CompilerServices",
    "CompilerGeneratedAttribute",
);
const TARGET_FRAMEWORK: (&str, &str) = ("System.Runtime.Versioning", "TargetFrameworkAttribute");

impl Assembly {
    /// Reads an assembly file's metadata.
    ///
    /// # Errors
    /// When the PE image or the metadata is malformed.
    pub fn read(bytes: &[u8]) -> Read<Self> {
        let image = PeImage::parse(bytes)?;
        let metadata = Metadata::parse(image.metadata, None)?;
        let reader = TypeReader {
            metadata: &metadata,
        };
        let (generated, target_framework) = reader.attributes()?;
        let types = reader.types(&generated)?;
        let name = if metadata.rows(id::ASSEMBLY) > 0 {
            metadata
                .string(metadata.tables.cell(id::ASSEMBLY, 1, 7)?)?
                .to_owned()
        } else {
            let module = metadata.string(metadata.tables.cell(id::MODULE, 1, 1)?)?;
            module
                .trim_end_matches(".dll")
                .trim_end_matches(".exe")
                .to_owned()
        };
        Ok(Self {
            name,
            target_framework,
            types,
            debug: image.debug,
        })
    }
}

struct TypeReader<'m, 'a> {
    metadata: &'m Metadata<'a>,
}

impl TypeReader<'_, '_> {
    /// The namespace and name of the type a `TypeDefOrRef`-style (table, row) pair names.
    fn type_name(&self, table: u8, row: u32) -> Read<(String, String)> {
        let t = &self.metadata.tables;
        let (name, namespace) = match table {
            id::TYPE_REF | id::TYPE_DEF => (t.cell(table, row, 1)?, t.cell(table, row, 2)?),
            _ => return Ok((String::new(), String::new())),
        };
        Ok((
            self.metadata.string(namespace)?.to_owned(),
            self.metadata.string(name)?.to_owned(),
        ))
    }

    /// The type that declares a custom attribute constructor.
    fn attribute_type(&self, constructor: u32, starts: &[u32]) -> Read<Option<(String, String)>> {
        let t = &self.metadata.tables;
        match Coded::CustomAttributeType.decode(constructor) {
            Some((id::MEMBER_REF, row)) => {
                let parent = t.cell(id::MEMBER_REF, row, 0)?;
                match Coded::MemberRefParent.decode(parent) {
                    Some((table, row)) => self.type_name(table, row).map(Some),
                    None => Ok(None),
                }
            }
            Some((id::METHOD_DEF, row)) => Self::owner_of_method(starts, row)
                .map(|owner| self.type_name(id::TYPE_DEF, owner))
                .transpose(),
            _ => Ok(None),
        }
    }

    fn method_start(&self, type_row: u32) -> Read<u32> {
        self.metadata.tables.cell(id::TYPE_DEF, type_row, 5)
    }

    /// Every type's first method row, in type order; the list is non-decreasing (II.22.37).
    fn method_starts(&self) -> Read<Vec<u32>> {
        (1..=self.metadata.rows(id::TYPE_DEF))
            .map(|row| self.method_start(row))
            .collect()
    }

    /// The type that owns method row `method`: the last type whose method list starts at or
    /// before it. A binary search, so an assembly with many attributes stays linear overall.
    fn owner_of_method(starts: &[u32], method: u32) -> Option<u32> {
        let owners = starts.partition_point(|start| *start <= method);
        u32::try_from(owners).ok().filter(|row| *row > 0)
    }

    /// Rows of types carrying `CompilerGeneratedAttribute`, and the target framework.
    fn attributes(&self) -> Read<(Vec<u32>, Option<String>)> {
        let t = &self.metadata.tables;
        let mut generated = Vec::new();
        let mut framework = None;
        let starts = self.method_starts()?;
        for row in 1..=self.metadata.rows(id::CUSTOM_ATTRIBUTE) {
            let parent = Coded::HasCustomAttribute.decode(t.cell(id::CUSTOM_ATTRIBUTE, row, 0)?);
            let Some(declaring) =
                self.attribute_type(t.cell(id::CUSTOM_ATTRIBUTE, row, 1)?, &starts)?
            else {
                continue;
            };
            let declaring = (declaring.0.as_str(), declaring.1.as_str());
            match parent {
                Some((id::TYPE_DEF, type_row)) if declaring == COMPILER_GENERATED => {
                    generated.push(type_row);
                }
                Some((id::ASSEMBLY, _)) if declaring == TARGET_FRAMEWORK => {
                    let value = self.metadata.blob(t.cell(id::CUSTOM_ATTRIBUTE, row, 2)?)?;
                    framework = first_string_argument(value)?;
                }
                _ => {}
            }
        }
        Ok((generated, framework))
    }

    fn types(&self, generated: &[u32]) -> Read<Vec<TypeInfo>> {
        let t = &self.metadata.tables;
        let count = self.metadata.rows(id::TYPE_DEF);
        let method_count = self.metadata.rows(id::METHOD_DEF);
        let mut enclosing = BTreeMap::new();
        for row in 1..=self.metadata.rows(id::NESTED_CLASS) {
            enclosing.insert(
                t.cell(id::NESTED_CLASS, row, 0)?,
                t.cell(id::NESTED_CLASS, row, 1)?,
            );
        }
        let mut types = Vec::with_capacity(count as usize);
        for row in 1..=count {
            let (namespace, name) = self.type_name(id::TYPE_DEF, row)?;
            let start = self.method_start(row)?;
            let end = if row < count {
                self.method_start(row + 1)?
            } else {
                method_count + 1
            };
            let mut first_constructor = None;
            for method in start..end.max(start) {
                if self.metadata.string(t.cell(id::METHOD_DEF, method, 3)?)? == ".ctor" {
                    first_constructor = Some(method);
                    break;
                }
            }
            types.push(TypeInfo {
                row,
                first_constructor,
                is_module_type: row == 1 && name == "<Module>",
                namespace,
                full_name: String::new(),
                name,
                enclosing: enclosing.get(&row).copied(),
                methods: start..end.max(start),
                compiler_generated: generated.contains(&row),
            });
        }
        // Full names walk the nesting chain; a malformed cycle stops at the type count.
        for index in 0..types.len() {
            let mut parts = vec![types[index].name.clone()];
            let mut namespace = types[index].namespace.clone();
            let mut current = types[index].enclosing;
            let mut steps = 0;
            while let Some(outer) = current.and_then(|r| row_index(r).and_then(|i| types.get(i))) {
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
        Ok(types)
    }
}

/// The 0-based index of a 1-based metadata row; `None` for the nil row 0, which a malformed
/// table can name anywhere a row is expected.
pub fn row_index(row: u32) -> Option<usize> {
    (row as usize).checked_sub(1)
}

/// The first fixed string argument of a custom attribute value blob (II.23.3).
fn first_string_argument(value: &[u8]) -> Read<Option<String>> {
    let mut r = Reader::new(value, 0, "custom attribute value");
    if r.u16()? != 1 {
        return Ok(None);
    }
    if value.get(2) == Some(&0xFF) {
        return Ok(None); // null string
    }
    let length = r.compressed_u32()? as usize;
    let bytes = r.bytes(length)?;
    Ok(std::str::from_utf8(bytes).ok().map(str::to_owned))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_method_belongs_to_the_last_type_starting_at_or_before_it() {
        // Types 1..=4 start their method lists at rows 1, 1, 3 and 6 (type 1 has none).
        let starts = [1, 1, 3, 6];
        assert_eq!(TypeReader::owner_of_method(&starts, 1), Some(2));
        assert_eq!(TypeReader::owner_of_method(&starts, 2), Some(2));
        assert_eq!(TypeReader::owner_of_method(&starts, 3), Some(3));
        assert_eq!(TypeReader::owner_of_method(&starts, 5), Some(3));
        assert_eq!(TypeReader::owner_of_method(&starts, 9), Some(4));
        assert_eq!(TypeReader::owner_of_method(&starts, 0), None);
        assert_eq!(TypeReader::owner_of_method(&[], 1), None);
    }

    #[test]
    fn row_zero_has_no_index() {
        assert_eq!(row_index(0), None);
        assert_eq!(row_index(1), Some(0));
    }

    #[test]
    fn reads_a_string_argument_or_nothing() {
        let mut value = vec![1, 0, 4];
        value.extend(b"abcd");
        value.extend([0, 0]);
        assert_eq!(first_string_argument(&value), Ok(Some("abcd".to_owned())));
        assert_eq!(first_string_argument(&[1, 0, 0xFF]), Ok(None));
        assert_eq!(first_string_argument(&[2, 0]), Ok(None));
        assert!(first_string_argument(&[1, 0, 9, b'x']).is_err());
    }
}
