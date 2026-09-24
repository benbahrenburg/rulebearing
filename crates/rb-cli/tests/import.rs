//! `rulebearing import archunit | import-linter | eslint` over the fixtures under
//! `tests/fixtures/import/`: each output byte-compared with its `expected.yaml`, repeated for
//! determinism, loaded by the configuration loader, and, for import-linter, run against the
//! fixture tree it came from.
//!
//! - Plan: [Wave 2, Step 11](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#211-step-11-the-three-importers-and-oracle-agreement-2f)
//!   ("importer fixtures per contract kind and per fluent form ..., each with the expected YAML")
//! - Source: [design § import-linter contracts](../../../docs/artifacts/design.md#import-linter-contracts-for-the-python-teams-who-know-them)
//! - Decision: [ADR-0006](../../../docs/adr/0006-embedded-quickjs-config-evaluator.md) (the
//!   `ESLint` configuration stays in the sandbox)
//! - Requirement: [FR-CLI-04](../../../docs/prd.md#fr-cli-04)
//!
//! Regenerate the expected files deliberately with `RB_UPDATE_SNAPSHOTS=1 cargo test -p rb-cli
//! --test import`; the diff is the review. `tests/fixtures/import/README.md` says where each input
//! came from and under which licence.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_rulebearing");

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/import")
}

fn run(dir: &Path, args: &[&str]) -> Result<Output, Box<dyn Error>> {
    Ok(Command::new(BIN).args(args).current_dir(dir).output()?)
}

/// A fresh temporary folder.
fn temp(tag: &str) -> Result<PathBuf, Box<dyn Error>> {
    let dir = std::env::temp_dir().join(format!("rb-cli-import-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Copies a fixture folder, dropping the `.fixture` suffix that keeps an input out of the
/// repository's own linters.
fn copy_tree(from: &Path, to: &Path) -> Result<(), Box<dyn Error>> {
    for entry in std::fs::read_dir(from)? {
        let path = entry?.path();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let target = to.join(name.strip_suffix(".fixture").unwrap_or(&name));
        if path.is_dir() {
            std::fs::create_dir_all(&target)?;
            copy_tree(&path, &target)?;
        } else if name != "expected.yaml" {
            std::fs::copy(&path, &target)?;
        }
    }
    Ok(())
}

/// Runs an import twice in `dir`, checks the two outputs agree, and compares with (or, under
/// `RB_UPDATE_SNAPSHOTS`, writes) `expected`.
fn snapshot(dir: &Path, args: &[&str], expected: &Path) -> Result<String, Box<dyn Error>> {
    let first = run(dir, args)?;
    assert_eq!(
        first.status.code(),
        Some(0),
        "{args:?} in {}: {}",
        dir.display(),
        String::from_utf8_lossy(&first.stderr)
    );
    let second = run(dir, args)?;
    assert_eq!(first.stdout, second.stdout, "{args:?}: two runs differ");
    let text = String::from_utf8(first.stdout)?;
    if std::env::var_os("RB_UPDATE_SNAPSHOTS").is_some() {
        std::fs::write(expected, &text)?;
        return Ok(text);
    }
    let want = std::fs::read_to_string(expected)?;
    assert_eq!(
        text,
        want,
        "{} changed; if that was intended, regenerate with RB_UPDATE_SNAPSHOTS=1",
        expected.display()
    );
    Ok(text)
}

#[test]
fn import_linter_fixtures_match_their_expected_yaml() -> Result<(), Box<dyn Error>> {
    for case in [
        "forbidden",
        "layers",
        "independence",
        "protected",
        "acyclic",
        "own",
    ] {
        let dir = fixtures().join("import-linter").join(case);
        let text = snapshot(
            &dir,
            &["import", "import-linter"],
            &dir.join("expected.yaml"),
        )?;
        assert!(
            text.starts_with("# Imported from "),
            "{case}: a header names the source"
        );
    }
    Ok(())
}

#[test]
fn import_linter_reproduces_the_hand_translation_of_its_own_contracts() -> Result<(), Box<dyn Error>>
{
    let dir = fixtures().join("import-linter/own");
    let output = run(
        &dir,
        &["import", "import-linter", "--from", ".importlinter"],
    )?;
    assert_eq!(output.status.code(), Some(0));
    let imported: Value = serde_yaml::from_slice(&output.stdout)?;
    let oracle: Value = serde_yaml::from_str(&std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testbeds/oracles/configs/seddonym__import-linter.yaml"),
    )?)?;
    assert_eq!(imported["languages"], oracle["languages"]);
    assert_eq!(
        imported["rules"], oracle["rules"],
        "the import must reproduce testbeds/oracles/configs/seddonym__import-linter.yaml"
    );
    Ok(())
}

/// A violation as `(rule, from, to)`.
type Violation = (String, String, String);

/// Every error violation of a cruise.
fn errors(dir: &Path, paths: &[&str]) -> Result<Vec<Violation>, Box<dyn Error>> {
    let mut args = vec!["cruise", "-T", "json", "--no-progress"];
    args.extend_from_slice(paths);
    let output = run(dir, &args)?;
    let result: Value = serde_json::from_slice(&output.stdout).map_err(|e| {
        format!(
            "{e}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })?;
    let mut found: Vec<Violation> = result["summary"]["violations"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|v| v["rule"]["severity"] == "error")
        .map(|v| {
            (
                v["rule"]["name"].as_str().unwrap_or_default().to_owned(),
                v["from"].as_str().unwrap_or_default().to_owned(),
                v["to"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect();
    found.sort();
    Ok(found)
}

/// Imports a fixture's contracts into a copy of its tree and cruises it.
fn gate(case: &str, paths: &[&str]) -> Result<Vec<Violation>, Box<dyn Error>> {
    let dir = temp(case)?;
    copy_tree(&fixtures().join("import-linter").join(case), &dir)?;
    let output = run(
        &dir,
        &["import", "import-linter", "--out", "rulebearing.yaml"],
    )?;
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let found = errors(&dir, paths)?;
    let _ = std::fs::remove_dir_all(&dir);
    Ok(found)
}

#[test]
fn imported_contracts_gate_the_tree_they_came_from() -> Result<(), Box<dyn Error>> {
    // `forbidden`: history.py reaches payments; checkout.py does too, but ignore_imports excuses
    // it through knownViolations.
    assert_eq!(
        gate("forbidden", &["src"])?,
        [(
            "no-payments".to_owned(),
            "src/shop/orders/history.py".to_owned(),
            "src/shop/payments/refunds.py".to_owned()
        )]
    );
    // `layers`: billing's domain imports its web layer.
    assert_eq!(
        gate("layers", &["app"])?,
        [(
            "layered:app.billing:domain-to-web".to_owned(),
            "app/billing/domain/__init__.py".to_owned(),
            "app/billing/web/__init__.py".to_owned()
        )]
    );
    // `protected`: the API may import the database; lib/web.py may not.
    let protected = gate("protected", &["lib"])?;
    assert_eq!(protected.len(), 1, "{protected:?}");
    assert_eq!(protected[0].1, "lib/web.py");
    assert_eq!(protected[0].2, "lib/db/__init__.py");
    // `independence`: the one import between features is excused for `features`, whose
    // ignore_imports names it, and breaks `features-direct`, which names no ignore.
    assert_eq!(
        gate("independence", &["pkg"])?,
        [(
            "features-direct".to_owned(),
            "pkg/features/cart/view.py".to_owned(),
            "pkg/features/search/__init__.py".to_owned()
        )]
    );
    Ok(())
}

#[test]
fn archunit_fixtures_match_their_expected_yaml() -> Result<(), Box<dyn Error>> {
    for (case, tests) in [
        ("fluent", "tests/RiverBooks.ArchitectureTests"),
        ("netarchtest", "tests/Shop.ArchitectureTests"),
    ] {
        let dir = fixtures().join("archunit").join(case);
        snapshot(
            &dir,
            &["import", "archunit", tests],
            &dir.join("expected.yaml"),
        )?;
    }
    Ok(())
}

#[test]
fn archunit_writes_to_a_file_and_refuses_a_folder_without_csharp() -> Result<(), Box<dyn Error>> {
    let out = temp("archunit-out")?;
    let target = out.join("nested/rulebearing.yaml");
    let dir = fixtures().join("archunit/fluent");
    let output = run(
        &dir,
        &[
            "import",
            "archunit",
            "tests/RiverBooks.ArchitectureTests",
            "--out",
            target.to_string_lossy().as_ref(),
        ],
    )?;
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    let written = std::fs::read_to_string(&target)?;
    assert_eq!(written, std::fs::read_to_string(dir.join("expected.yaml"))?);
    let empty = run(&out, &["import", "archunit", "nested"])?;
    assert_eq!(empty.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&empty.stderr).contains("holds no .cs file"));
    let missing = run(&out, &["import", "archunit", "no-such-folder"])?;
    assert_eq!(missing.status.code(), Some(3));
    let _ = std::fs::remove_dir_all(&out);
    Ok(())
}

#[test]
fn eslint_fixtures_match_their_expected_yaml() -> Result<(), Box<dyn Error>> {
    for case in ["flat", "legacy", "commonjs", "package"] {
        let source = fixtures().join("eslint").join(case);
        let dir = temp(&format!("eslint-{case}"))?;
        copy_tree(&source, &dir)?;
        snapshot(&dir, &["import", "eslint"], &source.join("expected.yaml"))?;
        let _ = std::fs::remove_dir_all(&dir);
    }
    Ok(())
}

#[test]
fn the_eslint_sandbox_still_refuses_node_built_ins() -> Result<(), Box<dyn Error>> {
    let dir = temp("eslint-escape")?;
    for (file, text) in [
        (
            "eslint.config.mjs",
            "import fs from 'fs';\nexport default [{ rules: { x: fs.readFileSync('/etc/passwd', 'utf8') } }];\n",
        ),
        (
            "escape.cjs",
            "const { execSync } = require('child_process');\nmodule.exports = { rules: { x: execSync('id').toString() } };\n",
        ),
        (
            "process.mjs",
            "export default [{ rules: { x: typeof process === 'undefined' ? 'none' : process.env.HOME } }];\n",
        ),
        (
            "node.mjs",
            "import os from 'node:os';\nexport default [{ rules: { x: os.homedir() } }];\n",
        ),
    ] {
        std::fs::write(dir.join(file), text)?;
    }
    for file in ["eslint.config.mjs", "escape.cjs", "node.mjs"] {
        let output = run(&dir, &["import", "eslint", "--from", file])?;
        assert_eq!(output.status.code(), Some(3), "{file} must be refused");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("not available in the configuration sandbox"),
            "{file}: {stderr}"
        );
    }
    // `process` does not exist inside the sandbox, so nothing of the host leaks into the output.
    let output = run(&dir, &["import", "eslint", "--from", "process.mjs"])?;
    assert_eq!(output.status.code(), Some(0));
    assert!(
        !String::from_utf8_lossy(&output.stdout)
            .contains(&std::env::var("HOME").unwrap_or_else(|_| "/".into()))
    );
    let missing = run(&dir, &["import", "eslint", "--from", "absent.js"])?;
    assert_eq!(missing.status.code(), Some(3));
    let none = temp("eslint-none")?;
    let nothing = run(&none, &["import", "eslint"])?;
    assert_eq!(nothing.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&nothing.stderr).contains("--from"));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&none);
    Ok(())
}

#[test]
fn an_eslint_configuration_that_loops_is_stopped() -> Result<(), Box<dyn Error>> {
    let dir = temp("eslint-loop")?;
    std::fs::write(
        dir.join("eslint.config.js"),
        "module.exports = (() => { for (;;) {} })();\n",
    )?;
    let output = run(&dir, &["import", "eslint"])?;
    assert_eq!(output.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&output.stderr).contains("limit"));
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn import_linter_needs_settings() -> Result<(), Box<dyn Error>> {
    let dir = temp("il-none")?;
    std::fs::write(dir.join("setup.cfg"), "[metadata]\nname = x\n")?;
    let output = run(&dir, &["import", "import-linter"])?;
    assert_eq!(output.status.code(), Some(3));
    let named = run(&dir, &["import", "import-linter", "--from", "setup.cfg"])?;
    assert_eq!(named.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&named.stderr).contains("no [importlinter]"));
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn imported_eslint_rules_gate_a_typescript_tree() -> Result<(), Box<dyn Error>> {
    let dir = temp("eslint-gate")?;
    copy_tree(&fixtures().join("eslint/flat"), &dir)?;
    for (file, text) in [
        (
            "src/client/app.ts",
            "import { b } from '../server/api';\nimport { c } from '../server/shared/types';\nexport const a = b + c;\n",
        ),
        ("src/server/api.ts", "export const b = 1;\n"),
        ("src/server/shared/types.ts", "export const c = 2;\n"),
        ("src/app/main/index.ts", "export const main = 3;\n"),
        (
            "src/features/cart/index.ts",
            "import { main } from '../../app/main';\nexport const cart = main;\n",
        ),
        (
            "src/features/search/index.ts",
            "import { cart } from '../cart';\nexport const search = cart;\n",
        ),
    ] {
        let path = dir.join(file);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, text)?;
    }
    let output = run(&dir, &["import", "eslint", "--out", "rulebearing.yaml"])?;
    assert_eq!(output.status.code(), Some(0));
    let found = errors(&dir, &["src"])?;
    let _ = std::fs::remove_dir_all(&dir);
    let names: Vec<(&str, &str, &str)> = found
        .iter()
        .map(|(r, f, t)| (r.as_str(), f.as_str(), t.as_str()))
        .collect();
    assert_eq!(
        names,
        [
            (
                "boundaries:feature",
                "src/features/cart/index.ts",
                "src/app/main/index.ts"
            ),
            (
                "boundaries:feature-to-feature",
                "src/features/search/index.ts",
                "src/features/cart/index.ts"
            ),
            (
                "no-restricted-paths:1",
                "src/client/app.ts",
                "src/server/api.ts"
            ),
            (
                "no-restricted-paths:2",
                "src/features/cart/index.ts",
                "src/app/main/index.ts"
            ),
        ]
    );
    Ok(())
}
