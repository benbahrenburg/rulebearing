//! Discovery and the loader options over committed solution layouts, with the extraction
//! fixture's built assemblies placed where a build would put them.
//!
//! - Plan: [Wave 2, Step 1](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#21-step-1-net-discovery-and-the-loader-options-2a)
//!   ("`tests/discover.rs` over fixture solutions in `tests/fixtures/solutions/`"; done when every
//!   row of the loader coverage has a fixture)
//! - Coverage: [ArchUnitNET § Loader and caches](../../../docs/artifacts/archunitnet-0.13.4-coverage.md#loader-and-caches)
//! - Requirement: [FR-EXT-DN-01](../../../docs/prd.md#fr-ext-dn-01)
//!
//! | Loader row | Test |
//! | --- | --- |
//! | `LoadAssembly`, `LoadAssemblies`, `...IncludingDependencies` | [`loader_globs_and_include_dependencies`] |
//! | `LoadFilteredDirectory` | [`a_filtered_directory_is_read`] |
//! | `LoadNamespacesWithinAssembly` | [`namespaces_keep_only_their_types`] |
//! | solution-driven loading | [`a_classic_solution_reads_every_built_project`], [`an_slnx_with_multi_targeting_and_exclusions`] |
//! | build output layouts | [`an_arcade_repository_builds_to_artifacts_bin`] (dotnet/aspnetcore), [`use_artifacts_output_builds_to_artifacts_bin_pivots`] |

use std::path::{Path, PathBuf};

use rb_extract_dotnet::DotnetExtractor;
use rb_extract_dotnet::discover::discover;
use rb_model::{DependencyType, DirectoryFilter, DotnetOptions, Extractor, options::Patterns};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// A scratch copy of a layout with the built assemblies placed at `(assembly, folder)`.
fn layout(name: &str, tag: &str, places: &[(&str, &str)]) -> std::io::Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("rb-layout-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    if !name.is_empty() {
        copy_tree(&fixtures().join("solutions").join(name), &dir)?;
    }
    for (assembly, folder) in places {
        let target = dir.join(folder);
        std::fs::create_dir_all(&target)?;
        for extension in ["dll", "pdb"] {
            let file = format!("{assembly}.{extension}");
            std::fs::copy(
                fixtures().join("sample/built").join(&file),
                target.join(&file),
            )?;
        }
    }
    Ok(dir)
}

#[test]
fn a_classic_solution_reads_every_built_project() -> Result<(), Box<dyn std::error::Error>> {
    let dir = layout(
        "classic",
        "classic",
        &[
            ("Sample", "src/Web/bin/Debug/net10.0"),
            ("Sample.Core", "src/Core/bin/Debug/net10.0"),
        ],
    )?;
    let workspace = discover(&dir, &DotnetOptions::default())?;
    let names: Vec<(&str, bool)> = workspace
        .projects
        .iter()
        .map(|p| (p.assembly_name.as_str(), p.is_test))
        .collect();
    assert_eq!(
        names,
        [
            ("Sample.Core", false),
            ("Sample", false),
            ("Web.Tests", true)
        ]
    );
    let web = &workspace.projects[1];
    assert_eq!(web.project_refs, [dir.join("src/Core/Core.csproj")]);
    assert_eq!(
        web.package_refs
            .iter()
            .map(|p| (p.id.as_str(), p.version.as_deref()))
            .collect::<Vec<_>>(),
        [("Newtonsoft.Json", Some("13.0.3"))],
        "the version comes from Directory.Packages.props"
    );
    assert_eq!(
        web.target_frameworks,
        ["net10.0"],
        "from Directory.Build.props"
    );

    let extraction =
        DotnetExtractor.extract(std::slice::from_ref(&dir), &DotnetOptions::default())?;
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        (
            extraction.inspected.projects,
            extraction.inspected.assemblies
        ),
        (Some(3), 2)
    );
    assert!(
        extraction
            .warnings
            .iter()
            .any(|w| w.message.contains("no built Web.Tests.dll")),
        "{:?}",
        extraction.warnings
    );
    let order = extraction
        .modules
        .iter()
        .find(|m| m.source.ends_with("src/Order.cs"))
        .ok_or("no Order.cs module")?;
    assert_eq!(order.project.as_deref(), Some("src/Web/Web.csproj"));
    let clock = order
        .dependencies
        .iter()
        .find(|d| d.resolved.ends_with("core/Clock.cs"))
        .ok_or("no edge to Clock.cs")?;
    assert_eq!(clock.dependency_types, [DependencyType::Project]);
    Ok(())
}

#[test]
fn an_slnx_with_multi_targeting_and_exclusions() -> Result<(), Box<dyn std::error::Error>> {
    let dir = layout(
        "xml",
        "xml",
        &[
            ("Sample", "src/App/bin/Debug/net8.0"),
            ("Sample", "src/App/bin/Debug/net10.0"),
            ("Sample.Core", "src/Lib/bin/Debug/net10.0"),
        ],
    )?;
    let all = discover(&dir, &DotnetOptions::default())?;
    assert_eq!(all.projects.len(), 3);
    assert_eq!(
        all.projects[1].assembly,
        Some(dir.join("src/App/bin/Debug/net8.0/Sample.dll")),
        "the first listed framework by default"
    );
    let options = DotnetOptions {
        target_framework: Some("net10.0".to_owned()),
        exclude_projects: Some(Patterns::One("^legacy/".to_owned())),
        ..DotnetOptions::default()
    };
    let chosen = discover(&dir, &options)?;
    assert_eq!(
        chosen
            .projects
            .iter()
            .map(|p| p.assembly_name.as_str())
            .collect::<Vec<_>>(),
        ["Sample", "Sample.Core"]
    );
    assert_eq!(
        chosen.projects[0].assembly,
        Some(dir.join("src/App/bin/Debug/net10.0/Sample.dll"))
    );
    let extraction = DotnetExtractor.extract(std::slice::from_ref(&dir), &options)?;
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(extraction.inspected.assemblies, 2);
    assert!(
        extraction
            .warnings
            .iter()
            .all(|w| !w.message.contains("Old")),
        "the excluded project is not read"
    );
    Ok(())
}

/// Each discovered project's assembly name and the assembly it was found at.
type Located = Vec<(String, Option<PathBuf>)>;

/// The assembly each discovered project was found at, by assembly name.
fn located(dir: &Path) -> Result<Located, Box<dyn std::error::Error>> {
    Ok(discover(dir, &DotnetOptions::default())?
        .projects
        .into_iter()
        .map(|p| (p.assembly_name, p.assembly))
        .collect())
}

#[test]
fn an_arcade_repository_builds_to_artifacts_bin() -> Result<(), Box<dyn std::error::Error>> {
    // dotnet/aspnetcore's layout: the Arcade SDK builds every project to
    // artifacts/bin/<Project>/<Configuration>/<TFM>/, nothing under the project folder.
    let dir = layout(
        "arcade",
        "arcade",
        &[
            ("Sample", "artifacts/bin/Web/Debug/net10.0"),
            ("Sample.Core", "artifacts/bin/Core/Debug/net10.0"),
        ],
    )?;
    let found = located(&dir)?;
    assert_eq!(
        found,
        [
            (
                "Sample.Core".to_owned(),
                Some(dir.join("artifacts/bin/Core/Debug/net10.0/Sample.Core.dll"))
            ),
            (
                "Sample".to_owned(),
                Some(dir.join("artifacts/bin/Web/Debug/net10.0/Sample.dll"))
            ),
        ]
    );
    let extraction =
        DotnetExtractor.extract(std::slice::from_ref(&dir), &DotnetOptions::default())?;
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        (
            extraction.inspected.projects,
            extraction.inspected.assemblies
        ),
        (Some(2), 2)
    );
    assert!(
        extraction
            .warnings
            .iter()
            .all(|w| !w.message.contains("no built")),
        "{:?}",
        extraction.warnings
    );
    Ok(())
}

#[test]
fn use_artifacts_output_builds_to_artifacts_bin_pivots() -> Result<(), Box<dyn std::error::Error>> {
    // The .NET 8 layout: artifacts/bin/<Project>/<pivot>/, the pivot the lower-cased
    // configuration, with the target framework only for a multi-targeting project.
    let dir = layout(
        "artifacts",
        "use-artifacts",
        &[
            ("Sample", "artifacts/bin/Web/debug_net10.0"),
            ("Sample", "artifacts/bin/Web/debug_net8.0"),
            ("Sample.Core", "artifacts/bin/Core/debug"),
        ],
    )?;
    let found = located(&dir)?;
    assert_eq!(
        found,
        [
            (
                "Sample.Core".to_owned(),
                Some(dir.join("artifacts/bin/Core/debug/Sample.Core.dll"))
            ),
            (
                "Sample".to_owned(),
                Some(dir.join("artifacts/bin/Web/debug_net10.0/Sample.dll"))
            ),
        ]
    );
    let options = DotnetOptions {
        target_framework: Some("net8.0".to_owned()),
        ..DotnetOptions::default()
    };
    let chosen = discover(&dir, &options)?;
    assert_eq!(
        chosen.projects[1].assembly,
        Some(dir.join("artifacts/bin/Web/debug_net8.0/Sample.dll"))
    );
    let extraction = DotnetExtractor.extract(std::slice::from_ref(&dir), &options)?;
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(extraction.inspected.assemblies, 2);
    Ok(())
}

#[test]
fn loader_globs_and_include_dependencies() -> Result<(), Box<dyn std::error::Error>> {
    let dir = layout("", "loader", &[("Sample", "out"), ("Sample.Core", "out")])?;
    let only = DotnetOptions {
        assemblies: Some(vec!["out/Sample.dll".to_owned()]),
        ..DotnetOptions::default()
    };
    let alone = DotnetExtractor.extract(std::slice::from_ref(&dir), &only)?;
    assert_eq!(alone.inspected.assemblies, 1);
    let unresolved = alone
        .modules
        .iter()
        .find(|m| m.source == "Sample.Core")
        .ok_or("Sample.Core is a named module when not loaded")?;
    assert_eq!(unresolved.could_not_resolve, Some(true));
    let with = DotnetOptions {
        include_dependencies: Some(true),
        ..only
    };
    let both = DotnetExtractor.extract(std::slice::from_ref(&dir), &with)?;
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        both.inspected.assemblies, 2,
        "the referenced assembly beside it is read"
    );
    assert!(both.modules.iter().all(|m| m.source != "Sample.Core"));
    Ok(())
}

#[test]
fn a_filtered_directory_is_read() -> Result<(), Box<dyn std::error::Error>> {
    let dir = layout(
        "",
        "directory",
        &[("Sample", "libs"), ("Sample.Core", "libs")],
    )?;
    let options = DotnetOptions {
        directories: Some(vec![DirectoryFilter {
            dir: "libs".to_owned(),
            filter: Some("*.Core.dll".to_owned()),
        }]),
        ..DotnetOptions::default()
    };
    let extraction = DotnetExtractor.extract(std::slice::from_ref(&dir), &options)?;
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(extraction.inspected.assemblies, 1);
    let code = extraction.code.unwrap_or_default();
    assert_eq!(
        code.types
            .iter()
            .filter(|t| t.referenced.is_none())
            .map(|t| t.full_name.as_str())
            .collect::<Vec<_>>(),
        ["Sample.Core.Clock"]
    );
    Ok(())
}

#[test]
fn namespaces_keep_only_their_types() -> Result<(), Box<dyn std::error::Error>> {
    let options = DotnetOptions {
        assemblies: Some(vec!["built/Sample.dll".to_owned()]),
        namespaces: Some(vec!["Sample.Orders".to_owned()]),
        ..DotnetOptions::default()
    };
    let extraction = DotnetExtractor.extract(&[fixtures().join("sample")], &options)?;
    let code = extraction.code.unwrap_or_default();
    assert!(!code.types.is_empty());
    assert!(
        code.types
            .iter()
            .filter(|t| t.referenced.is_none())
            .all(|t| t.full_name.starts_with("Sample.Orders"))
    );
    Ok(())
}
