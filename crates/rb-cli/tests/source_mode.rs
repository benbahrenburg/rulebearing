//! `cruise --mode source` through the binary: the flag and the configuration key, the receipt
//! and the marks, `--strict-schema`, the agent header, and the cache reading only the changed
//! `.cs` files while staying equal to a cold run.
//!
//! - Plan: [Wave 3, Step 14](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#24-steps-for-sub-wave-3d---mode-source-guard---watch-the-2-s-proof),
//!   [§ 1.5](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#15-interfaces-and-contracts-this-wave-freezes)
//!   ("Source mode")
//! - Decision: [ADR-0011](../../../docs/adr/0011-read-dotnet-assemblies-not-source.md),
//!   [ADR-0004](../../../docs/adr/0004-graph-document-is-cruise-result-superset.md)
//! - Requirement: [FR-EXT-DN-04](../../../docs/prd.md#fr-ext-dn-04),
//!   [FR-CLI-05](../../../docs/prd.md#fr-cli-05)

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

type Result<T = ()> = std::result::Result<T, Box<dyn Error>>;

const BIN: &str = env!("CARGO_BIN_EXE_rulebearing");

const GIT_LOCAL_ENV: [&str; 15] = [
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_CONFIG",
    "GIT_CONFIG_PARAMETERS",
    "GIT_CONFIG_COUNT",
    "GIT_OBJECT_DIRECTORY",
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_IMPLICIT_WORK_TREE",
    "GIT_GRAFT_FILE",
    "GIT_INDEX_FILE",
    "GIT_NO_REPLACE_OBJECTS",
    "GIT_REPLACE_REF_BASE",
    "GIT_PREFIX",
    "GIT_SHALLOW_FILE",
    "GIT_COMMON_DIR",
];

/// A rule the fixture breaks: `Order.cs` holds a `Customer`.
const CONFIG: &str = "rules:
  dependencies:
    forbidden:
      - name: orders-not-to-customers
        comment: \"plan:rulebearing-wave-3\"
        fix: \"Pass the customer's id, not the customer.\"
        severity: error
        from: { path: \"^Domain/Orders/\" }
        to: { path: \"^Domain/Customers/\" }
";

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../rb-extract-dotnet/tests/fixtures/source")
}

fn copy(from: &Path, to: &Path) -> Result {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// A copy of the fixture solution with `config` as `rulebearing.yaml`.
fn solution(name: &str, config: &str) -> Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("rb-cli-source-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    copy(&fixture(), &dir)?;
    std::fs::write(dir.join("rulebearing.yaml"), config)?;
    Ok(dir.canonicalize()?)
}

fn isolated(program: &str, dir: &Path) -> Command {
    let mut command = Command::new(program);
    command
        .current_dir(dir)
        .env("SOURCE_DATE_EPOCH", "1790000000");
    for name in GIT_LOCAL_ENV {
        command.env_remove(name);
    }
    // The caller's virtual environment is not the fixture's: a run finds the one in the tree.
    command.env_remove("VIRTUAL_ENV");
    command
}

fn git(dir: &Path, args: &[&str]) -> Result {
    let output = isolated("git", dir)
        .args([
            "-c",
            "user.name=rulebearing",
            "-c",
            "user.email=rulebearing@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(())
}

fn run(dir: &Path, args: &[&str]) -> Result<Output> {
    Ok(isolated(BIN, dir).args(args).output()?)
}

fn json(output: &Output) -> Result<Value> {
    Ok(serde_json::from_slice(&output.stdout)?)
}

fn violations(document: &Value) -> Vec<(String, String)> {
    document["summary"]["violations"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|v| {
            (
                v["from"].as_str().unwrap_or_default().to_owned(),
                v["to"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect()
}

#[test]
fn the_flag_reads_the_solution_from_source_and_marks_it() -> Result {
    let dir = solution("flag", CONFIG)?;
    let output = run(
        &dir,
        &["cruise", "--mode", "source", "-T", "json", "--no-progress"],
    )?;
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let document = json(&output)?;
    assert_eq!(document["summary"]["inspected"]["dotnet"]["mode"], "source");
    assert_eq!(document["summary"]["inspected"]["dotnet"]["assemblies"], 0);
    assert_eq!(
        violations(&document),
        vec![(
            "Domain/Orders/Order.cs".to_owned(),
            "Domain/Customers/Customer.cs".to_owned()
        )]
    );
    let dotnet: Vec<&Value> = document["modules"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| m["language"] == "dotnet")
        .collect();
    assert!(!dotnet.is_empty());
    for module in &dotnet {
        for dependency in module["dependencies"].as_array().into_iter().flatten() {
            assert_eq!(dependency["approximate"], true, "{}", module["source"]);
        }
        if module["followable"] == true {
            assert_eq!(module["attribution"], "source", "{}", module["source"]);
        }
    }
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn the_configuration_key_does_what_the_flag_does() -> Result {
    let dir = solution(
        "key",
        &format!("languages:\n  dotnet:\n    mode: source\n{CONFIG}"),
    )?;
    let keyed = run(&dir, &["cruise", "-T", "json", "--no-progress"])?;
    let flagged = run(
        &dir,
        &["cruise", "--mode", "source", "-T", "json", "--no-progress"],
    )?;
    assert_eq!(
        json(&keyed)?["summary"]["inspected"]["dotnet"]["mode"],
        "source"
    );
    assert_eq!(keyed.stdout, flagged.stdout);
    // The flag wins over the key: compiled mode finds no built assembly here.
    let compiled = run(
        &dir,
        &[
            "cruise",
            "--mode",
            "compiled",
            "-T",
            "json",
            "--no-progress",
        ],
    )?;
    assert_ne!(compiled.stdout, keyed.stdout);
    assert!(json(&compiled).map_or(true, |d| {
        d["summary"]["inspected"]["dotnet"]["mode"].is_null()
    }));
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn strict_schema_removes_the_marks() -> Result {
    let dir = solution("strict", CONFIG)?;
    let output = run(
        &dir,
        &[
            "cruise",
            "--mode",
            "source",
            "-T",
            "json",
            "--strict-schema",
            "--no-progress",
        ],
    )?;
    let text = String::from_utf8_lossy(&output.stdout);
    for gone in ["approximate", "\"attribution\"", "\"inspected\""] {
        assert!(!text.contains(gone), "{gone} survived --strict-schema");
    }
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn the_agent_report_opens_by_saying_it_is_approximate() -> Result {
    let dir = solution("agent", CONFIG)?;
    let output = run(
        &dir,
        &["cruise", "--mode", "source", "-T", "agent", "--no-progress"],
    )?;
    let report = json(&output)?;
    assert!(
        report["approximate"]
            .as_str()
            .is_some_and(|s| s.starts_with("approximate: "))
    );
    assert_eq!(report["rules"][0]["name"], "orders-not-to-customers");
    assert_eq!(report["rules"][0]["violations"][0]["approximate"], true);
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

/// The cached run's report, and what the cache says it did.
fn cached(dir: &Path) -> Result<(String, String)> {
    let output = run(
        dir,
        &[
            "cruise",
            "--mode",
            "source",
            "--cache",
            "-T",
            "json",
            "--progress",
            "performance-log",
        ],
    )?;
    let log = String::from_utf8_lossy(&output.stderr).into_owned();
    // The report without the cache's own receipt, to compare with an uncached run.
    let mut document = json(&output)?;
    if let Some(summary) = document["summary"].as_object_mut() {
        summary.retain(|key, _| key != "cache");
    }
    // The two runs differ in their cache and progress flags, and in nothing else.
    if let Some(used) = document["summary"]["optionsUsed"].as_object_mut() {
        used.retain(|key, _| key != "cache" && key != "progress");
    }
    Ok((serde_json::to_string_pretty(&document)?, log))
}

fn uncached(dir: &Path) -> Result<String> {
    let output = run(
        dir,
        &["cruise", "--mode", "source", "-T", "json", "--no-progress"],
    )?;
    let mut document = json(&output)?;
    if let Some(used) = document["summary"]["optionsUsed"].as_object_mut() {
        used.retain(|key, _| key != "progress");
    }
    Ok(serde_json::to_string_pretty(&document)?)
}

#[test]
fn the_cache_parses_only_the_changed_file_and_equals_a_cold_run() -> Result {
    let dir = solution("cache", CONFIG)?;
    git(&dir, &["init", "-q"])?;
    git(&dir, &["add", "-A"])?;
    git(&dir, &["commit", "-q", "-m", "fixture"])?;
    let (first, _) = cached(&dir)?;
    assert_eq!(first, uncached(&dir)?);
    let (hit, log) = cached(&dir)?;
    assert!(log.contains("from the cache"), "{log}");
    assert_eq!(hit, first);
    // An edit that removes the violating edge: only that file is parsed again.
    std::fs::write(
        dir.join("Domain/Orders/Order.cs"),
        "namespace Shop.Domain.Orders;\n\npublic partial class Order\n{\n    public Order(int customer)\n    {\n    }\n}\n",
    )?;
    let (edited, log) = cached(&dir)?;
    assert!(
        log.contains(".NET read again") && log.contains("incremental"),
        "{log}"
    );
    assert_eq!(edited, uncached(&dir)?);
    assert!(!edited.contains("orders-not-to-customers\",\"severity"));
    // A new file is structural: everything is read, and the result still equals a cold run.
    std::fs::write(
        dir.join("Domain/Orders/Refund.cs"),
        "namespace Shop.Domain.Orders;\n\npublic class Refund { private readonly Customer _to = null!; }\n",
    )?;
    let (added, log) = cached(&dir)?;
    assert!(!log.contains("incremental"), "{log}");
    assert_eq!(added, uncached(&dir)?);
    let document: Value = serde_json::from_str(&added)?;
    assert!(violations(&document).contains(&(
        "Domain/Orders/Refund.cs".to_owned(),
        "Domain/Customers/Customer.cs".to_owned()
    )));
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}
