//! The code layer end to end: the fixture tree under `tests/fixtures/codelayer/` extracted as
//! `TypeScriptExtractor::extract` does, its `code` section compared byte for byte with the
//! reviewed expectation, and the repeated-run and switched-off cases.
//!
//! - Plan: [Wave 2C](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#wave-2c-element-slice-and-diagram-rules-the-capability-table-gate-2-to-zero)
//!   (row "TypeScript and Python mappings over fixture packages");
//!   [§ 1.8](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#18-quality-attributes)
//!   (a repeated-run byte-identity test per extractor)
//! - Decisions: [ADR-0012](../../../docs/adr/0012-oxc-for-typescript.md),
//!   [ADR-0014](../../../docs/adr/0014-no-invented-cross-language-edges.md)
//! - Requirement: [FR-CORE-01](../../../docs/prd.md#fr-core-01)
//!
//! The tree covers classes (abstract, generic, decorated, with access modifiers, `#private`
//! members, accessors, an auto-accessor, parameter properties and a static class expression),
//! interfaces extending interfaces, enums and a `const enum`, type aliases, exported functions and
//! arrow constants, a barrel with `export *`, `export { X as Y } from` and `export * as ns`,
//! cross-file `extends` and `implements`, a `namespace`, a `.tsx` file, an anonymous default
//! export and a CommonJS JavaScript file. `RB_UPDATE_SNAPSHOTS=1 cargo test -p rb-extract-ts
//! --test codelayer` rewrites `tests/fixtures/codelayer.expected.json`; the diff is what a
//! reviewer reads.

use std::error::Error;
use std::path::{Path, PathBuf};

use rb_extract_ts::{extract_with, prepare};
use rb_model::{Extraction, TypeScriptOptions};

fn fixture() -> PathBuf {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/codelayer");
    path.canonicalize().unwrap_or(path)
}

fn expectation() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/codelayer.expected.json")
}

fn run(code_layer: bool) -> Result<Extraction, Box<dyn Error>> {
    let (mut settings, config) = prepare(&TypeScriptOptions::default(), &fixture())?;
    settings.code_layer = code_layer;
    Ok(extract_with(&[PathBuf::from("src")], &settings, &config)?)
}

fn serialise(extraction: &Extraction) -> Result<String, serde_json::Error> {
    let document = serde_json::json!({ "code": extraction.code });
    Ok(format!("{}\n", serde_json::to_string_pretty(&document)?))
}

#[test]
fn the_fixture_matches_its_expectation() -> Result<(), Box<dyn Error>> {
    let actual = serialise(&run(true)?)?;
    if std::env::var_os("RB_UPDATE_SNAPSHOTS").is_some() {
        std::fs::write(expectation(), &actual)?;
        return Ok(());
    }
    let expected = std::fs::read_to_string(expectation())?;
    assert_eq!(
        actual,
        expected.replace("\r\n", "\n"),
        "the code layer changed; if that was intended, regenerate with RB_UPDATE_SNAPSHOTS=1 and \
         review the diff"
    );
    Ok(())
}

#[test]
fn two_runs_serialise_byte_for_byte() -> Result<(), Box<dyn Error>> {
    let first = serialise(&run(true)?)?;
    let second = serialise(&run(true)?)?;
    assert_eq!(first, second);
    Ok(())
}

#[test]
fn every_file_contributes_and_every_element_is_located() -> Result<(), Box<dyn Error>> {
    let extraction = run(true)?;
    let code = extraction.code.ok_or("the code layer is missing")?;
    let files: std::collections::BTreeSet<&str> = code
        .types
        .iter()
        .filter_map(|t| t.location.file.as_deref())
        .collect();
    assert_eq!(
        files.into_iter().collect::<Vec<_>>(),
        [
            "src/legacy/plugin.js",
            "src/main.ts",
            "src/model/base.ts",
            "src/model/decorators.ts",
            "src/ui/store.ts",
            "src/ui/widget.tsx"
        ]
    );
    let located = code
        .types
        .iter()
        .map(|t| &t.location)
        .chain(code.members.iter().map(|m| &m.location))
        .chain(code.attributes.iter().map(|a| &a.location))
        .chain(code.calls.iter().map(|c| &c.location))
        .all(|l| {
            l.file.is_some() && l.line.is_some_and(|n| n > 0) && l.column.is_some_and(|n| n > 0)
        });
    assert!(located);
    // ADR-0014: no property TypeScript cannot express.
    assert!(code.types.iter().all(|t| t.sealed.is_none()
        && t.record.is_none()
        && t.value_type.is_none()
        && t.assembly_qualified_name.is_none()
        && t.attribution.is_none()
        && t.r#static.is_none()));
    Ok(())
}

#[test]
fn the_layer_can_be_switched_off() -> Result<(), Box<dyn Error>> {
    let on = run(true)?;
    let off = run(false)?;
    assert!(off.code.is_none());
    assert_eq!(
        serde_json::to_string(&on.modules)?,
        serde_json::to_string(&off.modules)?,
        "the module layer does not depend on the code layer"
    );
    Ok(())
}
