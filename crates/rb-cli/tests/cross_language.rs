//! The cross-language dependency-rule keys end to end: the .NET extractor's licence and assembly
//! facts reaching the rules, the refusal of the keys in a dependency-cruiser configuration, and
//! the `type-only` warning on a .NET rule.
//!
//! - Plan: [Wave 2, Step 8](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#28-step-8-cross-language-rule-additions-per-language-dependencytypes-license-moreunstable-2d)
//!   ("the exit 3 test for a dependency-cruiser-format config carrying `namespace`; the
//!   `type-only` warning test")
//! - Source: [design § Dependency rules](../../../docs/artifacts/design.md#dependency-rules-the-whole-of-dependency-cruiser-1820)
//!   ("additive; a dependency-cruiser config never sees them"), [dc coverage § Rules](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#rules)
//!   (`to.license`: .NET from the NuGet `.nuspec`)
//! - Decision: [ADR-0008](../../../docs/adr/0008-exit-code-contract.md) (an invalid
//!   configuration is exit 3)
//! - Requirement: [FR-RULE-02](../../../docs/prd.md#fr-rule-02)

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_rulebearing");

type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// A repository with the extraction fixture's `Sample.dll` built from `app/Sample.csproj`, which
/// references `Sample.Core` as a NuGet package whose `.nuspec` in `nuget/` declares GPL-3.0.
fn repository(tag: &str, config_name: &str, config: &str) -> Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("rb-cli-cross-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let files = [
        (config_name, config),
        (
            "app/Sample.csproj",
            r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <TargetFramework>net10.0</TargetFramework>
    <RootNamespace>Sample</RootNamespace>
  </PropertyGroup>
  <ItemGroup>
    <PackageReference Include="Sample.Core" Version="1.0.0" />
  </ItemGroup>
</Project>
"#,
        ),
        (
            "nuget/sample.core/1.0.0/sample.core.nuspec",
            r#"<?xml version="1.0" encoding="utf-8"?>
<package><metadata><id>Sample.Core</id><version>1.0.0</version><license type="expression">GPL-3.0-only</license></metadata></package>
"#,
        ),
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
    let output = dir.join("app/bin/Debug/net10.0");
    std::fs::create_dir_all(&output)?;
    for file in ["Sample.dll", "Sample.pdb"] {
        std::fs::copy(built.join(file), output.join(file))?;
    }
    Ok(dir)
}

struct Run {
    code: Option<i32>,
    json: Value,
    stderr: String,
}

fn cruise(tag: &str, config_name: &str, config: &str) -> Result<Run> {
    let dir = repository(tag, config_name, config)?;
    let output = Command::new(BIN)
        .args([
            "cruise",
            "-T",
            "json",
            "--no-progress",
            "--config",
            config_name,
            ".",
        ])
        .env("NUGET_PACKAGES", dir.join("nuget"))
        .current_dir(&dir)
        .output()?;
    let _ = std::fs::remove_dir_all(&dir);
    Ok(Run {
        code: output.status.code(),
        json: serde_json::from_slice(&output.stdout).unwrap_or(Value::Null),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

/// `(rule, from file name, to)` for every violation.
fn violations(json: &Value) -> Vec<(String, String, String)> {
    json["summary"]["violations"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|v| {
            let text = |key: &str| v[key].as_str().unwrap_or_default().to_owned();
            let from = text("from");
            let file = from.rsplit('/').next().unwrap_or_default().to_owned();
            (
                v["rule"]["name"].as_str().unwrap_or_default().to_owned(),
                file,
                text("to"),
            )
        })
        .collect()
}

const RULES: &str = r#"languages:
  dotnet: {}
rules:
  dependencies:
    forbidden:
      - name: no-gpl-packages
        comment: "adr:0019"
        severity: error
        from: { language: dotnet }
        to: { license: "GPL", dependencyTypes: [package] }
      - name: only-mit-packages
        comment: "adr:0019"
        severity: error
        from: { language: dotnet }
        to: { licenseNot: "^MIT$" }
      - name: orders-assembly-not-to-packages
        comment: "The assembly is the name Sample, the project the file app/Sample.csproj."
        severity: error
        from: { assembly: "^Sample$", project: "^app/Sample\\.csproj$", namespace: "^Sample\\.Orders$" }
        to: { language: dotnet, dependencyKind: body, dependencyTypes: [package] }
      - name: assembly-is-not-the-project-file
        comment: "Must stay silent: no assembly is named after the project file."
        severity: error
        allowEmpty: true
        from: { assembly: "csproj" }
        to: {}
"#;

#[test]
fn dotnet_licences_and_assemblies_reach_the_rules() -> Result<()> {
    let run = cruise("dotnet", "rulebearing.yaml", RULES)?;
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    let found = violations(&run.json);
    let expected: Vec<(String, String, String)> = [
        ("no-gpl-packages", "Order.cs", "Sample.Core"),
        ("only-mit-packages", "Order.cs", "Sample.Core"),
        ("orders-assembly-not-to-packages", "Order.cs", "Sample.Core"),
    ]
    .iter()
    .map(|(r, f, t)| ((*r).to_owned(), (*f).to_owned(), (*t).to_owned()))
    .collect();
    let mut sorted = found.clone();
    sorted.sort();
    assert_eq!(sorted, expected, "{found:?}");
    assert_eq!(run.json["summary"]["error"], 3);
    let package = run.json["modules"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|m| m["source"] == "Sample.Core")
        .cloned()
        .unwrap_or(Value::Null);
    assert_eq!(package["license"], "GPL-3.0-only", "read from the .nuspec");
    assert_eq!(package["dependencyTypes"], serde_json::json!(["package"]));
    Ok(())
}

#[test]
fn a_dependency_cruiser_config_refuses_the_keys() -> Result<()> {
    let config = r#"{ "forbidden": [{ "name": "web-not-infra", "severity": "error",
        "from": { "namespace": "^App\\.Web\\." }, "to": { "path": "^x" } }] }"#;
    let run = cruise("dc", ".dependency-cruiser.json", config)?;
    assert_eq!(run.code, Some(3), "{}", run.stderr);
    assert!(
        run.stderr.contains("`forbidden[0].from.namespace`")
            && run
                .stderr
                .contains("is a Rulebearing addition for native configurations"),
        "{}",
        run.stderr
    );
    let kind =
        r#"{ "forbidden": [{ "name": "k", "from": {}, "to": { "dependencyKind": "inherits" } }] }"#;
    let run = cruise("dc-kind", ".dependency-cruiser.json", kind)?;
    assert_eq!(run.code, Some(3), "{}", run.stderr);
    assert!(
        run.stderr.contains("`forbidden[0].to.dependencyKind`"),
        "{}",
        run.stderr
    );
    Ok(())
}

#[test]
fn type_only_on_a_dotnet_rule_warns() -> Result<()> {
    let config = r#"languages:
  dotnet: {}
rules:
  dependencies:
    forbidden:
      - name: no-type-imports
        comment: "adr:0001"
        severity: error
        from: { language: dotnet }
        to: { dependencyTypes: [type-only] }
        allowEmpty: true
"#;
    let run = cruise("type-only", "rulebearing.yaml", config)?;
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert!(
        run.stderr.contains(
            "warning: rule `no-type-imports`: to.dependencyTypes names `type-only`, but `from.language` limits the rule to .NET"
        ),
        "{}",
        run.stderr
    );
    Ok(())
}
