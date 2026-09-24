//! `impact FILE [--depth N]`, as text and as `--json`.
//!
//! - Plan: [Wave 2, Step 12](../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#212-step-12-agent-subcommands-2g)
//! - Source: [design § Questions an agent can ask](../../../../docs/artifacts/design.md#questions-an-agent-can-ask-before-it-writes-the-import)

use crate::common::{
    Result, clean, code, json, run, scratch, stderr, stdout, test_assembly, tree, write,
};

#[test]
fn impact_prints_text_by_default_and_json_on_request() -> Result {
    let dir = tree("impact")?;
    let output = run(&dir, &["impact", "src/web/view.ts", "--depth", "2"])?;
    assert_eq!(code(&output), Some(0), "{}", stderr(&output));
    assert_eq!(
        stdout(&output),
        "src/web/view.ts
  rules that mention it:
    domain-not-to-web (to, error): Move the shared type into src/domain
  dependents (depth 2):
    src/domain/model.ts
    src/main.ts
  on a cycle: no
  ratchets its edges count toward:
    domain-web-edges: 1 of 1
"
    );
    let value = json(&run(&dir, &["impact", "src/domain/model.ts", "--json"])?)?;
    assert_eq!(value["file"], "src/domain/model.ts");
    assert_eq!(value["rules"][0]["side"], "from");
    assert_eq!(value["dependents"], serde_json::json!(["src/main.ts"]));
    assert_eq!(value["onCycle"], false);
    let unknown = stdout(&run(&dir, &["impact", "src/new.ts"])?);
    assert!(
        unknown.starts_with("src/new.ts (not in the graph yet)\n  rules that mention it: none\n"),
        "{unknown}"
    );
    clean(&dir);
    Ok(())
}

#[test]
fn impact_over_the_test_assembly_graph_sees_the_cycle() -> Result {
    let dir = scratch("impact-assembly")?;
    write(
        &dir,
        "rulebearing.yaml",
        "forbidden:\n  - name: no-cycles\n    severity: error\n    from: {}\n    to: { circular: true }\n",
    )?;
    let value = json(&run(
        &dir,
        &[
            "impact",
            "--graph",
            &test_assembly(),
            "TestAssembly/Slices/Slice3/Slice3Class.cs",
            "--json",
        ],
    )?)?;
    assert_eq!(value["known"], true);
    assert_eq!(value["onCycle"], true);
    assert_eq!(
        value["dependents"],
        serde_json::json!([
            "TestAssembly/Slices/Slice2/Service/Service2Class.cs",
            "TestAssembly/Slices/Slice2/Slice2Class.cs",
            "TestAssembly/Slices/Slice3/Group1/Group1Class.cs"
        ])
    );
    let text = stdout(&run(
        &dir,
        &[
            "impact",
            "--graph",
            &test_assembly(),
            "TestAssembly/Slices/Slice3/Slice3Class.cs",
        ],
    )?);
    assert!(
        text.contains("  on a cycle: yes\n") && text.contains("no-cycles (from, error)"),
        "{text}"
    );
    clean(&dir);
    Ok(())
}
