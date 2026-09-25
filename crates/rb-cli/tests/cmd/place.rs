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

/// The reviewer's case: a reachability rule whose violation runs from the importer to the far
/// target, naming neither end the new module. The new module in any folder would let the UI
/// reach the database, so no folder is legal; without the importer every folder but the UI's is.
#[test]
fn place_counts_a_reachability_violation_the_new_module_creates() -> Result {
    let dir = scratch("place-reach")?;
    write(
        &dir,
        "rulebearing.yaml",
        "forbidden:
  - name: ui-never-reaches-db
    severity: error
    comment: The UI reaches data through services only.
    from: { path: \"^src/ui/\" }
    to: { path: \"^src/db/\", reachable: true }
",
    )?;
    write(&dir, "src/ui/view.ts", "export const v = 1;\n")?;
    write(&dir, "src/db/query.ts", "export const q = 1;\n")?;
    write(&dir, "src/svc/api.ts", "export const a = 1;\n")?;
    let args = [
        "place",
        "--imports",
        "src/db/query.ts",
        "--imported-by",
        "src/ui/view.ts",
        "--language",
        "ts",
        "--json",
    ];
    let output = run(&dir, &args)?;
    let value = json(&output)?;
    assert_eq!(code(&output), Some(1), "{value}");
    assert_eq!(value["legal"], serde_json::json!([]));
    let illegal = value["illegal"].as_array().cloned().unwrap_or_default();
    assert!(!illegal.is_empty());
    for folder in &illegal {
        assert_eq!(
            folder["rules"],
            serde_json::json!(["ui-never-reaches-db"]),
            "{folder}"
        );
    }
    let alone = run(
        &dir,
        &["place", "--imports", "src/db/query.ts", "--language", "ts"],
    )?;
    assert_eq!(code(&alone), Some(0), "{}", stderr(&alone));
    // Under src/ui/ the new module itself is UI importing the database.
    assert_eq!(stdout(&alone), "./\nsrc/\nsrc/db/\nsrc/svc/\n");
    clean(&dir);
    Ok(())
}

/// A violation the graph already has is not the new module's doing: a folder stays legal.
#[test]
fn place_leaves_out_what_the_graph_already_breaks() -> Result {
    let dir = tree("place-base")?;
    // The fixture's one violation (domain -> web) is there before any new module.
    let output = run(
        &dir,
        &["place", "--imports", "src/main.ts", "--language", "ts"],
    )?;
    assert_eq!(code(&output), Some(0), "{}", stderr(&output));
    assert!(
        stdout(&output).contains("src/domain/"),
        "{}",
        stdout(&output)
    );
    clean(&dir);
    Ok(())
}

/// The new module's edge is a static ES import: a rule on `require`, `dynamic` or `type-only`
/// that another edge to the same target matches does not rule a folder out.
#[test]
fn place_does_not_copy_how_another_edge_imports_the_target() -> Result {
    let dir = scratch("place-form")?;
    write(
        &dir,
        "rulebearing.yaml",
        "forbidden:
  - name: no-dynamic
    severity: error
    from: {}
    to: { dynamic: true }
  - name: no-type-only
    severity: error
    from: {}
    to: { dependencyTypes: [type-only] }
",
    )?;
    write(
        &dir,
        "src/a.ts",
        "export const lazy = import(\"./b\");\nimport type { T } from \"./c\";\nexport type U = T;\n",
    )?;
    write(&dir, "src/b.ts", "export const b = 1;\n")?;
    write(&dir, "src/c.ts", "export type T = number;\n")?;
    // The graph itself breaks both rules, through src/a.ts.
    let output = run(
        &dir,
        &[
            "place",
            "--imports",
            "src/b.ts,src/c.ts",
            "--language",
            "ts",
            "--json",
        ],
    )?;
    let value = json(&output)?;
    assert_eq!(code(&output), Some(0), "{value}");
    assert_eq!(value["illegal"], serde_json::json!([]), "{value}");
    clean(&dir);
    Ok(())
}
