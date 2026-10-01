//! `cruise --mode source` through the binary: the flag and the configuration key, the receipt
//! and the marks, `--strict-schema`, the agent header, the cache reading only the changed
//! `.cs` files while staying equal to a cold run, and the refusal of source mode as a gate by
//! `cruise`, `fmt --exit-code`, `diff --exit-code` and `attest`.
//!
//! - Plan: [Wave 3, Step 14](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#24-steps-for-sub-wave-3d---mode-source-guard---watch-the-2-s-proof),
//!   [§ 1.5](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#15-interfaces-and-contracts-this-wave-freezes)
//!   ("Source mode")
//! - Decision: [ADR-0011](../../../docs/adr/0011-read-dotnet-assemblies-not-source.md),
//!   [ADR-0004](../../../docs/adr/0004-graph-document-is-cruise-result-superset.md)
//! - Refusal: [Wave 3, Step 15](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#24-steps-for-sub-wave-3d---mode-source-guard---watch-the-2-s-proof),
//!   [§ 1.6](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#16-decisions-applied-and-decisions-this-wave-must-make)
//!   ("Whether source mode may ever feed `--exit-code`"), [ADR-0008](../../../docs/adr/0008-exit-code-contract.md)
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
            "--allow-approximate-gate",
            "--no-progress",
        ],
    )?;
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("\"modules\""), "{text}");
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
        &[
            "cruise",
            "--mode",
            "source",
            "-T",
            "agent",
            "--allow-approximate-gate",
            "--no-progress",
        ],
    )?;
    assert_eq!(
        output.status.code(),
        Some(1),
        "the one violation, allowed as a count"
    );
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

const REFUSED: &str = "approximate-mode-not-a-gate: ";

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn a_gating_cruise_in_source_mode_is_refused_unless_allowed() -> Result {
    let dir = solution("gate", CONFIG)?;
    // The table of Step 15: (arguments, exit code, refused).
    for (args, code, refused) in [
        (&["-T", "err"][..], 2, true),
        (&["-T", "err", "--exit-code-mode", "strict"][..], 2, true),
        (&["-T", "err", "--allow-approximate-gate"][..], 1, false),
        (
            &[
                "-T",
                "err",
                "--allow-approximate-gate",
                "--exit-code-mode",
                "strict",
            ][..],
            11,
            false,
        ),
        (&["-T", "json"][..], 0, false),
    ] {
        let mut full = vec!["cruise", "--mode", "source", "--no-progress"];
        full.extend_from_slice(args);
        let output = run(&dir, &full)?;
        assert_eq!(
            output.status.code(),
            Some(code),
            "{args:?}: {}",
            stderr(&output)
        );
        assert_eq!(
            stderr(&output).contains(REFUSED),
            refused,
            "{args:?}: {}",
            stderr(&output)
        );
        if refused {
            // The report is still written: the findings are what the inner loop wants.
            assert!(
                String::from_utf8_lossy(&output.stdout).contains("orders-not-to-customers"),
                "{args:?}"
            );
        }
    }
    // A passing run is refused too: an approximate pass is no pass.
    let clean = solution(
        "gate-clean",
        "rules:\n  dependencies:\n    forbidden:\n      - name: nothing-to-app\n        comment: \"plan:rulebearing-wave-3\"\n        severity: error\n        from: { path: \"^Domain/\" }\n        to: { path: \"^App/\" }\n        allowEmpty: true\n",
    )?;
    let output = run(
        &clean,
        &[
            "cruise",
            "--mode",
            "source",
            "-T",
            "err",
            "--no-progress",
            "--liveness",
            "off",
        ],
    )?;
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert!(stderr(&output).contains(REFUSED));
    std::fs::remove_dir_all(&dir)?;
    std::fs::remove_dir_all(&clean)?;
    Ok(())
}

#[test]
fn the_stop_hook_still_answers_in_source_mode() -> Result {
    let dir = solution("hook", CONFIG)?;
    let output = isolated(BIN, &dir)
        .args(["cruise", "--mode", "source", "--from-hook", "--no-progress"])
        .stdin(std::process::Stdio::null())
        .output()?;
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let answer: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(answer["decision"], "block");
    let reason = answer["reason"].as_str().unwrap_or_default();
    assert!(
        reason.contains("orders-not-to-customers") && reason.contains("approximate: "),
        "{reason}"
    );
    assert!(!stderr(&output).contains(REFUSED));
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn a_saved_source_mode_result_does_not_gate_fmt_or_diff() -> Result {
    let dir = solution("saved", CONFIG)?;
    let output = run(
        &dir,
        &["cruise", "--mode", "source", "-T", "json", "--no-progress"],
    )?;
    std::fs::write(dir.join("source.json"), &output.stdout)?;
    for (args, code, refused) in [
        (&["fmt", "source.json", "-T", "err"][..], 0, false),
        (
            &["fmt", "source.json", "-T", "err", "--exit-code"][..],
            2,
            true,
        ),
        (
            &[
                "fmt",
                "source.json",
                "-T",
                "err",
                "--exit-code",
                "--allow-approximate-gate",
            ][..],
            1,
            false,
        ),
        (
            &["fmt", "source.json", "-T", "json", "--exit-code"][..],
            0,
            false,
        ),
        (&["diff", "source.json", "source.json"][..], 0, false),
        (
            &["diff", "source.json", "source.json", "--exit-code"][..],
            2,
            true,
        ),
        (
            &[
                "diff",
                "source.json",
                "source.json",
                "--exit-code",
                "--allow-approximate-gate",
            ][..],
            0,
            false,
        ),
    ] {
        let output = run(&dir, args)?;
        assert_eq!(
            output.status.code(),
            Some(code),
            "{args:?}: {}",
            stderr(&output)
        );
        assert_eq!(
            stderr(&output).contains(REFUSED),
            refused,
            "{args:?}: {}",
            stderr(&output)
        );
    }
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn attest_refuses_to_sign_a_source_mode_run() -> Result {
    let dir = solution(
        "attest",
        &format!("languages:\n  dotnet:\n    mode: source\n{CONFIG}"),
    )?;
    let output = run(&dir, &["attest"])?;
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert!(stderr(&output).contains(REFUSED), "{}", stderr(&output));
    assert!(!dir.join(".graph/attest.json").exists());
    // A saved source-mode graph is refused the same way.
    let saved = run(&dir, &["cruise", "-T", "json", "--no-progress"])?;
    std::fs::write(dir.join("source.json"), &saved.stdout)?;
    let output = run(&dir, &["attest", "--graph", "source.json"])?;
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert!(stderr(&output).contains(REFUSED));
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

const STRIPPED: &str = "approximate-mode-not-strict: ";

#[test]
fn strict_schema_output_of_a_source_mode_run_is_refused() -> Result {
    let dir = solution("strict-refused", CONFIG)?;
    let strict = ["-T", "json", "--strict-schema", "--no-progress"];
    let output = run(
        &dir,
        &[&["cruise", "--mode", "source"][..], &strict].concat(),
    )?;
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert!(stderr(&output).contains(STRIPPED), "{}", stderr(&output));
    assert!(output.stdout.is_empty());
    let allowed = [
        &["cruise", "--mode", "source"][..],
        &strict,
        &["--allow-approximate-gate"],
    ]
    .concat();
    let output = run(&dir, &allowed)?;
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(json(&output)?.get("modules").is_some());
    // A saved result is refused the same way by fmt.
    let saved = run(
        &dir,
        &["cruise", "--mode", "source", "-T", "json", "--no-progress"],
    )?;
    std::fs::write(dir.join("source.json"), &saved.stdout)?;
    for (args, code, refused) in [
        (
            &["fmt", "source.json", "-T", "json", "--strict-schema"][..],
            2,
            true,
        ),
        (
            &[
                "fmt",
                "source.json",
                "-T",
                "json",
                "--strict-schema",
                "--allow-approximate-gate",
            ][..],
            0,
            false,
        ),
        (&["fmt", "source.json", "-T", "json"][..], 0, false),
    ] {
        let output = run(&dir, args)?;
        assert_eq!(
            output.status.code(),
            Some(code),
            "{args:?}: {}",
            stderr(&output)
        );
        assert_eq!(stderr(&output).contains(STRIPPED), refused, "{args:?}");
    }
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn the_flag_says_how_dotnet_is_read_never_whether() -> Result {
    let dir = std::env::temp_dir().join(format!("rb-cli-source-whether-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src"))?;
    std::fs::create_dir_all(dir.join("tools/T"))?;
    std::fs::write(
        dir.join("src/a.ts"),
        "import { b } from './b';\nexport const a = b;\n",
    )?;
    std::fs::write(dir.join("src/b.ts"), "export const b = 1;\n")?;
    std::fs::write(
        dir.join("tools/T/T.csproj"),
        "<Project Sdk=\"Microsoft.NET.Sdk\"></Project>\n",
    )?;
    std::fs::write(dir.join("tools/T/X.cs"), "namespace T; class X {}\n")?;
    let output = run(
        &dir,
        &[
            "cruise",
            "src",
            "--mode",
            "source",
            "-T",
            "json",
            "--no-progress",
        ],
    )?;
    let document = json(&output)?;
    let sources: Vec<&str> = document["modules"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| m["source"].as_str())
        .collect();
    assert_eq!(sources, ["src/a.ts", "src/b.ts"], "{}", stderr(&output));
    assert!(document["summary"]["inspected"]["dotnet"].is_null());
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}
