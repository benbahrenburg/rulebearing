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
//! Wave 0 (Spike B) built the reader far enough to attribute every type to a source file and
//! measure the share it attributes, the figure [ADR-0003](../../../docs/adr/0003-dotnet-extractor-fallback.md)
//! decided on ([ADR-0022](../../../docs/adr/0022-dotnet-reader-in-rust-confirmed.md)). Wave 2
//! ([Steps 1 to 3](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#21-step-1-net-discovery-and-the-loader-options-2a))
//! completes it: discovery, the full table set, signatures, IL, the edge set and the code layer.
//!
//! | Module | Does |
//! | --- | --- |
//! | [`bytes`] | bounds-checked little-endian reads and compressed integers |
//! | [`pe`] | PE headers, sections, the CLI header, the debug directory, method bodies by RVA |
//! | [`metadata`] | the metadata root, heaps (`#Strings`, `#US`, `#Blob`, `#GUID`) and the table stream |
//! | [`sig`] | field, method, property, local, `TypeSpec` and `MethodSpec` signatures |
//! | [`il`] | method bodies: the instructions whose operand is a token |
//! | [`loader`] | one assembly as an owned model; its module doc lists the table set and why each table is read |
//! | [`assembly`] | the light type view the attribution passes use |
//! | [`pdb`] | portable PDB documents, sequence points, `SourceLink` |
//! | [`attribute`] | one type to one file: `pdb`, `inferred`, `none` |
//! | [`discover`] | solutions, project files and the loader options, to find the built assemblies |
//! | [`names`] | resolving references across assemblies and spelling names as `ArchUnitNET` does |
//! | [`codelayer`], [`body`] | the code layer and every type's and member's dependencies |
//! | [`edges`] | the module layer: type dependencies projected to files, with the .NET `dependencyTypes` |
//!
//! Every reader returns an error naming the structure and offset on malformed input; the fuzz
//! targets `fuzz/fuzz_targets/metadata_reader.rs`, `ecma335.rs` and `pdb.rs` and the truncation
//! tests hold it to that.

pub mod assembly;
pub mod attribute;
pub mod body;
pub mod bytes;
pub mod codelayer;
pub mod discover;
pub mod edges;
pub mod il;
pub mod loader;
pub mod metadata;
pub mod names;
pub mod pdb;
pub mod pe;
pub mod sig;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use rb_model::{
    Attribution, DotnetOptions, ExtractError, Extraction, Extractor, Language, Module, Receipt,
    Warning,
};
use serde::Serialize;

use attribute::{AssemblyAttribution, Counts, PdbKind, attribute_assembly};
use discover::{DiscoverError, Project, solution_projects};

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
) -> Result<AttributionReport, DiscoverError> {
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
        let project = match Project::read(&path, configuration, None, repository) {
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
        match attribute_assembly(&dll, project.folder(), repository) {
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

/// The .NET extractor: one module per source file the built assemblies' types attribute to,
/// the edges between them, and the code layer.
#[derive(Debug, Clone, Copy, Default)]
pub struct DotnetExtractor;

/// The root's path below its git repository's root (`src/`), empty at the root or outside a
/// repository: deterministic builds write documents relative to the repository root (`/_/`).
fn repository_prefix(root: &Path) -> String {
    let absolute = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let mut current = absolute.as_path();
    loop {
        if current.join(".git").exists() {
            return absolute
                .strip_prefix(current)
                .map(|rest| {
                    let rest = rest.to_string_lossy().replace('\\', "/");
                    if rest.is_empty() {
                        rest
                    } else {
                        format!("{rest}/")
                    }
                })
                .unwrap_or_default();
        }
        match current.parent() {
            Some(parent) => current = parent,
            None => return String::new(),
        }
    }
}

/// A document path relative to the root: `/_/`-mapped paths are relative to the repository, so
/// the root's own prefix is removed.
fn under_root(path: &str, prefix: &str) -> String {
    path.strip_prefix(prefix).unwrap_or(path).to_owned()
}

fn read_error(path: &Path, reason: &dyn std::fmt::Display) -> ExtractError {
    ExtractError::UnsupportedFile {
        path: path.to_path_buf(),
        reason: reason.to_string(),
    }
}

/// One assembly, read.
struct Read {
    project: Project,
    loaded: loader::Loaded,
    attribution: AssemblyAttribution,
    points: BTreeMap<u32, Vec<pdb::SequencePoint>>,
    documents: Vec<String>,
}

/// Reads one built assembly: metadata, attribution and sequence points.
/// The assemblies beside the analysed ones that their references name, transitively, read only
/// to describe the types the analysed code references: `ArchUnitNET` resolves a reference from
/// the same folders. One that cannot be read is a warning, and its types stay unavailable.
fn beside_assemblies(
    reads: &[Read],
    seen: &BTreeSet<PathBuf>,
    warnings: &mut Vec<Warning>,
) -> Vec<loader::Loaded> {
    let refs = |loaded: &loader::Loaded| -> Vec<String> {
        loaded
            .assembly_refs
            .iter()
            .map(|a| a.identity.name.clone())
            .collect()
    };
    let mut visited = seen.clone();
    let mut queue: Vec<(PathBuf, Vec<String>)> = reads
        .iter()
        .map(|r| (r.project.folder().to_path_buf(), refs(&r.loaded)))
        .collect();
    let mut found = Vec::new();
    let mut next = 0;
    while next < queue.len() {
        let (folder, names) = queue[next].clone();
        next += 1;
        for name in names {
            let dll = folder.join(format!("{name}.dll"));
            if !dll.is_file() || !visited.insert(dll.clone()) {
                continue;
            }
            let read = std::fs::read(&dll)
                .map_err(|e| read_error(&dll, &e))
                .and_then(|bytes| loader::Loaded::read(&bytes).map_err(|e| read_error(&dll, &e)));
            match read {
                Ok(loaded) => {
                    queue.push((folder.clone(), refs(&loaded)));
                    found.push(loaded);
                }
                Err(e) => warnings.push(Warning::about(
                    &dll,
                    format!("not read, so the types it defines are unavailable: {e}"),
                )),
            }
        }
    }
    found
}

/// Whether a dependency target names a generic parameter (`Declarer+<T>`, or `!!0` when the
/// declarer is unknown) rather than a type.
fn is_generic_parameter(name: &str) -> bool {
    name.starts_with('!') || (name.ends_with('>') && name.contains("+<"))
}

/// Adds a referenced stub for every dependency target the code layer does not define.
fn add_referenced_types(
    code: &mut rb_model::CodeLayer,
    describer: &codelayer::Builder<'_>,
    first_beside: usize,
) {
    let defined: BTreeSet<&str> = code.types.iter().map(|t| t.full_name.as_str()).collect();
    let targets: BTreeSet<String> = code
        .types
        .iter()
        .flat_map(|t| &t.dependencies)
        .chain(code.members.iter().flat_map(|m| &m.dependencies))
        .map(|d| d.target.as_str())
        .filter(|t| !defined.contains(t) && !is_generic_parameter(t))
        .map(str::to_owned)
        .collect();
    for target in &targets {
        code.types.push(describer.referenced(first_beside, target));
    }
    code.normalise();
}

fn read_assembly(
    project: &Project,
    dll: &Path,
    root: &Path,
    prefix: &str,
) -> Result<Read, ExtractError> {
    let bytes = std::fs::read(dll)?;
    let loaded = loader::Loaded::read(&bytes).map_err(|e| read_error(dll, &e))?;
    let mut attribution =
        attribute_assembly(dll, project.folder(), root).map_err(|e| read_error(dll, &e))?;
    if attribution.pdb == PdbKind::Windows {
        return Err(ExtractError::NonPortablePdb {
            assembly: dll.to_path_buf(),
        });
    }
    for ty in &mut attribution.types {
        if let Some(file) = &ty.file {
            ty.file = Some(under_root(file, prefix));
        }
    }
    let assembly_view = assembly::Assembly::read(&bytes).map_err(|e| read_error(dll, &e))?;
    let (_, pdb_bytes) =
        attribute::debug_info(dll, &assembly_view).map_err(|e| read_error(dll, &e))?;
    let mut points = BTreeMap::new();
    let mut documents = Vec::new();
    if let Some(pdb_bytes) = pdb_bytes.as_deref()
        && let Ok(pdb) = pdb::PortablePdb::parse(pdb_bytes)
    {
        let pdb_path = dll.with_extension("pdb");
        documents = pdb
            .documents()
            .map_err(|e| read_error(&pdb_path, &e))?
            .iter()
            .map(|d| under_root(&attribute::normalise_document(d, root), prefix))
            .collect();
        for ty in &loaded.types {
            for method in &ty.methods {
                let found = pdb
                    .sequence_points(method.row)
                    .map_err(|e| read_error(&pdb_path, &e))?;
                if !found.is_empty() {
                    points.insert(method.row, found);
                }
            }
        }
    }
    Ok(Read {
        project: project.clone(),
        loaded,
        attribution,
        points,
        documents,
    })
}

impl Extractor for DotnetExtractor {
    type Options = DotnetOptions;

    #[expect(
        clippy::too_many_lines,
        reason = "the five stages of a .NET extraction in order"
    )]
    fn extract(
        &self,
        roots: &[PathBuf],
        options: &DotnetOptions,
    ) -> Result<Extraction, ExtractError> {
        let Some(root) = roots.first() else {
            return Err(ExtractError::NoModulesFound);
        };
        let workspace = discover::discover(root, options).map_err(|e| match e {
            DiscoverError::NothingFound { .. } => ExtractError::NoModulesFound,
            DiscoverError::Io { path, source } => read_error(&path, &source),
            other => read_error(root, &other),
        })?;
        let prefix = repository_prefix(root);
        let mut warnings: Vec<Warning> = workspace
            .errors
            .iter()
            .map(|(path, reason)| Warning::about(path, reason.clone()))
            .collect();
        let mut reads: Vec<Read> = Vec::new();
        let mut seen = BTreeSet::new();
        for project in &workspace.projects {
            let Some(dll) = &project.assembly else {
                warnings.push(Warning::about(
                    &project.path,
                    format!(
                        "no built {}.dll; run dotnet build -p:DebugType=portable",
                        project.assembly_name
                    ),
                ));
                continue;
            };
            if seen.insert(dll.clone()) {
                reads.push(read_assembly(project, dll, root, &prefix)?);
            }
        }
        if options.include_dependencies == Some(true) {
            let mut index = 0;
            while index < reads.len() {
                let folder = reads[index].project.folder().to_path_buf();
                let names: Vec<String> = reads[index]
                    .loaded
                    .assembly_refs
                    .iter()
                    .map(|a| a.identity.name.clone())
                    .collect();
                for name in names {
                    let dll = folder.join(format!("{name}.dll"));
                    if dll.is_file() && seen.insert(dll.clone()) {
                        reads.push(read_assembly(&Project::loose(&dll), &dll, root, &prefix)?);
                    }
                }
                index += 1;
            }
        }
        if reads.is_empty() {
            return Err(ExtractError::NoBuiltAssemblies {
                solution: workspace.solution.clone().unwrap_or_else(|| root.clone()),
            });
        }
        for read in &reads {
            if read.attribution.pdb == PdbKind::Missing {
                warnings.push(Warning::about(
                    &read.project.path,
                    "no PDB beside the assembly: types have attribution none and path rules skip them; build with -p:DebugType=portable",
                ));
            }
        }

        let universe = names::Universe::new(reads.iter().map(|r| &r.loaded).collect());
        let namespaces = options.namespaces.as_deref();
        let sources: Vec<codelayer::Source<'_>> = reads
            .iter()
            .map(|r| codelayer::Source {
                loaded: &r.loaded,
                attribution: &r.attribution.types,
                points: r.points.clone(),
                documents: r.documents.clone(),
                namespaces,
            })
            .collect();
        let mut built = codelayer::Builder::new(&universe, &sources).build();
        let beside = beside_assemblies(&reads, &seen, &mut warnings);
        let wide = names::Universe::new(
            reads
                .iter()
                .map(|r| &r.loaded)
                .chain(beside.iter())
                .collect(),
        );
        add_referenced_types(
            &mut built.code,
            &codelayer::Builder::new(&wide, &sources),
            reads.len(),
        );

        let display = |path: &Path| {
            under_root(
                &attribute::normalise_document(&path.to_string_lossy(), root),
                &prefix,
            )
        };
        let mut files: BTreeMap<String, (BTreeSet<String>, bool, String)> = BTreeMap::new();
        let mut counts = rb_model::AttributionCounts::default();
        let mut assemblies = Vec::with_capacity(reads.len());
        for read in &reads {
            let project_path = display(&read.project.path);
            let mut by_type = BTreeMap::new();
            for ty in &read.attribution.types {
                match (&ty.attribution, &ty.file) {
                    (Some(kind @ (Attribution::Pdb | Attribution::Inferred)), Some(file)) => {
                        let entry = files
                            .entry(file.clone())
                            .or_insert_with(|| (BTreeSet::new(), false, project_path.clone()));
                        if !ty.namespace.is_empty() {
                            entry.0.insert(ty.namespace.clone());
                        }
                        entry.1 |= *kind == Attribution::Pdb;
                        by_type.insert(ty.full_name.clone(), file.clone());
                        if *kind == Attribution::Pdb {
                            counts.pdb += 1;
                        } else {
                            counts.inferred += 1;
                        }
                    }
                    (Some(_), _) => {
                        counts.none += 1;
                        warnings.push(Warning::about(
                            &read.attribution.path,
                            format!("{}: no source file; path-based rules skip it", ty.full_name),
                        ));
                    }
                    (None, _) => {}
                }
            }
            assemblies.push(edges::AssemblyFiles {
                project: &read.project,
                project_path,
                files: by_type,
            });
        }
        let file_count = files.len() as u64;
        let mut modules: Vec<Module> = files
            .into_iter()
            .map(|(file, (namespaces, from_pdb, project))| Module {
                language: Some(Language::Dotnet),
                project: Some(project),
                namespaces: Some(namespaces.into_iter().collect()),
                attribution: Some(if from_pdb {
                    Attribution::Pdb
                } else {
                    Attribution::Inferred
                }),
                followable: Some(true),
                ..Module::new(file)
            })
            .collect();
        edges::project(
            &universe,
            &assemblies,
            &built.dependencies,
            &mut modules,
            edges::packages_root().as_deref(),
        );
        let module_count = modules.len() as u64;
        Ok(Extraction {
            modules,
            code: Some(built.code),
            inspected: Receipt {
                projects: Some(workspace.projects.len() as u64),
                pdb_documents: Some(reads.iter().map(|r| r.documents.len() as u64).sum()),
                attribution: Some(counts),
                ..Receipt::counts(file_count, reads.len() as u64, module_count)
            },
            warnings,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generic_parameters_are_not_referenced_types() {
        for name in ["Method+<T>", "Ns.Outer`1+<TKey>", "!!0", "!1"] {
            assert!(is_generic_parameter(name), "{name}");
        }
        for name in [
            "System.Object",
            "Ns.Outer+<>c",
            "Ns.Outer+<Run>d__0",
            "Global",
        ] {
            assert!(!is_generic_parameter(name), "{name}");
        }
    }

    #[test]
    fn recognises_portable_pdb_magic() {
        assert!(is_portable_pdb(b"BSJB\x01\x00\x01\x00"));
        assert!(!is_portable_pdb(b"Microsoft C/C++ MSF 7.00"));
        assert!(!is_portable_pdb(b""));
    }

    #[test]
    fn documents_are_made_relative_to_the_root() {
        assert_eq!(under_root("src/Web/A.cs", "src/"), "Web/A.cs");
        assert_eq!(under_root("TestAssembly/A.cs", "src/"), "TestAssembly/A.cs");
        assert_eq!(under_root("A.cs", ""), "A.cs");
        // This crate's folder sits two levels below the repository root.
        let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
        assert_eq!(repository_prefix(crate_root), "crates/rb-extract-dotnet/");
        assert_eq!(repository_prefix(&crate_root.join("../..")), "");
        assert_eq!(repository_prefix(Path::new("/")), "");
    }

    #[test]
    fn vocabularies_match_the_design() {
        assert_eq!(DEPENDENCY_KINDS.len(), 8);
        assert!(DEPENDENCY_TYPES.contains(&"signature-only"));
    }
}
