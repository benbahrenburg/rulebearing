//! `cruise` and `fmt` end to end over a small TypeScript tree: the reporters, the flags that
//! shape a report, progress, `--output-to`, a dependency-cruiser result through `fmt`, and the
//! gating exit codes.
//!
//! - Plan: [Wave 1, Step 13](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-13-rb-cli-cruise-fmt-exit-codes-flags-1d)
//! - Contract: [ADR-0008](../../../docs/adr/0008-exit-code-contract.md)
//! - Requirements: [FR-CORE-06](../../../docs/prd.md#fr-core-06), [FR-CLI-08](../../../docs/prd.md#fr-cli-08)

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_rulebearing");

const CONFIG: &str = r#"{
  "forbidden": [
    { "name": "domain-not-to-web", "severity": "error", "comment": "The domain stays apart. adr:0010",
      "from": { "path": "^src/domain/" }, "to": { "path": "^src/web/" } },
    { "name": "no-circular", "severity": "warn", "from": {}, "to": { "circular": true } }
  ],
  "options": { "tsPreCompilationDeps": true }
}
"#;

fn tree(name: &str) -> Result<PathBuf, Box<dyn Error>> {
    let dir = std::env::temp_dir().join(format!("rb-cli-cruise-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let files = [
        (
            "src/domain/model.ts",
            "import { w } from \"../web/view\";\nexport const d = w;\n",
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
            "import { d } from \"./domain/model\";\nconsole.log(d);\n",
        ),
        (".dependency-cruiser.json", CONFIG),
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
        .env("SOURCE_DATE_EPOCH", "1790000000")
        .output()?)
}

fn text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn every_wave_1_reporter_reports_and_the_gating_ones_exit_with_the_count()
-> Result<(), Box<dyn Error>> {
    let dir = tree("reporters")?;
    for output_type in [
        "err",
        "err-long",
        "teamcity",
        "azure-devops",
        "github-annotations",
        "agent",
        "null",
    ] {
        let out = run(&dir, &["cruise", "-T", output_type, "src"])?;
        assert_eq!(out.status.code(), Some(1), "{output_type}: one error");
        let again = run(&dir, &["cruise", "-T", output_type, "src"])?;
        assert_eq!(out.stdout, again.stdout, "{output_type}: two runs differ");
    }
    let err_long = text(&run(&dir, &["cruise", "-T", "err-long", "src"])?);
    assert!(err_long.contains("The domain stays apart."), "{err_long}");
    let annotations = text(&run(&dir, &["cruise", "-T", "github-annotations", "src"])?);
    assert!(annotations.starts_with("::"), "{annotations}");
    for output_type in ["json", "text", "csv"] {
        let out = run(&dir, &["cruise", "-T", output_type, "src"])?;
        assert!(!out.stdout.is_empty(), "{output_type}");
    }
    let unknown = run(&dir, &["cruise", "-T", "pdf", "src"])?;
    assert_eq!(unknown.status.code(), Some(3));
    let later = run(&dir, &["cruise", "-T", "dot", "src"])?;
    assert_ne!(later.status.code(), Some(0), "a wave 2 reporter is refused");
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn flags_shape_the_run() -> Result<(), Box<dyn Error>> {
    let dir = tree("flags")?;
    let json = |args: &[&str]| -> Result<Value, Box<dyn Error>> {
        let all = [&["cruise", "-T", "json"][..], args].concat();
        Ok(serde_json::from_slice(&run(&dir, &all)?.stdout)?)
    };
    let sources = |value: &Value| -> Vec<String> {
        value["modules"]
            .as_array()
            .map(|m| {
                m.iter()
                    .filter_map(|m| m["source"].as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    };
    let all = json(&["src"])?;
    assert_eq!(sources(&all).len(), 4);
    assert!(all["summary"]["violations"][0]["id"].is_string());
    let strict = json(&["--strict-schema", "src"])?;
    assert!(strict["summary"]["violations"][0].get("id").is_none());
    assert!(
        !sources(&json(&["-x", "^src/web/helper", "src"])?)
            .contains(&"src/web/helper.ts".to_owned())
    );
    assert!(
        !sources(&json(&["-I", "^src/(domain|web)", "src"])?).contains(&"src/main.ts".to_owned())
    );
    let focused = sources(&json(&["-F", "^src/main", "src"])?);
    assert!(
        focused.contains(&"src/main.ts".to_owned())
            && focused.contains(&"src/domain/model.ts".to_owned())
    );
    let metrics = json(&["-m", "src"])?;
    assert!(metrics["folders"].is_array());
    let shallow = sources(&json(&["--max-depth", "1", "src/main.ts"])?);
    assert_eq!(shallow, ["src/domain/model.ts", "src/main.ts"]);

    let progress = run(
        &dir,
        &[
            "cruise",
            "-T",
            "err",
            "--progress",
            "performance-log",
            "src",
        ],
    )?;
    assert!(String::from_utf8_lossy(&progress.stderr).contains("extract"));
    let ndjson = run(
        &dir,
        &["cruise", "-T", "err", "--progress", "ndjson", "src"],
    )?;
    assert!(String::from_utf8_lossy(&ndjson.stderr).contains('{'));
    let info = run(&dir, &["cruise", "--info"])?;
    assert_eq!(info.status.code(), Some(0));

    let written = run(
        &dir,
        &["cruise", "-T", "json", "-f", "out/result.json", "src"],
    )?;
    assert!(written.stdout.is_empty());
    assert!(dir.join("out/result.json").is_file());
    let no_config = run(&dir, &["cruise", "--no-config", "-T", "err", "src"])?;
    assert_eq!(no_config.status.code(), Some(0), "no rules, no violations");
    let bad = run(&dir, &["cruise", "--module-systems", "nope", "src"])?;
    assert_eq!(bad.status.code(), Some(3));
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn fmt_rereports_a_saved_result() -> Result<(), Box<dyn Error>> {
    let dir = tree("fmt")?;
    run(&dir, &["cruise", "-T", "json", "-f", "result.json", "src"])?;
    let quiet = run(&dir, &["fmt", "result.json"])?;
    assert_eq!(
        quiet.status.code(),
        Some(0),
        "without --exit-code, fmt exits 0"
    );
    assert!(text(&quiet).contains("domain-not-to-web"));
    let gate = run(&dir, &["fmt", "-e", "-T", "err", "result.json"])?;
    assert_eq!(gate.status.code(), Some(1));
    let excluded = run(
        &dir,
        &["fmt", "-e", "-T", "err", "-x", "^src/domain", "result.json"],
    )?;
    assert_eq!(
        excluded.status.code(),
        Some(0),
        "the excluded violation is re-summarised away"
    );
    let collapsed = run(&dir, &["fmt", "-T", "json", "-S", "2", "result.json"])?;
    let value: Value = serde_json::from_slice(&collapsed.stdout)?;
    assert!(value["modules"].as_array().is_some_and(|m| m.len() <= 3));
    let from_upstream = run(
        &dir,
        &[
            "fmt",
            "--from",
            "dependency-cruiser",
            "-T",
            "text",
            "result.json",
        ],
    )?;
    assert!(text(&from_upstream).contains(" → "));
    let wrong_tool = run(&dir, &["fmt", "--from", "madge", "result.json"])?;
    assert_eq!(wrong_tool.status.code(), Some(3));
    let missing = run(&dir, &["fmt", "nowhere.json"])?;
    assert_eq!(missing.status.code(), Some(2));
    std::fs::write(dir.join("broken.json"), "{ not json")?;
    assert_eq!(run(&dir, &["fmt", "broken.json"])?.status.code(), Some(2));
    let piped = Command::new(BIN)
        .args(["fmt", "-T", "err", "-"])
        .current_dir(&dir)
        .stdin(std::fs::File::open(dir.join("result.json"))?)
        .output()?;
    assert!(text(&piped).contains("domain-not-to-web"));
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}
