//! `baseline` end to end: the three modes, `expires`, `owner` and `reason`, `--ignore-known` and
//! `--no-ignore-known` on `cruise` and `fmt`, and element rules under a baseline.
//!
//! - Plan: [Wave 2, Step 10](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#210-step-10-reporters-and-baseline-semantics-2e)
//! - Decisions: [ADR-0015](../../../docs/adr/0015-stable-violation-id.md),
//!   [ADR-0008](../../../docs/adr/0008-exit-code-contract.md),
//!   [ADR-0031](../../../docs/adr/0031-a-saved-result-carries-what-the-exit-code-counts.md)
//! - Requirement: [FR-RULE-09](../../../docs/prd.md#fr-rule-09)

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{Value, json};

const BIN: &str = env!("CARGO_BIN_EXE_rulebearing");
const FILE: &str = ".dependency-cruiser-known-violations.json";

const CONFIG: &str = r#"{
  "forbidden": [
    { "name": "domain-not-to-web", "severity": "error", "from": { "path": "^src/domain/" }, "to": { "path": "^src/web/" } },
    { "name": "no-circular", "severity": "warn", "from": {}, "to": { "circular": true } }
  ],
  "options": { "tsPreCompilationDeps": true }
}
"#;

fn tree(name: &str, config: &str) -> Result<PathBuf, Box<dyn Error>> {
    let dir = std::env::temp_dir().join(format!("rb-cli-baseline-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let files = [
        (
            "src/domain/model.ts",
            "import { w } from \"../web/view\";\nexport const d = w;\n",
        ),
        (
            "src/domain/other.ts",
            "import { w } from \"../web/view\";\nexport const o = w;\n",
        ),
        (
            "src/web/view.ts",
            "import { h } from \"./helper\";\nexport const w = h;\n",
        ),
        (
            "src/web/helper.ts",
            "import { w } from \"./view\";\nexport const h = typeof w;\n",
        ),
        (
            "src/main.ts",
            "import { d } from \"./domain/model\";\nimport { o } from \"./domain/other\";\nconsole.log(d, o);\n",
        ),
        (".dependency-cruiser.json", config),
    ];
    for (file, text) in files {
        let path = dir.join(file);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, text)?;
    }
    Ok(dir)
}

fn run(dir: &Path, args: &[&str]) -> Result<Output, Box<dyn Error>> {
    Ok(Command::new(BIN)
        .args(args)
        .current_dir(dir)
        // 2026-09-21
        .env("SOURCE_DATE_EPOCH", "1790000000")
        .output()?)
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn read_json(path: &Path) -> Result<Value, Box<dyn Error>> {
    Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
}

fn entries(value: &Value) -> Vec<Value> {
    value.as_array().cloned().unwrap_or_default()
}

fn rules_of(value: &Value) -> Vec<String> {
    entries(value)
        .iter()
        .map(|e| {
            format!(
                "{} {}",
                e["rule"]["name"].as_str().unwrap_or("?"),
                e["from"].as_str().unwrap_or("?")
            )
        })
        .collect()
}

#[test]
fn full_writes_every_violation_keyed_by_id_and_keeps_the_lifecycle_of_each_entry()
-> Result<(), Box<dyn Error>> {
    let dir = tree("full", CONFIG)?;
    let out = run(&dir, &["baseline", "src"])?;
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(out.stdout.is_empty(), "the file is written, not printed");
    let first = std::fs::read_to_string(dir.join(FILE))?;
    let written: Value = serde_json::from_str(&first)?;
    assert_eq!(
        rules_of(&written),
        [
            "domain-not-to-web src/domain/model.ts",
            "domain-not-to-web src/domain/other.ts",
            "no-circular src/web/helper.ts"
        ]
    );
    assert!(entries(&written).iter().all(|e| {
        e["id"].as_str().is_some_and(|id| id.starts_with("RB-")) && e.get("owner").is_none()
    }));
    // Deterministic: a second run writes the same bytes.
    run(&dir, &["baseline", "src"])?;
    assert_eq!(std::fs::read_to_string(dir.join(FILE))?, first);
    // `-f -` prints the same entries.
    let printed = run(&dir, &["baseline", "-f", "-", "src"])?;
    assert_eq!(String::from_utf8_lossy(&printed.stdout), first);

    // The flags fill each entry that lacks them; an entry already there keeps its own.
    let mut edited = entries(&written);
    edited[0]["owner"] = json!("@domain-team");
    edited[0]["reason"] = json!("the port lands in plan:todo-port");
    std::fs::write(dir.join(FILE), serde_json::to_string_pretty(&edited)?)?;
    let out = run(
        &dir,
        &[
            "baseline",
            "--expires",
            "2026-12-31",
            "--owner",
            "@me",
            "--reason",
            "adopted",
            "src",
        ],
    )?;
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let again = read_json(&dir.join(FILE))?;
    assert_eq!(again[0]["owner"], "@domain-team");
    assert_eq!(again[0]["reason"], "the port lands in plan:todo-port");
    assert_eq!(again[0]["expires"], "2026-12-31");
    assert_eq!(again[1]["owner"], "@me");
    assert_eq!(again[1]["reason"], "adopted");
    assert_eq!(again[0]["id"], written[0]["id"], "the id survives");

    let bad = run(&dir, &["baseline", "--expires", "next week", "src"])?;
    assert_eq!(bad.status.code(), Some(3));
    assert!(stderr(&bad).contains("YYYY-MM-DD"), "{}", stderr(&bad));
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn ignore_known_softens_the_baseline_and_the_last_flag_wins() -> Result<(), Box<dyn Error>> {
    let dir = tree("ignore", CONFIG)?;
    assert_eq!(
        run(&dir, &["cruise", "-T", "err", "src"])?.status.code(),
        Some(2)
    );
    run(&dir, &["baseline", "src"])?;
    let gated = run(&dir, &["cruise", "src", "-T", "err", "--ignore-known"])?;
    assert_eq!(gated.status.code(), Some(0), "{}", stderr(&gated));
    let result: Value = serde_json::from_slice(
        &run(
            &dir,
            &["cruise", "-T", "json", "--ignore-known", FILE, "src"],
        )?
        .stdout,
    )?;
    assert_eq!(result["summary"]["error"], 0);
    assert_eq!(result["summary"]["warn"], 0);
    assert_eq!(result["summary"]["ignore"], 3);
    assert_eq!(
        result["summary"]["optionsUsed"]["knownViolations"]
            .as_array()
            .map(Vec::len),
        Some(3)
    );
    let last = run(
        &dir,
        &[
            "cruise",
            "-T",
            "err",
            "--ignore-known",
            "--no-ignore-known",
            "src",
        ],
    )?;
    assert_eq!(last.status.code(), Some(2), "--no-ignore-known came last");
    let first = run(
        &dir,
        &[
            "cruise",
            "src",
            "-T",
            "err",
            "--no-ignore-known",
            "--ignore-known",
        ],
    )?;
    assert_eq!(first.status.code(), Some(0), "--ignore-known came last");
    let missing = run(
        &dir,
        &["cruise", "-T", "err", "--ignore-known", "nope.json", "src"],
    )?;
    assert_eq!(missing.status.code(), Some(3));
    assert!(
        stderr(&missing).contains("nope.json"),
        "{}",
        stderr(&missing)
    );

    // `--no-ignore-known` also sets aside the configuration's own entries.
    let inline = CONFIG.replace(
        "\"options\": {",
        &format!(
            "\"options\": {{ \"knownViolations\": {},",
            std::fs::read_to_string(dir.join(FILE))?
        ),
    );
    std::fs::write(dir.join(".dependency-cruiser.json"), inline)?;
    assert_eq!(
        run(&dir, &["cruise", "-T", "err", "src"])?.status.code(),
        Some(0)
    );
    assert_eq!(
        run(&dir, &["cruise", "-T", "err", "--no-ignore-known", "src"])?
            .status
            .code(),
        Some(2)
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn an_expired_entry_is_not_honoured_and_the_run_says_so() -> Result<(), Box<dyn Error>> {
    let dir = tree("expires", CONFIG)?;
    run(&dir, &["baseline", "--expires", "2026-09-20", "src"])?;
    let out = run(&dir, &["cruise", "src", "-T", "err", "--ignore-known"])?;
    // Two errors, no longer softened, plus two expired entries that are errors themselves; the
    // expired warn entry counts, the warn finding does not.
    assert_eq!(out.status.code(), Some(5), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("knownViolation `RB-")
            && stderr(&out).contains("expired on 2026-09-20"),
        "{}",
        stderr(&out)
    );
    let saved = dir.join("saved.json");
    run(
        &dir,
        &[
            "cruise",
            "src",
            "-T",
            "json",
            "-f",
            "saved.json",
            "--ignore-known",
        ],
    )?;
    let result = read_json(&saved)?;
    assert_eq!(
        result["summary"]["expired"].as_array().map(Vec::len),
        Some(3)
    );
    assert_eq!(result["summary"]["error"], 2);
    // An entry expiring today still applies today.
    run(
        &dir,
        &[
            "baseline",
            "--expires",
            "2026-09-21",
            "-f",
            "today.json",
            "src",
        ],
    )?;
    let today = run(
        &dir,
        &["cruise", "-T", "err", "--ignore-known", "today.json", "src"],
    )?;
    assert_eq!(today.status.code(), Some(0), "{}", stderr(&today));
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn shrink_only_removes_what_no_longer_occurs_and_fails_naming_it() -> Result<(), Box<dyn Error>> {
    let dir = tree("shrink", CONFIG)?;
    run(&dir, &["baseline", "src"])?;
    let before = read_json(&dir.join(FILE))?;
    let fixed = entries(&before)
        .into_iter()
        .find(|e| e["from"] == "src/domain/other.ts")
        .unwrap_or(Value::Null);
    let untouched = run(&dir, &["baseline", "--baseline-mode", "shrink-only", "src"])?;
    assert_eq!(untouched.status.code(), Some(0), "{}", stderr(&untouched));
    assert_eq!(read_json(&dir.join(FILE))?, before, "nothing to remove");

    // Resolve one finding and add a new one.
    std::fs::write(dir.join("src/domain/other.ts"), "export const o = 1;\n")?;
    std::fs::write(
        dir.join("src/domain/new.ts"),
        "import { h } from \"../web/helper\";\nexport const n = h;\n",
    )?;
    let out = run(&dir, &["baseline", "--baseline-mode", "shrink-only", "src"])?;
    assert_eq!(out.status.code(), Some(1), "one entry no longer occurs");
    let text = stderr(&out);
    assert!(
        text.contains(&format!(
            "error: known violation {} (rule `domain-not-to-web`: src/domain/other.ts -> src/web/view.ts) no longer occurs",
            fixed["id"].as_str().unwrap_or("?")
        )),
        "{text}"
    );
    assert!(
        text.contains("warning: 1 finding(s) are not in the baseline"),
        "{text}"
    );
    let after = read_json(&dir.join(FILE))?;
    assert_eq!(
        rules_of(&after),
        [
            "domain-not-to-web src/domain/model.ts",
            "no-circular src/web/helper.ts"
        ],
        "the stale entry is gone and the new finding is not added"
    );
    let second = run(&dir, &["baseline", "--baseline-mode", "shrink-only", "src"])?;
    assert_eq!(second.status.code(), Some(0), "{}", stderr(&second));

    // Nothing to shrink is a usage error.
    let empty = tree("shrink-none", CONFIG)?;
    let none = run(
        &empty,
        &["baseline", "--baseline-mode", "shrink-only", "src"],
    )?;
    assert_eq!(none.status.code(), Some(3));
    assert!(
        stderr(&none).contains("--baseline-mode full"),
        "{}",
        stderr(&none)
    );

    // Without a file, the configuration's entries are checked and named, not rewritten.
    let inline = CONFIG.replace(
        "\"options\": {",
        &format!(
            "\"options\": {{ \"knownViolations\": {},",
            serde_json::to_string(&before)?
        ),
    );
    std::fs::write(empty.join(".dependency-cruiser.json"), inline)?;
    std::fs::write(empty.join("src/domain/other.ts"), "export const o = 1;\n")?;
    let configured = run(
        &empty,
        &["baseline", "--baseline-mode", "shrink-only", "src"],
    )?;
    assert_eq!(configured.status.code(), Some(1));
    assert!(
        stderr(&configured).contains("remove it from options.knownViolations in"),
        "{}",
        stderr(&configured)
    );
    assert!(!empty.join(FILE).exists());
    let flags = run(
        &empty,
        &[
            "baseline",
            "--baseline-mode",
            "shrink-only",
            "--owner",
            "@me",
            "src",
        ],
    )?;
    assert_eq!(flags.status.code(), Some(3));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&empty);
    Ok(())
}

#[test]
fn format_rewrites_the_file_in_canonical_order() -> Result<(), Box<dyn Error>> {
    let dir = tree("format", CONFIG)?;
    let missing = run(&dir, &["baseline", "--baseline-mode", "format"])?;
    assert_eq!(missing.status.code(), Some(3));
    run(&dir, &["baseline", "src"])?;
    let mut scrambled = entries(&read_json(&dir.join(FILE))?);
    scrambled.reverse();
    std::fs::write(dir.join(FILE), serde_json::to_string(&scrambled)?)?;
    let out = run(
        &dir,
        &["baseline", "--baseline-mode", "format", "--owner", "@me"],
    )?;
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let formatted = std::fs::read_to_string(dir.join(FILE))?;
    let value: Value = serde_json::from_str(&formatted)?;
    assert_eq!(
        rules_of(&value),
        [
            "domain-not-to-web src/domain/model.ts",
            "domain-not-to-web src/domain/other.ts",
            "no-circular src/web/helper.ts"
        ]
    );
    assert!(entries(&value).iter().all(|e| e["owner"] == "@me"));
    assert!(formatted.ends_with("]\n") && formatted.contains("\n  {"));
    run(&dir, &["baseline", "--baseline-mode", "format"])?;
    assert_eq!(
        std::fs::read_to_string(dir.join(FILE))?,
        formatted,
        "idempotent"
    );
    let stdout = run(&dir, &["baseline", "--baseline-mode", "format", "-f", "-"])?;
    assert_eq!(stdout.status.code(), Some(3));
    std::fs::write(dir.join(FILE), "{}")?;
    let object = run(&dir, &["baseline", "--baseline-mode", "format"])?;
    assert_eq!(object.status.code(), Some(3));
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn fmt_takes_a_baseline_on_and_off_a_saved_result() -> Result<(), Box<dyn Error>> {
    let dir = tree("fmt", CONFIG)?;
    run(&dir, &["baseline", "src"])?;
    run(&dir, &["cruise", "-T", "json", "-f", "plain.json", "src"])?;
    let exit = |args: &[&str]| -> Result<Option<i32>, Box<dyn Error>> {
        Ok(run(&dir, args)?.status.code())
    };
    assert_eq!(exit(&["fmt", "-T", "err", "-e", "plain.json"])?, Some(2));
    assert_eq!(
        exit(&["fmt", "plain.json", "-T", "err", "-e", "--ignore-known"])?,
        Some(0)
    );
    run(
        &dir,
        &[
            "cruise",
            "src",
            "-T",
            "json",
            "-f",
            "known.json",
            "--ignore-known",
        ],
    )?;
    assert_eq!(exit(&["fmt", "-T", "err", "-e", "known.json"])?, Some(0));
    assert_eq!(
        exit(&["fmt", "-T", "err", "-e", "--no-ignore-known", "known.json"])?,
        Some(2),
        "the softened errors are back at their rule's severity"
    );
    // Taken off, the baseline leaves the result `fmt` gives for the run without one.
    let summary = |args: &[&str]| -> Result<Value, Box<dyn Error>> {
        let value: Value = serde_json::from_slice(&run(&dir, args)?.stdout)?;
        let s = &value["summary"];
        Ok(json!([s["error"], s["warn"], s["ignore"], s["violations"]]))
    };
    assert_eq!(
        summary(&["fmt", "-T", "json", "--no-ignore-known", "known.json"])?,
        summary(&["fmt", "-T", "json", "plain.json"])?
    );
    assert_eq!(
        exit(&[
            "fmt",
            "-T",
            "err",
            "--ignore-known",
            "absent.json",
            "plain.json"
        ])?,
        Some(3)
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

const ELEMENT_CONFIG: &str = r"rules:
  elements:
    - name: classes-are-sealed
      fix: Seal the class.
      select: { kind: class }
      should: { beSealed: true }
";

fn graph(sealed_b: bool) -> Value {
    let class = |full: &str, file: &str, sealed: bool| {
        json!({ "fullName": full, "name": &full[2..], "namespace": "S", "kind": "class",
                "language": "dotnet", "file": file, "sealed": sealed })
    };
    let module = |source: &str| {
        json!({ "source": source, "dependencies": [], "dependents": [], "orphan": true, "valid": true,
                "language": "dotnet" })
    };
    json!({
        "modules": [module("src/A.cs"), module("src/B.cs")],
        "summary": { "violations": [], "error": 0, "warn": 0, "info": 0, "ignore": 0,
                     "totalCruised": 2, "totalDependenciesCruised": 0, "optionsUsed": {} },
        "code": { "types": [class("S.A", "src/A.cs", false), class("S.B", "src/B.cs", sealed_b)] }
    })
}

#[test]
fn element_violations_take_the_same_baseline() -> Result<(), Box<dyn Error>> {
    let dir = std::env::temp_dir().join(format!("rb-cli-baseline-elements-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("rulebearing.yaml"), ELEMENT_CONFIG)?;
    std::fs::write(
        dir.join("graph.json"),
        serde_json::to_string(&graph(false))?,
    )?;
    let gate = ["cruise", "-T", "err", "--graph", "graph.json"];
    assert_eq!(run(&dir, &gate)?.status.code(), Some(2));
    let out = run(&dir, &["baseline", "--graph", "graph.json"])?;
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let written = read_json(&dir.join(FILE))?;
    let objects: Vec<(&str, &str)> = written
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| Some((e["type"].as_str()?, e["to"].as_str()?)))
        .collect();
    assert_eq!(objects, [("element", "S.A"), ("element", "S.B")]);
    let known = run(&dir, &[&gate[..], &["--ignore-known"]].concat())?;
    assert_eq!(known.status.code(), Some(0), "{}", stderr(&known));

    // S.B is sealed now: its entry no longer occurs.
    std::fs::write(dir.join("graph.json"), serde_json::to_string(&graph(true))?)?;
    let shrink = run(
        &dir,
        &[
            "baseline",
            "--baseline-mode",
            "shrink-only",
            "--graph",
            "graph.json",
        ],
    )?;
    assert_eq!(shrink.status.code(), Some(1), "{}", stderr(&shrink));
    assert!(
        stderr(&shrink).contains("(rule `classes-are-sealed`: src/B.cs -> S.B) no longer occurs"),
        "{}",
        stderr(&shrink)
    );
    let left = read_json(&dir.join(FILE))?;
    assert_eq!(left.as_array().map(Vec::len), Some(1));
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}
