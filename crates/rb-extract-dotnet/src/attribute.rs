//! Attributing each type of an assembly to the source file that declares it.
//!
//! - Decision: [ADR-0011](../../../docs/adr/0011-read-dotnet-assemblies-not-source.md) (`pdb`,
//!   `inferred`, `none`); [ADR-0003](../../../docs/adr/0003-dotnet-extractor-fallback.md) (the 99%
//!   trigger this measures)
//! - Plan: [Wave 0, Step 9](../../../docs/plans/pending/0000-wave-0-spike.md#step-9-spike-b-rb-extract-dotnet-0d)
//!   (`attribute.rs`) and § 1.6 (the denominator)
//! - Source: [design § One engine](../../../docs/artifacts/design.md#one-engine-three-languages-one-monorepo),
//!   row Module identity (`/_/` prefixes unmapped)
//!
//! In order, a type is attributed:
//!
//! 1. `pdb`: by the portable PDB, from Roslyn's `TypeDefinitionDocuments` record (written for types
//!    with no method bodies) or the first visible sequence point of any of its methods;
//! 2. `inferred`: a nested type with no sequence point of its own takes its enclosing type's file,
//!    because C# declares a nested type inside its enclosing type's body;
//! 3. `inferred`: by convention, when exactly one `<TypeName>.cs` exists under the project (or
//!    several do and one sits in the folder its namespace names);
//! 4. otherwise `none`.
//!
//! Only a portable PDB permits 2 and 3: behind a Windows PDB or no PDB every type is `none`, since a
//! user's run would hit the same wall (§ 1.6). The denominator rule is the one the plan fixed before
//! measuring: `<Module>` and types carrying `CompilerGeneratedAttribute` are excluded from the
//! adjusted ratio and counted in the raw one. Compiler-synthesised types that do not carry the
//! attribute themselves (a nested `<PrivateImplementationDetails>+__StaticArrayInitTypeSize=88`,
//! ``<>y__InlineArray2`1``) stay in the denominator and count against the reader.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rb_model::Attribution;
use serde::Serialize;

use crate::assembly::{Assembly, TypeInfo, row_index};
use crate::bytes::ReadError;
use crate::pdb::{PdbError, PortablePdb};
use crate::pe::DebugInfo;

/// What kind of debugging information an assembly had.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PdbKind {
    /// A portable PDB file beside the assembly.
    Portable,
    /// A portable PDB embedded in the assembly.
    Embedded,
    /// A Windows (MSF) PDB.
    Windows,
    /// No PDB.
    Missing,
}

/// One type's attribution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeAttribution {
    /// `Namespace.Outer+Inner`.
    pub full_name: String,
    /// The namespace.
    pub namespace: String,
    /// `pdb`, `inferred` or `none`; absent for an excluded type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attribution: Option<Attribution>,
    /// Repository-relative file, posix-separated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// 1-based line of the first sequence point, for `pdb`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
}

/// Counts over a set of types.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    /// Every `TypeDef` row.
    pub types_total: u32,
    /// `<Module>` and compiler-generated types.
    pub types_excluded: u32,
    /// Attributed by the PDB.
    pub pdb_attributed: u32,
    /// Attributed by nesting or naming convention.
    pub inferred: u32,
    /// Not attributed.
    pub none: u32,
}

impl Counts {
    /// Adds another set.
    pub fn add(&mut self, other: &Self) {
        self.types_total += other.types_total;
        self.types_excluded += other.types_excluded;
        self.pdb_attributed += other.pdb_attributed;
        self.inferred += other.inferred;
        self.none += other.none;
    }

    /// Attributed over every type.
    pub fn raw(&self) -> f64 {
        ratio(self.pdb_attributed + self.inferred, self.types_total)
    }

    /// Attributed over the types a tool could attribute: the trigger figure (ADR-0003).
    pub fn adjusted(&self) -> f64 {
        ratio(
            self.pdb_attributed + self.inferred,
            self.types_total - self.types_excluded,
        )
    }
}

fn ratio(numerator: u32, denominator: u32) -> f64 {
    if denominator == 0 {
        1.0
    } else {
        f64::from(numerator) / f64::from(denominator)
    }
}

/// One assembly's result.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssemblyAttribution {
    /// Assembly name.
    pub assembly: String,
    /// The assembly file, repository-relative.
    pub path: String,
    /// `TargetFrameworkAttribute`, when present.
    pub target_framework: Option<String>,
    /// Debugging information found.
    pub pdb: PdbKind,
    /// The counts.
    #[serde(flatten)]
    pub counts: Counts,
    /// Every type with its attribution.
    pub types: Vec<TypeAttribution>,
}

/// Why an assembly could not be attributed at all.
#[derive(Debug, thiserror::Error)]
pub enum AttributeError {
    /// Reading a file failed.
    #[error("{path}: {source}", path = path.display())]
    Io {
        /// The file.
        path: PathBuf,
        /// The error.
        source: std::io::Error,
    },
    /// The assembly or its portable PDB is malformed.
    #[error("{path}: {source}", path = path.display())]
    Malformed {
        /// The file.
        path: PathBuf,
        /// What was malformed.
        source: ReadError,
    },
}

/// Maps a document path from the PDB to a repository-relative path: deterministic builds write
/// `/_/` for the repository root (`SourceRoot`), other builds write absolute paths.
pub fn normalise_document(document: &str, repository: &Path) -> String {
    let posix = document.replace('\\', "/");
    if let Some(rest) = posix.strip_prefix("/_/") {
        return rest.to_owned();
    }
    let root = repository.to_string_lossy().replace('\\', "/");
    let root = root.trim_end_matches('/');
    match posix.strip_prefix(root) {
        Some(rest) if rest.starts_with('/') => rest.trim_start_matches('/').to_owned(),
        _ => posix,
    }
}

/// `.cs` files under a project folder, by file stem, skipping build output.
fn source_index(project: &Path) -> BTreeMap<String, Vec<PathBuf>> {
    fn walk(dir: &Path, index: &mut BTreeMap<String, Vec<PathBuf>>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            // Not through symlinks: a cycle would recurse without end.
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                if !matches!(name.as_ref(), "bin" | "obj" | "node_modules" | ".git") {
                    walk(&path, index);
                }
            } else if let Some(stem) = name.strip_suffix(".cs") {
                index.entry(stem.to_owned()).or_default().push(path);
            }
        }
    }
    let mut index = BTreeMap::new();
    walk(project, &mut index);
    for paths in index.values_mut() {
        paths.sort();
    }
    index
}

/// Picks the file a type's name points at: the only one, or the one in its namespace's folder.
fn by_convention<'p>(
    ty: &TypeInfo,
    index: &'p BTreeMap<String, Vec<PathBuf>>,
) -> Option<&'p PathBuf> {
    let simple = ty.name.split('`').next().unwrap_or(&ty.name);
    let candidates = index.get(simple)?;
    if let [only] = candidates.as_slice() {
        return Some(only);
    }
    let folders: Vec<&str> = ty.namespace.split('.').collect();
    let matching: Vec<&PathBuf> = candidates
        .iter()
        .filter(|path| {
            let parents: Vec<String> = path
                .parent()
                .into_iter()
                .flat_map(Path::components)
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect();
            folders
                .last()
                .is_some_and(|last| parents.last().is_some_and(|p| p == last))
        })
        .collect();
    match matching.as_slice() {
        [only] => Some(only),
        _ => None,
    }
}

fn io_error(path: &Path) -> impl FnOnce(std::io::Error) -> AttributeError + '_ {
    move |source| AttributeError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// The assembly's debugging information: an embedded portable PDB, a PDB file beside it, or none.
fn debug_info(
    dll: &Path,
    assembly: &Assembly,
) -> Result<(PdbKind, Option<Vec<u8>>), AttributeError> {
    let embedded = assembly.debug.iter().find_map(|d| match d {
        DebugInfo::Embedded { pdb } => Some(pdb.clone()),
        DebugInfo::CodeView { .. } => None,
    });
    if let Some(pdb) = embedded {
        return Ok((PdbKind::Embedded, Some(pdb)));
    }
    let beside = dll.with_extension("pdb");
    if !beside.is_file() {
        return Ok((PdbKind::Missing, None));
    }
    let pdb = std::fs::read(&beside).map_err(io_error(&beside))?;
    Ok(if pdb.starts_with(b"BSJB") {
        (PdbKind::Portable, Some(pdb))
    } else {
        (PdbKind::Windows, None)
    })
}

/// Pass 1: attribution from the portable PDB, when there is one.
fn from_pdb(
    assembly: &Assembly,
    pdb: Option<&PortablePdb<'_>>,
    repository: &Path,
) -> Result<Vec<TypeAttribution>, ReadError> {
    let documents = pdb
        .map(PortablePdb::documents)
        .transpose()?
        .unwrap_or_default();
    let type_documents = pdb
        .map(PortablePdb::type_definition_documents)
        .transpose()?
        .unwrap_or_default();
    let document = |row: u32| {
        documents
            .get((row as usize).wrapping_sub(1))
            .map(|d| normalise_document(d, repository))
    };
    let mut results = Vec::with_capacity(assembly.types.len());
    for ty in &assembly.types {
        let excluded = ty.is_module_type || ty.compiler_generated;
        let mut found = TypeAttribution {
            full_name: ty.full_name.clone(),
            namespace: ty.namespace.clone(),
            attribution: (!excluded).then_some(Attribution::None),
            file: None,
            line: None,
        };
        if let (false, Some(pdb)) = (excluded, pdb) {
            let declared = type_documents
                .get(&ty.row)
                .and_then(|docs| docs.first())
                .and_then(|row| document(*row));
            if let Some(file) = declared {
                found.attribution = Some(Attribution::Pdb);
                found.file = Some(file);
            } else {
                for method in ty.methods.clone() {
                    if let Some(point) = pdb.first_point(method)? {
                        found.attribution = Some(Attribution::Pdb);
                        found.file = document(point.document);
                        found.line = Some(point.line);
                        break;
                    }
                }
            }
        }
        results.push(found);
    }
    Ok(results)
}

/// Pass 2: a nested type with no sequence point takes its enclosing type's file.
fn from_enclosing(assembly: &Assembly, results: &mut [TypeAttribution]) {
    for index in 0..results.len() {
        if results[index].attribution != Some(Attribution::None) {
            continue;
        }
        let mut outer = assembly.types[index].enclosing;
        let mut steps = 0;
        while let Some(row) = outer {
            let Some(candidate) = row_index(row).and_then(|i| results.get(i)) else {
                break;
            };
            if let (Some(Attribution::Pdb | Attribution::Inferred), Some(file)) =
                (candidate.attribution, &candidate.file)
            {
                let file = file.clone();
                results[index].attribution = Some(Attribution::Inferred);
                results[index].file = Some(file);
                break;
            }
            outer = row_index(row)
                .and_then(|i| assembly.types.get(i))
                .and_then(|t| t.enclosing);
            steps += 1;
            if steps > results.len() {
                break;
            }
        }
    }
}

/// Pass 3: the naming convention.
fn from_convention(
    assembly: &Assembly,
    results: &mut [TypeAttribution],
    project: &Path,
    repository: &Path,
) {
    let index = source_index(project);
    for (ty, result) in assembly.types.iter().zip(results.iter_mut()) {
        if result.attribution != Some(Attribution::None) {
            continue;
        }
        if let Some(path) = by_convention(ty, &index) {
            result.attribution = Some(Attribution::Inferred);
            result.file = Some(normalise_document(&path.to_string_lossy(), repository));
        }
    }
}

fn count(results: &[TypeAttribution]) -> Counts {
    let mut counts = Counts {
        types_total: u32::try_from(results.len()).unwrap_or(u32::MAX),
        ..Counts::default()
    };
    for result in results {
        match result.attribution {
            None => counts.types_excluded += 1,
            Some(Attribution::Pdb) => counts.pdb_attributed += 1,
            Some(Attribution::Inferred) => counts.inferred += 1,
            Some(Attribution::None) => counts.none += 1,
        }
    }
    counts
}

/// Attributes every type of the assembly at `dll`.
///
/// # Errors
/// When the assembly cannot be read, or a portable PDB is present but malformed.
pub fn attribute_assembly(
    dll: &Path,
    project: &Path,
    repository: &Path,
) -> Result<AssemblyAttribution, AttributeError> {
    let bytes = std::fs::read(dll).map_err(io_error(dll))?;
    let assembly = Assembly::read(&bytes).map_err(|source| AttributeError::Malformed {
        path: dll.to_path_buf(),
        source,
    })?;
    let (kind, pdb_bytes) = debug_info(dll, &assembly)?;
    let pdb_path = dll.with_extension("pdb");
    let malformed = |source| AttributeError::Malformed {
        path: pdb_path.clone(),
        source,
    };
    let pdb = match pdb_bytes.as_deref().map(PortablePdb::parse) {
        Some(Ok(pdb)) => Some(pdb),
        Some(Err(PdbError::Malformed(source))) => return Err(malformed(source)),
        Some(Err(PdbError::NotPortable)) | None => None,
    };
    let mut results = from_pdb(&assembly, pdb.as_ref(), repository).map_err(malformed)?;
    if pdb.is_some() {
        from_enclosing(&assembly, &mut results);
        from_convention(&assembly, &mut results, project, repository);
    }
    Ok(AssemblyAttribution {
        assembly: assembly.name,
        path: normalise_document(&dll.to_string_lossy(), repository),
        target_framework: assembly.target_framework,
        pdb: kind,
        counts: count(&results),
        types: results,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn the_source_index_does_not_follow_a_symlink_cycle() {
        let root = std::env::temp_dir().join(format!("rb-sources-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::create_dir_all(root.join("Domain"));
        let _ = std::fs::write(root.join("Domain/Order.cs"), "");
        let _ = std::os::unix::fs::symlink(&root, root.join("Domain/loop"));
        let index = source_index(&root);
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(index.keys().collect::<Vec<_>>(), ["Order"]);
        assert_eq!(index.get("Order").map(Vec::len), Some(1));
    }

    #[test]
    fn normalises_deterministic_and_absolute_paths() {
        let repo = Path::new("/work/repo");
        assert_eq!(normalise_document("/_/src/A.cs", repo), "src/A.cs");
        assert_eq!(normalise_document("/work/repo/src/B.cs", repo), "src/B.cs");
        assert_eq!(
            normalise_document("/work/repository/C.cs", repo),
            "/work/repository/C.cs"
        );
        assert_eq!(normalise_document("C:\\x\\D.cs", Path::new("C:/x")), "D.cs");
    }

    #[test]
    fn ratios_count_exclusions_only_in_the_adjusted_figure() {
        let counts = Counts {
            types_total: 10,
            types_excluded: 2,
            pdb_attributed: 6,
            inferred: 1,
            none: 1,
        };
        assert!((counts.raw() - 0.7).abs() < 1e-9);
        assert!((counts.adjusted() - 0.875).abs() < 1e-9);
        let mut sum = Counts::default();
        sum.add(&counts);
        sum.add(&counts);
        assert_eq!(sum.types_total, 20);
        assert_eq!(sum.none, 2);
        assert!((Counts::default().adjusted() - 1.0).abs() < 1e-9);
    }

    fn ty(name: &str, namespace: &str) -> TypeInfo {
        TypeInfo {
            row: 2,
            namespace: namespace.to_owned(),
            name: name.to_owned(),
            full_name: format!("{namespace}.{name}"),
            enclosing: None,
            methods: 1..1,
            compiler_generated: false,
            is_module_type: false,
        }
    }

    #[test]
    fn convention_needs_a_unique_or_namespace_matched_file() {
        let mut index = BTreeMap::new();
        index.insert("IRepo".to_owned(), vec![PathBuf::from("p/Data/IRepo.cs")]);
        index.insert(
            "Status".to_owned(),
            vec![
                PathBuf::from("p/Orders/Status.cs"),
                PathBuf::from("p/Users/Status.cs"),
            ],
        );
        assert_eq!(
            by_convention(&ty("IRepo", "App.Data"), &index),
            Some(&PathBuf::from("p/Data/IRepo.cs"))
        );
        assert_eq!(by_convention(&ty("List`1", "X"), &index), None);
        assert_eq!(
            by_convention(&ty("Status", "App.Users"), &index),
            Some(&PathBuf::from("p/Users/Status.cs"))
        );
        assert_eq!(by_convention(&ty("Status", "App.Billing"), &index), None);
    }
}
