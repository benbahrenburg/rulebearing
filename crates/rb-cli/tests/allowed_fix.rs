//! The `fix` of an `allowed` list through every reporter that prints a `fix`: a violation of
//! `allowed` (`not-in-allowed`, which names the list, not an entry) carries the text of the entry
//! that applies to its module, and `err-long`, `agent`, `sarif`, `junit`, `trx`,
//! `github-annotations` and `baseline` print it.
//!
//! - Plan: [plan 0004, Step 10](../../../docs/plans/pending/0004-wave-4-reach.md#23-steps-for-sub-wave-4c-fix---plan-and-the-eval-harness-funded-only-by-the--12-rule)
//!   (the eval found `agent` printing `fix: null` for the `not-in-allowed` case)
//! - Decisions: [ADR-0077](../../../docs/adr/0077-fix-plan-candidates-are-verified-and-the-eval-harness-scores-fix-text.md),
//!   [ADR-0021](../../../docs/adr/0021-agent-surface-cli-first.md)
//! - Requirements: [FR-REACH-03](../../../docs/prd.md#fr-reach-03), [FR-OUT-02](../../../docs/prd.md#fr-out-02)
//! - Reference: [reporters.md § `allowed` and `fix`](../../../docs/reporters.md#allowed-and-fix)
//!
//! `fixtures/allowed-fix/expected/<type>.txt` is each report byte for byte; regenerate them
//! deliberately with `RB_UPDATE_SNAPSHOTS=1 cargo test -p rb-cli --test allowed_fix`.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

type Result<T = ()> = std::result::Result<T, Box<dyn Error>>;

const BIN: &str = env!("CARGO_BIN_EXE_rulebearing");

/// The fix texts, each from one entry.
const OUTSIDE: &str = "Outside src/feature and src/lib any import is allowed.";
const FEATURE: &str =
    "Modules in src/feature import only each other. Have the caller pass in what the module needs.";
const LIBRARY: &str = "src/lib imports only src/lib. Move the code the library needs into it.";

const CONFIG: &str = r#"allowed:
  - from: { pathNot: "^src/(feature|lib)/" }
    to: {}
    fix: "Outside src/feature and src/lib any import is allowed."
  - from: { path: "^src/feature/" }
    to: { path: "^src/feature/" }
    comment: "A feature imports only itself. adr:0042"
    fix: "Modules in src/feature import only each other. Have the caller pass in what the module needs."
  - from: { path: "^src/lib/" }
    to: { path: "^src/lib/" }
    fix: "src/lib imports only src/lib. Move the code the library needs into it."
allowedSeverity: error
"#;

/// The reporters that print a `fix`.
const PRINTS_FIX: &[&str] = &[
    "err-long",
    "agent",
    "sarif",
    "junit",
    "trx",
    "github-annotations",
    "baseline",
];

fn expected(output_type: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/allowed-fix/expected")
        .join(format!("{output_type}.txt"))
}

/// Two violations of `allowed`: `src/feature/a.ts` imports `src/lib/util.ts`, and
/// `src/lib/util.ts` imports `src/feature/c.ts`; `src/main.ts`, outside both, imports freely.
fn tree(name: &str) -> Result<PathBuf> {
    let dir =
        std::env::temp_dir().join(format!("rb-cli-allowed-fix-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src/feature"))?;
    std::fs::create_dir_all(dir.join("src/lib"))?;
    std::fs::write(
        dir.join("src/feature/a.ts"),
        "import { c } from \"./c\";\nimport { u } from \"../lib/util\";\nexport const a = c + u;\n",
    )?;
    std::fs::write(dir.join("src/feature/c.ts"), "export const c = 1;\n")?;
    std::fs::write(
        dir.join("src/lib/util.ts"),
        "import { c } from \"../feature/c\";\nexport const u = c;\n",
    )?;
    std::fs::write(
        dir.join("src/main.ts"),
        "import { a } from \"./feature/a\";\nconsole.log(a);\n",
    )?;
    std::fs::write(dir.join("rulebearing.yaml"), CONFIG)?;
    Ok(rb_model::without_verbatim(&dir.canonicalize()?))
}

fn run(dir: &Path, args: &[&str]) -> Result<Output> {
    Ok(Command::new(BIN)
        .args(args)
        .current_dir(dir)
        .env("SOURCE_DATE_EPOCH", "1790000000")
        .output()?)
}

#[test]
fn each_reporter_that_prints_a_fix_prints_the_entry_that_applies() -> Result {
    let dir = tree("each")?;
    for output_type in PRINTS_FIX {
        let output = run(
            &dir,
            &[
                "cruise",
                "--config",
                "rulebearing.yaml",
                "--no-cache",
                "-T",
                output_type,
                "src",
            ],
        )?;
        let text = String::from_utf8_lossy(&output.stdout).into_owned();
        // A gating reporter exits with the two errors; the others exit 0.
        let gates = rb_report::gates(output_type);
        assert_eq!(
            output.status.code(),
            Some(if gates { 2 } else { 0 }),
            "{output_type}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        // `err-long` wraps the text, so each is found by its first words.
        for words in [&FEATURE[..30], &LIBRARY[..28]] {
            assert!(text.contains(words), "{output_type}: {words}: {text}");
        }
        // No finding carries the text of an entry that does not select its module. `sarif`'s
        // rule descriptor stands for the whole list and carries every entry's text.
        let outside = text.matches(&OUTSIDE[..30]).count();
        let whole_list = format!("{OUTSIDE} {FEATURE} {LIBRARY}");
        if *output_type == "sarif" {
            assert!(text.contains(&whole_list), "{text}");
            assert_eq!(outside, text.matches(&whole_list).count(), "{text}");
        } else {
            assert_eq!(outside, 0, "{output_type}: {text}");
        }
        assert!(
            !text.contains(&dir.to_string_lossy().into_owned()),
            "{output_type} names the machine's folder"
        );
        let path = expected(output_type);
        if std::env::var_os("RB_UPDATE_SNAPSHOTS").is_some() {
            std::fs::create_dir_all(path.parent().unwrap_or(&dir))?;
            std::fs::write(&path, &text)?;
        }
        assert_eq!(
            text,
            std::fs::read_to_string(&path)?,
            "{} changed; if that was intended, regenerate with RB_UPDATE_SNAPSHOTS=1",
            path.display()
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn the_agent_report_gives_not_in_allowed_its_fix_and_each_finding_its_own() -> Result {
    let dir = tree("agent")?;
    let output = run(
        &dir,
        &[
            "cruise",
            "--config",
            "rulebearing.yaml",
            "--no-cache",
            "-T",
            "agent",
            "src",
        ],
    )?;
    let report: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let rule = &report["rules"][0];
    assert_eq!(rule["name"], "not-in-allowed");
    assert_eq!(rule["count"], 2);
    let fix = rule["fix"].as_str().unwrap_or_default();
    assert!(fix == FEATURE || fix == LIBRARY, "{rule}");
    let own: Vec<&str> = rule["violations"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|v| v["fix"].as_str().unwrap_or(fix))
        .collect();
    assert_eq!(own.len(), 2);
    assert!(own.contains(&FEATURE) && own.contains(&LIBRARY), "{rule}");
    assert!(!own.contains(&OUTSIDE), "{rule}");
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}
