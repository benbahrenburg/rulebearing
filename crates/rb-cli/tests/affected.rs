//! `cruise --affected [revision]` over scratch git repositories: every kind of change git
//! reports, the report narrowed to the changed modules and the modules that reach them, the
//! receipt, the error paths, and the .NET mapping of a changed source file to its types' modules.
//!
//! - Plan: [Wave 3, Step 3](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#21-steps-for-sub-wave-3a-cache---affected-diff---exit-code-mode-strict)
//!   ("a .NET fixture where a changed `.cs` file affects a partial class across two files")
//! - Coverage: [coverage § Command line](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#command-line)
//!   row `--affected [revision]`
//! - Decision: [ADR-0008](../../../docs/adr/0008-exit-code-contract.md) (an unknown revision is
//!   exit 2)
//! - Requirement: [FR-CLI-05](../../../docs/prd.md#fr-cli-05)
//!
//! The TypeScript repository commits seven modules, then edits it once in each way git can: a
//! modified file (`a.ts`, which now closes a cycle with `b.ts` and imports the forbidden module),
//! a staged new file, an untracked file, a deleted file, a staged rename, and a file outside
//! every closure left alone. The expected report is what dependency-cruiser 18.2.0 gives for the
//! same tree, measured with the pinned binary and compared by layer 5's `zero-diff.mjs` with no
//! difference: the violations between modules of the closure (the cycle once, as upstream
//! reports it), and nothing on an edge that leaves it.

use std::collections::BTreeSet;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{Value, json};

type Result<T = ()> = std::result::Result<T, Box<dyn Error>>;

const BIN: &str = env!("CARGO_BIN_EXE_rulebearing");

/// The variables git sets for a hook; each command here runs without them, so a test run from a
/// hook never reaches this repository instead of the scratch one.
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

fn isolated(program: &str, dir: &Path) -> Command {
    let mut command = Command::new(program);
    command.current_dir(dir);
    for name in GIT_LOCAL_ENV {
        command.env_remove(name);
    }
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
            "init.defaultBranch=main",
        ])
        .args(args)
        .output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!("git {args:?}: {}", String::from_utf8_lossy(&output.stderr)).into())
    }
}

fn write(dir: &Path, file: &str, text: &str) -> Result {
    let path = dir.join(file);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, text)?;
    Ok(())
}

fn scratch(tag: &str) -> Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("rb-affected-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

const CONFIG: &str = r#"{
  "forbidden": [
    { "name": "no-forbidden", "severity": "error", "from": {}, "to": { "path": "forbidden" } },
    { "name": "no-circular", "severity": "error", "from": {}, "to": { "circular": true } },
    { "name": "no-orphans", "severity": "error", "from": { "orphan": true }, "to": {} }
  ]
}
"#;

/// The committed tree, then one edit of every kind.
fn typescript_repository(tag: &str) -> Result<PathBuf> {
    let dir = scratch(tag)?;
    git(&dir, &["init", "-q"])?;
    for (file, text) in [
        (".dependency-cruiser.json", CONFIG),
        ("src/a.ts", "export const a = 1;\n"),
        (
            "src/b.ts",
            "import { a } from \"./a\";\nexport const b = a;\n",
        ),
        (
            "src/c.ts",
            "import { b } from \"./b\";\nexport const c = b;\n",
        ),
        ("src/forbidden.ts", "export const x = 1;\n"),
        (
            "src/untouched.ts",
            "import { x } from \"./forbidden\";\nexport const u = x;\n",
        ),
        ("src/gone.ts", "export const g = 1;\n"),
        (
            "src/user.ts",
            "import { g } from \"./gone\";\nexport const v = g;\n",
        ),
        ("src/old-name.ts", "export const r = 1;\n"),
    ] {
        write(&dir, file, text)?;
    }
    git(&dir, &["add", "-A"])?;
    git(&dir, &["commit", "-qm", "base"])?;
    // Modified: a now imports b (a cycle) and the forbidden module (an edge leaving the closure).
    write(
        &dir,
        "src/a.ts",
        "import { b } from \"./b\";\nimport { x } from \"./forbidden\";\nexport const a = b + x;\n",
    )?;
    // Added and staged; untracked; deleted; renamed and staged.
    write(&dir, "src/staged.ts", "export const s = 1;\n")?;
    git(&dir, &["add", "src/staged.ts"])?;
    write(&dir, "src/untracked.ts", "export const t = 1;\n")?;
    git(&dir, &["rm", "-q", "src/gone.ts"])?;
    git(&dir, &["mv", "src/old-name.ts", "src/new-name.ts"])?;
    // A new file in a new, untracked folder, which imports the forbidden module.
    write(
        &dir,
        "src/newdir/fresh.ts",
        "import { x } from \"../forbidden\";\nexport const f = x;\n",
    )?;
    Ok(dir)
}

/// `cruise --config <native file> --affected HEAD -T <output_type> src`.
fn native(dir: &Path, config: &str, output_type: &str) -> Result<Output> {
    cruise(
        dir,
        &[
            "--config",
            config,
            "--affected",
            "HEAD",
            "-T",
            output_type,
            "src",
        ],
    )
}

/// The same rules as [`CONFIG`], in a native file outside the repository, so the tree is the
/// same as the dependency-cruiser runs see.
fn native_config(tag: &str) -> Result<PathBuf> {
    let dir = scratch(&format!("{tag}-config"))?;
    let file = dir.join("rulebearing.yaml");
    std::fs::write(
        &file,
        "rules:\n  dependencies:\n    forbidden:\n      - { name: no-forbidden, comment: t, severity: error, from: {}, to: { path: forbidden } }\n      - { name: no-circular, comment: t, severity: error, from: {}, to: { circular: true } }\n      - { name: no-orphans, comment: t, severity: error, from: { orphan: true }, to: {} }\n",
    )?;
    Ok(file)
}

fn cruise(dir: &Path, args: &[&str]) -> Result<Output> {
    Ok(isolated(BIN, dir)
        .args(["cruise", "--no-progress"])
        .args(args)
        .output()?)
}

fn violations(result: &Value) -> BTreeSet<(String, String, String)> {
    result["summary"]["violations"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|v| {
            (
                v["rule"]["name"].as_str().unwrap_or_default().to_owned(),
                v["from"].as_str().unwrap_or_default().to_owned(),
                v["to"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect()
}

fn triple(rule: &str, from: &str, to: &str) -> (String, String, String) {
    (rule.into(), from.into(), to.into())
}

fn sources(result: &Value) -> Vec<String> {
    result["modules"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| m["source"].as_str().map(str::to_owned))
        .collect()
}

#[test]
fn affected_reports_the_changed_modules_and_what_reaches_them() -> Result {
    let dir = typescript_repository("ts")?;
    let output = cruise(&dir, &["--affected", "HEAD", "-T", "json", "src"])?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(0),
        "json does not gate: {stderr}"
    );
    let result: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(
        result["summary"]["optionsUsed"]["reaches"],
        json!({ "path": "^(?:src/a[.]ts|src/new-name[.]ts|src/staged[.]ts|src/untracked[.]ts)$" }),
        "upstream's expression: the diff in git's order, then the untracked files; no deleted file"
    );
    assert!(
        result["summary"]["optionsUsed"].get("affected").is_none(),
        "dependency-cruiser does not share `affected` in optionsUsed"
    );
    assert_eq!(
        sources(&result),
        [
            "src/a.ts",
            "src/b.ts",
            "src/c.ts",
            "src/new-name.ts",
            "src/staged.ts",
            "src/untracked.ts"
        ]
    );
    assert_eq!(
        violations(&result),
        BTreeSet::from([
            triple("no-circular", "src/a.ts", "src/b.ts"),
            triple("no-orphans", "src/new-name.ts", "src/new-name.ts"),
            triple("no-orphans", "src/staged.ts", "src/staged.ts"),
            triple("no-orphans", "src/untracked.ts", "src/untracked.ts"),
        ]),
        "violations inside the closure only: src/a.ts -> src/forbidden.ts leaves it, and \
         src/untouched.ts and src/user.ts are not in it"
    );
    assert_eq!(result["summary"]["error"], 4);
    assert_eq!(
        result["summary"]["affected"],
        json!({
            "revision": "HEAD",
            "changed": [
                "src/a.ts",
                "src/gone.ts",
                "src/new-name.ts",
                "src/newdir/",
                "src/staged.ts",
                "src/untracked.ts"
            ],
            "closure": [
                "src/a.ts",
                "src/b.ts",
                "src/c.ts",
                "src/new-name.ts",
                "src/staged.ts",
                "src/untracked.ts"
            ]
        })
    );

    // The strict schema strips the receipt; nothing else changes.
    let strict = cruise(
        &dir,
        &["--affected", "HEAD", "-T", "json", "--strict-schema", "src"],
    )?;
    let strict: Value = serde_json::from_slice(&strict.stdout)?;
    assert!(strict["summary"].get("affected").is_none());
    assert_eq!(violations(&strict), violations(&result));

    // Two runs over the same tree serialise byte for byte.
    let again = cruise(&dir, &["--affected", "HEAD", "-T", "json", "src"])?;
    assert_eq!(again.stdout, output.stdout);
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn the_agent_report_holds_exactly_the_closures_violations() -> Result {
    let dir = typescript_repository("agent")?;
    let output = cruise(
        &dir,
        &["--affected", "HEAD", "--output-type", "agent", "src"],
    )?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(4),
        "agent gates with the closure's error count, not the repository's: {stderr}"
    );
    let report: Value = serde_json::from_slice(&output.stdout)?;
    let mut found = BTreeSet::new();
    for rule in report["rules"].as_array().into_iter().flatten() {
        for v in rule["violations"].as_array().into_iter().flatten() {
            found.insert(triple(
                rule["name"].as_str().unwrap_or_default(),
                v["from"].as_str().unwrap_or_default(),
                v["to"].as_str().unwrap_or_default(),
            ));
        }
    }
    assert_eq!(
        found,
        BTreeSet::from([
            triple("no-circular", "src/a.ts", "src/b.ts"),
            triple("no-orphans", "src/new-name.ts", "src/new-name.ts"),
            triple("no-orphans", "src/staged.ts", "src/staged.ts"),
            triple("no-orphans", "src/untracked.ts", "src/untracked.ts"),
        ])
    );
    // Without --affected the whole repository is reported, and gates with its own count.
    let whole = cruise(&dir, &["--output-type", "agent", "src"])?;
    assert!(whole.status.code() > Some(4), "{:?}", whole.status.code());
    // Strict exit codes: 10 + the count.
    let strict = cruise(
        &dir,
        &[
            "--affected",
            "HEAD",
            "-T",
            "agent",
            "--exit-code-mode",
            "strict",
            "src",
        ],
    )?;
    assert_eq!(strict.status.code(), Some(14));
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn depth_limits_the_dependents() -> Result {
    let dir = scratch("depth")?;
    git(&dir, &["init", "-q"])?;
    for (file, text) in [
        (".dependency-cruiser.json", "{}\n"),
        ("src/a.ts", "export const a = 1;\n"),
        (
            "src/b.ts",
            "import { a } from \"./a\";\nexport const b = a;\n",
        ),
        (
            "src/c.ts",
            "import { b } from \"./b\";\nexport const c = b;\n",
        ),
    ] {
        write(&dir, file, text)?;
    }
    git(&dir, &["add", "-A"])?;
    git(&dir, &["commit", "-qm", "base"])?;
    write(&dir, "src/a.ts", "export const a = 2;\n")?;
    let one = cruise(
        &dir,
        &["-A", "HEAD", "--affected-depth", "1", "-T", "json", "src"],
    )?;
    let one: Value = serde_json::from_slice(&one.stdout)?;
    assert_eq!(sources(&one), ["src/a.ts", "src/b.ts"]);
    assert_eq!(one["summary"]["affected"]["depth"], 1);
    assert_eq!(
        one["summary"]["optionsUsed"]["reaches"],
        json!({ "path": "^(?:src/a[.]ts)$" }),
        "the depth never reaches optionsUsed, whose reaches upstream's schema closes"
    );
    let all = cruise(
        &dir,
        &["-A", "HEAD", "--affected-depth", "0", "-T", "json", "src"],
    )?;
    let all: Value = serde_json::from_slice(&all.stdout)?;
    assert_eq!(sources(&all), ["src/a.ts", "src/b.ts", "src/c.ts"]);
    assert!(all["summary"]["affected"].get("depth").is_none());

    // From a subdirectory, paths are relative to it, as module names are.
    let sub = isolated(BIN, &dir.join("src"))
        .args([
            "cruise",
            "--no-progress",
            "--config",
            "../.dependency-cruiser.json",
            "-A",
            "HEAD",
            "-T",
            "json",
            ".",
        ])
        .output()?;
    let sub: Value = serde_json::from_slice(&sub.stdout)?;
    assert_eq!(sources(&sub), ["a.ts", "b.ts", "c.ts"]);
    assert_eq!(sub["summary"]["affected"]["changed"], json!(["a.ts"]));

    // Nothing changed: nothing to report.
    git(&dir, &["commit", "-qam", "edit"])?;
    let none = cruise(&dir, &["-A", "HEAD", "-T", "json", "src"])?;
    assert_eq!(none.status.code(), Some(0));
    let none: Value = serde_json::from_slice(&none.stdout)?;
    assert!(sources(&none).is_empty());
    assert_eq!(
        none["summary"]["optionsUsed"]["reaches"],
        json!({ "path": "^(?:)$" })
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn what_cannot_be_compared_is_named() -> Result {
    let dir = typescript_repository("errors")?;
    let unknown = cruise(&dir, &["--affected", "no-such-revision", "src"])?;
    assert_eq!(unknown.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&unknown.stderr).contains("revision 'no-such-revision' unknown"),
        "{}",
        String::from_utf8_lossy(&unknown.stderr)
    );
    let option = cruise(&dir, &["--affected=--output=x", "src"])?;
    assert_eq!(option.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&option.stderr).contains("is not a revision"));
    assert!(!dir.join("x").exists(), "nothing reached git as an option");
    let depth = cruise(&dir, &["--affected-depth", "1", "src"])?;
    assert_eq!(depth.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&depth.stderr).contains("--affected-depth needs --affected"));
    let _ = std::fs::remove_dir_all(&dir);

    let outside = scratch("no-repo")?;
    write(&outside, "src/a.ts", "export const a = 1;\n")?;
    let alone = isolated(BIN, &outside)
        .env(
            "GIT_CEILING_DIRECTORIES",
            outside.parent().unwrap_or(&outside),
        )
        .args(["cruise", "--no-progress", "--no-config", "-A", "src"])
        .output()?;
    assert_eq!(alone.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&alone.stderr).contains("does not seem to be a git repository"),
        "{}",
        String::from_utf8_lossy(&alone.stderr)
    );
    let _ = std::fs::remove_dir_all(&outside);
    Ok(())
}

#[test]
fn a_native_run_reports_every_violation_that_touches_the_closure() -> Result {
    let dir = typescript_repository("native")?;
    let config = native_config("native")?;
    let config = config.to_string_lossy().into_owned();
    let output = native(&dir, &config, "json")?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(0), "{stderr}");
    let result: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(
        sources(&result),
        [
            "src/a.ts",
            "src/b.ts",
            "src/c.ts",
            "src/new-name.ts",
            "src/newdir/fresh.ts",
            "src/staged.ts",
            "src/untracked.ts"
        ],
        "the file in the untracked folder counts; the forbidden module and src/untouched.ts do not"
    );
    let found = violations(&result);
    assert!(
        found.contains(&triple("no-forbidden", "src/a.ts", "src/forbidden.ts")),
        "the edited file's import of an unchanged forbidden module is reported: {found:?}"
    );
    assert!(found.contains(&triple(
        "no-forbidden",
        "src/newdir/fresh.ts",
        "src/forbidden.ts"
    )));
    assert!(
        !found.contains(&triple(
            "no-forbidden",
            "src/untouched.ts",
            "src/forbidden.ts"
        )),
        "a violation outside the closure is not"
    );
    for orphan in ["src/new-name.ts", "src/staged.ts", "src/untracked.ts"] {
        assert!(
            found.contains(&triple("no-orphans", orphan, orphan)),
            "{orphan}"
        );
    }
    assert!(found.contains(&triple("no-circular", "src/a.ts", "src/b.ts")));
    assert!(
        found
            .iter()
            .all(|(_, from, _)| sources(&result).contains(from)),
        "every violation starts in the report: {found:?}"
    );
    assert_eq!(result["summary"]["error"], found.len());
    assert!(
        result["summary"]["optionsUsed"].get("reaches").is_none(),
        "no reaches filter"
    );
    assert_eq!(
        result["summary"]["affected"]["changed"],
        json!([
            "src/a.ts",
            "src/gone.ts",
            "src/new-name.ts",
            "src/newdir/fresh.ts",
            "src/staged.ts",
            "src/untracked.ts"
        ])
    );
    // The agent report gates with the same count.
    let agent = native(&dir, &config, "agent")?;
    assert_eq!(
        agent.status.code().map(i64::from),
        result["summary"]["error"].as_i64()
    );
    let again = native(&dir, &config, "json")?;
    assert_eq!(again.stdout, output.stdout, "byte for byte");
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn the_same_tree_under_a_dependency_cruiser_configuration_is_upstreams() -> Result {
    // Neither forbidden import is reported, and the untracked folder does not count.
    let dir = typescript_repository("native-upstream")?;
    let upstream = cruise(&dir, &["--affected", "HEAD", "-T", "json", "src"])?;
    let upstream: Value = serde_json::from_slice(&upstream.stdout)?;
    let upstream_found = violations(&upstream);
    assert!(!upstream_found.contains(&triple("no-forbidden", "src/a.ts", "src/forbidden.ts")));
    assert!(!sources(&upstream).contains(&"src/newdir/fresh.ts".to_owned()));
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn a_native_run_follows_a_deleted_file_to_its_dependents_in_the_saved_graph() -> Result {
    let dir = scratch("deleted")?;
    git(&dir, &["init", "-q"])?;
    for (file, text) in [
        ("src/gone.ts", "export const g = 1;\n"),
        (
            "src/user.ts",
            "import { g } from \"./gone\";\nexport const v = g;\n",
        ),
        (
            "src/top.ts",
            "import { v } from \"./user\";\nexport const t = v;\n",
        ),
        ("src/alone.ts", "export const a = 1;\n"),
    ] {
        write(&dir, file, text)?;
    }
    git(&dir, &["add", "-A"])?;
    git(&dir, &["commit", "-qm", "base"])?;
    let config = native_config("deleted")?;
    let config = config.to_string_lossy().into_owned();
    let saved = cruise(
        &dir,
        &[
            "--config",
            &config,
            "--no-liveness",
            "-T",
            "json",
            "-f",
            ".graph/cruise.json",
            "src",
        ],
    )?;
    assert_eq!(saved.status.code(), Some(0));
    git(&dir, &["rm", "-q", "src/gone.ts"])?;
    let args = [
        "--config",
        &config,
        "--no-liveness",
        "-A",
        "HEAD",
        "-T",
        "json",
        "src",
    ];
    let output = cruise(&dir, &args)?;
    let result: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(
        result["summary"]["affected"]["closure"],
        json!(["src/top.ts", "src/user.ts"]),
        "the saved graph names the deleted file's importer, and what reaches it follows"
    );
    std::fs::write(dir.join(".graph/cruise.json"), "not json")?;
    let broken = cruise(&dir, &args)?;
    assert_eq!(broken.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&broken.stderr).contains("is not a cruise result"));
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn a_configuration_can_ask_for_it_only_natively() -> Result {
    let dir = scratch("config")?;
    git(&dir, &["init", "-q"])?;
    write(&dir, "src/a.ts", "export const a = 1;\n")?;
    write(&dir, "src/b.ts", "export const b = 1;\n")?;
    git(&dir, &["add", "-A"])?;
    git(&dir, &["commit", "-qm", "base"])?;
    write(&dir, "src/a.ts", "export const a = 2;\n")?;
    write(
        &dir,
        "rulebearing.yaml",
        "options:\n  affected: HEAD\nrules:\n  dependencies:\n    forbidden: []\n",
    )?;
    let native = cruise(&dir, &["-T", "json", "src"])?;
    let native: Value = serde_json::from_slice(&native.stdout)?;
    assert_eq!(sources(&native), ["src/a.ts"]);
    std::fs::remove_file(dir.join("rulebearing.yaml"))?;
    write(
        &dir,
        ".dependency-cruiser.json",
        "{\"options\":{\"affected\":\"HEAD\"}}\n",
    )?;
    let cruiser = cruise(&dir, &["-T", "json", "src"])?;
    let stderr = String::from_utf8_lossy(&cruiser.stderr);
    assert!(stderr.contains("options.affected"), "{stderr}");
    let cruiser: Value = serde_json::from_slice(&cruiser.stdout)?;
    assert_eq!(
        sources(&cruiser),
        ["src/a.ts", "src/b.ts"],
        "dependency-cruiser ignores it in a configuration file"
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

/// A saved .NET graph shaped as the extractor writes it: `Sample.Orders.Order` is a partial class
/// whose primary file is `src/Order.cs` and whose other part, `src/Order.Parts.cs`, declares no
/// type of its own, so it is no module. `src/Billing.cs` depends on the order.
fn dotnet_graph() -> Value {
    let module = |source: &str, to: &[&str]| {
        json!({
            "source": source,
            "dependencies": to.iter().map(|t| json!({
                "module": t, "resolved": t, "moduleSystem": "es6", "coreModule": false,
                "couldNotResolve": false, "followable": true, "exoticallyRequired": false,
                "dynamic": false, "dependencyTypes": ["local"], "valid": true, "circular": false
            })).collect::<Vec<_>>(),
            "valid": true,
            "language": "dotnet",
            "attribution": "pdb",
        })
    };
    let ty = |name: &str, file: &str, files: &[&str]| {
        json!({ "fullName": name, "name": name, "kind": "class", "language": "dotnet",
                "file": file, "attribution": "pdb", "files": files })
    };
    json!({
        "modules": [
            module("src/Billing.cs", &["src/Order.cs"]),
            module("src/Customer.cs", &[]),
            module("src/Order.cs", &["src/Customer.cs"]),
            module("src/Report.cs", &[]),
        ],
        "summary": { "violations": [], "error": 0, "warn": 0, "info": 0, "totalCruised": 4, "optionsUsed": {} },
        "code": { "types": [
            ty("Sample.Orders.Order", "src/Order.cs", &["src/Order.Parts.cs"]),
            ty("Sample.Customers.Customer", "src/Customer.cs", &[]),
            ty("Sample.Billing", "src/Billing.cs", &[]),
            ty("Sample.Report", "src/Report.cs", &[]),
        ] }
    })
}

#[test]
fn a_changed_part_of_a_partial_class_affects_the_type() -> Result {
    let dir = scratch("dotnet-partial")?;
    git(&dir, &["init", "-q"])?;
    write(
        &dir,
        "graph.json",
        &serde_json::to_string_pretty(&dotnet_graph())?,
    )?;
    for file in [
        "src/Order.cs",
        "src/Order.Parts.cs",
        "src/Customer.cs",
        "src/Billing.cs",
        "src/Report.cs",
    ] {
        write(&dir, file, "namespace Sample;\n")?;
    }
    git(&dir, &["add", "-A"])?;
    git(&dir, &["commit", "-qm", "base"])?;
    write(
        &dir,
        "src/Order.Parts.cs",
        "namespace Sample;\npartial class Order {}\n",
    )?;
    let output = cruise(
        &dir,
        &[
            "--no-config",
            "--graph",
            "graph.json",
            "-A",
            "HEAD",
            "-T",
            "json",
        ],
    )?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(0), "{stderr}");
    let result: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(
        sources(&result),
        ["src/Billing.cs", "src/Order.cs"],
        "the part maps to the type's module, and its dependent follows"
    );
    assert_eq!(
        result["summary"]["optionsUsed"]["reaches"],
        json!({ "path": "^(?:src/Order\\.cs)$" })
    );
    assert_eq!(
        result["summary"]["affected"]["changed"],
        json!(["src/Order.Parts.cs"])
    );
    assert_eq!(
        result["summary"]["affected"]["closure"],
        json!(["src/Billing.cs", "src/Order.cs"])
    );

    // The primary file affects the type too; a file with a type of its own affects only it.
    git(&dir, &["commit", "-qam", "part"])?;
    write(
        &dir,
        "src/Report.cs",
        "namespace Sample;\nclass Report {}\n",
    )?;
    let report = cruise(
        &dir,
        &[
            "--no-config",
            "--graph",
            "graph.json",
            "-A",
            "HEAD",
            "-T",
            "json",
        ],
    )?;
    let report: Value = serde_json::from_slice(&report.stdout)?;
    assert_eq!(sources(&report), ["src/Report.cs"]);
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

/// Where the sample's PDB says its sources were: their path in this repository, which is where
/// they sit in the scratch repository too, as sources sit beside the build in a real one.
const SAMPLE_SOURCES: &str = "crates/rb-extract-dotnet/tests/fixtures/sample/src";

/// The .NET extractor end to end: the sample assembly and its PDB, with the sources it was built
/// from in a git repository. A change to `Order.Lines.cs`, which holds part of the partial class
/// `Sample.Orders.Order`, affects the order's module `Order.cs` as well as its own.
#[test]
fn a_changed_source_file_maps_through_the_pdb() -> Result {
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../rb-extract-dotnet/tests/fixtures/sample");
    let dir = scratch("dotnet-pdb")?;
    git(&dir, &["init", "-q"])?;
    write(
        &dir,
        "rulebearing.yaml",
        "languages:\n  dotnet:\n    assemblies: [\"dotnet/*.dll\"]\nrules:\n  dependencies:\n    forbidden: []\n",
    )?;
    std::fs::create_dir_all(dir.join("dotnet"))?;
    for file in ["Sample.dll", "Sample.pdb"] {
        std::fs::copy(
            fixture.join("built").join(file),
            dir.join("dotnet").join(file),
        )?;
    }
    let sources_dir = dir.join(SAMPLE_SOURCES);
    std::fs::create_dir_all(&sources_dir)?;
    for file in ["Order.cs", "Order.Lines.cs", "Customer.cs", "Shared.cs"] {
        std::fs::copy(fixture.join("src").join(file), sources_dir.join(file))?;
    }
    git(&dir, &["add", "-A"])?;
    git(&dir, &["commit", "-qm", "base"])?;
    let lines = std::fs::read_to_string(sources_dir.join("Order.Lines.cs"))?;
    std::fs::write(
        sources_dir.join("Order.Lines.cs"),
        format!("{lines}// edited\n"),
    )?;
    let output = cruise(&dir, &["-A", "HEAD", "-T", "json", "--no-liveness"])?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(0), "{stderr}");
    let result: Value = serde_json::from_slice(&output.stdout)?;
    assert!(
        result["summary"]["optionsUsed"].get("reaches").is_none(),
        "a native run sets no reaches filter"
    );
    let closure = result["summary"]["affected"]["closure"].clone();
    let expected = [
        format!("{SAMPLE_SOURCES}/Order.Lines.cs"),
        format!("{SAMPLE_SOURCES}/Order.cs"),
    ];
    assert_eq!(closure, json!(expected));
    assert_eq!(sources(&result), expected);
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

/// A repository in a scratch folder with `files` committed.
fn committed(tag: &str, files: &[(&str, &str)]) -> Result<PathBuf> {
    let dir = scratch(tag)?;
    git(&dir, &["init", "-q"])?;
    for (file, text) in files {
        write(&dir, file, text)?;
    }
    git(&dir, &["add", "-A"])?;
    git(&dir, &["commit", "-qm", "base"])?;
    Ok(dir)
}

/// A native configuration holding `rules` (YAML list items under `forbidden`), outside the
/// repository.
fn native_rules(tag: &str, rules: &str) -> Result<String> {
    let dir = scratch(&format!("{tag}-config"))?;
    let file = dir.join("rulebearing.yaml");
    std::fs::write(
        &file,
        format!("rules:\n  dependencies:\n    forbidden:\n{rules}"),
    )?;
    Ok(file.to_string_lossy().into_owned())
}

fn sorted(mut list: Vec<String>) -> Vec<String> {
    list.sort();
    list
}

/// Review finding 1: a depth measured by the shortest path. `x` reaches `t` in one step and
/// through `a` in two, so `y` is two steps from `t` whichever path a walk finds first.
#[test]
fn the_depth_is_the_shortest_path_to_a_change() -> Result {
    let dir = committed(
        "shortest",
        &[
            (
                ".dependency-cruiser.json",
                r#"{"forbidden":[{"name":"no-y-to-x","severity":"error","from":{"path":"y"},"to":{"path":"x"}}]}"#,
            ),
            ("src/t.ts", "export const t = 1;\n"),
            (
                "src/a.ts",
                "import { t } from \"./t\";\nexport const a = t;\n",
            ),
            (
                "src/x.ts",
                "import { a } from \"./a\";\nimport { t } from \"./t\";\nexport const x = a + t;\n",
            ),
            (
                "src/y.ts",
                "import { x } from \"./x\";\nexport const y = x;\n",
            ),
        ],
    )?;
    write(&dir, "src/t.ts", "export const t = 2;\n")?;
    let output = cruise(
        &dir,
        &["-A", "HEAD", "--affected-depth", "2", "-T", "json", "src"],
    )?;
    let result: Value = serde_json::from_slice(&output.stdout)?;
    let expected = ["src/a.ts", "src/t.ts", "src/x.ts", "src/y.ts"];
    assert_eq!(sorted(sources(&result)), expected);
    assert_eq!(result["summary"]["affected"]["closure"], json!(expected));
    assert!(violations(&result).contains(&triple("no-y-to-x", "src/y.ts", "src/x.ts")));
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

/// Review finding 2: a rename leaves the old name's importers unresolved, and the saved graph
/// names them.
#[test]
fn a_native_rename_reaches_the_old_names_importers() -> Result {
    let dir = committed(
        "rename",
        &[
            ("src/a.ts", "export const a = 1;\n"),
            (
                "src/b.ts",
                "import { a } from \"./a\";\nexport const b = a;\n",
            ),
        ],
    )?;
    let config = native_rules(
        "rename",
        "      - { name: no-unresolvable, comment: t, severity: error, from: {}, to: { couldNotResolve: true } }\n",
    )?;
    let base = ["--config", config.as_str(), "--no-liveness"];
    let saved = cruise(
        &dir,
        &[
            &base[..],
            &["-T", "json", "-f", ".graph/cruise.json", "src"],
        ]
        .concat(),
    )?;
    assert_eq!(saved.status.code(), Some(0));
    git(&dir, &["mv", "src/a.ts", "src/c.ts"])?;
    let full = cruise(&dir, &[&base[..], &["-T", "err", "src"]].concat())?;
    assert_eq!(full.status.code(), Some(1), "the full cruise finds it");
    let affected = cruise(
        &dir,
        &[&base[..], &["-A", "HEAD", "-T", "err", "src"]].concat(),
    )?;
    assert_eq!(
        affected.status.code(),
        Some(1),
        "and so does the affected one: {}",
        String::from_utf8_lossy(&affected.stdout)
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

/// Review finding 3: dropping a module's last importer makes it an orphan and unreachable, which
/// the affected run reports because the saved graph names the dropped edge's target.
#[test]
fn a_native_run_reports_what_a_dropped_import_leaves_behind() -> Result {
    let dir = committed(
        "dropped",
        &[
            (
                "src/entry.ts",
                "import { a } from \"./a\";\nexport const e = a;\n",
            ),
            (
                "src/a.ts",
                "import { m } from \"./m\";\nexport const a = m;\n",
            ),
            ("src/m.ts", "export const m = 1;\n"),
        ],
    )?;
    let config = native_rules(
        "dropped",
        "      - { name: no-orphans, comment: t, severity: error, from: { orphan: true }, to: {} }\n      - { name: m-reached, comment: t, severity: error, from: { path: '^src/entry' }, to: { path: '^src/m', reachable: false } }\n",
    )?;
    let base = ["--config", config.as_str(), "--no-liveness"];
    let saved = cruise(
        &dir,
        &[
            &base[..],
            &["-T", "json", "-f", ".graph/cruise.json", "src"],
        ]
        .concat(),
    )?;
    assert_eq!(saved.status.code(), Some(0));
    write(&dir, "src/a.ts", "export const a = 1;\n")?;
    let output = cruise(
        &dir,
        &[&base[..], &["-A", "HEAD", "-T", "json", "src"]].concat(),
    )?;
    let result: Value = serde_json::from_slice(&output.stdout)?;
    let found = violations(&result);
    assert!(
        found.contains(&triple("no-orphans", "src/m.ts", "src/m.ts")),
        "{found:?}"
    );
    assert!(
        found
            .iter()
            .any(|(rule, from, _)| rule == "m-reached" && from == "src/m.ts"),
        "{found:?}"
    );
    assert!(
        result["summary"]["affected"]["closure"]
            .as_array()
            .is_some_and(|c| c.contains(&json!("src/m.ts")))
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

/// Review finding 5: a non-ASCII path, a `diff.relative` setting and a directory named as the
/// revision.
#[test]
fn git_settings_and_pathspecs_do_not_change_what_counts() -> Result {
    let dir = committed(
        "git-paths",
        &[
            (".dependency-cruiser.json", "{}\n"),
            ("src/\u{e9}t\u{e9}.ts", "export const e = 1;\n"),
            ("src/a.ts", "export const a = 1;\n"),
        ],
    )?;
    write(&dir, "src/\u{e9}t\u{e9}.ts", "export const e = 2;\n")?;
    write(&dir, "src/a.ts", "export const a = 2;\n")?;
    let output = cruise(&dir, &["-A", "HEAD", "-T", "json", "src"])?;
    let result: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(
        sorted(sources(&result)),
        ["src/a.ts", "src/\u{e9}t\u{e9}.ts"],
        "core.quotePath would have quoted the accented name"
    );

    git(&dir, &["config", "diff.relative", "true"])?;
    let sub = isolated(BIN, &dir.join("src"))
        .args([
            "cruise",
            "--no-progress",
            "--config",
            "../.dependency-cruiser.json",
            "-A",
            "HEAD",
            "-T",
            "json",
            ".",
        ])
        .output()?;
    let sub: Value = serde_json::from_slice(&sub.stdout)?;
    assert_eq!(
        sorted(sources(&sub)),
        ["a.ts", "\u{e9}t\u{e9}.ts"],
        "diff.relative does not empty the report"
    );

    let pathspec = cruise(&dir, &["-A", "src", "src"])?;
    assert_eq!(pathspec.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&pathspec.stderr).contains("revision 'src' unknown"),
        "{}",
        String::from_utf8_lossy(&pathspec.stderr)
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}
