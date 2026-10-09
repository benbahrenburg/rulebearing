//! Every reporter outside `rb_report::READS_CODE` renders a result the same with or without its
//! code layer, so the command line may hand it one without: the list cannot leave out a reporter
//! that reads the layer.
//!
//! - Plan: [Wave 3, 3G](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#wave-3g-the-rule-library-the-scale-table-adoption-action-5)
//!   (peak memory on compiled .NET graphs)
//! - Requirement: [NFR-PERF-03](../../../docs/prd.md#nfr-perf-03)

use std::error::Error;
use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

type Result<T = ()> = std::result::Result<T, Box<dyn Error>>;

const BIN: &str = env!("CARGO_BIN_EXE_rulebearing");

/// Rules over the extractor's sample fixture that give dependency and element violations, so the
/// result carries a code layer and violations that point into it.
const CONFIG: &str = "languages:
  dotnet:
    assemblies: [built/Sample.dll, built/Sample.Core.dll]
rules:
  dependencies:
    forbidden:
      - name: customers-not-to-orders
        comment: \"plan:rulebearing-wave-3\"
        severity: error
        from: { path: Customer }
        to: { path: Order }
  elements:
    - name: classes-are-sealed
      comment: \"plan:rulebearing-wave-3\"
      fix: \"Seal the class.\"
      select: { kind: class }
      should: { beSealed: true }
";

/// The sample fixture's cruise result, as the json reporter prints it.
fn result() -> Result<Value> {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../rb-extract-dotnet/tests/fixtures/sample")
        .canonicalize()?;
    let dir = std::env::temp_dir().join(format!("rb-cli-without-code-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    for folder in ["built", "src", "core"] {
        std::fs::create_dir_all(dir.join(folder))?;
        for entry in std::fs::read_dir(fixture.join(folder))? {
            let entry = entry?;
            std::fs::copy(entry.path(), dir.join(folder).join(entry.file_name()))?;
        }
    }
    std::fs::write(dir.join("rulebearing.yaml"), CONFIG)?;
    let output = Command::new(BIN)
        .args([
            "cruise",
            "-T",
            "json",
            "--no-progress",
            "--no-liveness",
            ".",
        ])
        .current_dir(&dir)
        .output()?;
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        output.status.code().is_some_and(|c| c < 2),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(serde_json::from_slice(&output.stdout)?)
}

#[test]
fn a_reporter_that_does_not_read_the_code_layer_renders_the_same_without_it() -> Result {
    let with = result()?;
    assert!(
        with.get("code").and_then(|c| c.get("types")).is_some(),
        "the fixture's result carries a code layer"
    );
    assert!(
        with["summary"]["violations"]
            .as_array()
            .is_some_and(|v| v.iter().any(|v| v["type"] == "element")),
        "and an element violation that points into it"
    );
    let mut without = with.clone();
    if let Some(map) = without.as_object_mut() {
        map.remove("code");
    }
    let options = rb_report::ReportOptions::default();
    let mut compared = 0;
    for (output_type, _) in rb_report::OUTPUT_TYPES {
        if rb_report::reads_code(output_type) {
            continue;
        }
        let full = rb_report::render(output_type, &with, &options).map(|r| (r.output, r.exit_code));
        let lean =
            rb_report::render(output_type, &without, &options).map(|r| (r.output, r.exit_code));
        assert_eq!(
            format!("{full:?}"),
            format!("{lean:?}"),
            "{output_type} reads the code layer: add it to rb_report::READS_CODE"
        );
        compared += 1;
    }
    assert!(compared >= 15, "compared {compared} reporters");
    Ok(())
}
