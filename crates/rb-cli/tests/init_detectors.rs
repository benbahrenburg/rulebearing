//! `init`'s .NET and Python detectors: a solution selects `rulebearing:dotnet` and proposes the
//! layers its namespaces name; a `pyproject.toml` selects `rulebearing:python` and proposes rules
//! from the top-level packages; a project in a folder below the root is found and named in
//! `languages`. Each proposal is cruised back and exits 0.
//!
//! - Plan: [Wave 2, Step 15](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#215-step-15-greenfield-init-proof-the-nightly-tables-upstream-offers-second-maintainer-2i)
//! - Source: [design § Test beds](../../../docs/artifacts/design.md#test-beds-open-source-repositories-to-validate-against)
//!   item 2 (greenfield `init` produces a config that passes)
//! - Requirements: [FR-CLI-03](../../../docs/prd.md#fr-cli-03), [NFR-ADOPT-02](../../../docs/prd.md#nfr-adopt-02)
//! - Fixture: [`fixtures/init-layers`](fixtures/init-layers/PROVENANCE.md)

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_rulebearing");
const PROJECTS: &[&str] = &[
    "Shop.Domain",
    "Shop.Application",
    "Shop.Infrastructure",
    "Shop.Web",
];

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/init-layers")
}

fn scratch(name: &str) -> Result<PathBuf, Box<dyn Error>> {
    let dir =
        std::env::temp_dir().join(format!("rb-cli-init-detect-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join(".git"))?;
    Ok(dir)
}

fn write(dir: &Path, files: &[(&str, &str)]) -> Result<(), Box<dyn Error>> {
    for (file, text) in files {
        let path = dir.join(file);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, text)?;
    }
    Ok(())
}

/// The fixture solution under `dir/<under>`, each assembly where `dotnet build` puts it.
fn layered_solution(dir: &Path, under: &str) -> Result<(), Box<dyn Error>> {
    let source = fixture();
    let target = dir.join(under);
    std::fs::create_dir_all(&target)?;
    std::fs::copy(source.join("Shop.slnx"), target.join("Shop.slnx"))?;
    for project in PROJECTS {
        let from = source.join("src").join(project);
        let to = target.join("src").join(project);
        let bin = to.join("bin/Debug/net10.0");
        std::fs::create_dir_all(&bin)?;
        for entry in std::fs::read_dir(&from)? {
            let entry = entry?;
            if entry.path().is_file() {
                std::fs::copy(entry.path(), to.join(entry.file_name()))?;
            }
        }
        for extension in ["dll", "pdb"] {
            let file = format!("{project}.{extension}");
            std::fs::copy(source.join("built").join(&file), bin.join(&file))?;
        }
    }
    Ok(())
}

fn run(dir: &Path, args: &[&str]) -> Result<Output, Box<dyn Error>> {
    let mut command = Command::new(BIN);
    for (key, _) in std::env::vars_os() {
        let key_text = key.to_string_lossy();
        if key_text.starts_with("GIT_") || key_text == "VIRTUAL_ENV" {
            command.env_remove(key);
        }
    }
    Ok(command
        .args(args)
        .current_dir(dir)
        .env("SOURCE_DATE_EPOCH", "1790000000")
        .output()?)
}

fn ok(output: &Output) -> Result<String, Box<dyn Error>> {
    if output.status.code() == Some(0) {
        Ok(String::from_utf8(output.stdout.clone())?)
    } else {
        Err(format!(
            "exit {:?}: {}{}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
        .into())
    }
}

/// The rule names of a proposal, in order.
fn rule_names(proposal: &str) -> Vec<&str> {
    proposal
        .lines()
        .filter_map(|l| l.strip_prefix("      - name: "))
        .collect()
}

#[test]
fn a_solution_proposes_the_layers_its_namespaces_name() -> Result<(), Box<dyn Error>> {
    let dir = scratch("dotnet")?;
    layered_solution(&dir, "")?;
    let dry = run(&dir, &["init", "--dry-run", "--owner", "@me"])?;
    let proposal = ok(&dry)?;
    assert!(
        proposal.contains("extends: [rulebearing:dotnet, rulebearing:recommended]"),
        "{proposal}"
    );
    assert!(proposal.contains("# Found: .NET.\n"), "{proposal}");
    assert!(
        !proposal.contains("languages:"),
        "the one root solution is found without being named: {proposal}"
    );
    assert_eq!(
        rule_names(&proposal),
        [
            "no-orphans",
            "domain-not-to-outer-layers",
            "application-not-to-outer-layers",
            "infrastructure-not-to-outer-layers"
        ],
        "{proposal}"
    );
    assert!(
        proposal.contains(r#"from: { namespace: "^Shop\\.Domain(\\.|$)" }"#)
            && proposal.contains(
                r#"to: { namespace: "^Shop\\.(Application|Infrastructure|Web)(\\.|$)" }"#
            ),
        "{proposal}"
    );
    // Application -> Infrastructure is today's one finding, and it is baselined.
    let baselined: Vec<&str> = proposal
        .lines()
        .filter(|l| l.trim_start().starts_with("- {\"id\""))
        .collect();
    assert_eq!(baselined.len(), 1, "{proposal}");
    assert!(
        baselined[0].contains("application-not-to-outer-layers")
            && baselined[0].contains("PlaceOrder.cs")
            && baselined[0].contains("OrderStore.cs"),
        "{}",
        baselined[0]
    );
    let stderr = String::from_utf8_lossy(&dry.stderr);
    assert!(
        stderr.contains("exits 0 (1 findings baselined)"),
        "{stderr}"
    );
    // Written, the proposal cruises back to exit 0; two runs write the same bytes.
    ok(&run(&dir, &["init", "--owner", "@me"])?)?;
    let written = std::fs::read_to_string(dir.join("rulebearing.yaml"))?;
    assert_eq!(written, proposal);
    ok(&run(&dir, &["cruise", "--no-progress", "."])?)?;
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

const WORKSPACE: &[(&str, &str)] = &[
    (
        "python/pyproject.toml",
        "[tool.uv.workspace]\nmembers = [\"packages/*\"]\nexclude = [\"packages/draft\"]\n",
    ),
    (
        "python/packages/core/pyproject.toml",
        "[project]\nname = \"core\"\nversion = \"0\"\n",
    ),
    ("python/packages/core/src/core/__init__.py", ""),
    ("python/packages/core/src/core/clock.py", "NOW = 0\n"),
    (
        "python/packages/core/src/core/run.py",
        "from core import clock\n\n\ndef run():\n    return clock.NOW\n",
    ),
    (
        "python/packages/chat/pyproject.toml",
        "[project]\nname = \"chat\"\nversion = \"0\"\n",
    ),
    ("python/packages/chat/src/chat/__init__.py", ""),
    (
        "python/packages/chat/src/chat/agent.py",
        "from core import run\n\n\ndef reply():\n    return run.run()\n",
    ),
    (
        "python/packages/draft/pyproject.toml",
        "[project]\nname = \"draft\"\nversion = \"0\"\n",
    ),
    ("python/packages/draft/src/draft/__init__.py", ""),
    ("python/packages/notes/README.md", "no project here\n"),
];

#[test]
fn projects_below_the_root_are_found_and_named() -> Result<(), Box<dyn Error>> {
    let dir = scratch("nested")?;
    layered_solution(&dir, "dotnet")?;
    // A second, smaller solution: the one listing the most projects is named.
    write(
        &dir,
        &[(
            "dotnet/Only.Domain.slnx",
            "<Solution>\n  <Project Path=\"src/Shop.Domain/Shop.Domain.csproj\" />\n</Solution>\n",
        )],
    )?;
    write(&dir, WORKSPACE)?;
    let proposal = ok(&run(&dir, &["init", "--dry-run", "--owner", "@me"])?)?;
    assert!(
        proposal.contains("# Found: .NET (dotnet/Shop.slnx), Python (python/packages/chat/src, python/packages/core/src).\n"),
        "{proposal}"
    );
    assert!(
        proposal.contains("extends: rulebearing:recommended\n"),
        "{proposal}"
    );
    assert!(
        proposal.contains(
            "languages:\n  dotnet: { solution: \"dotnet/Shop.slnx\" }\n  python:\n    roots: [\"python/packages/chat/src\", \"python/packages/core/src\"]\n"
        ),
        "the excluded member is not a root: {proposal}"
    );
    let names = rule_names(&proposal);
    for name in [
        "domain-not-to-outer-layers",
        "application-not-to-outer-layers",
        "infrastructure-not-to-outer-layers",
        "core-not-to-its-dependents",
    ] {
        assert!(names.contains(&name), "{name} missing: {names:?}");
    }
    assert!(
        !names.contains(&"chat-not-to-its-dependents"),
        "nothing imports chat: {names:?}"
    );
    assert!(
        proposal.contains(r#"from: { path: "^python/packages/core/src/core/" }"#)
            && proposal.contains(r#"to: { path: "^python/packages/chat/src/chat/" }"#),
        "{proposal}"
    );
    ok(&run(&dir, &["init", "--owner", "@me"])?)?;
    ok(&run(&dir, &["cruise", "--no-progress", "."])?)?;
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn a_root_pyproject_is_the_extractor_s_own_discovery() -> Result<(), Box<dyn Error>> {
    let dir = scratch("python")?;
    write(
        &dir,
        &[
            (
                "pyproject.toml",
                "[project]\nname = \"app\"\nversion = \"0\"\n",
            ),
            ("app/__init__.py", ""),
            ("app/core.py", "VALUE = 1\n"),
            ("tools/__init__.py", ""),
            (
                "tools/report.py",
                "from app import core\nprint(core.VALUE)\n",
            ),
        ],
    )?;
    let proposal = ok(&run(&dir, &["init", "--dry-run", "--owner", "@me"])?)?;
    assert!(
        proposal.contains("extends: [rulebearing:python, rulebearing:recommended]"),
        "{proposal}"
    );
    assert!(proposal.contains("# Found: Python.\n"), "{proposal}");
    assert!(!proposal.contains("languages:"), "{proposal}");
    assert!(
        rule_names(&proposal).contains(&"app-not-to-its-dependents"),
        "{proposal}"
    );
    ok(&run(&dir, &["init", "--owner", "@me"])?)?;
    ok(&run(&dir, &["cruise", "--no-progress", "."])?)?;
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn an_unbuilt_solution_names_the_build() -> Result<(), Box<dyn Error>> {
    let dir = scratch("unbuilt")?;
    layered_solution(&dir, "")?;
    for project in PROJECTS {
        std::fs::remove_dir_all(dir.join("src").join(project).join("bin"))?;
    }
    let output = run(&dir, &["init", "--dry-run"])?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("dotnet build"), "{stderr}");
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}
