//! Conformance gate 1, layer 4: what Rulebearing writes validates against dependency-cruiser's own
//! schemas.
//!
//! - Plan: [Wave 1, Step 12](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-12-reporters-1d)
//!   (layer 4: every `json` with `--strict-schema` against `cruise-result.schema.json`, every
//!   accepted config against `configuration.schema.json`)
//! - Decisions: [ADR-0004](../../../docs/adr/0004-graph-document-is-cruise-result-superset.md)
//!   (the additions strip cleanly), [ADR-0009](../../../docs/adr/0009-conformance-suites-as-specification.md)
//! - Requirements: [FR-CORE-04](../../../docs/prd.md#fr-core-04), [FR-CFG-01](../../../docs/prd.md#fr-cfg-01),
//!   [NFR-CONF-01](../../../docs/prd.md#nfr-conf-01)
//!
//! The schemas are the vendored copies under `conformance/dependency-cruiser/fixtures/schemas/`.
//! Results come from three places:
//!
//! 1. every upstream `test/report` mock the pinned schema accepts (`fixtures/report-json/INDEX.json`
//!    lists them under `valid`), re-reported with `fmt -T json --strict-schema`;
//! 2. a fresh `cruise -T json --strict-schema` over a small TypeScript tree with a violation, a
//!    cycle, an unresolvable import, metrics and a ratchet;
//! 3. the layer 5 results under `target/layer5/` when that layer has run.
//!
//! Configurations are every bundled dependency-cruiser preset and, when
//! `scripts/fetch-oracle-configs.sh` has run, every oracle configuration, each loaded with its
//! `extends` merged, as dependency-cruiser validates them.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_rulebearing");

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixtures() -> PathBuf {
    root().join("conformance/dependency-cruiser/fixtures")
}

fn validator(name: &str) -> Result<jsonschema::Validator, Box<dyn Error>> {
    let text = std::fs::read_to_string(fixtures().join("schemas").join(name))?;
    let schema: Value = serde_json::from_str(&text)?;
    Ok(jsonschema::draft7::new(&schema)?)
}

/// The schema errors for `value`, at most three, each with its JSON pointer.
fn errors(validator: &jsonschema::Validator, value: &Value) -> Vec<String> {
    validator
        .iter_errors(value)
        .take(3)
        .map(|e| format!("{}: {e}", e.instance_path()))
        .collect()
}

fn run(dir: &Path, args: &[&str]) -> Result<(Option<i32>, Value, String), Box<dyn Error>> {
    let output = Command::new(BIN).args(args).current_dir(dir).output()?;
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let value = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("{args:?} did not print JSON ({e}); stderr: {stderr}"))?;
    Ok((output.status.code(), value, stderr))
}

fn files_under(dir: &Path, keep: &dyn Fn(&Path) -> bool) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(files_under(&path, keep));
        } else if keep(&path) {
            out.push(path);
        }
    }
    out.sort();
    out
}

#[test]
fn upstream_results_revalidate_after_fmt() -> Result<(), Box<dyn Error>> {
    let schema = validator("cruise-result.schema.json")?;
    let index: Value = serde_json::from_str(&std::fs::read_to_string(
        fixtures().join("report-json/INDEX.json"),
    )?)?;
    let valid: Vec<&str> = index["valid"]
        .as_array()
        .map(|v| v.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    assert!(
        valid.len() >= 30,
        "INDEX.json lists {} valid results",
        valid.len()
    );
    let mut failures = Vec::new();
    for file in &valid {
        let path = fixtures().join(file);
        let path = path.to_string_lossy();
        let (_, value, stderr) = run(
            &fixtures(),
            &["fmt", "-T", "json", "--strict-schema", &path],
        )?;
        let found = errors(&schema, &value);
        if !found.is_empty() {
            failures.push(format!("{file}: {found:?} {stderr}"));
        }
    }
    println!(
        "layer 4: {} of {} upstream results validate after fmt",
        valid.len() - failures.len(),
        valid.len()
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    Ok(())
}

const CONFIG: &str = r#"{
  "forbidden": [
    { "name": "no-circular", "severity": "warn", "comment": "adr:0010", "from": {}, "to": { "circular": true } },
    { "name": "not-to-unresolvable", "severity": "error", "comment": "adr:0010", "from": {}, "to": { "couldNotResolve": true } },
    { "name": "domain-not-to-web", "severity": "error", "comment": "adr:0010", "from": { "path": "^src/domain/" }, "to": { "path": "^src/web/" } }
  ],
  "options": { "metrics": true, "tsPreCompilationDeps": true }
}
"#;

#[test]
fn a_fresh_cruise_validates_with_strict_schema() -> Result<(), Box<dyn Error>> {
    let schema = validator("cruise-result.schema.json")?;
    let dir = std::env::temp_dir().join(format!("rb-cli-layer4-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src/domain"))?;
    std::fs::create_dir_all(dir.join("src/web"))?;
    let files = [
        (
            "src/domain/model.ts",
            "import { w } from \"../web/view\";\nimport type { T } from \"./types\";\nexport const d: T = w;\n",
        ),
        (
            "src/domain/types.ts",
            "import { d } from \"./model\";\nexport type T = typeof d;\n",
        ),
        (
            "src/web/view.ts",
            "import missing from \"./nowhere\";\nexport const w = missing;\n",
        ),
        (
            "src/main.ts",
            "import { d } from \"./domain/model\";\nconst lazy = import(\"./web/view\");\nconsole.log(d, lazy);\n",
        ),
        (".dependency-cruiser.json", CONFIG),
    ];
    for (name, text) in files {
        std::fs::write(dir.join(name), text)?;
    }
    for extra in [&[][..], &["--strict-schema"][..]] {
        let args = [&["cruise", "-T", "json", "src"][..], extra].concat();
        let (code, value, stderr) = run(&dir, &args)?;
        assert_eq!(code, Some(2), "two errors, so the exit code is 2: {stderr}");
        let found = errors(&schema, &value);
        if extra.is_empty() {
            // Without the flag the additions are present, which the upstream schema refuses.
            assert!(
                !found.is_empty(),
                "the additions should be visible without --strict-schema"
            );
            assert!(value["summary"]["violations"][0]["id"].is_string());
        } else {
            assert!(found.is_empty(), "{found:?}");
            assert!(value["summary"].get("inspected").is_none());
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn layer_5_results_validate() -> Result<(), Box<dyn Error>> {
    let schema = validator("cruise-result.schema.json")?;
    let results = files_under(&root().join("target/layer5"), &|p| {
        p.extension().is_some_and(|e| e == "json") && p.to_string_lossy().contains("strict")
    });
    if results.is_empty() {
        eprintln!(
            "layer 4: no layer 5 results under target/layer5; run scripts/run-layer-5.sh first"
        );
    }
    for path in results {
        let value: Value = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
        let found = errors(&schema, &value);
        assert!(found.is_empty(), "{}: {found:?}", path.display());
    }
    Ok(())
}

fn is_dependency_cruiser_config(path: &Path) -> bool {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    name.starts_with(".dependency-cruiser.")
        || path.parent().is_some_and(|p| p.ends_with("configs"))
}

/// What dependency-cruiser validates: the configuration with `extends` merged, before its rules
/// are normalised (`assertRuleSetValid` in upstream's `src/main/cruise.mjs`).
///
/// `options.baseline` is removed first. dependency-cruiser releases after the pinned 18.2.0 accept
/// it and its own repository's configuration uses it; the loader records it with a warning
/// (`rb-config/src/normalize.rs`), and the pinned schema does not know it.
fn accepted_config(path: &Path) -> Result<Value, rb_config::ConfigError> {
    let mut merged = rb_config::load::merged(path, &rb_config::load::LoadOptions::default())?;
    if let Some(Value::Object(options)) = merged.get_mut("options") {
        options.remove("baseline");
    }
    Ok(Value::Object(merged))
}

#[test]
fn accepted_configs_validate() -> Result<(), Box<dyn Error>> {
    let schema = validator("configuration.schema.json")?;
    let mut configs = files_under(&root().join("presets/dependency-cruiser"), &|p| {
        p.extension().is_some_and(|e| e == "cjs")
            && p.parent()
                .is_some_and(|d| d.ends_with("dependency-cruiser"))
    });
    let oracles = files_under(
        &root().join("target/oracle-configs"),
        &is_dependency_cruiser_config,
    );
    if oracles.is_empty() {
        eprintln!(
            "layer 4: oracle configurations not fetched; run scripts/fetch-oracle-configs.sh"
        );
    }
    configs.extend(oracles);
    assert!(configs.len() >= 3, "the bundled presets are always there");
    let mut failures = Vec::new();
    let mut needs_repository = 0;
    for path in &configs {
        let file = path.to_string_lossy();
        let accepted = match accepted_config(path) {
            Ok(c) => c,
            Err(e) if e.to_string().contains("--config-via-node") => {
                // It reads its repository's files (ADR-0006), which a fetched configuration does
                // not have beside it; layer 5 loads it inside the full checkout.
                eprintln!("layer 4: {file} reads its repository; validated by layer 5");
                needs_repository += 1;
                continue;
            }
            Err(e) => {
                failures.push(format!("{file}: {e}"));
                continue;
            }
        };
        let found = errors(&schema, &accepted);
        if !found.is_empty() {
            failures.push(format!("{file}: {found:?}"));
        }
    }
    println!(
        "layer 4: {} of {} configurations validate ({needs_repository} need their repository)",
        configs.len() - failures.len() - needs_repository,
        configs.len() - needs_repository
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    Ok(())
}
