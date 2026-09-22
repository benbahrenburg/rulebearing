//! Just enough MSBuild reading to find each project's built assembly and PDB.
//!
//! - Plan: [Wave 0, Step 9](../../../docs/plans/pending/0000-wave-0-spike.md#step-9-spike-b-rb-extract-dotnet-0d)
//!   (`msbuild.rs`: "just enough for the spike; the full discovery of FR-EXT-DN-01 is wave 2")
//! - Requirement: [FR-EXT-DN-01](../../../docs/prd.md#fr-ext-dn-01) (partial)
//! - Architecture: [Extractors](../../../docs/architecture.md#extractors) (discovery row)
//!
//! It reads the solution's project list, each project's properties and the `Directory.Build.props`
//! files above it, without evaluating MSBuild. A value may name `$(MSBuildProjectName)`,
//! `$(Configuration)`, `$(TargetFramework)` or another property the project or its props define;
//! anything still unexpanded after that is treated as unset. The built assembly is looked for where the SDK puts it (`OutputPath`, or
//! `bin/<Configuration>/<TargetFramework>/`, or the `artifacts/` layout), and found by searching
//! the output folders for `<AssemblyName>.dll` when the conventional place is empty.

use std::path::{Path, PathBuf};

/// A project file and where its build put the assembly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    /// The project file.
    pub path: PathBuf,
    /// `AssemblyName`, defaulting to the file stem.
    pub assembly_name: String,
    /// The target frameworks, first one first.
    pub target_frameworks: Vec<String>,
    /// Whether the project is a test project.
    pub is_test: bool,
    /// The built assembly, when found.
    pub assembly: Option<PathBuf>,
}

/// Why a solution could not be read.
#[derive(Debug, thiserror::Error)]
pub enum SolutionError {
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
}

fn read(path: &Path) -> Result<String, SolutionError> {
    std::fs::read_to_string(path).map_err(|source| SolutionError::Io {
        path: path.to_path_buf(),
        source,
    })
}

/// The project files a `.sln` or `.slnx` lists, resolved against the solution's folder.
///
/// # Errors
/// When the solution cannot be read or a `.slnx` is not XML.
pub fn solution_projects(solution: &Path) -> Result<Vec<PathBuf>, SolutionError> {
    let text = read(solution)?;
    let folder = solution.parent().unwrap_or(Path::new("."));
    let mut relative: Vec<String> = Vec::new();
    if solution
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("slnx"))
    {
        let document = roxmltree::Document::parse(&text).map_err(|e| SolutionError::Xml {
            path: solution.to_path_buf(),
            reason: e.to_string(),
        })?;
        relative.extend(
            document
                .descendants()
                .filter(|n| n.has_tag_name("Project"))
                .filter_map(|n| n.attribute("Path"))
                .map(str::to_owned),
        );
    } else {
        for line in text
            .lines()
            .filter(|l| l.trim_start().starts_with("Project("))
        {
            // Project("{type}") = "Name", "relative\path.csproj", "{guid}"
            if let Some(path) = line.split('"').nth(5) {
                relative.push(path.to_owned());
            }
        }
    }
    let mut projects: Vec<PathBuf> = relative
        .into_iter()
        .map(|p| p.replace('\\', "/"))
        .filter(|p| {
            let lower = p.to_ascii_lowercase();
            lower.ends_with(".csproj") || lower.ends_with(".fsproj") || lower.ends_with(".vbproj")
        })
        .map(|p| folder.join(p))
        .collect();
    projects.sort();
    projects.dedup();
    Ok(projects)
}

/// Properties read from one project or props file, first value wins.
#[derive(Debug, Default, Clone)]
struct Properties {
    values: Vec<(String, String)>,
    test_sdk: bool,
}

impl Properties {
    fn read(path: &Path) -> Result<Self, SolutionError> {
        let text = read(path)?;
        let document = roxmltree::Document::parse(&text).map_err(|e| SolutionError::Xml {
            path: path.to_path_buf(),
            reason: e.to_string(),
        })?;
        let mut properties = Self::default();
        for node in document.descendants().filter(roxmltree::Node::is_element) {
            let parent_is_group = node
                .parent_element()
                .is_some_and(|p| p.has_tag_name("PropertyGroup"));
            if parent_is_group {
                let value = node.text().unwrap_or_default().trim().to_owned();
                if !value.is_empty() {
                    properties
                        .values
                        .push((node.tag_name().name().to_owned(), value));
                }
            }
            if node.has_tag_name("PackageReference")
                && node
                    .attribute("Include")
                    .is_some_and(|i| i == "Microsoft.NET.Test.Sdk")
            {
                properties.test_sdk = true;
            }
        }
        Ok(properties)
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.values
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// The `Directory.Build.props` files from the project's folder up to `root`, nearest first.
fn props_above(project: &Path, root: &Path) -> Vec<(PathBuf, Properties)> {
    let mut found = Vec::new();
    let mut folder = project.parent();
    while let Some(current) = folder {
        let props = current.join("Directory.Build.props");
        if let Ok(properties) = Properties::read(&props) {
            found.push((current.to_path_buf(), properties));
        }
        if current == root {
            break;
        }
        folder = current.parent();
    }
    found
}

impl Project {
    /// Reads a project and locates its built assembly for `configuration`.
    ///
    /// # Errors
    /// When the project file cannot be read or is not XML.
    pub fn read(path: &Path, configuration: &str, root: &Path) -> Result<Self, SolutionError> {
        let own = Properties::read(path)?;
        let props = props_above(path, root);
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_owned();
        let raw = |name: &str| -> Option<String> {
            own.get(name)
                .or_else(|| props.iter().find_map(|(_, p)| p.get(name)))
                .map(str::to_owned)
        };
        let lookup = |name: &str| -> Option<String> {
            let value = raw(name)?;
            let expanded = expand(&value, &|property: &str| match property {
                "MSBuildProjectName" => Some(stem.clone()),
                "Configuration" => Some(configuration.to_owned()),
                other if other.eq_ignore_ascii_case(name) => None,
                other => raw(other),
            });
            (!expanded.contains("$(")).then_some(expanded)
        };
        let assembly_name = lookup("AssemblyName").unwrap_or_else(|| stem.clone());
        let mut target_frameworks: Vec<String> = lookup("TargetFramework")
            .or_else(|| lookup("TargetFrameworks"))
            .map(|v| {
                v.split(';')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        target_frameworks.dedup();
        let is_test =
            own.test_sdk || lookup("IsTestProject").is_some_and(|v| v.eq_ignore_ascii_case("true"));
        let folder = path.parent().unwrap_or(Path::new("."));
        let artifacts_root = props
            .iter()
            .find(|(_, p)| {
                p.get("UseArtifactsOutput")
                    .is_some_and(|v| v.eq_ignore_ascii_case("true"))
            })
            .map(|(dir, p)| {
                p.get("ArtifactsPath")
                    .map_or_else(|| dir.join("artifacts"), |a| dir.join(a))
            });
        let output_path = lookup("OutputPath");
        let file = format!("{assembly_name}.dll");

        let mut candidates: Vec<PathBuf> = Vec::new();
        for tfm in target_frameworks
            .iter()
            .map(String::as_str)
            .chain(std::iter::once(""))
        {
            if let Some(output) = &output_path {
                candidates.push(folder.join(output.replace('\\', "/")).join(tfm).join(&file));
            }
            candidates.push(folder.join("bin").join(configuration).join(tfm).join(&file));
            if let Some(artifacts) = &artifacts_root {
                let pivot = if tfm.is_empty() {
                    configuration.to_ascii_lowercase()
                } else {
                    format!("{}_{tfm}", configuration.to_ascii_lowercase())
                };
                candidates.push(artifacts.join("bin").join(&stem).join(pivot).join(&file));
            }
        }
        let mut assembly = candidates.into_iter().find(|c| c.is_file());
        if assembly.is_none() {
            let mut roots = vec![folder.join("bin")];
            if let Some(artifacts) = &artifacts_root {
                roots.push(artifacts.join("bin").join(&stem));
            }
            let mut found = Vec::new();
            for search in roots {
                find_file(&search, &file, 6, &mut found);
            }
            found.sort();
            let config = configuration.to_ascii_lowercase();
            assembly = found
                .iter()
                .find(|p| p.to_string_lossy().to_ascii_lowercase().contains(&config))
                .or_else(|| found.first())
                .cloned();
        }
        Ok(Self {
            path: path.to_path_buf(),
            assembly_name,
            target_frameworks,
            is_test,
            assembly,
        })
    }
}

/// Expands `$(Name)` references through `resolve`, a few levels deep; an unknown name is left
/// as written so the caller can see the value is not fully known.
fn expand(value: &str, resolve: &dyn Fn(&str) -> Option<String>) -> String {
    let mut current = value.to_owned();
    for _ in 0..4 {
        let mut next = String::with_capacity(current.len());
        let mut rest = current.as_str();
        let mut changed = false;
        while let Some(start) = rest.find("$(") {
            next.push_str(&rest[..start]);
            let after = &rest[start + 2..];
            let Some(end) = after.find(')') else {
                next.push_str(&rest[start..]);
                rest = "";
                break;
            };
            match resolve(&after[..end]) {
                Some(value) => {
                    next.push_str(&value);
                    changed = true;
                }
                None => next.push_str(&rest[start..start + 3 + end]),
            }
            rest = &after[end + 1..];
        }
        next.push_str(rest);
        current = next;
        if !changed {
            break;
        }
    }
    current
}

fn find_file(dir: &Path, name: &str, depth: usize, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if depth > 0 && !path.ends_with("ref") && !path.ends_with("refint") {
                find_file(&path, name, depth - 1, found);
            }
        } else if path.file_name().is_some_and(|n| n == name) {
            found.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rb-msbuild-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    fn write(path: &Path, text: &str) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, text);
    }

    #[test]
    fn lists_projects_from_sln_and_slnx() {
        let dir = scratch("sln");
        write(
            &dir.join("A.sln"),
            "Microsoft Visual Studio Solution File\nProject(\"{FAE04EC0}\") = \"Web\", \"src\\Web\\Web.csproj\", \"{1}\"\nEndProject\nProject(\"{2150E333}\") = \"Items\", \"Items\", \"{2}\"\nEndProject\n",
        );
        write(
            &dir.join("B.slnx"),
            r#"<Solution><Folder Name="/src/"><Project Path="src/Core/Core.csproj" /></Folder><Project Path="tests/T.fsproj"/></Solution>"#,
        );
        assert_eq!(
            solution_projects(&dir.join("A.sln")).ok(),
            Some(vec![dir.join("src/Web/Web.csproj")])
        );
        assert_eq!(
            solution_projects(&dir.join("B.slnx")).ok(),
            Some(vec![
                dir.join("src/Core/Core.csproj"),
                dir.join("tests/T.fsproj")
            ])
        );
        write(&dir.join("C.slnx"), "<Solution");
        assert!(matches!(
            solution_projects(&dir.join("C.slnx")),
            Err(SolutionError::Xml { .. })
        ));
        assert!(matches!(
            solution_projects(&dir.join("missing.sln")),
            Err(SolutionError::Io { .. })
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn finds_the_conventional_output_and_reads_properties() {
        let dir = scratch("proj");
        write(
            &dir.join("Directory.Build.props"),
            "<Project><PropertyGroup><TargetFramework>net8.0</TargetFramework></PropertyGroup></Project>",
        );
        write(
            &dir.join("src/Core/Core.csproj"),
            "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><AssemblyName>My.Core</AssemblyName><RootNamespace>$(X)</RootNamespace></PropertyGroup><ItemGroup><PackageReference Include=\"Microsoft.NET.Test.Sdk\" /></ItemGroup></Project>",
        );
        write(&dir.join("src/Core/bin/Release/net8.0/My.Core.dll"), "");
        let project = Project::read(&dir.join("src/Core/Core.csproj"), "Release", &dir);
        let project = project.ok();
        assert_eq!(
            project.as_ref().map(|p| p.assembly_name.as_str()),
            Some("My.Core")
        );
        assert_eq!(
            project.as_ref().map(|p| p.target_frameworks.clone()),
            Some(vec!["net8.0".to_owned()])
        );
        assert_eq!(project.as_ref().map(|p| p.is_test), Some(true));
        assert_eq!(
            project.and_then(|p| p.assembly),
            Some(dir.join("src/Core/bin/Release/net8.0/My.Core.dll"))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn expands_known_properties_and_leaves_unknown_ones() {
        let resolve = |name: &str| match name {
            "A" => Some("x$(B)".to_owned()),
            "B" => Some("y".to_owned()),
            _ => None,
        };
        assert_eq!(expand("pre.$(A).post", &resolve), "pre.xy.post");
        assert_eq!(expand("$(Unknown)", &resolve), "$(Unknown)");
        assert_eq!(expand("$(A", &resolve), "$(A");
        assert_eq!(expand("plain", &resolve), "plain");
        let looping = |_: &str| Some("$(Self)".to_owned());
        assert_eq!(expand("$(Self)", &looping), "$(Self)");
    }

    #[test]
    fn assembly_names_built_from_the_project_name_are_found() {
        let dir = scratch("expand");
        write(
            &dir.join("Directory.Build.props"),
            "<Project><PropertyGroup><AssemblyName>Org.$(MSBuildProjectName)</AssemblyName><TargetFramework>net10.0</TargetFramework></PropertyGroup></Project>",
        );
        write(&dir.join("Api/Api.csproj"), "<Project/>");
        write(&dir.join("Api/bin/Release/net10.0/Org.Api.dll"), "");
        let project = Project::read(&dir.join("Api/Api.csproj"), "Release", &dir).ok();
        assert_eq!(
            project.as_ref().map(|p| p.assembly_name.as_str()),
            Some("Org.Api")
        );
        assert_eq!(
            project.and_then(|p| p.assembly),
            Some(dir.join("Api/bin/Release/net10.0/Org.Api.dll"))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn falls_back_to_the_artifacts_layout_and_to_a_search() {
        let dir = scratch("artifacts");
        write(
            &dir.join("Directory.Build.props"),
            "<Project><PropertyGroup><UseArtifactsOutput>true</UseArtifactsOutput></PropertyGroup></Project>",
        );
        write(
            &dir.join("src/Api/Api.csproj"),
            "<Project><PropertyGroup><TargetFrameworks>net9.0;net8.0</TargetFrameworks><IsTestProject>false</IsTestProject></PropertyGroup></Project>",
        );
        write(&dir.join("artifacts/bin/Api/release_net9.0/Api.dll"), "");
        let project = Project::read(&dir.join("src/Api/Api.csproj"), "Release", &dir).ok();
        assert_eq!(project.as_ref().map(|p| p.target_frameworks.len()), Some(2));
        assert_eq!(
            project.and_then(|p| p.assembly),
            Some(dir.join("artifacts/bin/Api/release_net9.0/Api.dll"))
        );

        write(
            &dir.join("src/Odd/Odd.csproj"),
            "<Project><PropertyGroup><OutputPath>out\\</OutputPath></PropertyGroup></Project>",
        );
        write(&dir.join("src/Odd/bin/x/Release/y/Odd.dll"), "");
        let project = Project::read(&dir.join("src/Odd/Odd.csproj"), "Release", &dir).ok();
        assert_eq!(
            project.and_then(|p| p.assembly),
            Some(dir.join("src/Odd/bin/x/Release/y/Odd.dll"))
        );

        write(&dir.join("src/None/None.csproj"), "<Project/>");
        let project = Project::read(&dir.join("src/None/None.csproj"), "Release", &dir).ok();
        assert_eq!(project.and_then(|p| p.assembly), None);
        assert!(Project::read(&dir.join("src/Missing/M.csproj"), "Release", &dir).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
