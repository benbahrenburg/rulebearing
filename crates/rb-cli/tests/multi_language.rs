//! One cruise over three languages: TypeScript, .NET and Python modules in one graph, one rule
//! pass, one receipt per language.
//!
//! - Requirement: [FR-CORE-01](../../../docs/prd.md#fr-core-01) ("oracle runs on dify and
//!   `OpenMetadata` produce one graph with `language` on every module")
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

fn repository(tag: &str) -> Result<PathBuf, Box<dyn Error>> {
    let dir = std::env::temp_dir().join(format!("rb-cli-multi-{tag}-{}", std::process::id()));
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
        ("py/app/shapes.py", "class Shape:\n    pass\n"),
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
    let dir = repository("three")?;
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

const ELEMENTS: &str = r#"languages:
  dotnet:
    assemblies: ["dotnet/*.dll"]
  python:
    roots: ["py"]
rules:
  elements:
    - name: customers-are-sealed
      comment: "adr:0004"
      fix: "Seal the class, or move it out of Sample.Customers."
      select: { kind: class, language: dotnet, where: { resideInNamespace: Sample.Customers } }
      should: { beSealed: true }
"#;

fn run_with(config: &str, dir_tag: &str) -> Result<(Option<i32>, Value, String), Box<dyn Error>> {
    run_as(config, dir_tag, "json")
}

fn run_as(
    config: &str,
    dir_tag: &str,
    output_type: &str,
) -> Result<(Option<i32>, Value, String), Box<dyn Error>> {
    let moved = repository(dir_tag)?;
    std::fs::write(moved.join("rulebearing.yaml"), config)?;
    let output = Command::new(BIN)
        .args(["cruise", "-T", output_type, "--no-progress", "py"])
        .current_dir(&moved)
        .output()?;
    let _ = std::fs::remove_dir_all(&moved);
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let json = serde_json::from_slice(&output.stdout).unwrap_or(Value::Null);
    Ok((output.status.code(), json, stderr))
}

#[test]
fn element_rules_report_failing_objects_and_refuse_unanswerable_keys() -> Result<(), Box<dyn Error>>
{
    let (code, result, stderr) = run_with(ELEMENTS, "elements")?;
    assert_eq!(code, Some(0), "{stderr}");
    let failing: Vec<&str> = result["summary"]["violations"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|v| v["rule"]["name"] == "customers-are-sealed")
        .filter_map(|v| v["to"].as_str())
        .collect();
    assert!(
        failing.contains(&"Sample.Customers.Customer"),
        "{failing:?}"
    );
    assert!(
        failing.contains(&"Sample.Customers.Repository`1"),
        "{failing:?}"
    );
    assert!(
        !failing.contains(&"Sample.Customers.Constants"),
        "a static class is sealed"
    );
    let first = result["summary"]["violations"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|v| v["rule"]["name"] == "customers-are-sealed")
        .cloned()
        .unwrap_or(Value::Null);
    assert_eq!(first["type"], "element");
    assert_eq!(first["decision"], "adr:0004");
    assert!(first["id"].as_str().is_some_and(|id| id.starts_with("RB-")));
    let unscoped = ELEMENTS.replace("language: dotnet, ", "");
    let (code, _, stderr) = run_with(&unscoped, "unscoped")?;
    assert_eq!(
        code,
        Some(3),
        "beSealed over Python classes cannot be answered: {stderr}"
    );
    assert!(
        stderr.contains("`beSealed` has no meaning in python"),
        "{stderr}"
    );
    Ok(())
}

const PASSING_ELEMENT: &str = r#"languages:
  dotnet:
    assemblies: ["dotnet/*.dll"]
  python:
    roots: ["py"]
rules:
  elements:
    - name: customers-exist
      comment: "adr:0004"
      select: { kind: class, language: dotnet, where: { resideInNamespace: Sample.Customers } }
      should: { exist: true }
"#;

#[test]
fn element_rule_metadata_and_empty_conditions_reach_the_exit_code() -> Result<(), Box<dyn Error>> {
    let (code, _, stderr) = run_as(PASSING_ELEMENT, "element-pass", "err")?;
    assert_eq!(code, Some(0), "the rule holds: {stderr}");

    // An expired element rule fails the run the day after, as a dependency rule does.
    let expired = PASSING_ELEMENT.replace(
        "      comment: \"adr:0004\"\n",
        "      comment: \"adr:0004\"\n      owner: \"@team\"\n      expires: \"2020-01-01\"\n",
    );
    let (code, _, stderr) = run_as(&expired, "element-expired-err", "err")?;
    assert_eq!(code, Some(1), "{stderr}");
    assert!(
        stderr.contains("rule `customers-exist` expired on 2020-01-01"),
        "{stderr}"
    );
    let (_, result, _) = run_with(&expired, "element-expired")?;
    assert_eq!(
        result["summary"]["expired"][0]["name"], "customers-exist",
        "{}",
        result["summary"]
    );

    // An empty `should` would pass every object: a configuration error naming the rule.
    let empty = PASSING_ELEMENT.replace("should: { exist: true }", "should: {}");
    let (code, _, stderr) = run_with(&empty, "element-empty")?;
    assert_eq!(code, Some(3), "{stderr}");
    assert!(
        stderr.contains("rules.elements[customers-exist].should"),
        "{stderr}"
    );

    // `examples` has no meaning on an element rule: refused, not ignored.
    let examples = PASSING_ELEMENT.replace(
        "should: { exist: true }",
        "should: { exist: true }\n      examples: { allowed: [\"a -> b\"] }",
    );
    let (code, _, stderr) = run_with(&examples, "element-examples")?;
    assert_eq!(code, Some(3), "{stderr}");
    assert!(stderr.contains("`examples`"), "{stderr}");
    Ok(())
}
