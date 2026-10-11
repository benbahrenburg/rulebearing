//! The command line's side of the playground's equality: `rulebearing cruise -T <type>` and
//! `rulebearing explain <rule>` over the playground's fixtures print the committed outputs that
//! `rb-wasm`'s `cruise_ts`, `cruise_py` and `explain` are compared with
//! (`crates/rb-wasm/tests/cruise.rs`). `rb-wasm` never links `rb-cli`, so the two meet at these
//! files.
//!
//! - Plan: [Wave 4, Step 2](../../../docs/plans/pending/0004-wave-4-reach.md#21-steps-for-sub-wave-4a-docs-site-playground-cookbook-live-tables)
//!   ("the `cruise_ts` and `cruise_py` fixtures")
//! - Decision: [ADR-0073](../../../docs/adr/0073-extractors-read-files-through-rb-model.md)
//! - Requirement: [FR-REACH-01](../../../docs/prd.md#fr-reach-01)
//!
//! `RB_UPDATE_SNAPSHOTS=1` rewrites the outputs. Two things in them name the command line's own
//! folder, and are written as the playground's: `optionsUsed.baseDir` (the playground's working
//! directory is `/playground`) and `optionsUsed.rulesFile` (a pasted configuration has no file).
//! `sarif` and `github-annotations` put the folder's path from the repository root before each
//! path, `anon` draws its strings from a hash of the whole document (`baseDir` included), and
//! `x-dot-webpage` runs GraphViz, so those four are not compared.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_rulebearing");

/// 2026-10-10T12:00:00Z, the moment the playground's tests evaluate at.
const EPOCH: &str = "1791633600";

/// The reporters compared: every output type but the four the module doc names.
const COMPARED: &[&str] = &[
    "err",
    "err-long",
    "err-html",
    "json",
    "text",
    "csv",
    "teamcity",
    "azure-devops",
    "agent",
    "null",
    "dot",
    "ddot",
    "archi",
    "cdot",
    "flat",
    "fdot",
    "mermaid",
    "baseline",
    "metrics",
    "junit",
    "trx",
    "html",
    "markdown",
    "plantuml",
];

/// The rules each fixture's `explain` is compared on.
const EXPLAINED: &[(&str, &[&str])] = &[
    ("typescript", &["no-circular", "no-unresolvable"]),
    ("python", &["no-circular", "ui-not-to-models"]),
];

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../rb-wasm/tests/fixtures")
        .join(name)
}

fn rulebearing(dir: &Path, args: &[&str]) -> Result<String, Box<dyn Error>> {
    let output = Command::new(BIN)
        .args(args)
        .current_dir(dir)
        .env("SOURCE_DATE_EPOCH", EPOCH)
        .env_remove("VIRTUAL_ENV")
        .env_remove("NO_COLOR")
        .output()?;
    // A report with violations exits with their count, so the exit code is not the check; a
    // run that printed nothing where it should have is.
    if output.stdout.is_empty() && !args.contains(&"null") {
        return Err(format!(
            "{args:?} printed nothing: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(String::from_utf8(output.stdout)?)
}

/// The command line's `json` with the two keys that name its own folder written as the
/// playground's.
fn as_the_playground_s(json: &str) -> Result<String, Box<dyn Error>> {
    let mut value: Value = serde_json::from_str(json)?;
    if let Some(used) = value
        .pointer_mut("/summary/optionsUsed")
        .and_then(Value::as_object_mut)
    {
        used.insert("baseDir".into(), Value::from("/playground"));
        // Removed in place, so every other key keeps its position.
        used.shift_remove("rulesFile");
    }
    let mut text = serde_json::to_string_pretty(&value)?;
    text.push('\n');
    Ok(text)
}

fn compare(path: &Path, actual: &str) -> Result<(), Box<dyn Error>> {
    if std::env::var_os("RB_UPDATE_SNAPSHOTS").is_some() {
        std::fs::write(path, actual)?;
        return Ok(());
    }
    let expected = std::fs::read_to_string(path)?;
    assert_eq!(
        actual,
        expected,
        "{} differs; run with RB_UPDATE_SNAPSHOTS=1 and explain the change",
        path.display()
    );
    Ok(())
}

#[test]
fn the_command_line_prints_the_playground_s_expected_outputs() -> Result<(), Box<dyn Error>> {
    for (name, rules) in EXPLAINED {
        let root = fixture(name);
        let repo = root.join("repo");
        let expected = root.join("expected");
        for output_type in COMPARED {
            let mut printed = rulebearing(&repo, &["cruise", "-T", output_type])?;
            if *output_type == "json" {
                printed = as_the_playground_s(&printed)?;
            }
            compare(&expected.join(format!("{output_type}.out")), &printed)?;
        }
        for rule in *rules {
            let printed =
                rulebearing(&repo, &["explain", rule, "--graph", "../expected/json.out"])?;
            compare(&expected.join(format!("explain-{rule}.out")), &printed)?;
        }
    }
    Ok(())
}

#[test]
fn both_fixtures_find_violations_so_the_comparison_compares_something() -> Result<(), Box<dyn Error>>
{
    for (name, _) in EXPLAINED {
        let json = std::fs::read_to_string(fixture(name).join("expected/json.out"))?;
        let value: Value = serde_json::from_str(&json)?;
        assert!(
            value["summary"]["violations"]
                .as_array()
                .is_some_and(|v| v.len() >= 3),
            "{name}"
        );
    }
    Ok(())
}
