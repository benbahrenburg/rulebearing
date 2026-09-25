//! SDK-style project files and the `Directory.*.props` files above them, read without evaluating
//! MSBuild.
//!
//! - Plan: [Wave 2, Step 1](../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#21-step-1-net-discovery-and-the-loader-options-2a)
//!   (`csproj.rs`: `Directory.Build.props` and `Directory.Packages.props` merged by MSBuild's
//!   walk-up rule, `ProjectReference`, `PackageReference`, `IsTestProject`, `OutputPath`,
//!   `TargetFramework(s)`, `AssemblyName`, `RootNamespace`)
//! - Requirement: [FR-EXT-DN-01](../../../../docs/prd.md#fr-ext-dn-01)
//!
//! A repository on the Arcade SDK (a `Directory.Build.props` that imports
//! `Microsoft.DotNet.Arcade.Sdk`, as dotnet/aspnetcore does) builds every project to
//! `artifacts/bin/<OutDirName>/<Configuration>/<TFM>/` under the folder holding `global.json`,
//! `OutDirName` defaulting to the project name (Arcade's documented "Build output layout", set in
//! its `RepoLayout.props` and `ProjectLayout.props`); [`ProjectFile::arcade_output`] records that
//! folder. An `ArtifactsDir` or `OutDirName` the repository sets is honoured when it expands.
//!
//! A property is looked up in the project first, then in each `Directory.Build.props` from the
//! project's folder upwards, first value wins. A value may name `$(MSBuildProjectName)`,
//! `$(Configuration)`, `$(TargetFramework)` or another property the project or its props define;
//! anything still unexpanded after that is treated as unset. A `PackageReference` without a
//! `Version` takes the version the nearest `Directory.Packages.props` gives its id (central
//! package management). An expansion longer than 32 KiB (a property defined through itself many
//! times over) is treated as unset too.
//!
//! MSBuild conditions are not evaluated: an element carrying a `Condition` attribute, or inside
//! one that does (a conditioned `PropertyGroup` or `ItemGroup`, a `Choose`/`When`/`Otherwise`),
//! is skipped, so a property, package or import that applies only under a condition applies to
//! no project rather than to every project. A `Directory.Build.props` that adds a test framework
//! under `Condition="'$(IsTestProject)' == 'true'"` therefore does not make every project a test
//! project.
//!
//! `ArtifactsPath` may name `$(MSBuildThisFileDirectory)` (the props file's folder, as .NET's
//! documentation writes it) or a property the same props file defines; `\` is read as `/`, and a
//! value that does not fully expand is treated as unset (the `artifacts/` default).

use std::path::{Path, PathBuf};

use super::{DiscoverError, read};

/// One `PackageReference`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PackageRef {
    /// The package id, as written.
    pub id: String,
    /// The version, from the reference or from central package management.
    pub version: Option<String>,
}

/// What one project or props file declares.
#[derive(Debug, Default, Clone)]
pub struct Properties {
    values: Vec<(String, String)>,
    project_refs: Vec<String>,
    package_refs: Vec<(String, Option<String>)>,
    package_versions: Vec<(String, String)>,
    sdk_imports: Vec<String>,
}

impl Properties {
    /// Reads the properties and items of one MSBuild XML file.
    ///
    /// # Errors
    /// When the file cannot be read or is not XML.
    pub fn read(path: &Path) -> Result<Self, DiscoverError> {
        let text = read(path)?;
        Self::parse(&text).map_err(|reason| DiscoverError::Xml {
            path: path.to_path_buf(),
            reason,
        })
    }

    fn parse(text: &str) -> Result<Self, String> {
        let document = roxmltree::Document::parse(text).map_err(|e| e.to_string())?;
        let mut properties = Self::default();
        for node in document
            .descendants()
            .filter(roxmltree::Node::is_element)
            .filter(|n| !conditional(*n))
        {
            let parent = node.parent_element().map(|p| p.tag_name().name());
            let value = || node.text().unwrap_or_default().trim().to_owned();
            if parent == Some("PropertyGroup") {
                let text = value();
                if !text.is_empty() {
                    properties
                        .values
                        .push((node.tag_name().name().to_owned(), text));
                }
                continue;
            }
            let version = || {
                node.attribute("Version").map(str::to_owned).or_else(|| {
                    node.children()
                        .find(|c| c.has_tag_name("Version"))
                        .and_then(|c| c.text())
                        .map(|t| t.trim().to_owned())
                })
            };
            if node.has_tag_name("Import") {
                if let Some(sdk) = node.attribute("Sdk") {
                    properties.sdk_imports.push(sdk.trim().to_owned());
                }
                continue;
            }
            match (node.tag_name().name(), node.attribute("Include")) {
                ("ProjectReference", Some(include)) => {
                    properties.project_refs.push(include.to_owned());
                }
                ("PackageReference", Some(include)) => {
                    properties
                        .package_refs
                        .push((include.to_owned(), version()));
                }
                ("PackageVersion", Some(include)) => {
                    if let Some(v) = version() {
                        properties.package_versions.push((include.to_owned(), v));
                    }
                }
                _ => {}
            }
        }
        Ok(properties)
    }

    /// The first value of a property, case-insensitively.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.values
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// Whether the file imports the MSBuild SDK `sdk` (`<Import Project="..." Sdk="sdk" />`).
    pub fn imports_sdk(&self, sdk: &str) -> bool {
        self.sdk_imports.iter().any(|s| s.eq_ignore_ascii_case(sdk))
    }

    /// The central version of a package id.
    fn central_version(&self, id: &str) -> Option<&str> {
        self.package_versions
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(id))
            .map(|(_, v)| v.as_str())
    }
}

/// Whether an element applies only under an MSBuild condition this reader does not evaluate: it
/// or an ancestor carries `Condition`, or it sits inside a `Choose`.
fn conditional(node: roxmltree::Node<'_, '_>) -> bool {
    node.ancestors()
        .filter(roxmltree::Node::is_element)
        .any(|n| n.attribute("Condition").is_some() || n.has_tag_name("Choose"))
}

/// The `name` files (`Directory.Build.props`, `Directory.Packages.props`) from the project's
/// folder up to `root`, nearest first.
fn files_above(project: &Path, root: &Path, name: &str) -> Vec<(PathBuf, Properties)> {
    let mut found = Vec::new();
    let mut folder = project.parent();
    while let Some(current) = folder {
        if let Ok(properties) = Properties::read(&current.join(name)) {
            found.push((current.to_path_buf(), properties));
        }
        if current == root {
            break;
        }
        folder = current.parent();
    }
    found
}

/// The Arcade SDK's package name, as a `Directory.Build.props` imports it.
pub const ARCADE_SDK: &str = "Microsoft.DotNet.Arcade.Sdk";

/// The nearest folder from the project's up to `root` holding `global.json`: Arcade's
/// `RepoRoot`.
fn global_json_folder(project: &Path, root: &Path) -> Option<PathBuf> {
    let mut folder = project.parent();
    while let Some(current) = folder {
        if current.join("global.json").is_file() {
            return Some(current.to_path_buf());
        }
        if current == root {
            return None;
        }
        folder = current.parent();
    }
    None
}

/// The .NET 8 `artifacts/` folder, when a `Directory.Build.props` turns `UseArtifactsOutput`
/// on: its `ArtifactsPath` expanded (`$(MSBuildThisFileDirectory)` is the props file's folder),
/// else `artifacts/` beside it.
fn artifacts_output(props: &[(PathBuf, Properties)]) -> Option<PathBuf> {
    let (dir, p) = props.iter().find(|(_, p)| {
        p.get("UseArtifactsOutput")
            .is_some_and(|v| v.eq_ignore_ascii_case("true"))
    })?;
    let this_file_directory = format!("{}/", dir.display());
    let path = p
        .get("ArtifactsPath")
        .map(|value| {
            expand(value, &|name: &str| {
                if name.eq_ignore_ascii_case("MSBuildThisFileDirectory") {
                    Some(this_file_directory.clone())
                } else if name.eq_ignore_ascii_case("ArtifactsPath") {
                    None
                } else {
                    p.get(name).map(str::to_owned)
                }
            })
        })
        .filter(|expanded| !expanded.contains("$(") && !expanded.trim().is_empty())
        .map_or_else(
            || dir.join("artifacts"),
            |expanded| dir.join(expanded.trim().replace('\\', "/")),
        );
    Some(normalise(&path))
}

/// A project file read, before its output is located.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectFile {
    /// The project file.
    pub path: PathBuf,
    /// The file stem (`MSBuildProjectName`).
    pub stem: String,
    /// `AssemblyName`, defaulting to the file stem.
    pub assembly_name: String,
    /// `RootNamespace`, defaulting to the assembly name.
    pub root_namespace: String,
    /// The target frameworks, first one first.
    pub target_frameworks: Vec<String>,
    /// `IsTestProject`, or a reference to `Microsoft.NET.Test.Sdk`.
    pub is_test: bool,
    /// `OutputPath`, when set.
    pub output_path: Option<String>,
    /// The `artifacts/` folder, when `UseArtifactsOutput` is on.
    pub artifacts: Option<PathBuf>,
    /// The Arcade SDK's `artifacts/bin/<OutDirName>/` folder, when the repository builds with
    /// Arcade; the assembly is under `<Configuration>/<TFM>/` in it.
    pub arcade_output: Option<PathBuf>,
    /// Referenced projects, resolved and sorted.
    pub project_refs: Vec<PathBuf>,
    /// Referenced packages, sorted by id.
    pub package_refs: Vec<PackageRef>,
}

impl ProjectFile {
    /// Reads a project file and the props files above it, expanding properties for
    /// `configuration`.
    ///
    /// # Errors
    /// When the project file cannot be read or is not XML.
    pub fn read(path: &Path, configuration: &str, root: &Path) -> Result<Self, DiscoverError> {
        let own = Properties::read(path)?;
        let props = files_above(path, root, "Directory.Build.props");
        let central = files_above(path, root, "Directory.Packages.props");
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_owned();
        let repo_root = props
            .iter()
            .any(|(_, p)| p.imports_sdk(ARCADE_SDK))
            .then(|| global_json_folder(path, root))
            .flatten();
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
                "RepoRoot" => repo_root.as_ref().map(|r| format!("{}/", r.display())),
                other if other.eq_ignore_ascii_case(name) => None,
                other => raw(other),
            });
            (!expanded.contains("$(")).then_some(expanded)
        };
        let assembly_name = lookup("AssemblyName").unwrap_or_else(|| stem.clone());
        let root_namespace = lookup("RootNamespace").unwrap_or_else(|| assembly_name.clone());
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
        let package_refs: Vec<(String, Option<String>)> = own
            .package_refs
            .iter()
            .chain(props.iter().flat_map(|(_, p)| &p.package_refs))
            .cloned()
            .collect();
        let is_test = package_refs
            .iter()
            .any(|(id, _)| id.eq_ignore_ascii_case("Microsoft.NET.Test.Sdk"))
            || lookup("IsTestProject").is_some_and(|v| v.eq_ignore_ascii_case("true"));
        let artifacts = artifacts_output(&props);
        let output_path = lookup("OutputPath");
        let folder = path.parent().unwrap_or(Path::new("."));
        // A relative `ArtifactsDir` is relative to the project, as any MSBuild path property is;
        // Arcade's own default is absolute (`$(RepoRoot)artifacts/`).
        let arcade_output = repo_root.as_ref().map(|repo| {
            let artifacts = lookup("ArtifactsDir").map_or_else(
                || repo.join("artifacts"),
                |dir| normalise(&folder.join(dir.replace('\\', "/"))),
            );
            let name = lookup("OutDirName").unwrap_or_else(|| stem.clone());
            artifacts.join("bin").join(name.replace('\\', "/"))
        });
        let mut project_refs: Vec<PathBuf> = own
            .project_refs
            .iter()
            .map(|r| normalise(&folder.join(r.replace('\\', "/"))))
            .collect();
        project_refs.sort();
        project_refs.dedup();
        let mut packages: Vec<PackageRef> = package_refs
            .into_iter()
            .map(|(id, version)| {
                let version = version.or_else(|| {
                    central
                        .iter()
                        .find_map(|(_, c)| c.central_version(&id))
                        .map(str::to_owned)
                });
                PackageRef { id, version }
            })
            .collect();
        sort_packages(&mut packages);
        Ok(Self {
            path: path.to_path_buf(),
            stem,
            assembly_name,
            root_namespace,
            target_frameworks,
            is_test,
            output_path,
            artifacts,
            arcade_output,
            project_refs,
            package_refs: packages,
        })
    }
}

/// Sorts package references by id case-insensitively first, so every spelling of one id is
/// adjacent, then keeps one per id.
fn sort_packages(packages: &mut Vec<PackageRef>) {
    packages.sort_by(|a, b| {
        (a.id.to_ascii_lowercase(), &a.id, &a.version).cmp(&(
            b.id.to_ascii_lowercase(),
            &b.id,
            &b.version,
        ))
    });
    packages.dedup_by(|a, b| a.id.eq_ignore_ascii_case(&b.id));
}

/// Removes `.` and `a/..` components without touching the file system, so a `ProjectReference`
/// written as `..\Core\Core.csproj` compares equal to the solution's own path for it.
pub fn normalise(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// The longest value [`expand`] builds before giving up on it.
pub const MAX_EXPANDED: usize = 32 * 1024;

/// Expands `$(Name)` references through `resolve`, a few levels deep; an unknown name is left
/// as written so the caller can see the value is not fully known. An expansion that grows past
/// [`MAX_EXPANDED`] (four levels of a property naming itself many times) returns `value`
/// unexpanded, so it reads as not fully known too.
pub fn expand(value: &str, resolve: &dyn Fn(&str) -> Option<String>) -> String {
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
            if next.len() > MAX_EXPANDED {
                return value.to_owned();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discover::tests::{scratch, write};

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
    fn normalise_resolves_dots_lexically() {
        assert_eq!(
            normalise(Path::new("/r/src/Web/../Core/./Core.csproj")),
            PathBuf::from("/r/src/Core/Core.csproj")
        );
        assert_eq!(normalise(Path::new("../x")), PathBuf::from("../x"));
        assert_eq!(normalise(Path::new("a/../../b")), PathBuf::from("../b"));
    }

    #[test]
    fn reads_properties_references_and_central_versions() {
        let dir = scratch("csproj");
        write(
            &dir.join("Directory.Build.props"),
            "<Project><PropertyGroup><TargetFramework>net8.0</TargetFramework><RootNamespace>Org.$(MSBuildProjectName)</RootNamespace></PropertyGroup><ItemGroup><PackageReference Include=\"Shared.Analyzers\" Version=\"2.0\" /></ItemGroup></Project>",
        );
        write(
            &dir.join("Directory.Packages.props"),
            "<Project><ItemGroup><PackageVersion Include=\"Newtonsoft.Json\" Version=\"13.0.3\" /><PackageVersion Include=\"NoVersion\" /></ItemGroup></Project>",
        );
        write(
            &dir.join("src/Web/Web.csproj"),
            r#"<Project Sdk="Microsoft.NET.Sdk.Web"><PropertyGroup><AssemblyName>My.Web</AssemblyName><OutputPath>out\</OutputPath></PropertyGroup>
<ItemGroup><ProjectReference Include="..\Core\Core.csproj" /><ProjectReference Include="../Core/Core.csproj" />
<PackageReference Include="Newtonsoft.Json" /><PackageReference Include="Serilog"><Version>3.1.1</Version></PackageReference>
<PackageReference Include="NoVersion" /></ItemGroup></Project>"#,
        );
        let project = ProjectFile::read(&dir.join("src/Web/Web.csproj"), "Debug", &dir);
        let project = project.ok();
        let p = project.as_ref();
        assert_eq!(p.map(|p| p.assembly_name.as_str()), Some("My.Web"));
        assert_eq!(p.map(|p| p.root_namespace.as_str()), Some("Org.Web"));
        assert_eq!(p.map(|p| p.stem.as_str()), Some("Web"));
        assert_eq!(p.and_then(|p| p.output_path.as_deref()), Some("out\\"));
        assert_eq!(
            p.map(|p| p.project_refs.clone()),
            Some(vec![dir.join("src/Core/Core.csproj")])
        );
        let packages: Vec<(String, Option<String>)> = p
            .map(|p| {
                p.package_refs
                    .iter()
                    .map(|r| (r.id.clone(), r.version.clone()))
                    .collect()
            })
            .unwrap_or_default();
        assert_eq!(
            packages,
            vec![
                ("Newtonsoft.Json".to_owned(), Some("13.0.3".to_owned())),
                ("NoVersion".to_owned(), None),
                ("Serilog".to_owned(), Some("3.1.1".to_owned())),
                ("Shared.Analyzers".to_owned(), Some("2.0".to_owned())),
            ]
        );
        assert_eq!(p.map(|p| p.is_test), Some(false));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_test_sdk_reference_or_the_property_makes_a_test_project() {
        let dir = scratch("testproj");
        write(
            &dir.join("A/A.csproj"),
            "<Project><ItemGroup><PackageReference Include=\"microsoft.net.test.sdk\" /></ItemGroup></Project>",
        );
        write(
            &dir.join("B/B.csproj"),
            "<Project><PropertyGroup><IsTestProject>True</IsTestProject></PropertyGroup></Project>",
        );
        write(&dir.join("C/C.csproj"), "<Project/>");
        for (name, expected) in [("A/A", true), ("B/B", true), ("C/C", false)] {
            let path = dir.join(format!("{name}.csproj"));
            let project = ProjectFile::read(&path, "Debug", &dir).ok();
            assert_eq!(project.map(|p| p.is_test), Some(expected), "{name}");
        }
        write(&dir.join("D/D.csproj"), "<Project");
        assert!(matches!(
            ProjectFile::read(&dir.join("D/D.csproj"), "Debug", &dir),
            Err(DiscoverError::Xml { .. })
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_self_multiplying_property_is_unset_not_a_billion_laughs() {
        let dir = scratch("laughs");
        let mut group = String::from("<L0>lol</L0>");
        for level in 1..=8 {
            let body = format!("$(L{})", level - 1).repeat(10);
            group.extend([format!("<L{level}>"), body, format!("</L{level}>")]);
        }
        write(
            &dir.join("Directory.Build.props"),
            &format!(
                "<Project><PropertyGroup>{group}<AssemblyName>$(L8)</AssemblyName><RootNamespace>$(L1)</RootNamespace></PropertyGroup></Project>"
            ),
        );
        write(&dir.join("A/A.csproj"), "<Project/>");
        let project = ProjectFile::read(&dir.join("A/A.csproj"), "Debug", &dir).ok();
        assert_eq!(
            project.as_ref().map(|p| p.assembly_name.as_str()),
            Some("A"),
            "an expansion past the cap is unset"
        );
        assert_eq!(
            project.as_ref().map(|p| p.root_namespace.len()),
            Some(30),
            "a small expansion still expands"
        );
        let wide = |_: &str| Some("$(X)".repeat(20_000));
        assert_eq!(expand("$(X)", &wide), "$(X)");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn conditioned_properties_items_and_choices_are_skipped() {
        let dir = scratch("conditions");
        write(
            &dir.join("Directory.Build.props"),
            r#"<Project>
  <ItemGroup Condition="'$(IsTestProject)' == 'true'"><PackageReference Include="Microsoft.NET.Test.Sdk" /><PackageReference Include="xunit" /></ItemGroup>
  <PropertyGroup Condition="'$(Configuration)' == 'Release'"><AssemblyName>Wrong</AssemblyName></PropertyGroup>
  <PropertyGroup><RootNamespace Condition="false">AlsoWrong</RootNamespace><TargetFramework>net10.0</TargetFramework></PropertyGroup>
  <Choose><When Condition="true"><PropertyGroup><OutputPath>chosen</OutputPath></PropertyGroup></When><Otherwise><ItemGroup><PackageReference Include="Otherwise" /></ItemGroup></Otherwise></Choose>
  <ItemGroup><PackageReference Include="Always" Version="1.0" /></ItemGroup>
</Project>"#,
        );
        write(&dir.join("A/A.csproj"), "<Project/>");
        let project = ProjectFile::read(&dir.join("A/A.csproj"), "Debug", &dir).ok();
        let p = project.as_ref();
        assert_eq!(
            p.map(|p| p.is_test),
            Some(false),
            "no project becomes a test project"
        );
        assert_eq!(p.map(|p| p.assembly_name.as_str()), Some("A"));
        assert_eq!(p.map(|p| p.root_namespace.as_str()), Some("A"));
        assert_eq!(p.and_then(|p| p.output_path.clone()), None);
        assert_eq!(
            p.map(|p| p.target_frameworks.clone()),
            Some(vec!["net10.0".to_owned()])
        );
        assert_eq!(
            p.map(|p| p
                .package_refs
                .iter()
                .map(|r| r.id.as_str())
                .collect::<Vec<_>>()),
            Some(vec!["Always"])
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn artifacts_path_expands_the_props_folder_and_backslashes() {
        let dir = scratch("artifacts-path");
        let props = |path: &str| {
            format!(
                "<Project><PropertyGroup><UseArtifactsOutput>true</UseArtifactsOutput><Out>out</Out><ArtifactsPath>{path}</ArtifactsPath></PropertyGroup></Project>"
            )
        };
        write(&dir.join("A/A.csproj"), "<Project/>");
        for (path, expected) in [
            (
                "$(MSBuildThisFileDirectory)build\\artifacts",
                dir.join("build/artifacts"),
            ),
            (
                "$(MSBuildThisFileDirectory)$(Out)\\..\\artifacts",
                dir.join("artifacts"),
            ),
            ("custom", dir.join("custom")),
            ("$(Unknown)\\x", dir.join("artifacts")),
        ] {
            write(&dir.join("Directory.Build.props"), &props(path));
            let project = ProjectFile::read(&dir.join("A/A.csproj"), "Debug", &dir).ok();
            assert_eq!(
                project.and_then(|p| p.artifacts),
                Some(normalise(&expected)),
                "{path}"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn package_ids_dedup_case_insensitively_even_when_not_adjacent() {
        let dir = scratch("dedup");
        write(
            &dir.join("A/A.csproj"),
            r#"<Project><ItemGroup><PackageReference Include="Abc" Version="1" /><PackageReference Include="Abd" /><PackageReference Include="abc" Version="2" /></ItemGroup></Project>"#,
        );
        let project = ProjectFile::read(&dir.join("A/A.csproj"), "Debug", &dir).ok();
        assert_eq!(
            project.map(|p| p
                .package_refs
                .iter()
                .map(|r| r.id.clone())
                .collect::<Vec<_>>()),
            Some(vec!["Abc".to_owned(), "Abd".to_owned()])
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
