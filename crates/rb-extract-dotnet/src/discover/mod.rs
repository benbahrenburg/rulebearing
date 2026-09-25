//! Discovery: which projects a run reads and where their built assemblies are.
//!
//! - Plan: [Wave 2, Step 1](../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#21-step-1-net-discovery-and-the-loader-options-2a)
//! - Requirement: [FR-EXT-DN-01](../../../../docs/prd.md#fr-ext-dn-01)
//! - Coverage: [ArchUnitNET § Loader and caches](../../../../docs/artifacts/archunitnet-0.13.4-coverage.md#loader-and-caches)
//! - Architecture: [Extractors](../../../../docs/architecture.md#extractors) (discovery row)
//!
//! Two modes, as `ArchLoader` has:
//!
//! | Mode | Chosen when | Reads |
//! | --- | --- | --- |
//! | solution | no `assemblies` or `directories` option | `languages.dotnet.solution`, else the only `.sln` / `.slnx` in the root, else every project file under the root; each project's built assembly |
//! | loader | `assemblies` (globs, `LoadAssemblies`) or `directories` (`LoadFilteredDirectory`) | the matching assemblies directly, each its own project |
//!
//! `excludeProjects` removes projects whose repository-relative path matches; the configuration
//! defaults to `Debug`, and a project built only in another configuration is found by the search
//! in [`locate`]. `includeDependencies` and `namespaces` act on the loaded assemblies and are
//! applied by the extractor, not here.
//!
//! A project's package references include those of the projects it references, transitively
//! (a `PackageReference` flows through a `ProjectReference`, as NuGet restores it); the
//! project's own reference wins when both name one id.

pub mod locate;
pub mod project;
pub mod solution;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use rb_model::DotnetOptions;

pub use project::{PackageRef, ProjectFile};
pub use solution::solution_projects;

/// Why discovery failed.
#[derive(Debug, thiserror::Error)]
pub enum DiscoverError {
    /// Reading a file failed.
    #[error("{path}: {source}", path = path.display())]
    Io {
        /// The file.
        path: PathBuf,
        /// The error.
        source: std::io::Error,
    },
    /// A `.slnx` or project file is not XML.
    #[error("{path}: not valid XML: {reason}", path = path.display())]
    Xml {
        /// The file.
        path: PathBuf,
        /// The parser's message.
        reason: String,
    },
    /// More than one solution in the root and none configured.
    #[error("{root}: more than one solution; set languages.dotnet.solution", root = root.display())]
    AmbiguousSolution {
        /// The root searched.
        root: PathBuf,
    },
    /// Nothing .NET under the root.
    #[error("{root}: no solution, project or assembly found", root = root.display())]
    NothingFound {
        /// The root searched.
        root: PathBuf,
    },
    /// An `excludeProjects` or `assemblies` pattern does not compile.
    #[error("languages.dotnet.{key}: `{pattern}` is not a valid pattern: {reason}")]
    Pattern {
        /// The option key.
        key: &'static str,
        /// The pattern.
        pattern: String,
        /// The compiler's message.
        reason: String,
    },
}

pub(crate) fn read(path: &Path) -> Result<String, DiscoverError> {
    std::fs::read_to_string(path).map_err(|source| DiscoverError::Io {
        path: path.to_path_buf(),
        source,
    })
}

/// One project a run reads: a project file with its located output, or an assembly loaded
/// directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    /// The project file, or the assembly for a directly loaded one.
    pub path: PathBuf,
    /// `AssemblyName`.
    pub assembly_name: String,
    /// `RootNamespace`.
    pub root_namespace: String,
    /// The target frameworks, first one first.
    pub target_frameworks: Vec<String>,
    /// Whether the project is a test project.
    pub is_test: bool,
    /// The built assembly, when found.
    pub assembly: Option<PathBuf>,
    /// Referenced projects.
    pub project_refs: Vec<PathBuf>,
    /// Referenced packages.
    pub package_refs: Vec<PackageRef>,
}

impl Project {
    /// The folder the project's sources sit in (for naming-convention attribution).
    pub fn folder(&self) -> &Path {
        self.path.parent().unwrap_or(Path::new("."))
    }

    /// Reads a project file and locates its assembly.
    ///
    /// # Errors
    /// When the project file cannot be read or is not XML.
    pub fn read(
        path: &Path,
        configuration: &str,
        target_framework: Option<&str>,
        root: &Path,
    ) -> Result<Self, DiscoverError> {
        let file = ProjectFile::read(path, configuration, root)?;
        let assembly = locate::built_assembly(&file, configuration, target_framework);
        Ok(Self {
            path: file.path,
            assembly_name: file.assembly_name,
            root_namespace: file.root_namespace,
            target_frameworks: file.target_frameworks,
            is_test: file.is_test,
            assembly,
            project_refs: file.project_refs,
            package_refs: file.package_refs,
        })
    }

    /// A project standing for an assembly loaded directly.
    pub fn loose(dll: &Path) -> Self {
        let name = dll
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        Self {
            path: dll.to_path_buf(),
            root_namespace: name.clone(),
            assembly_name: name,
            target_frameworks: Vec::new(),
            is_test: false,
            assembly: Some(dll.to_path_buf()),
            project_refs: Vec::new(),
            package_refs: Vec::new(),
        }
    }
}

/// What discovery found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    /// The solution read, in solution mode.
    pub solution: Option<PathBuf>,
    /// Every project, sorted by path.
    pub projects: Vec<Project>,
    /// Project files that could not be read, with the reason.
    pub errors: Vec<(PathBuf, String)>,
}

/// Directory names never searched: build output, tool caches, other ecosystems.
const SKIPPED_DIRECTORIES: &[&str] = &["bin", "obj", "node_modules", ".git", ".vs", "artifacts"];

/// Files under `dir` for which `keep` is true, recursively, not through symbolic links.
fn walk(dir: &Path, keep: &dyn Fn(&Path) -> bool, found: &mut Vec<PathBuf>, skip_output: bool) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            let name = entry.file_name();
            let skipped = SKIPPED_DIRECTORIES.contains(&name.to_string_lossy().as_ref());
            if !(skip_output && skipped) {
                walk(&path, keep, found, skip_output);
            }
        } else if kind.is_file() && keep(&path) {
            found.push(path);
        }
    }
}

/// The solution to read: the configured one, else the only one in the root.
fn find_solution(root: &Path, options: &DotnetOptions) -> Result<Option<PathBuf>, DiscoverError> {
    if let Some(solution) = &options.solution {
        return Ok(Some(root.join(solution)));
    }
    let mut found: Vec<PathBuf> = std::fs::read_dir(root)
        .map_err(|source| DiscoverError::Io {
            path: root.to_path_buf(),
            source,
        })?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("sln") || e.eq_ignore_ascii_case("slnx"))
        })
        .collect();
    found.sort();
    match found.len() {
        0 => Ok(None),
        1 => Ok(found.pop()),
        _ => Err(DiscoverError::AmbiguousSolution {
            root: root.to_path_buf(),
        }),
    }
}

fn pattern_error(
    key: &'static str,
    pattern: &str,
    reason: &dyn std::fmt::Display,
) -> DiscoverError {
    DiscoverError::Pattern {
        key,
        pattern: pattern.to_owned(),
        reason: reason.to_string(),
    }
}

/// The assemblies the loader options name, sorted.
fn loader_assemblies(root: &Path, options: &DotnetOptions) -> Result<Vec<PathBuf>, DiscoverError> {
    let mut found = Vec::new();
    if let Some(globs) = &options.assemblies {
        let mut builder = globset::GlobSetBuilder::new();
        for glob in globs {
            let compiled =
                globset::Glob::new(glob).map_err(|e| pattern_error("assemblies", glob, &e))?;
            builder.add(compiled);
        }
        let set = builder
            .build()
            .map_err(|e| pattern_error("assemblies", &globs.join(", "), &e))?;
        let keep = |path: &Path| {
            path.strip_prefix(root)
                .is_ok_and(|rel| set.is_match(rel.to_string_lossy().replace('\\', "/")))
        };
        walk(root, &keep, &mut found, false);
    }
    for directory in options.directories.iter().flatten() {
        let filter = directory.filter.as_deref().unwrap_or("*.dll");
        let glob = globset::Glob::new(filter)
            .map_err(|e| pattern_error("directories.filter", filter, &e))?
            .compile_matcher();
        // A directory that does not exist is an error, as `ArchLoader.LoadFilteredDirectory`'s
        // `Directory.GetFiles` throws, never a silently smaller run.
        let dir = root.join(&directory.dir);
        let entries = std::fs::read_dir(&dir).map_err(|source| DiscoverError::Io {
            path: dir.clone(),
            source,
        })?;
        found.extend(
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_file() && p.file_name().is_some_and(|n| glob.is_match(n))),
        );
    }
    found.sort();
    found.dedup();
    Ok(found)
}

/// Discovers the projects under `root`.
///
/// # Errors
/// [`DiscoverError`] when the solution is ambiguous or unreadable, a pattern does not compile, or
/// nothing .NET is found. A project file that cannot be read is recorded in
/// [`Workspace::errors`] instead.
pub fn discover(root: &Path, options: &DotnetOptions) -> Result<Workspace, DiscoverError> {
    let exclude = options
        .exclude_projects
        .as_ref()
        .map(|p| {
            let joined = p.joined();
            regex::Regex::new(&joined).map_err(|e| pattern_error("excludeProjects", &joined, &e))
        })
        .transpose()?;
    let excluded = |path: &Path| {
        exclude.as_ref().is_some_and(|re| {
            let rel = path.strip_prefix(root).unwrap_or(path);
            re.is_match(&rel.to_string_lossy().replace('\\', "/"))
        })
    };
    if options.assemblies.is_some() || options.directories.is_some() {
        let projects: Vec<Project> = loader_assemblies(root, options)?
            .into_iter()
            .filter(|dll| !excluded(dll))
            .map(|dll| Project::loose(&dll))
            .collect();
        if projects.is_empty() {
            return Err(DiscoverError::NothingFound {
                root: root.to_path_buf(),
            });
        }
        return Ok(Workspace {
            solution: None,
            projects,
            errors: Vec::new(),
        });
    }
    let solution = find_solution(root, options)?;
    let paths = if let Some(solution) = &solution {
        solution_projects(solution)?
    } else {
        let mut found = Vec::new();
        let keep = |p: &Path| solution::is_project_file(&p.to_string_lossy());
        walk(root, &keep, &mut found, true);
        found.sort();
        found
    };
    let configuration = options.configuration();
    let mut workspace = Workspace {
        solution,
        projects: Vec::new(),
        errors: Vec::new(),
    };
    for path in paths.into_iter().filter(|p| !excluded(p)) {
        match Project::read(
            &path,
            configuration,
            options.target_framework.as_deref(),
            root,
        ) {
            Ok(project) => workspace.projects.push(project),
            Err(error) => workspace.errors.push((path, error.to_string())),
        }
    }
    if workspace.projects.is_empty() && workspace.errors.is_empty() {
        return Err(DiscoverError::NothingFound {
            root: root.to_path_buf(),
        });
    }
    workspace.projects.sort_by(|a, b| a.path.cmp(&b.path));
    close_package_refs(&mut workspace.projects, configuration, root);
    Ok(workspace)
}

/// Adds to each project the package references of the projects it references, transitively. A
/// referenced project outside the workspace is read for its references; one that cannot be read
/// adds none.
pub fn close_package_refs(projects: &mut [Project], configuration: &str, root: &Path) {
    let mut declared: BTreeMap<PathBuf, (Vec<PathBuf>, Vec<PackageRef>)> = projects
        .iter()
        .map(|p| {
            (
                p.path.clone(),
                (p.project_refs.clone(), p.package_refs.clone()),
            )
        })
        .collect();
    for project in projects.iter_mut() {
        let mut seen_ids: BTreeSet<String> = project
            .package_refs
            .iter()
            .map(|r| r.id.to_ascii_lowercase())
            .collect();
        let mut visited = BTreeSet::from([project.path.clone()]);
        let mut stack = project.project_refs.clone();
        while let Some(path) = stack.pop() {
            if !visited.insert(path.clone()) {
                continue;
            }
            let (refs, packages) = declared
                .entry(path.clone())
                .or_insert_with(|| {
                    ProjectFile::read(&path, configuration, root)
                        .map(|f| (f.project_refs, f.package_refs))
                        .unwrap_or_default()
                })
                .clone();
            for package in packages {
                if seen_ids.insert(package.id.to_ascii_lowercase()) {
                    project.package_refs.push(package);
                }
            }
            stack.extend(refs);
        }
        project
            .package_refs
            .sort_by_key(|r| (r.id.to_ascii_lowercase(), r.id.clone()));
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use rb_model::{DirectoryFilter, options::Patterns};

    pub(crate) fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rb-discover-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    pub(crate) fn write(path: &Path, text: &str) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, text);
    }

    fn names(workspace: &Workspace) -> Vec<&str> {
        workspace
            .projects
            .iter()
            .map(|p| p.assembly_name.as_str())
            .collect()
    }

    #[test]
    fn solution_mode_reads_the_only_solution_and_excludes_by_pattern() {
        let dir = scratch("mode-sln");
        write(
            &dir.join("App.slnx"),
            // The second entry is written through `build/..`: `excludeProjects` still sees `tests/`.
            r#"<Solution><Project Path="src/Web/Web.csproj"/><Project Path="build/../tests/Web.Tests/Web.Tests.csproj"/></Solution>"#,
        );
        write(&dir.join("src/Web/Web.csproj"), "<Project/>");
        write(&dir.join("src/Web/bin/Debug/Web.dll"), "");
        write(
            &dir.join("tests/Web.Tests/Web.Tests.csproj"),
            "<Project><PropertyGroup><IsTestProject>true</IsTestProject></PropertyGroup></Project>",
        );
        let all = discover(&dir, &DotnetOptions::default());
        let all = all.ok();
        assert_eq!(all.as_ref().map(names), Some(vec!["Web", "Web.Tests"]));
        assert_eq!(
            all.as_ref().and_then(|w| w.solution.clone()),
            Some(dir.join("App.slnx"))
        );
        assert_eq!(
            all.as_ref().map(|w| w.projects[1].is_test),
            Some(true),
            "the test project is recognised"
        );
        assert_eq!(
            all.as_ref().map(|w| w.projects[0].assembly.is_some()),
            Some(true)
        );
        let options = DotnetOptions {
            exclude_projects: Some(Patterns::One("^tests/".to_owned())),
            ..DotnetOptions::default()
        };
        assert_eq!(
            discover(&dir, &options).ok().as_ref().map(names),
            Some(vec!["Web"])
        );
        let bad = DotnetOptions {
            exclude_projects: Some(Patterns::One("(".to_owned())),
            ..DotnetOptions::default()
        };
        assert!(matches!(
            discover(&dir, &bad),
            Err(DiscoverError::Pattern { .. })
        ));
        write(&dir.join("Other.sln"), "");
        assert!(matches!(
            discover(&dir, &DotnetOptions::default()),
            Err(DiscoverError::AmbiguousSolution { .. })
        ));
        let named = DotnetOptions {
            solution: Some("App.slnx".to_owned()),
            ..DotnetOptions::default()
        };
        assert_eq!(
            discover(&dir, &named).ok().map(|w| w.projects.len()),
            Some(2)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn package_references_flow_through_project_references() {
        let dir = scratch("transitive");
        write(
            &dir.join("App.slnx"),
            r#"<Solution><Project Path="src/Web/Web.csproj"/><Project Path="src/Core/Core.csproj"/></Solution>"#,
        );
        write(
            &dir.join("src/Web/Web.csproj"),
            r#"<Project><ItemGroup><ProjectReference Include="..\Core\Core.csproj" /><PackageReference Include="serilog" Version="3.0" /></ItemGroup></Project>"#,
        );
        write(
            &dir.join("src/Core/Core.csproj"),
            r#"<Project><ItemGroup><ProjectReference Include="../Outside/Outside.csproj" /><ProjectReference Include="../Web/Web.csproj" /><PackageReference Include="Serilog" Version="2.0" /><PackageReference Include="Newtonsoft.Json" Version="13.0.3" /></ItemGroup></Project>"#,
        );
        write(
            &dir.join("src/Outside/Outside.csproj"),
            r#"<Project><ItemGroup><PackageReference Include="Dapper" Version="2.1" /></ItemGroup></Project>"#,
        );
        let workspace = discover(&dir, &DotnetOptions::default()).ok();
        let packages = |name: &str| -> Vec<(String, Option<String>)> {
            workspace
                .iter()
                .flat_map(|w| &w.projects)
                .filter(|p| p.assembly_name == name)
                .flat_map(|p| &p.package_refs)
                .map(|r| (r.id.clone(), r.version.clone()))
                .collect()
        };
        let pair = |id: &str, v: &str| (id.to_owned(), Some(v.to_owned()));
        assert_eq!(
            packages("Web"),
            vec![
                pair("Dapper", "2.1"),
                pair("Newtonsoft.Json", "13.0.3"),
                pair("serilog", "3.0"),
            ],
            "a cycle back to Web ends; Web's own Serilog version wins"
        );
        assert_eq!(
            packages("Core"),
            vec![
                pair("Dapper", "2.1"),
                pair("Newtonsoft.Json", "13.0.3"),
                pair("Serilog", "2.0"),
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn without_a_solution_every_project_file_is_read_and_output_is_skipped() {
        let dir = scratch("mode-walk");
        write(&dir.join("a/A.csproj"), "<Project/>");
        write(&dir.join("b/B.fsproj"), "<Project/>");
        write(&dir.join("a/bin/Debug/Stray.csproj"), "<Project/>");
        write(&dir.join("c/Broken.csproj"), "<Project");
        let workspace = discover(&dir, &DotnetOptions::default()).ok();
        assert_eq!(workspace.as_ref().map(names), Some(vec!["A", "B"]));
        assert_eq!(workspace.as_ref().map(|w| w.errors.len()), Some(1));
        assert_eq!(workspace.and_then(|w| w.solution), None);
        let empty = scratch("mode-empty");
        assert!(matches!(
            discover(&empty, &DotnetOptions::default()),
            Err(DiscoverError::NothingFound { .. })
        ));
        assert!(matches!(
            discover(&empty.join("missing"), &DotnetOptions::default()),
            Err(DiscoverError::Io { .. })
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&empty);
    }

    #[test]
    fn loader_mode_reads_globbed_and_filtered_assemblies() {
        let dir = scratch("mode-loader");
        write(&dir.join("out/App.Core.dll"), "");
        write(&dir.join("out/App.Web.dll"), "");
        write(&dir.join("out/Other.dll"), "");
        write(&dir.join("libs/Lib.dll"), "");
        write(&dir.join("libs/sub/Deep.dll"), "");
        let options = DotnetOptions {
            assemblies: Some(vec!["out/App.*.dll".to_owned()]),
            directories: Some(vec![DirectoryFilter {
                dir: "libs".to_owned(),
                filter: None,
            }]),
            ..DotnetOptions::default()
        };
        let workspace = discover(&dir, &options).ok();
        assert_eq!(
            workspace.as_ref().map(names),
            Some(vec!["Lib", "App.Core", "App.Web"])
        );
        let loose = workspace.as_ref().map(|w| w.projects[0].clone());
        assert_eq!(
            loose
                .as_ref()
                .map(|p| (p.root_namespace.as_str(), p.is_test)),
            Some(("Lib", false))
        );
        assert_eq!(
            loose.as_ref().map(Project::folder),
            Some(dir.join("libs").as_path())
        );
        let filtered = DotnetOptions {
            directories: Some(vec![DirectoryFilter {
                dir: "out".to_owned(),
                filter: Some("Other.*".to_owned()),
            }]),
            ..DotnetOptions::default()
        };
        assert_eq!(
            discover(&dir, &filtered).ok().as_ref().map(names),
            Some(vec!["Other"])
        );
        let missing = DotnetOptions {
            directories: Some(vec![DirectoryFilter {
                dir: "no-such-dir".to_owned(),
                filter: None,
            }]),
            ..DotnetOptions::default()
        };
        assert!(
            matches!(discover(&dir, &missing), Err(DiscoverError::Io { path, .. }) if path == dir.join("no-such-dir")),
            "a missing directory names itself"
        );
        let none = DotnetOptions {
            assemblies: Some(vec!["nothing/*.dll".to_owned()]),
            ..DotnetOptions::default()
        };
        assert!(matches!(
            discover(&dir, &none),
            Err(DiscoverError::NothingFound { .. })
        ));
        let bad = DotnetOptions {
            assemblies: Some(vec!["[".to_owned()]),
            ..DotnetOptions::default()
        };
        assert!(matches!(
            discover(&dir, &bad),
            Err(DiscoverError::Pattern { .. })
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
