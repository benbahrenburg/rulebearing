//! The fixture package against its committed expectation, byte for byte.
//!
//! - Plan: [Wave 2, Step 4](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#24-step-4-the-python-extractor-2b)
//!   (the fixture package, the expectation JSON, the byte-identity repeated run)
//! - Quality attribute: [plan § 1.8](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#18-quality-attributes)
//!   (reliability: a repeated-run byte-identity test per extractor; observability: the receipt)
//!
//! `tests/fixtures/pkg/` is a `src/` layout project: absolute and relative imports, `__all__`,
//! the three `TYPE_CHECKING` spellings, literal dynamic imports, a namespace package, a stub, a
//! standard-library import that the 3.12 snapshot no longer has, a third-party import resolved
//! through a committed fake `.venv`, unresolved imports, a file that does not parse, files under
//! no root, and the code-layer file `src/app/shapes.py`. The expectation is
//! `tests/fixtures/pkg.expected.json`; regenerate it deliberately with
//! `RB_UPDATE_SNAPSHOTS=1 cargo test -p rb-extract-python` and review the diff.

use std::error::Error;
use std::path::{Path, PathBuf};

use rb_extract_python::{PythonExtractor, extract_at};
use rb_model::{Extraction, Extractor, PythonOptions};
use serde_json::{Value, json};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pkg")
}

fn expectation_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pkg.expected.json")
}

/// The extraction as the expectation records it.
fn serialise(extraction: &Extraction) -> Result<String, serde_json::Error> {
    let warnings: Vec<Value> = extraction
        .warnings
        .iter()
        .map(|w| {
            json!({
                "path": w.path.as_ref().map(|p| p.to_string_lossy().replace('\\', "/")),
                "message": w.message,
            })
        })
        .collect();
    let document = json!({
        "modules": extraction.modules,
        "code": extraction.code,
        "inspected": extraction.inspected,
        "warnings": warnings,
    });
    Ok(format!("{}\n", serde_json::to_string_pretty(&document)?))
}

fn run(base: &Path, options: &PythonOptions) -> Result<Extraction, Box<dyn Error>> {
    Ok(extract_at(base, &[], options, None)?)
}

#[test]
fn the_fixture_matches_its_expectation() -> Result<(), Box<dyn Error>> {
    let actual = serialise(&run(&fixture(), &PythonOptions::default())?)?;
    if std::env::var_os("RB_UPDATE_SNAPSHOTS").is_some() {
        std::fs::write(expectation_path(), &actual)?;
        return Ok(());
    }
    // Compared as JSON: a workspace build turns on serde_json's `preserve_order` (through
    // `oxc_resolver`), which changes key order but not content.
    let expected: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(expectation_path())?)?;
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&actual)?,
        expected,
        "the extraction changed; if that was intended, regenerate with RB_UPDATE_SNAPSHOTS=1"
    );
    Ok(())
}

#[test]
fn two_runs_serialise_byte_for_byte() -> Result<(), Box<dyn Error>> {
    let first = serialise(&run(&fixture(), &PythonOptions::default())?)?;
    let second = serialise(&run(&fixture(), &PythonOptions::default())?)?;
    assert_eq!(first, second);
    Ok(())
}

#[test]
fn stubs_are_modules_only_when_asked() -> Result<(), Box<dyn Error>> {
    let has_stub = |e: &Extraction| e.modules.iter().any(|m| m.source == "src/app/typed.pyi");
    let without = run(&fixture(), &PythonOptions::default())?;
    assert!(!has_stub(&without));
    let options = PythonOptions {
        stubs: Some(true),
        ..PythonOptions::default()
    };
    let with = run(&fixture(), &options)?;
    assert!(has_stub(&with));
    let core = with.modules.iter().find(|m| m.source == "src/app/core.py");
    assert!(core.is_some_and(|m| {
        m.dependencies
            .iter()
            .any(|d| d.module == "app.typed" && d.resolved == "src/app/typed.pyi")
    }));
    Ok(())
}

#[test]
fn configured_roots_and_version_win() -> Result<(), Box<dyn Error>> {
    let options = PythonOptions {
        version: Some("3.11".to_owned()),
        roots: Some(vec!["src".to_owned(), ".".to_owned()]),
        stubs: None,
    };
    let extraction = run(&fixture(), &options)?;
    assert_eq!(extraction.inspected.stdlib_version.as_deref(), Some("3.11"));
    assert_eq!(
        extraction.inspected.roots,
        Some(vec!["src".to_owned(), ".".to_owned()])
    );
    // distutils is standard library in 3.11 and not in 3.12.
    let core = extraction
        .modules
        .iter()
        .find(|m| m.source == "src/app/core.py");
    assert!(core.is_some_and(|m| {
        m.dependencies
            .iter()
            .any(|d| d.module == "distutils" && d.core_module)
    }));
    let unsupported = PythonOptions {
        version: Some("3.7".to_owned()),
        ..PythonOptions::default()
    };
    let error = extract_at(&fixture(), &[], &unsupported, None)
        .err()
        .map(|e| e.to_string());
    assert!(error.is_some_and(|e| e.contains("languages.python.version")));
    Ok(())
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rb-py-fixture-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::create_dir_all(&dir);
    dir
}

#[test]
fn without_an_environment_the_receipt_says_none() -> Result<(), Box<dyn Error>> {
    let dir = scratch("no-site");
    std::fs::write(dir.join("main.py"), "import requests\nimport os\n")?;
    let extraction = run(&dir, &PythonOptions::default())?;
    assert_eq!(extraction.inspected.site.as_deref(), Some("none"));
    assert_eq!(extraction.inspected.stdlib_version.as_deref(), Some("3.13"));
    assert_eq!(extraction.inspected.roots, Some(vec![".".to_owned()]));
    let main = extraction.modules.iter().find(|m| m.source == "main.py");
    let requests = main.and_then(|m| m.dependencies.iter().find(|d| d.module == "requests"));
    assert!(requests.is_some_and(|d| d.could_not_resolve && !d.followable));
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn virtual_env_is_read_when_given() -> Result<(), Box<dyn Error>> {
    let dir = scratch("virtual-env");
    std::fs::write(dir.join("main.py"), "import fancylib\n")?;
    let venv = fixture().join(".venv");
    let extraction = extract_at(&dir, &[], &PythonOptions::default(), Some(&venv))?;
    let site = extraction.inspected.site.unwrap_or_default();
    assert!(
        site.ends_with(".venv/lib/python3.12/site-packages"),
        "{site}"
    );
    let main = extraction.modules.iter().find(|m| m.source == "main.py");
    let fancy = main.and_then(|m| m.dependencies.first());
    assert_eq!(fancy.and_then(|d| d.license.as_deref()), Some("MIT"));
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn empty_inputs_and_bad_projects_are_errors() {
    let dir = scratch("empty");
    let _ = std::fs::write(dir.join("README.md"), "no python here");
    let empty = run(&dir, &PythonOptions::default())
        .err()
        .map(|e| e.to_string());
    assert!(empty.is_some_and(|e| e.contains("no modules found")));
    let _ = std::fs::write(dir.join("pyproject.toml"), "[project\n");
    let _ = std::fs::write(dir.join("a.py"), "");
    let broken = run(&dir, &PythonOptions::default())
        .err()
        .map(|e| e.to_string());
    assert!(broken.is_some_and(|e| e.contains("pyproject.toml") && e.contains("not valid TOML")));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_extractor_trait_reads_the_working_directory() -> Result<(), Box<dyn Error>> {
    // Cargo runs integration tests in the package folder.
    let inputs = [PathBuf::from("tests/fixtures/pkg/src/app/sub")];
    let extraction = PythonExtractor.extract(&inputs, &PythonOptions::default())?;
    let sources: Vec<&str> = extraction
        .modules
        .iter()
        .filter(|m| m.language.is_some())
        .map(|m| m.source.as_str())
        .collect();
    assert!(
        sources.contains(&"tests/fixtures/pkg/src/app/sub/deep.py"),
        "{sources:?}"
    );
    Ok(())
}

fn write(dir: &Path, file: &str, text: &str) -> std::io::Result<()> {
    let path = dir.join(file);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, text)
}

#[test]
fn a_stub_beside_its_module_adds_no_code_layer() -> Result<(), Box<dyn Error>> {
    let dir = scratch("stub-beside");
    write(&dir, "pkg/__init__.py", "")?;
    write(&dir, "pkg/a.py", "class Foo: ...\n")?;
    write(&dir, "pkg/a.pyi", "class Foo: ...\n")?;
    write(&dir, "pkg/only.pyi", "class Bar: ...\n")?;
    let options = PythonOptions {
        stubs: Some(true),
        ..PythonOptions::default()
    };
    let extraction = run(&dir, &options)?;
    let code = extraction.code.unwrap_or_default();
    let foos: Vec<Option<&str>> = code
        .types
        .iter()
        .filter(|t| t.full_name == "pkg.a.Foo")
        .map(|t| t.location.file.as_deref())
        .collect();
    assert_eq!(foos, [Some("pkg/a.py")]);
    assert!(code.types.iter().any(|t| t.full_name == "pkg.only.Bar"));
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn a_deeply_nested_file_is_a_warning_and_the_run_goes_on() -> Result<(), Box<dyn Error>> {
    let dir = scratch("deep");
    write(
        &dir,
        "deep.py",
        &format!("x = 1{}\n", " + 1".repeat(200_000)),
    )?;
    write(&dir, "fine.py", "import os\n")?;
    let extraction = run(&dir, &PythonOptions::default())?;
    let warned: Vec<(Option<String>, bool)> = extraction
        .warnings
        .iter()
        .map(|w| {
            (
                w.path.as_ref().map(|p| p.to_string_lossy().into_owned()),
                w.message.contains("could not be analysed"),
            )
        })
        .collect();
    assert_eq!(warned, [(Some("deep.py".to_owned()), true)]);
    let fine = extraction.modules.iter().find(|m| m.source == "fine.py");
    assert!(fine.is_some_and(|m| m.dependencies.len() == 1));
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn a_namespace_two_distributions_share_names_neither() -> Result<(), Box<dyn Error>> {
    let dir = scratch("shared-namespace");
    let site = "venv/lib/python3.13/site-packages";
    write(
        &dir,
        &format!("{site}/google_auth-2.0.dist-info/top_level.txt"),
        "google\n",
    )?;
    write(
        &dir,
        &format!("{site}/google_auth-2.0.dist-info/METADATA"),
        "Name: google-auth\nLicense: Apache-2.0\n",
    )?;
    write(
        &dir,
        &format!("{site}/protobuf-4.0.dist-info/top_level.txt"),
        "google\n",
    )?;
    write(
        &dir,
        &format!("{site}/protobuf-4.0.dist-info/RECORD"),
        "google/protobuf/__init__.py,,\n",
    )?;
    write(
        &dir,
        &format!("{site}/protobuf-4.0.dist-info/METADATA"),
        "Name: protobuf\nLicense: BSD-3-Clause\n",
    )?;
    write(&dir, "a.py", "import google.protobuf\nimport google.auth\n")?;
    let extraction = extract_at(
        &dir,
        &[],
        &PythonOptions::default(),
        Some(&dir.join("venv")),
    )?;
    let a = extraction.modules.iter().find(|m| m.source == "a.py");
    let licence = |module: &str| {
        a.and_then(|m| m.dependencies.iter().find(|d| d.module == module))
            .map(|d| d.license.clone())
    };
    assert_eq!(
        licence("google.protobuf"),
        Some(Some("BSD-3-Clause".to_owned()))
    );
    assert_eq!(licence("google.auth"), Some(None));
    assert!(
        extraction
            .warnings
            .iter()
            .any(|w| w.message.contains("google.auth")
                && w.message.contains("google-auth, protobuf"))
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}
