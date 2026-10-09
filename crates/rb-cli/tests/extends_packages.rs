//! `extends: rulebearing-rules/nextjs` resolves from each of the three package layouts the rule
//! library ships in: npm's `node_modules/`, the NuGet global packages folder and a Python
//! environment's `site-packages`, in that order.
//!
//! - Plan: [Wave 3, Step 23](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#27-steps-for-sub-wave-3g-the-rule-library-the-scale-table-adoption)
//!   ("`extends: rulebearing-rules/nextjs` resolves in a fixture repository for each of the three
//!   package layouts")
//! - Requirement: [FR-REACH-04](../../../docs/prd.md#fr-reach-04)

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

type Result<T = ()> = std::result::Result<T, Box<dyn Error>>;

const BIN: &str = env!("CARGO_BIN_EXE_rulebearing");

/// The library's preset, as the package carries it.
const PRESET: &str = include_str!("../../../presets/frameworks/nextjs.yaml");

/// A repository extending the library, with one module, and the folder for its packages.
fn repository(name: &str) -> Result<(PathBuf, PathBuf)> {
    let dir = std::env::temp_dir().join(format!("rb-cli-extends-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let repo = dir.join("repo");
    std::fs::create_dir_all(repo.join("app"))?;
    std::fs::create_dir(repo.join(".git"))?;
    std::fs::write(repo.join("package.json"), "{ \"name\": \"fixture\" }\n")?;
    std::fs::write(repo.join("app/page.ts"), "export const page = 1;\n")?;
    std::fs::write(
        repo.join("rulebearing.yaml"),
        "extends: rulebearing-rules/nextjs\n",
    )?;
    Ok((dir, repo))
}

fn put(file: &Path) -> Result {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(file, PRESET)?;
    Ok(())
}

/// The rule names the run used, with the environment given and no other package locations; the
/// preset's rules match nothing in a one-file tree, which this test is not about (no liveness).
fn rules(repo: &Path, env: &[(&str, &Path)]) -> Result<Vec<String>> {
    let mut command = Command::new(BIN);
    command
        .args([
            "cruise",
            "-T",
            "json",
            "--no-progress",
            "--no-liveness",
            "app",
        ])
        .current_dir(repo)
        .env_remove("NUGET_PACKAGES")
        .env_remove("VIRTUAL_ENV")
        .env("HOME", repo.join("no-home"))
        .env("USERPROFILE", repo.join("no-home"));
    for (name, value) in env {
        command.env(name, value);
    }
    let output = command.output()?;
    assert!(
        output.status.code().is_some_and(|c| c < 2),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: Value = serde_json::from_slice(&output.stdout)?;
    Ok(result["summary"]["ruleSetUsed"]["forbidden"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| r["name"].as_str().map(str::to_owned))
        .collect())
}

fn assert_nextjs(names: &[String]) {
    assert!(
        names
            .iter()
            .any(|n| n == "nextjs-no-import-of-route-entries"),
        "{names:?}"
    );
}

#[test]
fn the_library_resolves_from_node_modules() -> Result {
    let (dir, repo) = repository("npm")?;
    put(&repo.join("node_modules/rulebearing-rules/nextjs.yaml"))?;
    assert_nextjs(&rules(&repo, &[])?);
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn the_library_resolves_from_the_nuget_global_packages_folder() -> Result {
    let (dir, repo) = repository("nuget")?;
    let packages = dir.join("nuget");
    put(&packages.join("rulebearing.rules/0.4.0/rulebearing-rules/nextjs.yaml"))?;
    assert_nextjs(&rules(&repo, &[("NUGET_PACKAGES", &packages)])?);
    // The home folder's .nuget/packages when NUGET_PACKAGES is not set.
    let home = repo.join("no-home");
    put(&home.join(".nuget/packages/rulebearing.rules/0.4.0/rulebearing-rules/nextjs.yaml"))?;
    assert_nextjs(&rules(&repo, &[])?);
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn the_library_resolves_from_site_packages() -> Result {
    let (dir, repo) = repository("pypi")?;
    put(&repo.join(".venv/lib/python3.12/site-packages/rulebearing_rules/nextjs.yaml"))?;
    assert_nextjs(&rules(&repo, &[])?);
    let elsewhere = dir.join("env");
    std::fs::remove_dir_all(repo.join(".venv"))?;
    put(&elsewhere.join("lib/python3.13/site-packages/rulebearing_rules/nextjs.yaml"))?;
    assert_nextjs(&rules(&repo, &[("VIRTUAL_ENV", &elsewhere)])?);
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn node_modules_comes_first_and_a_missing_library_is_named() -> Result {
    let (dir, repo) = repository("order")?;
    let packages = dir.join("nuget");
    put(&packages.join("rulebearing.rules/0.4.0/rulebearing-rules/nextjs.yaml"))?;
    std::fs::create_dir_all(repo.join("node_modules/rulebearing-rules"))?;
    std::fs::write(
        repo.join("node_modules/rulebearing-rules/nextjs.yaml"),
        "rules:\n  dependencies:\n    forbidden:\n      - name: from-npm\n        from: {}\n        to: { path: never-matches }\n",
    )?;
    assert_eq!(
        rules(&repo, &[("NUGET_PACKAGES", &packages)])?,
        ["from-npm"]
    );
    std::fs::remove_dir_all(repo.join("node_modules"))?;
    std::fs::remove_dir_all(&packages)?;
    let output = Command::new(BIN)
        .args(["cruise", "-T", "json", "app"])
        .current_dir(&repo)
        .env("NUGET_PACKAGES", &packages)
        .env_remove("VIRTUAL_ENV")
        .output()?;
    assert_eq!(output.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&output.stderr).contains("rulebearing-rules/nextjs"));
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}
