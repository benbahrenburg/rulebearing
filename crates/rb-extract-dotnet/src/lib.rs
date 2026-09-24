//! `rb-extract-dotnet`: the .NET extractor over ECMA-335 metadata, IL operands and portable PDBs.
//!
//! - Architecture: [`docs/architecture.md#extractors`](../../../docs/architecture.md#extractors)
//! - Decisions: [ADR-0011](../../../docs/adr/0011-read-dotnet-assemblies-not-source.md),
//!   [ADR-0003](../../../docs/adr/0003-dotnet-extractor-fallback.md) (the C# fallback trigger)
//! - Plans: [Wave 0, Spike B](../../../docs/plans/pending/0000-wave-0-spike.md),
//!   [Wave 2, sub-wave 2A](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md),
//!   [Wave 3](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md) (`--mode source`)
//! - Requirements: [FR-EXT-DN-01](../../../docs/prd.md#fr-ext-dn-01) to [FR-EXT-DN-04](../../../docs/prd.md#fr-ext-dn-04)
//! - Specification: ECMA-335 partition II; the portable PDB format; `ArchUnitNET`'s `TestAssembly`
//!   ([ADR-0009](../../../docs/adr/0009-conformance-suites-as-specification.md))
//!
//! Rule of the boundary: this crate reads assemblies and PDBs and writes `rb_model` types only.
//!
//! Wave 0 (Spike B) builds the reader far enough to attribute every type to a source file and
//! measure the share it attributes, the figure [ADR-0003](../../../docs/adr/0003-dotnet-extractor-fallback.md)
//! decides on. The edge set (the IL operand scan, the code layer) is wave 2.
//!
//! | Module | Reads |
//! | --- | --- |
//! | [`bytes`] | bounds-checked little-endian reads and compressed integers |
//! | [`pe`] | PE headers, sections, the CLI header, the debug directory |
//! | [`metadata`] | the metadata root, heaps and the table stream |
//! | [`assembly`] | types, nesting, method ranges, compiler-generated markers |
//! | [`pdb`] | portable PDB documents and sequence points |
//! | [`attribute`] | one type to one file: `pdb`, `inferred`, `none` |
//! | [`msbuild`] | solution and project files, to find the built assemblies |
//!
//! Every reader returns an error naming the structure and offset on malformed input; the fuzz
//! target `fuzz/fuzz_targets/metadata_reader.rs` and a truncation test hold it to that.

pub mod assembly;
pub mod attribute;
pub mod bytes;
pub mod metadata;
pub mod msbuild;
pub mod pdb;
pub mod pe;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use rb_model::{
    Attribution, DotnetOptions, ExtractError, Extraction, Extractor, Language, Module, Receipt,
    Warning,
};
use serde::Serialize;

use attribute::{AssemblyAttribution, Counts, PdbKind, attribute_assembly};
use msbuild::{Project, SolutionError, solution_projects};

/// The edge kinds this extractor records, from
/// [design § Dependency rules](../../../docs/artifacts/design.md#dependency-rules-the-whole-of-dependency-cruiser-1820).
pub const DEPENDENCY_KINDS: &[&str] = &[
    "inherits",
    "implements",
    "field",
    "signature",
    "body",
    "attribute",
    "generic-argument",
    "typeof",
];

/// The `dependencyTypes` vocabulary for .NET, from
/// [design § One engine, three languages](../../../docs/artifacts/design.md#one-engine-three-languages-one-monorepo).
pub const DEPENDENCY_TYPES: &[&str] = &[
    "local",
    "project",
    "package",
    "framework",
    "test-only",
    "signature-only",
    "unresolved",
];

/// Whether the four-byte magic of a PDB stream is the portable PDB signature `BSJB`.
///
/// A classic Windows PDB gives no file attribution and makes the run untrustworthy
/// ([ADR-0008](../../../docs/adr/0008-exit-code-contract.md)).
pub fn is_portable_pdb(header: &[u8]) -> bool {
    header.starts_with(b"BSJB")
}

/// One project that could not be measured, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProjectError {
    /// The project file, repository-relative.
    pub project: String,
    /// What went wrong.
    pub reason: String,
}

/// Attribution across a solution: the Spike B measurement (plan 0000, Step 9).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttributionReport {
    /// The solution, repository-relative.
    pub solution: String,
    /// The configuration read.
    pub configuration: String,
    /// One entry per built assembly.
    pub assemblies: Vec<AssemblyAttribution>,
    /// Projects with no built assembly or an unreadable one.
    pub errors: Vec<ProjectError>,
    /// The counts over every assembly.
    pub pooled: Counts,
    /// Attributed over every type.
    pub raw: f64,
    /// Attributed over the types a tool could attribute: the ADR-0003 trigger figure.
    pub adjusted: f64,
    /// Projects targeting .NET Framework (`net4*`).
    pub net4x_projects: u32,
    /// Types in assemblies whose PDB is a Windows PDB or missing.
    pub non_portable_pdb_types: u32,
}

/// Relative, posix-separated, for output.
fn display(path: &Path, repository: &Path) -> String {
    attribute::normalise_document(&path.to_string_lossy(), repository)
}

/// Measures attribution over every project of `solution` built in `configuration`.
///
/// # Errors
/// When the solution itself cannot be read; a project that cannot be measured is recorded in
/// [`AttributionReport::errors`] instead.
pub fn attribute_solution(
    solution: &Path,
    configuration: &str,
    repository: &Path,
) -> Result<AttributionReport, SolutionError> {
    let mut report = AttributionReport {
        solution: display(solution, repository),
        configuration: configuration.to_owned(),
        assemblies: Vec::new(),
        errors: Vec::new(),
        pooled: Counts::default(),
        raw: 0.0,
        adjusted: 0.0,
        net4x_projects: 0,
        non_portable_pdb_types: 0,
    };
    let mut seen = BTreeSet::new();
    for path in solution_projects(solution)? {
        let project = match Project::read(&path, configuration, repository) {
            Ok(project) => project,
            Err(error) => {
                report.errors.push(ProjectError {
                    project: display(&path, repository),
                    reason: error.to_string(),
                });
                continue;
            }
        };
        if project
            .target_frameworks
            .iter()
            .any(|t| t.starts_with("net4"))
        {
            report.net4x_projects += 1;
        }
        let Some(dll) = project.assembly.clone() else {
            report.errors.push(ProjectError {
                project: display(&path, repository),
                reason: format!("no built {}.dll for {configuration}", project.assembly_name),
            });
            continue;
        };
        if !seen.insert(dll.clone()) {
            continue;
        }
        let folder = path.parent().unwrap_or(repository);
        match attribute_assembly(&dll, folder, repository) {
            Ok(assembly) => {
                if matches!(assembly.pdb, PdbKind::Windows | PdbKind::Missing) {
                    report.non_portable_pdb_types +=
                        assembly.counts.types_total - assembly.counts.types_excluded;
                }
                report.pooled.add(&assembly.counts);
                report.assemblies.push(assembly);
            }
            Err(error) => report.errors.push(ProjectError {
                project: display(&path, repository),
                reason: error.to_string(),
            }),
        }
    }
    report.raw = report.pooled.raw();
    report.adjusted = report.pooled.adjusted();
    Ok(report)
}

/// The .NET extractor: one module per source file its built assemblies' types attribute to.
#[derive(Debug, Clone, Copy, Default)]
pub struct DotnetExtractor;

/// The solution to read: the configured one, else the only `.sln` or `.slnx` in the root.
fn find_solution(root: &Path, options: &DotnetOptions) -> Result<PathBuf, ExtractError> {
    if let Some(solution) = &options.solution {
        return Ok(root.join(solution));
    }
    let mut found: Vec<PathBuf> = std::fs::read_dir(root)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "sln" || e == "slnx"))
        .collect();
    found.sort();
    match found.as_slice() {
        [only] => Ok(only.clone()),
        [] => Err(ExtractError::NoModulesFound),
        _ => Err(ExtractError::UnsupportedFile {
            path: root.to_path_buf(),
            reason: "more than one solution; set languages.dotnet.solution".to_owned(),
        }),
    }
}

impl Extractor for DotnetExtractor {
    type Options = DotnetOptions;

    fn extract(
        &self,
        roots: &[PathBuf],
        options: &DotnetOptions,
    ) -> Result<Extraction, ExtractError> {
        let Some(root) = roots.first() else {
            return Err(ExtractError::NoModulesFound);
        };
        let solution = find_solution(root, options)?;
        let report =
            attribute_solution(&solution, options.configuration(), root).map_err(|error| {
                ExtractError::UnsupportedFile {
                    path: solution.clone(),
                    reason: error.to_string(),
                }
            })?;
        if report.assemblies.is_empty() {
            return Err(ExtractError::NoBuiltAssemblies { solution });
        }
        if let Some(assembly) = report
            .assemblies
            .iter()
            .find(|a| matches!(a.pdb, PdbKind::Windows | PdbKind::Missing))
        {
            return Err(ExtractError::NonPortablePdb {
                assembly: root.join(&assembly.path),
            });
        }
        let mut files: BTreeMap<String, (BTreeSet<String>, bool)> = BTreeMap::new();
        let mut warnings = Vec::new();
        for assembly in &report.assemblies {
            for ty in &assembly.types {
                match (&ty.attribution, &ty.file) {
                    (Some(kind @ (Attribution::Pdb | Attribution::Inferred)), Some(file)) => {
                        let entry = files.entry(file.clone()).or_default();
                        if !ty.namespace.is_empty() {
                            entry.0.insert(ty.namespace.clone());
                        }
                        entry.1 |= *kind == Attribution::Pdb;
                    }
                    (Some(_), _) => warnings.push(Warning::about(
                        &assembly.path,
                        format!("{}: no source file; path-based rules skip it", ty.full_name),
                    )),
                    (None, _) => {}
                }
            }
        }
        let modules: Vec<Module> = files
            .into_iter()
            .map(|(file, (namespaces, from_pdb))| Module {
                language: Some(Language::Dotnet),
                namespaces: Some(namespaces.into_iter().collect()),
                attribution: Some(if from_pdb {
                    Attribution::Pdb
                } else {
                    Attribution::Inferred
                }),
                ..Module::new(file)
            })
            .collect();
        let count = modules.len() as u64;
        Ok(Extraction {
            modules,
            code: None,
            inspected: Receipt::counts(count, report.assemblies.len() as u64, count),
            warnings,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_portable_pdb_magic() {
        assert!(is_portable_pdb(b"BSJB\x01\x00\x01\x00"));
        assert!(!is_portable_pdb(b"Microsoft C/C++ MSF 7.00"));
        assert!(!is_portable_pdb(b""));
    }

    #[test]
    fn vocabularies_match_the_design() {
        assert_eq!(DEPENDENCY_KINDS.len(), 8);
        assert!(DEPENDENCY_TYPES.contains(&"signature-only"));
    }
}
