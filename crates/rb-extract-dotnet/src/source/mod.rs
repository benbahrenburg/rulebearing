//! `--mode source`: the .NET graph read from the `.cs` files, without a build.
//!
//! - Plan: [Wave 3, Step 14](../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#24-steps-for-sub-wave-3d---mode-source-guard---watch-the-2-s-proof)
//!   and the contract in [§ 1.5](../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#15-interfaces-and-contracts-this-wave-freezes)
//!   ("Source mode")
//! - Decision: [ADR-0011](../../../../docs/adr/0011-read-dotnet-assemblies-not-source.md)
//!   (compiled mode is the gate; source mode is namespace-level and marked `approximate`),
//!   [ADR-0010](../../../../docs/adr/0010-crate-layout-and-extractor-boundary.md) (it lives in
//!   the extractor and writes document types only)
//! - Architecture: [§ Extractors](../../../../docs/architecture.md#extractors),
//!   [§ Performance model](../../../../docs/architecture.md#performance-model)
//! - Requirement: [FR-EXT-DN-04](../../../../docs/prd.md#fr-ext-dn-04),
//!   [NFR-PERF-02](../../../../docs/prd.md#nfr-perf-02)
//!
//! The projects are discovered as compiled mode discovers them (the solution, else every project
//! file; `excludeProjects` applied), without needing their output. Every `.cs` file under a
//! discovered project's folder is read, except build output (`bin/`, `obj/`, `artifacts/`) and
//! generated code (`*.g.cs`, `*.g.i.cs`, `*.Designer.cs`); a file belongs to the project whose
//! folder is nearest above it, and a file whose nearest project is not discovered (outside the
//! solution, or excluded) is not read. With no project file anywhere, every `.cs` file is read.
//! Files are parsed in parallel ([`tree_sitter`]) and resolved together ([`namespaces`]):
//! every module is `attribution: source`, every edge `approximate: true`, and the receipt says
//! `mode: source`. `languages.dotnet.namespaces` keeps the files declaring a type in the listed
//! namespaces and the edges between them. The loader keys (`assemblies`, `directories`) name
//! built assemblies, which source mode does not read, so a configuration with them is refused.
//!
//! What a later incremental run needs of each file is its parse, kept in
//! [`rb_model::FileState::code`]: an unchanged file is not parsed again, and the resolution,
//! which depends on every file, is always run over all of them, so an incremental extraction is
//! the full one.

pub mod namespaces;
pub mod tree_sitter;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use rayon::prelude::*;
use rb_model::options::DotnetMode;
use rb_model::{
    DotnetOptions, ExtractError, ExtractRequest, Extraction, FileState, Receipt, Warning,
    source_name,
};
use serde::{Deserialize, Serialize};

use self::namespaces::{ProjectInfo, SourceFile};
use self::tree_sitter::{CSharpParser, FileFacts};
use crate::discover::{self, DiscoverError};

/// Folders never searched for sources: build output, tool caches, other ecosystems, and
/// Rulebearing's own output.
const SKIPPED_DIRECTORIES: &[&str] = &[
    "bin",
    "obj",
    "node_modules",
    ".git",
    ".vs",
    "artifacts",
    ".graph",
];

/// The name reason a refused loader configuration carries.
pub const LOADER_REASON: &str = "source-mode-reads-no-assemblies: languages.dotnet.assemblies and directories name built assemblies, which --mode source does not read; remove them to read the solution's .cs files, or run in compiled mode";

/// Whether a file name is generated code a build writes, which source mode skips.
pub fn is_generated(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".g.cs") || lower.ends_with(".g.i.cs") || lower.ends_with(".designer.cs")
}

fn is_source(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("cs"))
        && !is_generated(name)
}

fn is_project(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("csproj"))
}

/// Every `.cs` file and every `.csproj` under `dir`, not through symbolic links.
fn walk(dir: &Path, sources: &mut Vec<PathBuf>, projects: &mut Vec<PathBuf>) {
    let mut pending = vec![dir.to_path_buf()];
    while let Some(folder) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if kind.is_dir() {
                if !SKIPPED_DIRECTORIES.contains(&name.as_ref()) {
                    pending.push(entry.path());
                }
            } else if kind.is_file() {
                if is_source(&name) {
                    sources.push(entry.path());
                } else if is_project(&name) {
                    projects.push(entry.path());
                }
            }
        }
    }
    sources.sort();
    projects.sort();
}

/// What discovery and the walk found: the projects source mode reads, and each `.cs` file with
/// the project it belongs to.
struct Layout {
    projects: Vec<ProjectInfo>,
    files: Vec<(PathBuf, Option<usize>)>,
    warnings: Vec<Warning>,
    /// The solution discovery read, relative to the root, `/`-separated.
    solution: Option<String>,
}

fn relative(root: &Path, path: &Path) -> String {
    source_name(path.strip_prefix(root).unwrap_or(path))
}

fn layout(root: &Path, options: &DotnetOptions) -> Result<Layout, ExtractError> {
    let mut solution = None;
    let (discovered, mut warnings) = match discover::discover(root, options) {
        Ok(workspace) => (
            {
                solution = workspace.solution.as_deref().map(|s| relative(root, s));
                workspace.projects
            },
            workspace
                .errors
                .iter()
                .map(|(path, reason)| Warning::about(path, reason.clone()))
                .collect(),
        ),
        Err(DiscoverError::NothingFound { .. }) => (Vec::new(), Vec::new()),
        Err(DiscoverError::Io { path, source }) => {
            return Err(ExtractError::UnsupportedFile {
                path,
                reason: source.to_string(),
            });
        }
        Err(other) => {
            return Err(ExtractError::UnsupportedFile {
                path: root.to_path_buf(),
                reason: other.to_string(),
            });
        }
    };
    let (mut sources, mut project_files) = (Vec::new(), Vec::new());
    walk(root, &mut sources, &mut project_files);
    let canonical = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    // Each folder holding a project file: the discovered project there, if any.
    let mut by_folder: BTreeMap<PathBuf, Option<usize>> = BTreeMap::new();
    for file in &project_files {
        if let Some(folder) = file.parent() {
            by_folder.entry(canonical(folder)).or_insert(None);
        }
    }
    let mut projects = Vec::new();
    for project in &discovered {
        let index = projects.len();
        projects.push(ProjectInfo {
            path: relative(root, &project.path),
            is_test: project.is_test,
            package_refs: project.package_refs.clone(),
        });
        let folder = canonical(project.folder());
        let slot = by_folder.entry(folder).or_insert(None);
        if slot.is_none() {
            *slot = Some(index);
        }
    }
    let loose = by_folder.is_empty();
    let mut files = Vec::new();
    for source in sources {
        if loose {
            files.push((source, None));
            continue;
        }
        let mut folder = source.parent().map(canonical);
        let mut owner = None;
        while let Some(f) = folder {
            if let Some(found) = by_folder.get(&f) {
                owner = Some(*found);
                break;
            }
            folder = f.parent().map(Path::to_path_buf);
        }
        // Under a discovered project: read. Under another project file, or under none: not
        // compiled by the solution read.
        if let Some(Some(project)) = owner {
            files.push((source, Some(project)));
        }
    }
    if files.is_empty() && discovered.is_empty() {
        return Err(ExtractError::NoModulesFound);
    }
    warnings.sort_by(|a, b| (&a.path, &a.message).cmp(&(&b.path, &b.message)));
    Ok(Layout {
        projects,
        files,
        warnings,
        solution,
    })
}

/// The `.cs` files source mode reads under `root`, relative to it: the inputs a cache records.
///
/// # Errors
/// As [`extract`].
pub fn source_files(root: &Path, options: &DotnetOptions) -> Result<Vec<PathBuf>, ExtractError> {
    refuse_loader(root, options)?;
    Ok(layout(root, options)?
        .files
        .into_iter()
        .map(|(path, _)| PathBuf::from(relative(root, &path)))
        .collect())
}

/// The loader keys name built assemblies, which source mode does not read.
fn refuse_loader(root: &Path, options: &DotnetOptions) -> Result<(), ExtractError> {
    if options.assemblies.is_some() || options.directories.is_some() {
        return Err(ExtractError::UnsupportedFile {
            path: root.to_path_buf(),
            reason: LOADER_REASON.to_owned(),
        });
    }
    Ok(())
}

/// The solution and project files an extraction kept in its file states (the files whose change
/// reads everything again), relative to `root`; `None` when it kept no table, which a caller
/// answers with [`crate::project_files`]. Unlike that function it reads no project file.
pub fn kept_project_files(root: &Path, extraction: &Extraction) -> Option<Vec<PathBuf>> {
    let table = extraction
        .files
        .values()
        .next()
        .and_then(kept)
        .and_then(|k| k.table)?;
    let mut files: BTreeSet<PathBuf> = table.projects.iter().map(|p| root.join(&p.path)).collect();
    files.extend(table.solution.iter().map(|s| root.join(s)));
    files.extend(table.warnings.iter().filter_map(|w| w.path.clone()));
    Some(files.into_iter().collect())
}

thread_local! {
    /// Each worker thread's parser, made on its first file.
    static PARSER: std::cell::RefCell<Option<CSharpParser>> = const { std::cell::RefCell::new(None) };
}

/// Parses with this thread's parser.
fn parse_on_this_thread(text: &str) -> Result<FileFacts, tree_sitter::ParseError> {
    PARSER.with(|cell| {
        let mut slot = cell.borrow_mut();
        let parser = match slot.as_mut() {
            Some(parser) => parser,
            None => slot.insert(CSharpParser::new()?),
        };
        parser.parse(text)
    })
}

fn read_text(path: &Path) -> std::io::Result<String> {
    let bytes = std::fs::read(path)?;
    let text = String::from_utf8_lossy(&bytes);
    Ok(text.strip_prefix('\u{feff}').unwrap_or(&text).to_owned())
}

/// What a file keeps for the next incremental run, as one JSON string in
/// [`FileState::code`]: its parse and its project; the first file by path also carries the
/// project table and discovery's warnings, so a run that reads only changed files neither walks
/// the tree nor reads a project file again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Kept {
    /// An index into the project table.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    project: Option<usize>,
    /// The project table and discovery's warnings, on the first file only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    table: Option<Table>,
    facts: FileFacts,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Table {
    projects: Vec<KeptProject>,
    warnings: Vec<Warning>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    solution: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct KeptProject {
    path: String,
    is_test: bool,
    packages: Vec<(String, Option<String>)>,
}

impl KeptProject {
    fn of(info: &ProjectInfo) -> Self {
        Self {
            path: info.path.clone(),
            is_test: info.is_test,
            packages: info
                .package_refs
                .iter()
                .map(|p| (p.id.clone(), p.version.clone()))
                .collect(),
        }
    }

    fn info(&self) -> ProjectInfo {
        ProjectInfo {
            path: self.path.clone(),
            is_test: self.is_test,
            package_refs: self
                .packages
                .iter()
                .map(|(id, version)| crate::discover::PackageRef {
                    id: id.clone(),
                    version: version.clone(),
                })
                .collect(),
        }
    }
}

fn kept(state: &FileState) -> Option<Kept> {
    match &state.code {
        Some(serde_json::Value::String(text)) => serde_json::from_str(text).ok(),
        _ => None,
    }
}

/// The layout an incremental request describes: its files, each with the project and the parse
/// the earlier run kept. `None` when any file kept nothing readable, or no table was kept; the
/// caller then walks the tree.
fn kept_layout(root: &Path, request: &ExtractRequest) -> Option<(Layout, BTreeMap<String, Kept>)> {
    let mut sources: BTreeSet<String> = request.unchanged_sources();
    sources.extend(request.changed.iter().map(|p| source_name(p)));
    let mut kept_files = BTreeMap::new();
    for source in &sources {
        let state = request.previous.files.get(source)?;
        kept_files.insert(source.clone(), kept(state)?);
    }
    let table = kept_files.values().next()?.table.clone()?;
    let files = kept_files
        .iter()
        .map(|(source, k)| (root.join(source), k.project))
        .collect();
    Some((
        Layout {
            projects: table.projects.iter().map(KeptProject::info).collect(),
            files,
            warnings: table.warnings,
            solution: table.solution,
        },
        kept_files,
    ))
}

/// Extracts the .NET graph under `root` from source. With a request, the files are the
/// request's and the unchanged ones' parses are taken from `request.previous` where it kept
/// them; with `keep_file_states`, each file's parse is kept for the next run.
///
/// # Errors
/// [`ExtractError::NoModulesFound`] when there is nothing .NET under `root`; an
/// [`ExtractError::UnsupportedFile`] naming the reason for a loader configuration, an
/// unreadable solution or project file, or a file that cannot be read.
pub fn extract(
    root: &Path,
    options: &DotnetOptions,
    request: Option<&ExtractRequest>,
    keep_file_states: bool,
) -> Result<Extraction, ExtractError> {
    refuse_loader(root, options)?;
    let (layout, reusable) = match request.and_then(|r| kept_layout(root, r)) {
        Some((layout, kept_files)) => {
            let unchanged = request
                .map(ExtractRequest::unchanged_sources)
                .unwrap_or_default();
            let reusable: BTreeMap<String, FileFacts> = kept_files
                .into_iter()
                .filter(|(source, _)| unchanged.contains(source))
                .map(|(source, k)| (source, k.facts))
                .collect();
            (layout, reusable)
        }
        None => (layout(root, options)?, BTreeMap::new()),
    };
    let mut reusable = reusable;
    // The files whose parse is the earlier run's: their kept state is too.
    let reused: BTreeSet<String> = reusable.keys().cloned().collect();
    let work: Vec<(PathBuf, Option<usize>, Option<FileFacts>)> = layout
        .files
        .iter()
        .map(|(path, project)| {
            let facts = reusable.remove(&relative(root, path));
            (path.clone(), *project, facts)
        })
        .collect();
    let parsed: Vec<Result<SourceFile, ExtractError>> = work
        .into_par_iter()
        .map(|(path, project, facts)| {
            let source = relative(root, &path);
            let facts = if let Some(facts) = facts {
                facts
            } else {
                let failed = |reason: String| ExtractError::UnsupportedFile {
                    path: path.clone(),
                    reason,
                };
                let text = read_text(&path).map_err(|e| failed(e.to_string()))?;
                parse_on_this_thread(&text).map_err(|e| failed(e.to_string()))?
            };
            Ok(SourceFile {
                source,
                project,
                facts,
            })
        })
        .collect();
    let files: Vec<SourceFile> = parsed.into_iter().collect::<Result<_, _>>()?;
    let mut modules = namespaces::modules(&files, &layout.projects);
    if let Some(kept) = &options.namespaces {
        keep_namespaces(&mut modules, kept);
    }
    let mut warnings = layout.warnings.clone();
    let broken: Vec<&str> = files
        .iter()
        .filter(|f| f.facts.syntax_errors)
        .map(|f| f.source.as_str())
        .collect();
    if let Some(warning) = syntax_warning(&broken) {
        warnings.push(warning);
    }
    let file_states = if keep_file_states {
        let previous = request.map(|r| (&r.previous.files, &reused));
        file_states(&files, &layout, previous)
    } else {
        BTreeMap::new()
    };
    let count = |n: usize| u64::try_from(n).unwrap_or(u64::MAX);
    let inspected = Receipt {
        projects: Some(count(layout.projects.len())),
        mode: Some(DotnetMode::Source),
        ..Receipt::counts(count(files.len()), 0, count(modules.len()))
    };
    Ok(Extraction {
        modules,
        code: None,
        inspected,
        warnings,
        files: file_states,
        sidecar: None,
    })
}

/// Each file's [`Kept`] state, the first by path carrying the table. A file whose parse was
/// reused keeps the earlier run's state as it was, since its parse, its project and (on the
/// first file) the table it was read from are unchanged.
fn file_states(
    files: &[SourceFile],
    layout: &Layout,
    previous: Option<(&BTreeMap<String, FileState>, &BTreeSet<String>)>,
) -> BTreeMap<String, FileState> {
    let first = files.iter().map(|f| f.source.as_str()).min();
    files
        .par_iter()
        .filter_map(|f| {
            if let Some((states, reused)) = previous
                && reused.contains(&f.source)
                && let Some(state) = states.get(&f.source)
            {
                return Some((f.source.clone(), state.clone()));
            }
            let table = (Some(f.source.as_str()) == first).then(|| Table {
                projects: layout.projects.iter().map(KeptProject::of).collect(),
                warnings: layout.warnings.clone(),
                solution: layout.solution.clone(),
            });
            let kept = Kept {
                project: f.project,
                table,
                facts: f.facts.clone(),
            };
            serde_json::to_string(&kept).ok().map(|text| {
                (
                    f.source.clone(),
                    FileState {
                        code: Some(serde_json::Value::String(text)),
                        warnings: Vec::new(),
                    },
                )
            })
        })
        .collect()
}

/// One warning for every file tree-sitter recovered from a syntax error in, naming the first
/// few: their edges come from what it recognised.
fn syntax_warning(broken: &[&str]) -> Option<Warning> {
    let (first, rest) = broken.split_first()?;
    let shown: Vec<&str> = std::iter::once(*first)
        .chain(rest.iter().copied())
        .take(3)
        .collect();
    let more = broken.len().saturating_sub(shown.len());
    let tail = if more > 0 {
        format!(" and {more} more")
    } else {
        String::new()
    };
    Some(Warning {
        path: None,
        message: format!(
            "--mode source: {} file(s) hold syntax tree-sitter-c-sharp did not recognise ({}{tail}); their edges come from the parts it did",
            broken.len(),
            shown.join(", ")
        ),
    })
}

/// `languages.dotnet.namespaces`: the files declaring a type in a listed namespace or below,
/// and only the edges among them or to external modules.
fn keep_namespaces(modules: &mut Vec<rb_model::Module>, kept: &[String]) {
    let inside = |namespace: &str| {
        kept.iter().any(|k| {
            namespace == k
                || namespace
                    .strip_prefix(k.as_str())
                    .is_some_and(|rest| rest.starts_with('.'))
        })
    };
    let files: BTreeSet<String> = modules
        .iter()
        .filter(|m| m.followable == Some(true))
        .map(|m| m.source.clone())
        .collect();
    modules.retain(|m| {
        m.followable != Some(true)
            || m.namespaces
                .iter()
                .flatten()
                .any(|namespace| inside(namespace))
    });
    let kept_files: BTreeSet<String> = modules.iter().map(|m| m.source.clone()).collect();
    for module in modules.iter_mut() {
        module
            .dependencies
            .retain(|d| !files.contains(&d.resolved) || kept_files.contains(&d.resolved));
    }
    let used: BTreeSet<String> = modules
        .iter()
        .flat_map(|m| m.dependencies.iter().map(|d| d.resolved.clone()))
        .collect();
    modules.retain(|m| m.followable == Some(true) || used.contains(&m.source));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_code_is_skipped() {
        for (name, source) in [
            ("Order.cs", true),
            ("Order.CS", true),
            ("Resources.Designer.cs", false),
            ("View.g.cs", false),
            ("View.g.i.cs", false),
            ("Order.csproj", false),
        ] {
            assert_eq!(is_source(name), source, "{name}");
        }
    }

    #[test]
    fn the_syntax_warning_names_three_and_counts_the_rest() {
        assert_eq!(syntax_warning(&[]), None);
        let one = syntax_warning(&["a.cs"]).map(|w| w.message);
        assert!(one.is_some_and(|m| m.contains("1 file(s)") && m.contains("(a.cs)")));
        let many = syntax_warning(&["a.cs", "b.cs", "c.cs", "d.cs", "e.cs"]).map(|w| w.message);
        assert!(many.is_some_and(|m| m.contains("(a.cs, b.cs, c.cs and 2 more)")));
    }
}
