//! `cruise --cache`: the entry, both strategies, compression, incremental extraction across the
//! three languages, and the promise that a cached run reports what a cold run reports.
//!
//! - Plan: [Wave 3, Steps 1 and 2](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#21-steps-for-sub-wave-3a-cache---affected-diff---exit-code-mode-strict)
//!   (hit; miss on configuration and tool version; miss on a changed input under each strategy;
//!   a corrupt manifest discarded; a compressed round trip; worktree isolation; incremental equals
//!   full), [§ 1.7](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#17-quality-attributes)
//!   (a hit byte-identical to a cold run; `summary.cache`)
//! - Coverage: [coverage § Options](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#options)
//!   (row `cache`), [coverage § Command line](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#command-line)
//!   (`--cache [folder]`, `--cache-strategy`, `--no-cache`)
//! - Decisions: [ADR-0008](../../../docs/adr/0008-exit-code-contract.md),
//!   [ADR-0004](../../../docs/adr/0004-graph-document-is-cruise-result-superset.md)
//! - Requirement: [FR-CLI-05](../../../docs/prd.md#fr-cli-05)
//!
//! Every comparison is against a cold run of the same command: the same flags and options, with
//! the cache folder moved aside so the run finds no entry. The JSON output of a cached run
//! differs from the cold run's only in `summary.cache`, the receipt of what the cache did
//! (`hit: true` against `hit: false`); with that key removed the two are byte-identical, and a
//! reporter that does not print the summary (`err` here) is byte-identical as it stands. How a run was served is read from
//! `--progress cli-feedback`, which names it on the extract stage.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

type Result<T = ()> = std::result::Result<T, Box<dyn Error>>;

const BIN: &str = env!("CARGO_BIN_EXE_rulebearing");

/// The variables git sets for a hook; each command here runs without them, so a `pre-push` hook
/// cannot point git at this repository instead of the scratch one.
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

const CONFIG: &str = "forbidden:
  - name: no-circular
    severity: warn
    comment: \"adr:0010\"
    from: {}
    to: { circular: true }
  - name: not-to-unresolvable
    severity: error
    comment: \"adr:0010\"
    from: {}
    to: { couldNotResolve: true }
";

const TREE: &[(&str, &str)] = &[
    ("rulebearing.yaml", CONFIG),
    ("package.json", "{ \"name\": \"fixture\" }\n"),
    (
        "src/a.ts",
        "import { b } from \"./b\";\nimport { c } from \"./c\";\nexport const a = b + c;\n",
    ),
    (
        "src/b.ts",
        "import { c } from \"./c\";\nexport const b = c;\n",
    ),
    (
        "src/c.ts",
        "import { d } from \"./lib/d\";\nexport class C { run() { return d; } }\nexport const c = 1;\n",
    ),
    (
        "src/lib/d.ts",
        "import type { C } from \"../c\";\nexport const d = 2;\nexport type Seen = C;\n",
    ),
    ("src/lib/e.ts", "export const e = 3;\n"),
];

/// A fresh scratch folder holding `files`, canonical.
fn tree(name: &str, files: &[(&str, &str)]) -> Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("rb-cli-cache-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    for (file, text) in files {
        write(&dir, file, text)?;
    }
    Ok(dir.canonicalize()?)
}

fn write(dir: &Path, file: &str, text: &str) -> Result {
    let path = dir.join(file);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, text)?;
    Ok(())
}

/// Copies a fixture folder, dot-folders included.
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

fn isolated(program: &str, dir: &Path) -> Command {
    let mut command = Command::new(program);
    command
        .current_dir(dir)
        .env("SOURCE_DATE_EPOCH", "1790000000");
    for name in GIT_LOCAL_ENV {
        command.env_remove(name);
    }
    command
}

fn git(dir: &Path, args: &[&str]) -> Result<String> {
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
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// `rulebearing cruise <roots> -T <type> --no-liveness --progress cli-feedback <extra>`; the
/// extra flags come last, so a bare `--cache` never takes a root as its folder.
fn cruise(dir: &Path, roots: &[&str], output_type: &str, extra: &[&str]) -> Result<Output> {
    Ok(isolated(BIN, dir)
        .arg("cruise")
        .args(roots)
        .args([
            "-T",
            output_type,
            "--no-liveness",
            "--progress",
            "cli-feedback",
        ])
        .args(extra)
        .output()?)
}

/// How a run was served, from its extract stage: `from the cache`, `incremental: ...`,
/// `in full: ...`, or `not cached`.
fn served(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    stderr
        .lines()
        .find_map(|l| l.trim().strip_prefix("extract ("))
        .and_then(|rest| rest.rsplit_once(") ..."))
        .map_or_else(|| "not cached".to_owned(), |(inside, _)| inside.to_owned())
}

/// A JSON result as text with `summary.cache` removed, and that receipt.
fn without_receipt(output: &Output) -> Result<(String, Value)> {
    let mut value: Value = serde_json::from_slice(&output.stdout).map_err(|e| {
        format!(
            "not JSON ({e}): {}",
            String::from_utf8_lossy(&output.stderr)
        )
    })?;
    let receipt = value
        .get_mut("summary")
        .and_then(Value::as_object_mut)
        .and_then(|s| s.remove("cache"))
        .unwrap_or(Value::Null);
    Ok((
        format!("{}\n", serde_json::to_string_pretty(&value)?),
        receipt,
    ))
}

/// The same command run cold: `folder` (the cache folder, relative to `dir` or absolute) moved
/// aside so no entry is found, then put back as it was.
fn cold(
    dir: &Path,
    roots: &[&str],
    output_type: &str,
    extra: &[&str],
    folder: &str,
) -> Result<Output> {
    let folder = dir.join(folder);
    let aside = folder.with_file_name(format!(
        "{}-aside",
        folder
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    ));
    let had = folder.exists();
    if had {
        std::fs::rename(&folder, &aside)?;
    }
    let output = cruise(dir, roots, output_type, extra);
    let _ = std::fs::remove_dir_all(&folder);
    if had {
        std::fs::rename(&aside, &folder)?;
    }
    let output = output?;
    assert_eq!(served(&output), "in full: no entry", "a cold run");
    Ok(output)
}

/// Asserts `cached` reports what `cold` reports, `summary.cache` aside, and returns the receipt.
fn same_as_cold(cached: &Output, cold: &Output) -> Result<Value> {
    assert_eq!(
        cold.status.code(),
        cached.status.code(),
        "{}",
        String::from_utf8_lossy(&cached.stderr)
    );
    let (cold_text, cold_receipt) = without_receipt(cold)?;
    assert_eq!(
        cold_receipt["hit"],
        Value::Bool(false),
        "a cold run is no hit"
    );
    let (text, receipt) = without_receipt(cached)?;
    assert_eq!(text, cold_text);
    // Without the receipt removed, the texts differ only in it.
    let original = String::from_utf8_lossy(&cached.stdout).into_owned();
    let original_cold = String::from_utf8_lossy(&cold.stdout).into_owned();
    if receipt["hit"] == Value::Bool(false) {
        assert_eq!(original, original_cold);
    } else {
        assert_eq!(
            original.replace("\"hit\": true", "\"hit\": false"),
            original_cold
        );
    }
    Ok(receipt)
}

#[test]
fn a_warm_run_is_the_cold_run_served_from_the_cache() -> Result {
    let dir = tree("hit", TREE)?;
    let plain = cruise(&dir, &["src"], "json", &[])?;
    assert_eq!(served(&plain), "not cached");
    let first = cruise(&dir, &["src"], "json", &["--cache"])?;
    assert_eq!(served(&first), "in full: no entry");
    assert_eq!(
        without_receipt(&first)?.1,
        serde_json::json!({ "hit": false, "strategy": "metadata" })
    );
    let second = cruise(&dir, &["src"], "json", &["--cache"])?;
    assert_eq!(served(&second), "from the cache");
    let receipt = same_as_cold(&second, &first)?;
    assert_eq!(
        receipt,
        serde_json::json!({ "hit": true, "strategy": "metadata" })
    );
    // Against a run without the cache, only `optionsUsed.cache` and the receipt differ.
    let mut with: Value = serde_json::from_slice(&second.stdout)?;
    let mut without: Value = serde_json::from_slice(&plain.stdout)?;
    for value in [&mut with, &mut without] {
        if let Some(summary) = value.get_mut("summary").and_then(Value::as_object_mut) {
            summary.remove("cache");
            if let Some(used) = summary
                .get_mut("optionsUsed")
                .and_then(Value::as_object_mut)
            {
                used.remove("cache");
            }
        }
    }
    assert_eq!(with, without);
    // A reporter that does not print the summary is byte-identical as it stands.
    let cold_err = cold(&dir, &["src"], "err", &["--cache"], ".graph/cache")?;
    let warm_err = cruise(&dir, &["src"], "err", &["--cache"])?;
    assert_eq!(served(&warm_err), "from the cache");
    assert_eq!(warm_err.stdout, cold_err.stdout);
    assert_eq!(warm_err.status.code(), cold_err.status.code());
    // Two cached runs agree byte for byte, receipt included.
    let third = cruise(&dir, &["src"], "json", &["--cache"])?;
    assert_eq!(third.stdout, second.stdout);
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn the_manifest_carries_the_frozen_fields() -> Result {
    let dir = tree("manifest", TREE)?;
    cruise(&dir, &["src"], "err", &["--cache"])?;
    let text = std::fs::read_to_string(dir.join(".graph/cache/manifest.json"))?;
    let manifest: Value = serde_json::from_str(&text)?;
    let keys: Vec<&str> = manifest
        .as_object()
        .map(|m| m.keys().map(String::as_str).collect())
        .unwrap_or_default();
    assert_eq!(
        keys,
        [
            "toolVersion",
            "configHash",
            "worktree",
            "head",
            "strategy",
            "inputs",
            "stamps",
            "extraction"
        ]
    );
    assert_eq!(manifest["toolVersion"], env!("CARGO_PKG_VERSION"));
    assert!(
        manifest["configHash"]
            .as_str()
            .is_some_and(|h| h.starts_with("sha256:") && h.len() == 71)
    );
    assert_eq!(manifest["strategy"], "metadata");
    let inputs = manifest["inputs"].as_object().cloned().unwrap_or_default();
    for file in [
        "src/a.ts",
        "src/b.ts",
        "src/c.ts",
        "src/lib/d.ts",
        "src/lib/e.ts",
        "package.json",
    ] {
        assert!(
            inputs
                .get(file)
                .and_then(Value::as_str)
                .is_some_and(|h| h.starts_with("sha256:")),
            "{file}: {inputs:?}"
        );
    }
    assert!(
        !inputs.contains_key("rulebearing.yaml"),
        "the configuration is in the configHash"
    );
    let extraction = manifest["extraction"].as_str().unwrap_or_default();
    let hex = extraction.strip_prefix("sha256:").unwrap_or_default();
    let short = hex.get(..16).unwrap_or_default();
    assert!(
        dir.join(format!(".graph/cache/extraction-{short}.json"))
            .is_file()
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn a_configuration_or_a_flag_change_is_a_miss() -> Result {
    let dir = tree("config", TREE)?;
    cruise(&dir, &["src"], "err", &["--cache"])?;
    write(
        &dir,
        "rulebearing.yaml",
        &format!("{CONFIG}# one more line\n"),
    )?;
    let changed = cruise(&dir, &["src"], "err", &["--cache"])?;
    assert_eq!(
        served(&changed),
        "in full: the entry was written with another configHash"
    );
    let flagged = cruise(
        &dir,
        &["src"],
        "err",
        &["--ts-pre-compilation-deps", "--cache"],
    )?;
    assert_eq!(
        served(&flagged),
        "in full: the entry was written with another configHash"
    );
    let rooted = cruise(&dir, &["src/lib"], "err", &["--cache"])?;
    assert_eq!(
        served(&rooted),
        "in full: the entry was written with another configHash",
        "the positional paths are part of the key"
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn another_tool_version_is_a_miss() -> Result {
    let dir = tree("version", TREE)?;
    cruise(&dir, &["src"], "err", &["--cache"])?;
    let path = dir.join(".graph/cache/manifest.json");
    let mut manifest: Value = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
    manifest["toolVersion"] = Value::from("0.0.0-older");
    std::fs::write(&path, serde_json::to_vec(&manifest)?)?;
    let run = cruise(&dir, &["src"], "err", &["--cache"])?;
    assert_eq!(
        served(&run),
        "in full: the entry was written with another toolVersion"
    );
    assert_eq!(
        served(&cruise(&dir, &["src"], "err", &["--cache"])?),
        "from the cache",
        "the entry is written again for this version"
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn each_strategy_reads_again_only_what_changed() -> Result {
    for strategy in ["metadata", "content"] {
        let dir = tree(&format!("strategy-{strategy}"), TREE)?;
        let flags = ["--cache-strategy", strategy, "--cache"];
        let first = cruise(&dir, &["src"], "json", &flags)?;
        assert_eq!(served(&first), "in full: no entry");
        let hit = cruise(&dir, &["src"], "json", &flags)?;
        assert_eq!(served(&hit), "from the cache", "{strategy}");
        assert_eq!(
            without_receipt(&hit)?.1["strategy"],
            Value::from(strategy),
            "{strategy}"
        );
        // An edit that adds an unresolvable import and drops one: read again alone.
        write(
            &dir,
            "src/b.ts",
            "import { c } from \"./c\";\nimport { gone } from \"./missing\";\nexport const b = c + gone;\n",
        )?;
        let edited = cruise(&dir, &["src"], "json", &flags)?;
        assert_eq!(
            served(&edited),
            "incremental: 1 TypeScript and 0 Python files read again, .NET reused",
            "{strategy}"
        );
        same_as_cold(
            &edited,
            &cold(&dir, &["src"], "json", &flags, ".graph/cache")?,
        )?;
        assert_eq!(
            served(&cruise(&dir, &["src"], "json", &flags)?),
            "from the cache"
        );
        // A new file can change what an unchanged file resolves to: everything is read.
        write(&dir, "src/missing.ts", "export const gone = 0;\n")?;
        let added = cruise(&dir, &["src"], "json", &flags)?;
        assert_eq!(
            served(&added),
            "in full: src/missing.ts was added",
            "{strategy}"
        );
        same_as_cold(
            &added,
            &cold(&dir, &["src"], "json", &flags, ".graph/cache")?,
        )?;
        std::fs::remove_file(dir.join("src/lib/e.ts"))?;
        let deleted = cruise(&dir, &["src"], "json", &flags)?;
        assert_eq!(
            served(&deleted),
            "in full: src/lib/e.ts was deleted",
            "{strategy}"
        );
        same_as_cold(
            &deleted,
            &cold(&dir, &["src"], "json", &flags, ".graph/cache")?,
        )?;
        write(
            &dir,
            "package.json",
            "{ \"name\": \"fixture\", \"type\": \"module\" }\n",
        )?;
        let manifest = cruise(&dir, &["src"], "json", &flags)?;
        assert_eq!(
            served(&manifest),
            "in full: package.json changed",
            "{strategy}"
        );
        same_as_cold(
            &manifest,
            &cold(&dir, &["src"], "json", &flags, ".graph/cache")?,
        )?;
        // A file no extractor reads changes nothing.
        write(&dir, "notes.txt", "hello\n")?;
        assert_eq!(
            served(&cruise(&dir, &["src"], "json", &flags)?),
            "from the cache"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
    Ok(())
}

#[test]
fn switching_strategy_is_a_miss() -> Result {
    let dir = tree("switch", TREE)?;
    cruise(&dir, &["src"], "err", &["--cache"])?;
    let content = cruise(&dir, &["src"], "err", &["--cache-strategy", "content"])?;
    assert_eq!(
        served(&content),
        "in full: the entry was written with another strategy",
        "--cache-strategy alone turns the cache on, in the default folder"
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn a_corrupt_entry_is_discarded_never_trusted() -> Result {
    let dir = tree("corrupt", TREE)?;
    let reference = cold(&dir, &["src"], "json", &["--cache"], ".graph/cache")?;
    cruise(&dir, &["src"], "err", &["--cache"])?;
    let manifest = dir.join(".graph/cache/manifest.json");
    for garbage in [
        "",
        "{",
        "[1, 2]",
        "{\"toolVersion\": 7}",
        "\u{0}\u{1}binary",
    ] {
        std::fs::write(&manifest, garbage)?;
        let run = cruise(&dir, &["src"], "json", &["--cache"])?;
        assert!(
            served(&run).starts_with("in full: the entry is unusable"),
            "{garbage:?}: {}",
            served(&run)
        );
        same_as_cold(&run, &reference)?;
    }
    // A damaged extraction under an intact manifest is refused by its digest, then repaired.
    // The evaluated run is set aside first, since a full hit it answers never reads the
    // extraction.
    std::fs::remove_file(dir.join(".graph/cache/evaluated.json"))?;
    let text: Value = serde_json::from_str(&std::fs::read_to_string(&manifest)?)?;
    let hex = text["extraction"]
        .as_str()
        .and_then(|e| e.strip_prefix("sha256:"))
        .unwrap_or_default()
        .get(..16)
        .unwrap_or_default()
        .to_owned();
    let extraction = dir.join(format!(".graph/cache/extraction-{hex}.json"));
    let bytes = std::fs::read(&extraction)?;
    std::fs::write(
        &extraction,
        bytes.get(..bytes.len() / 2).unwrap_or_default(),
    )?;
    let run = cruise(&dir, &["src"], "json", &["--cache"])?;
    assert!(
        served(&run).contains("does not match its digest"),
        "{}",
        served(&run)
    );
    same_as_cold(&run, &reference)?;
    assert_eq!(
        served(&cruise(&dir, &["src"], "json", &["--cache"])?),
        "from the cache"
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn a_compressed_entry_round_trips() -> Result {
    let config = format!("{CONFIG}options:\n  cache:\n    compress: true\n");
    let mut files = TREE.to_vec();
    files[0] = ("rulebearing.yaml", config.as_str());
    let dir = tree("compress", &files)?;
    let cold = cruise(&dir, &["src"], "json", &["--no-cache"])?;
    assert_eq!(
        served(&cold),
        "not cached",
        "--no-cache wins over options.cache"
    );
    assert!(!dir.join(".graph/cache/manifest.json").exists());
    let first = cruise(&dir, &["src"], "json", &[])?;
    assert_eq!(
        served(&first),
        "in full: no entry",
        "options.cache alone turns it on"
    );
    let second = cruise(&dir, &["src"], "json", &[])?;
    assert_eq!(served(&second), "from the cache");
    let result: Value = serde_json::from_slice(&second.stdout)?;
    assert_eq!(
        result["summary"]["optionsUsed"]["cache"],
        serde_json::json!({ "folder": ".graph/cache", "strategy": "metadata", "compress": true })
    );
    let names: Vec<String> = std::fs::read_dir(dir.join(".graph/cache"))?
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("extraction-"))
        .collect();
    assert!(
        names.len() == 1 && names.iter().all(|n| n.ends_with(".json.z")),
        "{names:?}"
    );
    let bytes = std::fs::read(dir.join(".graph/cache").join(&names[0]))?;
    assert_eq!(bytes.first(), Some(&0x78), "a zlib stream");
    // The no-cache run's optionsUsed records `cache: false`, as dependency-cruiser's does.
    let off: Value = serde_json::from_slice(&cold.stdout)?;
    assert_eq!(off["summary"]["optionsUsed"]["cache"], Value::Bool(false));
    let (warm, _) = without_receipt(&second)?;
    let (first_text, _) = without_receipt(&first)?;
    assert_eq!(warm, first_text);
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn a_dependency_cruiser_configuration_caches_where_dependency_cruiser_does() -> Result {
    let dir = tree(
        "dependency-cruiser",
        &[
            (
                ".dependency-cruiser.json",
                r#"{ "forbidden": [{ "name": "no-circular", "severity": "warn", "from": {}, "to": { "circular": true } }], "options": { "cache": true } }"#,
            ),
            (
                "src/a.js",
                "const b = require(\"./b\");\nmodule.exports = b;\n",
            ),
            ("src/b.js", "module.exports = 1;\n"),
        ],
    )?;
    let first = cruise(&dir, &["src"], "json", &[])?;
    assert_eq!(served(&first), "in full: no entry");
    assert!(
        dir.join("node_modules/.cache/dependency-cruiser/manifest.json")
            .is_file()
    );
    let result: Value = serde_json::from_slice(&first.stdout)?;
    assert_eq!(
        result["summary"]["optionsUsed"]["cache"],
        serde_json::json!({ "folder": "node_modules/.cache/dependency-cruiser", "strategy": "metadata" })
    );
    // `--cache FOLDER` names the folder.
    let named = cruise(&dir, &["src"], "err", &["--cache", "tmp/cache"])?;
    assert_eq!(served(&named), "in full: no entry");
    assert!(dir.join("tmp/cache/manifest.json").is_file());
    assert_eq!(
        served(&cruise(&dir, &["src"], "err", &["--cache", "tmp/cache"])?),
        "from the cache",
        "the cache folder is never an input of its own"
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn strict_schema_strips_the_receipt_and_the_result_validates() -> Result {
    let dir = tree("strict", TREE)?;
    cruise(&dir, &["src"], "err", &["--cache"])?;
    let strict = cruise(&dir, &["src"], "json", &["--strict-schema", "--cache"])?;
    assert_eq!(served(&strict), "from the cache");
    let value: Value = serde_json::from_slice(&strict.stdout)?;
    assert!(value["summary"].get("cache").is_none());
    assert_eq!(
        value["summary"]["optionsUsed"]["cache"]["strategy"],
        "metadata"
    );
    let schema_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../conformance/dependency-cruiser/fixtures/schemas/cruise-result.schema.json");
    let schema: Value = serde_json::from_str(&std::fs::read_to_string(schema_path)?)?;
    let validator = jsonschema::draft7::new(&schema)?;
    let errors: Vec<String> = validator
        .iter_errors(&value)
        .take(3)
        .map(|e| format!("{}: {e}", e.instance_path()))
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn two_worktrees_keep_their_own_entries() -> Result {
    let main = tree("worktree-main", TREE)?;
    git(&main, &["init", "-q"])?;
    git(&main, &["add", "-A"])?;
    git(&main, &["commit", "-q", "-m", "one"])?;
    let linked = main.with_file_name(format!(
        "{}-linked",
        main.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    ));
    let _ = std::fs::remove_dir_all(&linked);
    git(
        &main,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "other",
            &linked.to_string_lossy(),
        ],
    )?;
    // The linked worktree diverges: b.ts no longer imports c.ts.
    write(&linked, "src/b.ts", "export const b = 5;\n")?;
    git(&linked, &["commit", "-q", "-am", "two"])?;
    for dir in [&main, &linked] {
        let first = cruise(dir, &["src"], "json", &["--cache"])?;
        assert_eq!(served(&first), "in full: no entry");
        let second = cruise(dir, &["src"], "json", &["--cache"])?;
        assert_eq!(served(&second), "from the cache");
        same_as_cold(
            &second,
            &cold(dir, &["src"], "json", &["--cache"], ".graph/cache")?,
        )?;
        let manifest: Value = serde_json::from_str(&std::fs::read_to_string(
            dir.join(".graph/cache/manifest.json"),
        )?)?;
        assert_eq!(
            manifest["worktree"],
            Value::from(dir.to_string_lossy().replace('\\', "/"))
        );
        assert_eq!(
            manifest["head"],
            Value::from(git(dir, &["rev-parse", "HEAD"])?)
        );
    }
    // One folder named by both: the other worktree's entry is a miss, never an answer.
    let shared = main.with_file_name(format!(
        "{}-shared",
        main.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    ));
    let _ = std::fs::remove_dir_all(&shared);
    let folder = shared.to_string_lossy().into_owned();
    cruise(&main, &["src"], "err", &["--cache", &folder])?;
    let other = cruise(&linked, &["src"], "json", &["--cache", &folder])?;
    assert_eq!(
        served(&other),
        "in full: the entry was written with another worktree"
    );
    same_as_cold(
        &other,
        &cold(&linked, &["src"], "json", &["--cache", &folder], &folder)?,
    )?;
    let _ = std::fs::remove_dir_all(&linked);
    let _ = std::fs::remove_dir_all(&shared);
    let _ = std::fs::remove_dir_all(&main);
    Ok(())
}

#[test]
fn in_a_repository_the_metadata_strategy_follows_edits_and_commits() -> Result {
    let dir = tree("git-metadata", TREE)?;
    write(&dir, ".gitignore", ".graph/\n")?;
    git(&dir, &["init", "-q"])?;
    git(&dir, &["add", "-A"])?;
    git(&dir, &["commit", "-q", "-m", "one"])?;
    cruise(&dir, &["src"], "err", &["--cache"])?;
    write(&dir, "src/c.ts", "export class C {}\nexport const c = 1;\n")?;
    let edited = cruise(&dir, &["src"], "json", &["--cache"])?;
    assert_eq!(
        served(&edited),
        "incremental: 1 TypeScript and 0 Python files read again, .NET reused"
    );
    same_as_cold(
        &edited,
        &cold(&dir, &["src"], "json", &["--cache"], ".graph/cache")?,
    )?;
    // Committing the edit moves HEAD; the files are what the entry holds, so it is a hit.
    git(&dir, &["commit", "-q", "-am", "two"])?;
    assert_eq!(
        served(&cruise(&dir, &["src"], "err", &["--cache"])?),
        "from the cache"
    );
    // An untracked file git lists is an addition; one it ignores is not listed at all.
    write(&dir, "src/f.ts", "export const f = 1;\n")?;
    let added = cruise(&dir, &["src"], "json", &["--cache"])?;
    assert_eq!(served(&added), "in full: src/f.ts was added");
    same_as_cold(
        &added,
        &cold(&dir, &["src"], "json", &["--cache"], ".graph/cache")?,
    )?;
    // A commit that deletes a file is seen through the diff against the recorded HEAD.
    git(&dir, &["add", "-A"])?;
    git(&dir, &["commit", "-q", "-m", "three"])?;
    cruise(&dir, &["src"], "err", &["--cache"])?;
    git(&dir, &["rm", "-q", "src/f.ts"])?;
    git(&dir, &["commit", "-q", "-m", "four"])?;
    let removed = cruise(&dir, &["src"], "json", &["--cache"])?;
    assert_eq!(served(&removed), "in full: src/f.ts was deleted");
    same_as_cold(
        &removed,
        &cold(&dir, &["src"], "json", &["--cache"], ".graph/cache")?,
    )?;
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn a_python_file_is_read_again_alone() -> Result {
    let dir = tree("python", &[])?;
    copy(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../rb-extract-python/tests/fixtures/pkg"),
        &dir,
    )?;
    let first = cruise(&dir, &["."], "json", &["--cache"])?;
    assert_eq!(served(&first), "in full: no entry");
    same_as_cold(
        &first,
        &cold(&dir, &["."], "json", &["--cache"], ".graph/cache")?,
    )?;
    assert_eq!(
        served(&cruise(&dir, &["."], "json", &["--cache"])?),
        "from the cache"
    );
    let file = dir.join("src/app/shapes.py");
    let mut text = std::fs::read_to_string(&file)?;
    text.push_str("\nimport json\n");
    std::fs::write(&file, text)?;
    let edited = cruise(&dir, &["."], "json", &["--cache"])?;
    assert_eq!(
        served(&edited),
        "incremental: 0 TypeScript and 1 Python files read again, .NET reused"
    );
    same_as_cold(
        &edited,
        &cold(&dir, &["."], "json", &["--cache"], ".graph/cache")?,
    )?;
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn an_assembly_change_reads_the_dotnet_graph_again() -> Result {
    let dir = tree("dotnet", &[])?;
    copy(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../rb-extract-dotnet/tests/fixtures/sample"),
        &dir,
    )?;
    write(
        &dir,
        "rulebearing.yaml",
        "languages:\n  dotnet:\n    assemblies: [built/Sample.dll]\nforbidden: []\n",
    )?;
    let first = cruise(&dir, &[], "json", &["--cache"])?;
    assert_eq!(served(&first), "in full: no entry");
    same_as_cold(
        &first,
        &cold(&dir, &[], "json", &["--cache"], ".graph/cache")?,
    )?;
    let manifest: Value = serde_json::from_str(&std::fs::read_to_string(
        dir.join(".graph/cache/manifest.json"),
    )?)?;
    for input in [
        "built/Sample.dll",
        "built/Sample.pdb",
        "built/Sample.Core.dll",
        "built/Sample.Core.pdb",
    ] {
        assert!(manifest["inputs"].get(input).is_some(), "{input}");
    }
    assert_eq!(
        served(&cruise(&dir, &[], "json", &["--cache"])?),
        "from the cache"
    );
    // Bytes after the image change the assembly's digest, not what it declares.
    let core = dir.join("built/Sample.Core.dll");
    let mut bytes = std::fs::read(&core)?;
    bytes.extend_from_slice(&[0; 16]);
    std::fs::write(&core, bytes)?;
    let rebuilt = cruise(&dir, &[], "json", &["--cache"])?;
    assert_eq!(
        served(&rebuilt),
        "incremental: 0 TypeScript and 0 Python files read again, .NET read again"
    );
    same_as_cold(
        &rebuilt,
        &cold(&dir, &[], "json", &["--cache"], ".graph/cache")?,
    )?;
    assert_eq!(
        served(&cruise(&dir, &[], "json", &["--cache"])?),
        "from the cache"
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn an_unwritable_cache_is_a_warning_and_the_run_goes_on() -> Result {
    let dir = tree("unwritable", TREE)?;
    write(&dir, "blocked", "a file where the cache folder would be")?;
    let reference = cruise(&dir, &["src"], "json", &["--cache", "blocked/cache"])?;
    let run = cruise(&dir, &["src"], "json", &["--cache", "blocked/cache"])?;
    // A folder that cannot hold an entry cannot hold a manifest either: never a hit.
    assert!(
        served(&run).starts_with("in full: the entry is unusable"),
        "{}",
        served(&run)
    );
    same_as_cold(&run, &reference)?;
    let plain: Value = serde_json::from_slice(&cruise(&dir, &["src"], "json", &[])?.stdout)?;
    let mut cached: Value = serde_json::from_slice(&run.stdout)?;
    if let Some(summary) = cached.get_mut("summary").and_then(Value::as_object_mut) {
        summary.remove("cache");
        if let Some(used) = summary
            .get_mut("optionsUsed")
            .and_then(Value::as_object_mut)
        {
            used.remove("cache");
        }
    }
    assert_eq!(cached, plain, "the run itself is the run without the cache");
    assert!(
        String::from_utf8_lossy(&run.stderr).contains("warning: the cache was not written"),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

/// A generated tree of `count` TypeScript modules in chains of ten, each importing the one
/// before it and a shared module, with a class each so the code layer has something to link.
fn generated(name: &str, count: usize) -> Result<PathBuf> {
    let dir = tree(
        name,
        &[("rulebearing.yaml", CONFIG), ("package.json", "{}\n")],
    )?;
    write(
        &dir,
        "src/shared.ts",
        "export class Shared {}\nexport const shared = 1;\n",
    )?;
    for n in 0..count {
        let previous = if n % 10 == 0 {
            String::new()
        } else {
            format!("import {{ m{} }} from \"./m{}\";\n", n - 1, n - 1)
        };
        write(
            &dir,
            &format!("src/m{n}.ts"),
            &format!(
                "{previous}import {{ Shared }} from \"./shared\";\nexport class C{n} extends Shared {{}}\nexport const m{n} = {n};\n"
            ),
        )?;
    }
    Ok(dir)
}

/// The fastest of `runs` runs, in milliseconds.
fn fastest(runs: usize, mut run: impl FnMut() -> Result<Output>) -> Result<(u128, Output)> {
    let mut best: Option<(u128, Output)> = None;
    for _ in 0..runs {
        let started = std::time::Instant::now();
        let output = run()?;
        let took = started.elapsed().as_millis();
        if best.as_ref().is_none_or(|(b, _)| took < *b) {
            best = Some((took, output));
        }
    }
    best.ok_or_else(|| "no run".into())
}

#[test]
fn a_warm_run_over_a_generated_tree_is_byte_identical_and_its_speed_recorded() -> Result {
    let dir = generated("speed", 400)?;
    let (plain_ms, _) = fastest(3, || cruise(&dir, &["src"], "json", &[]))?;
    let (cold_ms, reference) = fastest(3, || {
        cold(&dir, &["src"], "json", &["--cache"], ".graph/cache")
    })?;
    cruise(&dir, &["src"], "json", &["--cache"])?;
    let (warm_ms, warm) = fastest(3, || cruise(&dir, &["src"], "json", &["--cache"]))?;
    assert_eq!(served(&warm), "from the cache");
    same_as_cold(&warm, &reference)?;
    write(&dir, "src/m5.ts", "export const m5 = 5;\n")?;
    let (partial_ms, partial) = fastest(1, || cruise(&dir, &["src"], "json", &["--cache"]))?;
    assert_eq!(
        served(&partial),
        "incremental: 1 TypeScript and 0 Python files read again, .NET reused"
    );
    same_as_cold(
        &partial,
        &cold(&dir, &["src"], "json", &["--cache"], ".graph/cache")?,
    )?;
    #[allow(clippy::cast_precision_loss)] // milliseconds of a test run
    let ratio = plain_ms as f64 / warm_ms.max(1) as f64;
    println!(
        "cache: modules=402 uncached_ms={plain_ms} cold_ms={cold_ms} warm_ms={warm_ms} partial_ms={partial_ms} ratio={ratio:.2}"
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

/// A configuration whose evaluation has something to say: an error, a warning, a rule past its
/// `expires`, a ratchet over its budget, and a rule that matches nothing.
const JUDGED: &str = "forbidden:
  - name: no-circular
    severity: warn
    comment: \"adr:0010\"
    from: {}
    to: { circular: true }
  - name: not-to-unresolvable
    severity: error
    comment: \"adr:0010\"
    from: {}
    to: { couldNotResolve: true }
  - name: lapsed
    severity: warn
    comment: \"adr:0010\"
    expires: \"2026-06-01\"
    from: { path: \"^src/lib/\" }
    to: { path: \"^src/c\" }
  - name: nothing-here
    severity: error
    comment: \"adr:0010\"
    from: { path: \"^nowhere/\" }
    to: {}
rules:
  ratchets:
    - name: a-to-b
      comment: \"adr:0010\"
      from: { path: \"^src/a\" }
      to: { path: \"^src/b\" }
      budget: eng/budget.json
";

/// [`TREE`] under [`JUDGED`], with a cycle, an unresolvable import and a budget of 0.
fn judged(name: &str) -> Result<PathBuf> {
    let mut files: Vec<(&str, &str)> = TREE.to_vec();
    files[0] = ("rulebearing.yaml", JUDGED);
    files.push((
        "src/a.ts",
        "import { b } from \"./b\";\nimport { c } from \"./c\";\nimport { x } from \"./missing\";\nexport const a = b + c + x;\n",
    ));
    files.push((
        "src/lib/d.ts",
        "import { C } from \"../c\";\nexport const d = 2;\nexport const seen = C;\n",
    ));
    files.push(("eng/budget.json", "{ \"ceiling\": 0 }\n"));
    let dir = tree(name, &files)?;
    Ok(dir)
}

/// `rulebearing cruise src -T <type> <extra>` on the day `epoch` names, without progress, so its
/// stderr is comparable byte for byte.
fn judge(dir: &Path, output_type: &str, extra: &[&str], epoch: &str) -> Result<Output> {
    Ok(isolated(BIN, dir)
        .env("SOURCE_DATE_EPOCH", epoch)
        .args(["cruise", "src", "-T", output_type])
        .args(extra)
        .output()?)
}

/// How the extract and evaluate stages of the same run were served, from `--progress`.
fn stages(dir: &Path, output_type: &str, extra: &[&str], epoch: &str) -> Result<(String, bool)> {
    let mut flags = extra.to_vec();
    flags.extend(["--progress", "cli-feedback"]);
    let output = judge(dir, output_type, &flags, epoch)?;
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    Ok((
        served(&output),
        stderr.contains("evaluate (from the cache) ..."),
    ))
}

const DAY: &str = "1790000000";

/// Asserts `warm` is the cold run's output, stderr and exit code, `summary.cache.hit` aside.
fn same_run(warm: &Output, cold: &Output, output_type: &str) {
    assert_eq!(warm.status.code(), cold.status.code(), "{output_type}");
    assert_eq!(
        String::from_utf8_lossy(&warm.stderr),
        String::from_utf8_lossy(&cold.stderr),
        "{output_type}"
    );
    let warm_text =
        String::from_utf8_lossy(&warm.stdout).replace("\"hit\": true", "\"hit\": false");
    assert_eq!(
        warm_text,
        String::from_utf8_lossy(&cold.stdout),
        "{output_type}"
    );
}

#[test]
fn a_full_hit_serves_the_evaluated_run_byte_for_byte() -> Result {
    let dir = judged("evaluated")?;
    for output_type in [
        "err", "err-long", "json", "csv", "agent", "teamcity", "dot", "text",
    ] {
        let flags = ["--cache"];
        let aside = dir.join(".graph/cache-aside");
        let _ = std::fs::remove_dir_all(&aside);
        let had = dir.join(".graph/cache").exists();
        if had {
            std::fs::rename(dir.join(".graph/cache"), &aside)?;
        }
        let cold = judge(&dir, output_type, &flags, DAY)?;
        let _ = std::fs::remove_dir_all(dir.join(".graph/cache"));
        if had {
            std::fs::rename(&aside, dir.join(".graph/cache"))?;
        }
        assert!(
            !cold.stdout.is_empty() || output_type == "err",
            "{output_type}: {}",
            String::from_utf8_lossy(&cold.stderr)
        );
        // The first run with this reporter stores its evaluation; the second is served from it.
        judge(&dir, output_type, &flags, DAY)?;
        assert_eq!(
            stages(&dir, output_type, &flags, DAY)?,
            ("from the cache".to_owned(), true),
            "{output_type}"
        );
        let warm = judge(&dir, output_type, &flags, DAY)?;
        same_run(&warm, &cold, output_type);
    }
    let json = judge(&dir, "json", &["--cache"], DAY)?;
    let result: Value = serde_json::from_slice(&json.stdout)?;
    assert_eq!(result["summary"]["cache"]["hit"], Value::Bool(true));
    assert_eq!(result["summary"]["expired"][0]["name"], "lapsed");
    assert_eq!(result["summary"]["ratchets"][0]["status"], "exceeded");
    assert!(
        String::from_utf8_lossy(&json.stderr).contains("nothing-here"),
        "the vacuous rule is reported from the stored run"
    );
    assert_eq!(
        json.status.code(),
        Some(2),
        "a vacuous rule under strict liveness exits 2 whatever the reporter"
    );
    let err = judge(&dir, "err", &["--cache"], DAY)?;
    assert_eq!(
        err.status.code(),
        Some(2),
        "a vacuous rule under strict liveness"
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

/// A change to one input of the evaluation.
type Change = fn(&Path) -> Result;

/// Each change the evaluated layer must see, with the flags and the day the run takes after it,
/// in order: each row changes one input from the row before.
fn key_changes() -> [(&'static str, Change, Vec<&'static str>, &'static str); 7] {
    let unchanged: Change = |_| Ok(());
    [
        (
            "baseline",
            unchanged,
            vec!["--ignore-known", "eng/known.json"],
            DAY,
        ),
        (
            "the known violations",
            |d| {
                // One entry given an owner: a different file with the same findings.
                let path = d.join("eng/known.json");
                let mut entries: Value = serde_json::from_slice(&std::fs::read(&path)?)?;
                if let Some(first) = entries.get_mut(0).and_then(Value::as_object_mut) {
                    first.insert("owner".into(), Value::from("someone"));
                }
                Ok(std::fs::write(path, serde_json::to_vec(&entries)?)?)
            },
            vec!["--ignore-known", "eng/known.json"],
            DAY,
        ),
        (
            "--no-ignore-known",
            unchanged,
            vec!["--no-ignore-known"],
            DAY,
        ),
        (
            "the date",
            unchanged,
            vec!["--no-ignore-known"],
            "1690000000",
        ),
        (
            "the liveness",
            unchanged,
            vec!["--no-ignore-known", "--liveness", "warn"],
            DAY,
        ),
        (
            "a ratchet budget",
            |d| {
                Ok(std::fs::write(
                    d.join("eng/budget.json"),
                    "{ \"ceiling\": 5 }\n",
                )?)
            },
            vec!["--no-ignore-known", "--liveness", "warn"],
            DAY,
        ),
        (
            "a report filter",
            unchanged,
            vec![
                "--no-ignore-known",
                "--liveness",
                "warn",
                "--focus",
                "^src/lib",
            ],
            DAY,
        ),
    ]
}

/// Asserts two JSON results are equal once `summary.cache` and `optionsUsed.cache` are removed.
fn same_json_without_cache(warm: &Output, cold: &Output, what: &str) -> Result {
    let mut warm_value: Value = serde_json::from_slice(&warm.stdout)?;
    let mut cold_value: Value = serde_json::from_slice(&cold.stdout)?;
    for value in [&mut warm_value, &mut cold_value] {
        if let Some(summary) = value.get_mut("summary").and_then(Value::as_object_mut) {
            summary.remove("cache");
            if let Some(used) = summary
                .get_mut("optionsUsed")
                .and_then(Value::as_object_mut)
            {
                used.remove("cache");
            }
        }
    }
    assert_eq!(warm_value, cold_value, "{what}");
    Ok(())
}

#[test]
fn each_evaluation_input_misses_the_evaluated_run_and_keeps_the_extraction() -> Result {
    let dir = judged("evaluated-keys")?;
    let known = judge(&dir, "err", &[], DAY)?;
    assert!(!known.stdout.is_empty());
    let baseline = isolated(BIN, &dir)
        .env("SOURCE_DATE_EPOCH", DAY)
        .args(["baseline", "src", "-f", "eng/known.json"])
        .output()?;
    assert!(
        dir.join("eng/known.json").is_file(),
        "{}",
        String::from_utf8_lossy(&baseline.stderr)
    );
    let table = key_changes();
    // Warm the entry, extraction and evaluation both, under the first row's inputs; each row
    // then changes one input from the row before it.
    judge(
        &dir,
        "json",
        &["--ignore-known", "eng/known.json", "--cache"],
        DAY,
    )?;
    judge(
        &dir,
        "json",
        &["--ignore-known", "eng/known.json", "--cache"],
        DAY,
    )?;
    for (what, change, extra, epoch) in table {
        let mut flags = extra.clone();
        flags.push("--cache");
        change(&dir)?;
        if what != "baseline" {
            assert_eq!(
                stages(&dir, "json", &flags, epoch)?,
                ("from the cache".to_owned(), false),
                "{what}: the extraction is reused and the evaluation is not"
            );
        }
        assert_eq!(
            stages(&dir, "json", &flags, epoch)?,
            ("from the cache".to_owned(), true),
            "{what}: stored again"
        );
        let warm = judge(&dir, "json", &flags, epoch)?;
        let cold = judge(&dir, "json", &extra, epoch)?;
        assert_eq!(warm.status.code(), cold.status.code(), "{what}");
        assert_eq!(warm.stderr, cold.stderr, "{what}");
        same_json_without_cache(&warm, &cold, what)?;
    }
    // An earlier date brings the lapsed rule back to life: the verdicts really differ.
    let before = judge(
        &dir,
        "json",
        &["--no-ignore-known", "--cache"],
        "1690000000",
    )?;
    let after = judge(&dir, "json", &["--no-ignore-known", "--cache"], DAY)?;
    let expired = |o: &Output| -> Result<Value> {
        Ok(serde_json::from_slice::<Value>(&o.stdout)?["summary"]["expired"].clone())
    };
    assert_ne!(expired(&before)?, expired(&after)?);
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn affected_runs_do_not_use_the_evaluated_run() -> Result {
    let dir = judged("evaluated-affected")?;
    write(&dir, ".gitignore", ".graph/\n")?;
    git(&dir, &["init", "-q"])?;
    git(&dir, &["add", "-A"])?;
    git(&dir, &["commit", "-q", "-m", "one"])?;
    let flags = ["--affected", "HEAD", "--cache"];
    judge(&dir, "json", &flags, DAY)?;
    judge(&dir, "json", &flags, DAY)?;
    assert_eq!(
        stages(&dir, "json", &flags, DAY)?,
        ("from the cache".to_owned(), false)
    );
    assert!(!dir.join(".graph/cache/evaluated.json").exists());
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

/// `rulebearing cruise` for `flags` with the cache folder set aside, so it finds no entry.
fn judge_cold(dir: &Path, output_type: &str, flags: &[&str]) -> Result<Output> {
    let aside = dir.join(".graph/cache-aside");
    let _ = std::fs::remove_dir_all(&aside);
    std::fs::rename(dir.join(".graph/cache"), &aside)?;
    let cold = judge(dir, output_type, flags, DAY);
    std::fs::remove_dir_all(dir.join(".graph/cache"))?;
    std::fs::rename(&aside, dir.join(".graph/cache"))?;
    cold
}

#[test]
fn a_report_option_misses_the_rendered_output_and_keeps_the_evaluated_run() -> Result {
    let dir = judged("rendered-keys")?;
    for _ in 0..3 {
        judge(&dir, "json", &["--cache"], DAY)?;
    }
    assert!(dir.join(".graph/cache/rendered.json").is_file());
    for (what, extra) in [
        ("--strict-schema", vec!["--strict-schema"]),
        ("--output-to", vec!["-f", "out.json"]),
    ] {
        let mut flags = extra.clone();
        flags.push("--cache");
        let warm = judge(&dir, "json", &flags, DAY)?;
        let written = std::fs::read(dir.join("out.json")).unwrap_or_default();
        let _ = std::fs::remove_file(dir.join("out.json"));
        let cold = judge_cold(&dir, "json", &flags)?;
        let cold_written = std::fs::read(dir.join("out.json")).unwrap_or_default();
        let _ = std::fs::remove_file(dir.join("out.json"));
        same_run(&warm, &cold, what);
        assert_eq!(
            String::from_utf8_lossy(&written).replace("\"hit\": true", "\"hit\": false"),
            String::from_utf8_lossy(&cold_written),
            "{what}"
        );
    }
    // A damaged rendered output is a miss: the stored verdict is rendered again.
    std::fs::write(dir.join(".graph/cache/rendered.json"), "{")?;
    let cold = judge_cold(&dir, "json", &["--cache"])?;
    let warm = judge(&dir, "json", &["--cache"], DAY)?;
    same_run(&warm, &cold, "a damaged rendered output");
    // A stored verdict that cannot be read sends the run back through the extraction.
    std::fs::write(dir.join(".graph/cache/rendered.json"), "{")?;
    for entry in std::fs::read_dir(dir.join(".graph/cache"))? {
        let path = entry?.path();
        if path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with("evaluated-"))
        {
            std::fs::write(&path, "damaged")?;
        }
    }
    let (extract, evaluate) = stages(&dir, "json", &["--cache"], DAY)?;
    assert_eq!((extract.as_str(), evaluate), ("from the cache", false));
    let warm = judge(&dir, "json", &["--cache"], DAY)?;
    same_run(&warm, &cold, "a damaged verdict");
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

/// `--from` is part of the rendered layer's key: a warm `-T plantuml --from types` after a run
/// `--from folders` is drawn from types, never the cached folders diagram
/// ([plan 0003, Step 9](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#22-steps-for-sub-wave-3b-the-remaining-reporters-and-the-sidecar)).
#[test]
fn plantuml_from_is_part_of_the_rendered_key() -> Result {
    let dir = tree("plantuml-from", TREE)?;
    let folders = |dir: &Path| cruise(dir, &["src"], "plantuml", &["--from", "folders", "--cache"]);
    let types = |dir: &Path| cruise(dir, &["src"], "plantuml", &["--from", "types", "--cache"]);
    let first = folders(&dir)?;
    assert_eq!(
        first.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let warm = folders(&dir)?;
    assert_eq!(served(&warm), "from the cache");
    assert_eq!(warm.stdout, first.stdout);
    assert!(String::from_utf8_lossy(&warm.stdout).contains("[src] <<"));
    let drawn = types(&dir)?;
    assert_eq!(served(&drawn), "from the cache", "the graph is reused");
    assert_ne!(drawn.stdout, warm.stdout, "not the stale folders diagram");
    let cold = cruise(&dir, &["src"], "plantuml", &["--from", "types"])?;
    assert_eq!(
        drawn.stdout, cold.stdout,
        "the diagram a run without the cache draws"
    );
    assert!(String::from_utf8_lossy(&drawn.stdout).contains("!include "));
    // And back: the folders diagram again, not the types one.
    assert_eq!(folders(&dir)?.stdout, first.stdout);
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}
