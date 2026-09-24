//! `init` chooses presets from the languages it finds, `--preset` names them, and
//! `cruise --init [oneshot]` is dependency-cruiser's `depcruise --init` without the questions.
//!
//! - Plan: [Wave 2, Step 9](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#29-step-9-presets---init-presets-vue-svelte-markdown-webpackconfig-collapse-highlight-experimentalstats-2d)
//!   ("`rulebearing --init` gains `--preset dotnet | python | typescript` and the `oneshot` names
//!   dependency-cruiser accepts; `init` (wave 1) selects presets from the languages it detects")
//! - Coverage: [coverage § Command line](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#command-line),
//!   row `--init` (`oneshot` presets)
//! - Source: [design § What stays honest across the boundary](../../../docs/artifacts/design.md#what-stays-honest-across-the-boundary)
//! - Requirements: [FR-CLI-03](../../../docs/prd.md#fr-cli-03), [FR-CLI-08](../../../docs/prd.md#fr-cli-08),
//!   [FR-CFG-06](../../../docs/prd.md#fr-cfg-06)

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_rulebearing");

fn tree(name: &str, files: &[(&str, &str)]) -> Result<PathBuf, Box<dyn Error>> {
    let dir =
        std::env::temp_dir().join(format!("rb-cli-init-presets-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (file, text) in files {
        let path = dir.join(file);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, text)?;
    }
    std::fs::create_dir_all(dir.join(".git"))?;
    Ok(dir)
}

fn run(dir: &Path, args: &[&str]) -> Result<Output, Box<dyn Error>> {
    let mut command = Command::new(BIN);
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
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

const PYTHON: &[(&str, &str)] = &[
    (
        "pyproject.toml",
        "[project]\nname = \"pkg\"\nversion = \"0\"\n",
    ),
    ("src/pkg/__init__.py", ""),
    ("src/pkg/__main__.py", "from pkg import core\ncore.run()\n"),
    ("src/pkg/core.py", "def run():\n    return 1\n"),
];

const TYPESCRIPT: &[(&str, &str)] = &[
    ("tsconfig.json", "{ \"compilerOptions\": {} }\n"),
    (
        "package.json",
        "{\n  \"name\": \"web\",\n  \"scripts\": {\n    \"test\": \"vitest\"\n  }\n}\n",
    ),
    (
        "src/main.ts",
        "import { a } from \"./a\";\nconsole.log(a);\n",
    ),
    ("src/a.ts", "export const a = 1;\n"),
];

#[test]
fn one_language_extends_its_own_preset_first() -> Result<(), Box<dyn Error>> {
    let dir = tree("python", PYTHON)?;
    let proposal = ok(&run(&dir, &["init", "--dry-run", "--owner", "@me"])?)?;
    assert!(proposal.contains("# Found: Python."), "{proposal}");
    assert!(
        proposal.contains("extends: [rulebearing:python, rulebearing:recommended]"),
        "{proposal}"
    );
    assert!(
        proposal.contains("__main__"),
        "the preset's orphan exclusions start the list"
    );
    assert!(
        !proposal.contains("Program"),
        "no .NET exclusion: {proposal}"
    );
    ok(&run(&dir, &["init", "--owner", "@me"])?)?;
    ok(&run(&dir, &["cruise", "src"])?)?;
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn several_languages_extend_the_composition() -> Result<(), Box<dyn Error>> {
    let mut files = TYPESCRIPT.to_vec();
    files.extend_from_slice(PYTHON);
    let dir = tree("mixed", &files)?;
    let proposal = ok(&run(&dir, &["init", "--dry-run", "--owner", "@me"])?)?;
    assert!(
        proposal.contains("# Found: TypeScript, Python."),
        "{proposal}"
    );
    assert!(
        proposal.contains("extends: rulebearing:recommended\n"),
        "{proposal}"
    );
    // --preset names the languages instead.
    let named = ok(&run(
        &dir,
        &[
            "init",
            "--dry-run",
            "--owner",
            "@me",
            "--preset",
            "typescript",
        ],
    )?)?;
    assert!(
        named.contains("extends: [rulebearing:typescript, rulebearing:recommended]"),
        "{named}"
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn nothing_to_initialise_names_every_marker_and_the_flag() -> Result<(), Box<dyn Error>> {
    let dir = tree("empty", &[("README.md", "nothing\n")])?;
    let output = run(&dir, &["init"])?;
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("pyproject.toml") && stderr.contains("--preset"),
        "{stderr}"
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn cruise_init_writes_the_configuration_init_writes() -> Result<(), Box<dyn Error>> {
    let dir = tree("oneshot", TYPESCRIPT)?;
    let report = ok(&run(&dir, &["cruise", "--init"])?)?;
    assert!(report.contains("wrote rulebearing.yaml"), "{report}");
    let written = std::fs::read_to_string(dir.join("rulebearing.yaml"))?;
    assert!(written.contains("extends: [rulebearing:typescript, rulebearing:recommended]"));
    ok(&run(&dir, &["cruise", "src"])?)?;
    // `--init yes` over an existing configuration is refused, as upstream leaves it be.
    assert_eq!(
        run(&dir, &["cruise", "--init", "yes"])?.status.code(),
        Some(3)
    );
    // `x-scripts` leaves the configuration and adds the run scripts after the existing ones.
    let scripts = ok(&run(&dir, &["cruise", "--init", "x-scripts"])?)?;
    assert!(
        scripts.contains(
            "added run scripts to package.json: rulebearing, rulebearing:text, rulebearing:focus"
        ),
        "{scripts}"
    );
    let manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("package.json"))?)?;
    let names: Vec<&str> = manifest["scripts"]
        .as_object()
        .map(|s| s.keys().map(String::as_str).collect())
        .unwrap_or_default();
    assert_eq!(
        names,
        [
            "test",
            "rulebearing",
            "rulebearing:text",
            "rulebearing:focus"
        ]
    );
    assert_eq!(manifest["scripts"]["rulebearing"], "rulebearing cruise src");
    assert_eq!(
        std::fs::read_to_string(dir.join("rulebearing.yaml"))?,
        written
    );
    let again = ok(&run(&dir, &["cruise", "--init", "x-scripts"])?)?;
    assert!(again.contains("already has"), "{again}");
    // --preset belongs to --init.
    assert_ne!(
        run(&dir, &["cruise", "--preset", "python", "src"])?
            .status
            .code(),
        Some(0)
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn cruise_init_takes_the_config_name_and_the_preset() -> Result<(), Box<dyn Error>> {
    let dir = tree("oneshot-named", TYPESCRIPT)?;
    ok(&run(
        &dir,
        &[
            "cruise",
            "--init",
            "whatever",
            "--preset",
            "typescript",
            "-c",
            "arch.yaml",
        ],
    )?)?;
    let written = std::fs::read_to_string(dir.join("arch.yaml"))?;
    assert!(written.contains("extends: [rulebearing:typescript, rulebearing:recommended]"));
    assert!(!dir.join("rulebearing.yaml").exists());
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}
