//! Solution files: the classic `.sln` project lines and the XML `.slnx`.
//!
//! - Plan: [Wave 2, Step 1](../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#21-step-1-net-discovery-and-the-loader-options-2a)
//!   (`sln.rs`, `slnx.rs`)
//! - Requirement: [FR-EXT-DN-01](../../../../docs/prd.md#fr-ext-dn-01)
//!
//! Only project entries whose file is a C#, F# or Visual Basic project are kept; solution folders
//! and other items are skipped. Paths are resolved against the solution's folder with `.` and
//! `..` removed, sorted and de-duplicated, so the project order and the paths `excludeProjects`
//! matches never depend on how the solution was written.

use std::path::{Path, PathBuf};

use super::project::normalise;
use super::{DiscoverError, read};

/// The project files a `.sln` or `.slnx` lists, resolved against the solution's folder.
///
/// # Errors
/// When the solution cannot be read or a `.slnx` is not XML.
pub fn solution_projects(solution: &Path) -> Result<Vec<PathBuf>, DiscoverError> {
    let text = read(solution)?;
    let folder = solution.parent().unwrap_or(Path::new("."));
    let relative = if solution
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("slnx"))
    {
        slnx_paths(&text).map_err(|reason| DiscoverError::Xml {
            path: solution.to_path_buf(),
            reason,
        })?
    } else {
        sln_paths(&text)
    };
    let mut projects: Vec<PathBuf> = relative
        .into_iter()
        .map(|p| p.replace('\\', "/"))
        .filter(|p| is_project_file(p))
        .map(|p| normalise(&folder.join(p)))
        .collect();
    projects.sort();
    projects.dedup();
    Ok(projects)
}

/// Whether a path names a project file this extractor reads.
pub fn is_project_file(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".csproj") || lower.ends_with(".fsproj") || lower.ends_with(".vbproj")
}

/// `Project("{type}") = "Name", "relative\path.csproj", "{guid}"` lines of a `.sln`.
fn sln_paths(text: &str) -> Vec<String> {
    text.lines()
        .filter(|l| l.trim_start().starts_with("Project("))
        .filter_map(|line| line.split('"').nth(5))
        .map(str::to_owned)
        .collect()
}

/// `<Project Path="..."/>` elements anywhere in a `.slnx`.
fn slnx_paths(text: &str) -> Result<Vec<String>, String> {
    let document = roxmltree::Document::parse(text).map_err(|e| e.to_string())?;
    Ok(document
        .descendants()
        .filter(|n| n.has_tag_name("Project"))
        .filter_map(|n| n.attribute("Path"))
        .map(str::to_owned)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discover::tests::{scratch, write};

    #[test]
    fn lists_projects_from_sln_and_slnx() {
        let dir = scratch("sln");
        write(
            &dir.join("A.sln"),
            "Microsoft Visual Studio Solution File\nProject(\"{FAE04EC0}\") = \"Web\", \"src\\Web\\Web.csproj\", \"{1}\"\nEndProject\nProject(\"{2150E333}\") = \"Items\", \"Items\", \"{2}\"\nEndProject\n",
        );
        write(
            &dir.join("B.slnx"),
            r#"<Solution><Folder Name="/src/"><Project Path="src/Core/Core.csproj" /></Folder><Project Path="tests/T.fsproj"/><Project Path="build/../tests/./T.fsproj"/></Solution>"#,
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
            Err(DiscoverError::Xml { .. })
        ));
        assert!(matches!(
            solution_projects(&dir.join("missing.sln")),
            Err(DiscoverError::Io { .. })
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_project_files_are_projects() {
        for (path, expected) in [
            ("a/B.csproj", true),
            ("a/B.FSPROJ", true),
            ("a/B.vbproj", true),
            ("a/B.shproj", false),
            ("Items", false),
            ("a/B.csproj.user", false),
        ] {
            assert_eq!(is_project_file(path), expected, "{path}");
        }
    }
}
