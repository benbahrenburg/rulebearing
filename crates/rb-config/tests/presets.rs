//! The bundled Rulebearing presets: what each resolves to, as a committed snapshot, and the
//! composition `rulebearing:recommended` promises.
//!
//! - Plan: [Wave 2, Step 9](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#29-step-9-presets---init-presets-vue-svelte-markdown-webpackconfig-collapse-highlight-experimentalstats-2d)
//!   ("a preset snapshot test")
//! - Source: [design § What stays honest across the boundary](../../../docs/artifacts/design.md#what-stays-honest-across-the-boundary)
//!   ("Per-language defaults are presets ... composed by `rulebearing:recommended`, so a repo with
//!   one language does not carry the others' excludes")
//! - Requirement: [FR-CFG-06](../../../docs/prd.md#fr-cfg-06)
//! - Decision: [ADR-0024](../../../docs/adr/0024-test-quality-gates.md) (snapshots)
//!
//! Each snapshot under `tests/presets/` is the configuration an `extends` line resolves to, with
//! `extends` merged (dependency-cruiser's canonical shape, what `summary.ruleSetUsed` starts
//! from). Regenerate deliberately with `RB_UPDATE_SNAPSHOTS=1 cargo test -p rb-config --test
//! presets`, and explain the diff in review.

use std::error::Error;
use std::path::{Path, PathBuf};

use rb_config::extends::{NATIVE_PRESETS, Target, resolve};
use rb_config::read::Syntax;
use rb_config::{LoadOptions, load_text};
use serde_json::Value;

/// The `extends` lines snapshotted, and the file each goes to.
const LINES: &[(&str, &str)] = &[
    ("rulebearing:recommended", "recommended"),
    ("rulebearing:typescript", "typescript"),
    ("rulebearing:dotnet", "dotnet"),
    ("rulebearing:python", "python"),
    (
        "[rulebearing:typescript, rulebearing:recommended]",
        "typescript-only",
    ),
    (
        "[rulebearing:dotnet, rulebearing:recommended]",
        "dotnet-only",
    ),
    (
        "[rulebearing:python, rulebearing:recommended]",
        "python-only",
    ),
];

fn resolved(line: &str) -> Result<String, Box<dyn Error>> {
    let config = load_text(
        &format!("extends: {line}\n"),
        Syntax::Yaml,
        Path::new(env!("CARGO_MANIFEST_DIR")),
        &LoadOptions::default(),
    )?;
    let mut text = serde_json::to_string_pretty(&sorted(Value::Object(config.canonical)))?;
    text.push('\n');
    Ok(text)
}

/// `value` with every object's keys in order, so the snapshot does not depend on whether
/// `serde_json`'s `preserve_order` is on in this build (`oxc_resolver` turns it on when the
/// TypeScript extractor is in the same build).
fn sorted(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<(String, Value)> = map.into_iter().collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            Value::Object(entries.into_iter().map(|(k, v)| (k, sorted(v))).collect())
        }
        Value::Array(items) => Value::Array(items.into_iter().map(sorted).collect()),
        other => other,
    }
}

fn snapshot(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/presets")
        .join(format!("{name}.json"))
}

#[test]
fn every_preset_resolves_to_its_snapshot() -> Result<(), Box<dyn Error>> {
    for (line, name) in LINES {
        let text = resolved(line)?;
        assert_eq!(text, resolved(line)?, "{line}: two loads serialise alike");
        let path = snapshot(name);
        if std::env::var_os("RB_UPDATE_SNAPSHOTS").is_some() {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(&path, &text)?;
            continue;
        }
        let committed = std::fs::read_to_string(&path)
            .map_err(|e| format!("{}: {e}; run with RB_UPDATE_SNAPSHOTS=1", path.display()))?;
        assert_eq!(
            text, committed,
            "{line} changed; if that was intended, regenerate with RB_UPDATE_SNAPSHOTS=1"
        );
    }
    Ok(())
}

/// A preset's file as written, before `extends`.
fn written(name: &str) -> Result<Value, Box<dyn Error>> {
    match resolve(&format!("rulebearing:{name}"), Path::new("."))? {
        Target::NativePreset(_, text) => Ok(serde_yaml::from_str(text)?),
        other => Err(format!("{name} is {other:?}").into()),
    }
}

fn strings(value: &Value, pointer: &str) -> Vec<String> {
    value
        .pointer(pointer)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect()
}

fn orphan_exclusions(preset: &Value) -> Vec<String> {
    preset
        .pointer("/rules/dependencies/forbidden")
        .and_then(Value::as_array)
        .and_then(|rules| rules.iter().find(|r| r["name"] == "no-orphans"))
        .map(|rule| strings(rule, "/from/pathNot"))
        .unwrap_or_default()
}

#[test]
fn recommended_is_the_union_of_the_language_presets() -> Result<(), Box<dyn Error>> {
    let recommended = written("recommended")?;
    let languages = strings(&recommended, "/extends");
    assert_eq!(
        languages,
        [
            "rulebearing:typescript",
            "rulebearing:dotnet",
            "rulebearing:python"
        ]
    );
    let mut orphans = Vec::new();
    let mut excludes = Vec::new();
    for language in &languages {
        let name = language.trim_start_matches("rulebearing:");
        let preset = written(name)?;
        orphans.extend(orphan_exclusions(&preset));
        excludes.extend(strings(&preset, "/options/exclude/path"));
    }
    assert_eq!(orphan_exclusions(&recommended), orphans);
    assert_eq!(strings(&recommended, "/options/exclude/path"), excludes);
    Ok(())
}

#[test]
fn the_language_presets_carry_their_exclusions_and_nothing_else() -> Result<(), Box<dyn Error>> {
    for name in ["dotnet", "python"] {
        let preset = written(name)?;
        let mut keys: Vec<&str> = preset
            .as_object()
            .map(|o| o.keys().map(String::as_str).collect())
            .unwrap_or_default();
        keys.sort_unstable();
        let expected: &[&str] = if name == "python" {
            // `stubs: false` is step 4's `.pyi` exclusion.
            &["languages", "options", "rules"]
        } else {
            &["options", "rules"]
        };
        assert_eq!(keys, expected, "{name}");
        let rules = preset
            .pointer("/rules/dependencies/forbidden")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        assert_eq!(rules, 1, "{name}: no-orphans only");
        let options: Vec<&str> = preset["options"]
            .as_object()
            .map(|o| o.keys().map(String::as_str).collect())
            .unwrap_or_default();
        assert_eq!(options, ["exclude"], "{name}");
    }
    Ok(())
}

#[test]
fn a_single_language_repository_carries_no_other_language_s_exclusions()
-> Result<(), Box<dyn Error>> {
    // FR-CFG-06: `rulebearing:python` alone excludes `.venv/`, `site-packages/`,
    // `__pycache__/` and nothing from `obj/` or `node_modules/`.
    for (line, own, foreign) in [
        (
            "rulebearing:python",
            &[".venv", "site-packages", "__pycache__"][..],
            &["obj", "node_modules", "dist"][..],
        ),
        (
            "[rulebearing:python, rulebearing:recommended]",
            &[".venv", "site-packages", "__pycache__"][..],
            &["obj", "node_modules", "dist"][..],
        ),
        (
            "[rulebearing:dotnet, rulebearing:recommended]",
            &["obj", "Designer"][..],
            &["venv", "dist"][..],
        ),
        (
            "[rulebearing:typescript, rulebearing:recommended]",
            &["dist", "coverage"][..],
            &["obj", "venv"][..],
        ),
    ] {
        let config: Value = serde_json::from_str(&resolved(line)?)?;
        let exclude = config["options"]["exclude"].to_string();
        let orphans = config["forbidden"]
            .as_array()
            .and_then(|rules| rules.iter().find(|r| r["name"] == "no-orphans"))
            .map(|r| r["from"]["pathNot"].to_string())
            .unwrap_or_default();
        for part in own {
            assert!(exclude.contains(part), "{line}: {part} in {exclude}");
        }
        for part in foreign {
            assert!(!exclude.contains(part), "{line}: {part} not in {exclude}");
        }
        let foreign_orphans = ["Program", "__main__", "tsconfig"]
            .iter()
            .filter(|p| orphans.contains(*p))
            .count();
        assert_eq!(
            foreign_orphans, 1,
            "{line}: its own orphan exclusions only: {orphans}"
        );
    }
    Ok(())
}

#[test]
fn every_native_preset_is_snapshotted() {
    for (name, _) in NATIVE_PRESETS {
        assert!(
            LINES.iter().any(|(_, file)| file == name),
            "rulebearing:{name} has no snapshot"
        );
    }
}
