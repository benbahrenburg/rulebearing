//! One cruise over three languages: TypeScript, .NET and Python modules in one graph, one rule
//! pass, one receipt per language.
//!
//! - Requirement: [FR-CORE-01](../../../docs/prd.md#fr-core-01) ("oracle runs on dify and
//!   OpenMetadata produce one graph with `language` on every module")
//! - Source: [design § One engine, three languages](../../../docs/artifacts/design.md#one-engine-three-languages-one-monorepo)
//! - Decision: [ADR-0014](../../../docs/adr/0014-no-invented-cross-language-edges.md) (no edge
//!   joins two languages)
//! - Plan: [Wave 2 § 1.3](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#13-requirements-traceability)

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_rulebearing");

const CONFIG: &str = r#"languages:
  dotnet:
    assemblies: ["dotnet/*.dll"]
  python:
    roots: ["py"]
rules:
  dependencies:
    forbidden:
      - name: order-lines-not-to-customers
        comment: "A test rule over the .NET half."
        severity: error
        from: { path: "Order\\.Lines\\.cs$" }
        to: { path: "Customer\\.cs$", dependencyTypes: [local] }
      - name: python-app-not-to-util
        comment: "A test rule over the Python half."
        severity: warn
        from: { path: "^py/app/core\\.py$" }
        to: { path: "^py/app/util\\.py$" }
"#;

fn repository() -> Result<PathBuf, Box<dyn Error>> {
    let dir = std::env::temp_dir().join(format!("rb-cli-multi-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let files = [
        ("rulebearing.yaml", CONFIG),
        (
            "web/app.ts",
            "import { x } from \"./lib\";\nconsole.log(x);\n",
        ),
        ("web/lib.ts", "export const x = 1;\n"),
        ("py/app/__init__.py", ""),
        ("py/app/core.py", "from app import util\nimport json\n"),
        ("py/app/util.py", "VALUE = 1\n"),
    ];
    for (file, text) in files {
        let path = dir.join(file);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, text)?;
    }
    let built = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../rb-extract-dotnet/tests/fixtures/sample/built");
    std::fs::create_dir_all(dir.join("dotnet"))?;
    for file in ["Sample.dll", "Sample.pdb"] {
        std::fs::copy(built.join(file), dir.join("dotnet").join(file))?;
    }
    Ok(dir)
}

#[test]
fn one_cruise_reads_three_languages() -> Result<(), Box<dyn Error>> {
    let dir = repository()?;
    let output = Command::new(BIN)
        .args(["cruise", "-T", "json", "--no-progress", "web", "py"])
        .current_dir(&dir)
        .output()?;
    let _ = std::fs::remove_dir_all(&dir);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(0), "{stderr}");
    let result: Value = serde_json::from_slice(&output.stdout)?;
    let languages: std::collections::BTreeSet<String> = result["modules"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| m["language"].as_str().map(str::to_owned))
        .collect();
    for language in ["typescript", "dotnet", "python"] {
        assert!(
            languages.contains(language),
            "{language} missing from {languages:?}"
        );
    }
    let inspected = &result["summary"]["inspected"];
    assert!(
        inspected["dotnet"]["attribution"]["pdb"].as_u64() > Some(0),
        "{inspected}"
    );
    assert!(
        inspected["python"]["stdlibVersion"].is_string(),
        "{inspected}"
    );
    assert!(
        inspected["typescript"]["files"].as_u64() >= Some(2),
        "{inspected}"
    );
    let fired: Vec<&str> = result["summary"]["violations"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v["rule"]["name"].as_str())
        .collect();
    assert!(fired.contains(&"order-lines-not-to-customers"), "{fired:?}");
    assert!(fired.contains(&"python-app-not-to-util"), "{fired:?}");
    // ADR-0014: every edge stays inside its language.
    let language_of: std::collections::BTreeMap<&str, &str> = result["modules"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| Some((m["source"].as_str()?, m["language"].as_str()?)))
        .collect();
    for module in result["modules"].as_array().into_iter().flatten() {
        let from = module["language"].as_str();
        for dependency in module["dependencies"].as_array().into_iter().flatten() {
            if let Some(to) = dependency["resolved"]
                .as_str()
                .and_then(|r| language_of.get(r))
            {
                assert_eq!(
                    from,
                    Some(*to),
                    "{} -> {}",
                    module["source"],
                    dependency["resolved"]
                );
            }
        }
    }
    assert!(
        result["code"]["types"]
            .as_array()
            .is_some_and(|t| t.len() > 5)
    );
    Ok(())
}
