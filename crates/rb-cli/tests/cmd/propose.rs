//! `propose --from/--to`, `propose --select/--where/--should`, `propose --from-example`.
//!
//! - Plan: [Wave 2, Step 12](../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#212-step-12-agent-subcommands-2g)
//! - Source: [design § Rules an agent writes](../../../../docs/artifacts/design.md#rules-an-agent-writes-held-to-the-same-bar)

use crate::common::{
    Result, clean, code, run, scratch, stderr, stdout, test_assembly, tree, write,
};

#[test]
fn from_and_to_globs_draft_a_forbidden_rule_that_loads_and_fires() -> Result {
    let dir = tree("propose-globs")?;
    let output = run(
        &dir,
        &["propose", "--from", "src/domain/**", "--to", "src/web/**"],
    )?;
    assert_eq!(code(&output), Some(0), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.starts_with(
        "# rulebearing propose: `from` matches 1 module, `to` matches 1 dependency; 1 edge would be flagged today\n"
    ), "{text}");
    assert!(text.contains(
        "forbidden:\n  - name: no-src-domain-to-src-web\n    severity: error\n    from: { path: \"^src/domain/\" }\n    to: { path: \"^src/web/\" }\n"
    ));
    assert!(text.contains("#   src/domain/model.ts -> src/web/view.ts\n"));
    assert!(dir.join(".graph/cache").is_dir(), "answered from the cache");
    // The draft is a rule: loaded as a configuration, it flags the edge it said it would.
    write(&dir, "draft.yaml", &text)?;
    let cruise = run(
        &dir,
        &["cruise", "--config", "draft.yaml", "-T", "err", "src"],
    )?;
    assert_eq!(code(&cruise), Some(1), "{}", stdout(&cruise));
    assert!(stdout(&cruise).contains("no-src-domain-to-src-web"));
    clean(&dir);
    Ok(())
}

#[test]
fn from_and_to_over_the_test_assembly_graph() -> Result {
    let dir = scratch("propose-assembly")?;
    write(&dir, "rulebearing.yaml", "forbidden: []\n")?;
    let graph = test_assembly();
    let output = run(
        &dir,
        &[
            "propose",
            "--graph",
            &graph,
            "--from",
            "TestAssembly/Slices/Slice1/**",
            "--to",
            "TestAssembly/Slices/Slice2/**",
            "--name",
            "slice1-not-to-slice2",
        ],
    )?;
    assert_eq!(code(&output), Some(0), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("2 edges would be flagged today"), "{text}");
    assert!(text.contains("  - name: slice1-not-to-slice2\n"));
    assert!(text.contains(
        "#   TestAssembly/Slices/Slice1/Service/Service1Class.cs -> TestAssembly/Slices/Slice2/Service/Service2Class.cs\n"
    ));
    assert!(!dir.join(".graph").exists(), "--graph reads no cache");
    clean(&dir);
    Ok(())
}

#[test]
fn select_and_where_draft_an_element_rule_with_its_selection() -> Result {
    let dir = scratch("propose-select")?;
    write(&dir, "rulebearing.yaml", "forbidden: []\n")?;
    let graph = test_assembly();
    let base = [
        "propose",
        "--graph",
        graph.as_str(),
        "--select",
        "class",
        "--where",
        "{ resideInNamespace: TestAssembly.Domain.Services }",
    ];
    let selection = run(&dir, &base)?;
    assert_eq!(code(&selection), Some(0), "{}", stderr(&selection));
    let text = stdout(&selection);
    assert!(text.starts_with(
        "# rulebearing propose: 3 class objects selected today; sample: TestAssembly.Domain.Services.NotWellNamedService1, TestAssembly.Domain.Services.ServiceClassInWrongNamespaceService, TestAssembly.Domain.Services.TestService\n"
    ), "{text}");
    assert!(
        text.contains(
            "elements:\n  - name: class-resideinnamespace-testassembly-domain-services\n"
        )
    );
    assert!(text.contains("# add `should`"));

    let with_should = run(
        &dir,
        &[&base[..], &["--should", "{ haveNameEndingWith: Service }"]].concat(),
    )?;
    assert_eq!(code(&with_should), Some(0), "{}", stderr(&with_should));
    let text = stdout(&with_should);
    assert!(
        text.contains("    should: {\"haveNameEndingWith\":\"Service\"}\n"),
        "{text}"
    );
    assert!(text.contains(
        "# 1 of 3 fail `should` today: TestAssembly.Domain.Services.NotWellNamedService1\n"
    ));
    // The draft loads as a configuration.
    write(&dir, "draft.yaml", &text)?;
    let listed = run(
        &dir,
        &["rules", "--config", "draft.yaml", "--graph", &graph],
    )?;
    assert_eq!(code(&listed), Some(0), "{}", stderr(&listed));

    let unknown = run(
        &dir,
        &[&base[..5], &["--where", "{ haveNameEndsWith: X }"]].concat(),
    )?;
    assert_eq!(code(&unknown), Some(3));
    assert!(
        stderr(&unknown).contains("haveNameEndingWith"),
        "names the nearest key: {}",
        stderr(&unknown)
    );
    let not_yaml = run(&dir, &[&base[..5], &["--where", "{ a: [ }"]].concat())?;
    assert_eq!(code(&not_yaml), Some(3));
    let bad_kind = run(&dir, &["propose", "--graph", &graph, "--select", "gadget"])?;
    assert_eq!(code(&bad_kind), Some(3));
    clean(&dir);
    Ok(())
}

#[test]
fn from_example_generalises_one_edge_to_the_narrowest_rule() -> Result {
    let dir = tree("propose-example")?;
    write(
        &dir,
        "src/features/cart/ui/button.ts",
        "import { pay } from \"../../billing/api\";\nexport const b = pay;\n",
    )?;
    write(&dir, "src/features/cart/model.ts", "export const m = 1;\n")?;
    write(
        &dir,
        "src/features/billing/api.ts",
        "export const pay = 1;\n",
    )?;
    let output = run(
        &dir,
        &[
            "propose",
            "--from-example",
            "src/features/cart/ui/button.ts -> src/features/billing/api.ts",
        ],
    )?;
    assert_eq!(code(&output), Some(0), "{}", stderr(&output));
    let text = stdout(&output);
    // `ui/` holds only the example, so the `from` side widens one segment, to `cart/`.
    assert!(text.contains(
        "    from: { path: \"^src/features/cart/\" }\n    to: { path: \"^src/features/billing/\" }\n"
    ), "{text}");
    assert!(text.contains("# generalised from src/features/cart/ui/button.ts -> src/features/billing/api.ts, which is in the graph today\n"));
    assert!(text.contains("`from` matches 2 modules"));

    let assembly = run(
        &dir,
        &[
            "propose",
            "--graph",
            &test_assembly(),
            "--from-example",
            "TestAssembly/Slices/Slice1/Service/Service1Class.cs -> TestAssembly/Slices/Slice2/Service/Service2Class.cs",
        ],
    )?;
    let text = stdout(&assembly);
    assert!(text.contains(
        "    from: { path: \"^TestAssembly/Slices/Slice1/\" }\n    to: { path: \"^TestAssembly/Slices/Slice2/Service/\" }\n"
    ), "{text}");

    let malformed = run(&dir, &["propose", "--from-example", "just one file"])?;
    assert_eq!(code(&malformed), Some(3));
    let no_form = run(&dir, &["propose"])?;
    assert_eq!(code(&no_form), Some(3), "one form is required");
    clean(&dir);
    Ok(())
}
