//! `place --imports a,b --imported-by c --language LANG`.
//!
//! - Plan: [Wave 2, Step 12](../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#212-step-12-agent-subcommands-2g)
//! - Source: [design § Questions an agent can ask](../../../../docs/artifacts/design.md#questions-an-agent-can-ask-before-it-writes-the-import)

use crate::common::{
    Result, clean, code, json, run, scratch, stderr, stdout, test_assembly, tree, write,
};

#[test]
fn place_lists_the_folders_where_the_edges_are_legal() -> Result {
    let dir = tree("place")?;
    let output = run(
        &dir,
        &["place", "--imports", "src/web/view.ts", "--language", "ts"],
    )?;
    assert_eq!(code(&output), Some(0), "{}", stderr(&output));
    // Importing the web layer is illegal only under src/domain/.
    assert_eq!(stdout(&output), "./\nsrc/\nsrc/web/\n");
    let detail = json(&run(
        &dir,
        &[
            "place",
            "--imports",
            "src/web/view.ts",
            "--language",
            "typescript",
            "--json",
        ],
    )?)?;
    assert_eq!(detail["module"], "new-module.ts");
    assert_eq!(
        detail["illegal"],
        serde_json::json!([{ "folder": "src/domain/", "rules": ["domain-not-to-web"] }])
    );

    // Imported by the domain, a module under src/web/ is a new domain-to-web edge.
    let imported = run(
        &dir,
        &[
            "place",
            "--imported-by",
            "src/domain/model.ts",
            "--language",
            "ts",
        ],
    )?;
    assert_eq!(stdout(&imported), "./\nsrc/\nsrc/domain/\n");

    let unknown = run(
        &dir,
        &["place", "--imports", "src/nowhere.ts", "--language", "ts"],
    )?;
    assert_eq!(code(&unknown), Some(2));
    let stranger = run(
        &dir,
        &[
            "place",
            "--imported-by",
            "src/nowhere.ts",
            "--language",
            "ts",
        ],
    )?;
    assert_eq!(code(&stranger), Some(2));
    let language = run(&dir, &["place", "--language", "cobol"])?;
    assert_eq!(code(&language), Some(3));
    clean(&dir);
    Ok(())
}

#[test]
fn place_exits_one_when_no_folder_takes_the_module() -> Result {
    let dir = tree("place-none")?;
    write(
        &dir,
        "strict.yaml",
        "forbidden:\n  - name: nothing-imports-web\n    severity: error\n    from: {}\n    to: { path: \"^src/web/\" }\n",
    )?;
    let output = run(
        &dir,
        &[
            "place",
            "--config",
            "strict.yaml",
            "--imports",
            "src/web/view.ts",
            "--language",
            "ts",
        ],
    )?;
    assert_eq!(code(&output), Some(1));
    assert!(stdout(&output).is_empty());
    assert!(stderr(&output).contains("no folder"));
    clean(&dir);
    Ok(())
}

#[test]
fn place_over_the_test_assembly_graph() -> Result {
    let dir = scratch("place-assembly")?;
    write(
        &dir,
        "rulebearing.yaml",
        "forbidden:\n  - name: domain-not-to-slices\n    severity: error\n    from: { path: \"^TestAssembly/Domain/\" }\n    to: { path: \"^TestAssembly/Slices/\" }\n  - name: no-cycles\n    severity: error\n    from: {}\n    to: { circular: true }\n",
    )?;
    let graph = test_assembly();
    let args = [
        "place",
        "--graph",
        graph.as_str(),
        "--language",
        "dotnet",
        "--imports",
        "TestAssembly/Slices/Slice1/Slice1Class.cs",
        "--json",
    ];
    let value = json(&run(&dir, &args)?)?;
    assert_eq!(value["module"], "NewModule.cs");
    let legal: Vec<&str> = value["legal"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .collect();
    assert!(legal.contains(&"TestAssembly/") && legal.contains(&"TestAssembly/Slices/"));
    assert!(
        !legal.iter().any(|f| f.starts_with("TestAssembly/Domain")),
        "{legal:?}"
    );
    // Imported by Slice3Class, the new module closes Slice1 -> Slice2 -> Slice3 -> new -> Slice1.
    let cycle = run(
        &dir,
        &[
            &args[..7],
            &["--imported-by", "TestAssembly/Slices/Slice3/Slice3Class.cs"],
        ]
        .concat(),
    )?;
    assert_eq!(code(&cycle), Some(1), "{}", stdout(&cycle));
    clean(&dir);
    Ok(())
}
