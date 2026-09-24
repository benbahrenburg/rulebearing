//! `test --generate [RULE] [--force]`.
//!
//! - Plan: [Wave 2, Step 12](../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#212-step-12-agent-subcommands-2g)
//! - Source: [design § The agentic engineering hat](../../../../docs/artifacts/design.md#the-agentic-engineering-hat-turn-two)

use crate::common::{
    CONFIG, Result, clean, code, read, run, scratch, stderr, stdout, test_assembly, tree, write,
};

#[test]
fn generate_writes_examples_that_test_then_proves() -> Result {
    let dir = tree("generate")?;
    let output = run(&dir, &["test", "--generate", "domain-not-to-web"])?;
    assert_eq!(code(&output), Some(0), "{}", stderr(&output));
    assert_eq!(
        stdout(&output),
        "domain-not-to-web: 1 forbidden, 1 allowed\nwrote rulebearing.yaml; run `rulebearing test` to prove them\n"
    );
    let config = read(&dir, "rulebearing.yaml")?;
    assert!(config.starts_with("# The fixture's rules (a hand-written comment the edits keep).\n"));
    assert!(config.contains(
        "    to: { path: \"^src/web/\" }\n    examples:\n      forbidden: [\"src/domain/model.ts -> src/web/view.ts\"]\n      allowed: [\"src/main.ts -> src/domain/model.ts\"]\nrules:\n"
    ), "{config}");
    assert!(
        config.contains("rules:\n  ratchets:"),
        "the rest of the file is untouched"
    );
    let tested = run(&dir, &["test"])?;
    assert_eq!(code(&tested), Some(0), "{}", stdout(&tested));
    assert!(stdout(&tested).contains("1 rule(s) with examples, 0 failing"));

    let refused = run(&dir, &["test", "--generate", "domain-not-to-web"])?;
    assert_eq!(code(&refused), Some(3));
    assert!(stderr(&refused).contains("--force"));
    assert_eq!(read(&dir, "rulebearing.yaml")?, config, "nothing written");
    let forced = run(
        &dir,
        &["test", "--generate", "domain-not-to-web", "--force"],
    )?;
    assert_eq!(code(&forced), Some(0), "{}", stderr(&forced));
    assert_eq!(
        read(&dir, "rulebearing.yaml")?,
        config,
        "the same graph, the same examples"
    );

    let all = run(&dir, &["test", "--generate"])?;
    assert!(stdout(&all).contains("domain-not-to-web: kept its examples"));
    let unknown = run(&dir, &["test", "--generate", "no-such-rule"])?;
    assert_eq!(code(&unknown), Some(3));
    let stray = run(&dir, &["test", "--force"])?;
    assert_eq!(code(&stray), Some(3), "--force needs --generate");
    clean(&dir);
    Ok(())
}

#[test]
fn generate_over_the_test_assembly_graph_and_a_cycle_rule() -> Result {
    let dir = scratch("generate-assembly")?;
    write(
        &dir,
        "rulebearing.yaml",
        "forbidden:\n  - name: slice3-not-back-to-slice1\n    severity: error\n    from: { path: \"^TestAssembly/Slices/Slice3/\" }\n    to: { path: \"^TestAssembly/Slices/Slice1/\" }\n  - name: no-cycles\n    severity: error\n    from: {}\n    to: { circular: true }\nallowed:\n  - from: {}\n    to: {}\n",
    )?;
    let graph = test_assembly();
    let output = run(&dir, &["test", "--generate", "--graph", &graph])?;
    assert_eq!(code(&output), Some(0), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(
        text.contains("slice3-not-back-to-slice1: 1 forbidden, 3 allowed"),
        "{text}"
    );
    // A cycle needs more than one edge, so the single-edge proof admits no forbidden example.
    assert!(text.contains("no-cycles: 0 forbidden, 3 allowed"), "{text}");
    assert!(text.contains("  no forbidden example"));
    assert!(
        text.contains("allowed[0]: 0 forbidden, 3 allowed"),
        "{text}"
    );
    let config = read(&dir, "rulebearing.yaml")?;
    assert!(config.contains(
        "      forbidden: [\"TestAssembly/Slices/Slice3/Slice3Class.cs -> TestAssembly/Slices/Slice1/Slice1Class.cs\"]\n"
    ), "{config}");
    let tested = run(&dir, &["test"])?;
    assert_eq!(code(&tested), Some(0), "{}", stdout(&tested));
    assert!(stdout(&tested).contains("3 rule(s) with examples, 0 failing"));
    clean(&dir);
    Ok(())
}

#[test]
fn a_configuration_that_is_not_yaml_gets_the_block_to_paste() -> Result {
    let dir = tree("generate-json")?;
    write(
        &dir,
        "rules.json",
        "{ \"forbidden\": [{ \"name\": \"domain-not-to-web\", \"severity\": \"error\", \"from\": { \"path\": \"^src/domain/\" }, \"to\": { \"path\": \"^src/web/\" } }] }\n",
    )?;
    let before = read(&dir, "rules.json")?;
    let output = run(&dir, &["test", "--generate", "--config", "rules.json"])?;
    assert_eq!(code(&output), Some(3));
    assert!(stdout(&output).contains("# domain-not-to-web\nexamples:\n  forbidden: [\"src/domain/model.ts -> src/web/view.ts\"]"), "{}", stdout(&output));
    assert!(stderr(&output).contains("is not YAML"));
    assert_eq!(read(&dir, "rules.json")?, before);
    assert!(CONFIG.contains("domain-not-to-web"));
    clean(&dir);
    Ok(())
}
