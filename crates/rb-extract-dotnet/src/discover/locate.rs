//! Where a project's build put its assembly.
//!
//! - Plan: [Wave 2, Step 1](../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#21-step-1-net-discovery-and-the-loader-options-2a)
//!   (`locate.rs`: each project to its built assembly and PDB under `OutputPath` or
//!   `bin/<Configuration>/<TargetFramework>/`)
//! - Requirement: [FR-EXT-DN-01](../../../../docs/prd.md#fr-ext-dn-01)
//!
//! The conventional places are tried in order (`OutputPath`, the Arcade SDK's
//! `artifacts/bin/<Project>/<Configuration>/<TFM>/`, `bin/<Configuration>/<TFM>/`, the .NET 8
//! `UseArtifactsOutput` layout `artifacts/bin/<Project>/<pivot>/`), the preferred target
//! framework first; when all are empty, the output
//! folders are searched for `<AssemblyName>.dll`, skipping `ref/` and `refint/` (reference
//! assemblies carry no method bodies), and a path naming the configuration wins.

use std::path::{Path, PathBuf};

use super::project::ProjectFile;

/// The built assembly of `project` for `configuration`, preferring `target_framework`.
pub fn built_assembly(
    project: &ProjectFile,
    configuration: &str,
    target_framework: Option<&str>,
) -> Option<PathBuf> {
    let folder = project.path.parent().unwrap_or(Path::new("."));
    let file = format!("{}.dll", project.assembly_name);
    let mut frameworks: Vec<&str> = Vec::new();
    if let Some(preferred) = target_framework {
        frameworks.push(preferred);
    }
    frameworks.extend(project.target_frameworks.iter().map(String::as_str));
    frameworks.push("");
    let mut candidates: Vec<PathBuf> = Vec::new();
    for tfm in frameworks {
        if let Some(output) = &project.output_path {
            candidates.push(folder.join(output.replace('\\', "/")).join(tfm).join(&file));
        }
        if let Some(arcade) = &project.arcade_output {
            candidates.push(arcade.join(configuration).join(tfm).join(&file));
        }
        candidates.push(folder.join("bin").join(configuration).join(tfm).join(&file));
        if let Some(artifacts) = &project.artifacts {
            let pivot = if tfm.is_empty() {
                configuration.to_ascii_lowercase()
            } else {
                format!("{}_{tfm}", configuration.to_ascii_lowercase())
            };
            candidates.push(
                artifacts
                    .join("bin")
                    .join(&project.stem)
                    .join(pivot)
                    .join(&file),
            );
        }
    }
    if let Some(found) = candidates.into_iter().find(|c| c.is_file()) {
        return Some(found);
    }
    let mut roots = vec![folder.join("bin")];
    roots.extend(project.arcade_output.clone());
    if let Some(artifacts) = &project.artifacts {
        roots.push(artifacts.join("bin").join(&project.stem));
    }
    let mut found = Vec::new();
    for search in roots {
        find_file(&search, &file, 6, &mut found);
    }
    found.sort();
    let config = configuration.to_ascii_lowercase();
    found
        .iter()
        .find(|p| p.to_string_lossy().to_ascii_lowercase().contains(&config))
        .or_else(|| found.first())
        .cloned()
}

/// Collects files named `name` under `dir`, `depth` levels deep, skipping reference-assembly
/// folders and not following symbolic links.
fn find_file(dir: &Path, name: &str, depth: usize, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
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
    use crate::discover::tests::{scratch, write};

    fn project(dir: &Path, rel: &str) -> Option<ProjectFile> {
        ProjectFile::read(&dir.join(rel), "Release", dir).ok()
    }

    #[test]
    fn finds_the_conventional_output_preferring_the_named_framework() {
        let dir = scratch("locate");
        write(
            &dir.join("src/Api/Api.csproj"),
            "<Project><PropertyGroup><TargetFrameworks>net8.0;net9.0</TargetFrameworks></PropertyGroup></Project>",
        );
        write(&dir.join("src/Api/bin/Release/net8.0/Api.dll"), "");
        write(&dir.join("src/Api/bin/Release/net9.0/Api.dll"), "");
        let p = project(&dir, "src/Api/Api.csproj");
        assert_eq!(
            p.as_ref().and_then(|p| built_assembly(p, "Release", None)),
            Some(dir.join("src/Api/bin/Release/net8.0/Api.dll"))
        );
        assert_eq!(
            p.as_ref()
                .and_then(|p| built_assembly(p, "Release", Some("net9.0"))),
            Some(dir.join("src/Api/bin/Release/net9.0/Api.dll"))
        );
        assert_eq!(
            p.as_ref().and_then(|p| built_assembly(p, "Debug", None)),
            Some(dir.join("src/Api/bin/Release/net8.0/Api.dll")),
            "a search finds the other configuration when the named one is empty"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_arcade_layout_is_read_from_the_sdk_import_and_global_json() {
        let dir = scratch("arcade");
        let arcade = r#"<Project><Import Project="Sdk.props" Sdk="Microsoft.DotNet.Arcade.Sdk" /><PropertyGroup><TargetFramework>net10.0</TargetFramework></PropertyGroup></Project>"#;
        write(&dir.join("Directory.Build.props"), arcade);
        write(&dir.join("src/Api/Api.csproj"), "<Project/>");
        write(&dir.join("artifacts/bin/Api/Release/net10.0/Api.dll"), "");
        // Without global.json the SDK cannot be resolved, so the layout does not apply.
        let p = project(&dir, "src/Api/Api.csproj");
        assert_eq!(p.as_ref().and_then(|p| p.arcade_output.clone()), None);
        assert_eq!(
            p.as_ref().and_then(|p| built_assembly(p, "Release", None)),
            None
        );
        write(
            &dir.join("global.json"),
            r#"{ "msbuild-sdks": { "Microsoft.DotNet.Arcade.Sdk": "10.0.0-beta.1" } }"#,
        );
        let p = project(&dir, "src/Api/Api.csproj");
        assert_eq!(
            p.as_ref().and_then(|p| p.arcade_output.clone()),
            Some(dir.join("artifacts/bin/Api"))
        );
        assert_eq!(
            p.as_ref().and_then(|p| built_assembly(p, "Release", None)),
            Some(dir.join("artifacts/bin/Api/Release/net10.0/Api.dll"))
        );
        assert_eq!(
            p.as_ref().and_then(|p| built_assembly(p, "Debug", None)),
            Some(dir.join("artifacts/bin/Api/Release/net10.0/Api.dll")),
            "the search covers the Arcade folder too"
        );
        // OutDirName and ArtifactsDir, as a repository may set them, expanded.
        write(
            &dir.join("src/Tool/Tool.csproj"),
            r"<Project><PropertyGroup><OutDirName>Tools\$(MSBuildProjectName)</OutDirName><ArtifactsDir>$(RepoRoot)out\</ArtifactsDir></PropertyGroup></Project>",
        );
        write(&dir.join("out/bin/Tools/Tool/Release/net10.0/Tool.dll"), "");
        let p = project(&dir, "src/Tool/Tool.csproj");
        assert_eq!(
            p.as_ref().and_then(|p| built_assembly(p, "Release", None)),
            Some(dir.join("out/bin/Tools/Tool/Release/net10.0/Tool.dll"))
        );
        // A relative ArtifactsDir is relative to the project.
        write(
            &dir.join("src/Rel/Rel.csproj"),
            "<Project><PropertyGroup><ArtifactsDir>../../rel</ArtifactsDir></PropertyGroup></Project>",
        );
        let p = project(&dir, "src/Rel/Rel.csproj");
        assert_eq!(
            p.as_ref().and_then(|p| p.arcade_output.clone()),
            Some(dir.join("rel/bin/Rel"))
        );
        // A props file that does not import Arcade leaves the layout off, global.json or not.
        write(
            &dir.join("plain/Directory.Build.props"),
            r#"<Project><Import Project="Other.props" Sdk="Some.Other.Sdk" /></Project>"#,
        );
        write(&dir.join("plain/global.json"), "{}");
        write(&dir.join("plain/P/P.csproj"), "<Project/>");
        let p =
            ProjectFile::read(&dir.join("plain/P/P.csproj"), "Release", &dir.join("plain")).ok();
        assert_eq!(p.and_then(|p| p.arcade_output), None);
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
            "<Project><PropertyGroup><TargetFrameworks>net9.0;net8.0</TargetFrameworks></PropertyGroup></Project>",
        );
        write(&dir.join("artifacts/bin/Api/release_net9.0/Api.dll"), "");
        let p = project(&dir, "src/Api/Api.csproj");
        assert_eq!(
            p.as_ref().and_then(|p| built_assembly(p, "Release", None)),
            Some(dir.join("artifacts/bin/Api/release_net9.0/Api.dll"))
        );
        write(
            &dir.join("src/Odd/Odd.csproj"),
            "<Project><PropertyGroup><OutputPath>out\\</OutputPath></PropertyGroup></Project>",
        );
        write(&dir.join("src/Odd/out/Odd.dll"), "");
        let p = project(&dir, "src/Odd/Odd.csproj");
        assert_eq!(
            p.as_ref().and_then(|p| built_assembly(p, "Release", None)),
            Some(dir.join("src/Odd/out/Odd.dll"))
        );
        write(&dir.join("src/Deep/Deep.csproj"), "<Project/>");
        write(&dir.join("src/Deep/bin/x/ref/Deep.dll"), "");
        write(&dir.join("src/Deep/bin/x/Release/y/Deep.dll"), "");
        let p = project(&dir, "src/Deep/Deep.csproj");
        assert_eq!(
            p.as_ref().and_then(|p| built_assembly(p, "Release", None)),
            Some(dir.join("src/Deep/bin/x/Release/y/Deep.dll"))
        );
        write(&dir.join("src/None/None.csproj"), "<Project/>");
        let p = project(&dir, "src/None/None.csproj");
        assert_eq!(
            p.as_ref().and_then(|p| built_assembly(p, "Release", None)),
            None
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
