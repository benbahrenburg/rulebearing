//! The `plantuml` reporter end to end: the round trip (`--output-type plantuml`, then an
//! `adhereTo` diagram rule over the file it wrote, which must report nothing) over every
//! committed .NET graph, `--from` on `cruise` and `fmt`, and the refusals.
//!
//! - Plan: [Wave 3, Step 9](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#22-steps-for-sub-wave-3b-the-remaining-reporters-and-the-sidecar)
//!   (the round trip is the reporter's acceptance test, § 1.3 row FR-OUT-02)
//! - Coverage: [`ArchUnitNET` § `PlantUML`](../../../docs/artifacts/archunitnet-0.13.4-coverage.md#plantuml)
//! - Decision: [ADR-0009](../../../docs/adr/0009-conformance-suites-as-specification.md)
//! - Requirements: [FR-OUT-02](../../../docs/prd.md#fr-out-02), [FR-RULE-05](../../../docs/prd.md#fr-rule-05)
//!
//! The nightly oracle harness (`testbeds/oracles/dotnet.sh`) runs the same round trip over each
//! .NET oracle's own graph.

use std::collections::BTreeMap;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_rulebearing");

fn conformance() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance")
}

fn run(dir: &Path, args: &[&str]) -> Result<Output, Box<dyn Error>> {
    Ok(Command::new(BIN)
        .args(args)
        .current_dir(dir)
        .env("NO_COLOR", "1")
        .output()?)
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Every committed .NET graph of both halves of gate 2.
fn graphs() -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut found = Vec::new();
    for suite in ["archunitnet", "netarchtest"] {
        for entry in std::fs::read_dir(conformance().join(suite).join("graphs"))? {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == "json") {
                found.push(path);
            }
        }
    }
    found.sort();
    Ok(found)
}

/// The first namespace segment most of the graph's loaded types share: the slice pattern's
/// prefix (`<segment>.(*)`).
fn top_segment(graph: &Path) -> Result<String, Box<dyn Error>> {
    let document: Value = serde_json::from_str(&std::fs::read_to_string(graph)?)?;
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for ty in document["code"]["types"].as_array().into_iter().flatten() {
        if ty["referenced"] == Value::Bool(true) {
            continue;
        }
        let namespace = ty["namespace"].as_str().unwrap_or_default();
        let segment = namespace.split('.').next().unwrap_or_default();
        *counts.entry(segment.to_owned()).or_default() += 1;
    }
    Ok(counts
        .into_iter()
        .max_by_key(|(_, n)| *n)
        .map(|(s, _)| s)
        .unwrap_or_default())
}

/// The namespaces a diagram's components hold, as one pattern: the alternation of their
/// stereotypes, each of which matches the namespaces of its component and no other namespace
/// the graph has, so the rule selects exactly the types the diagram describes.
fn described(diagram: &str) -> Result<String, Box<dyn Error>> {
    let parsed = rb_rules::plantuml::parse(diagram)?;
    let alternatives: Vec<String> = parsed
        .components
        .iter()
        .flat_map(|c| c.stereotypes.iter())
        .map(|s| format!("(?:{s})"))
        .collect();
    Ok(alternatives.join("|"))
}

/// Writes the diagram of `graph` with `generate` as the configuration.
fn generate(
    dir: &Path,
    graph: &Path,
    generate: &str,
    from: &str,
) -> Result<String, Box<dyn Error>> {
    std::fs::write(dir.join("generate.yaml"), generate)?;
    let graph = graph.to_string_lossy();
    let out = run(
        dir,
        &[
            "cruise",
            "--config",
            "generate.yaml",
            "--graph",
            &graph,
            "-T",
            "plantuml",
            "--from",
            from,
            "-f",
            "diagram.puml",
        ],
    )?;
    assert_eq!(
        out.status.code(),
        Some(0),
        "{graph} {from}: {}",
        text(&out.stderr)
    );
    Ok(std::fs::read_to_string(dir.join("diagram.puml"))?)
}

/// Cruises `graph` with one diagram rule over `diagram.puml` for the types in the namespaces
/// `select` matches: the violations it reports.
fn enforce(dir: &Path, graph: &Path, select: &str) -> Result<Vec<Value>, Box<dyn Error>> {
    let enforce = format!(
        "rules:\n  diagrams:\n    - name: adheres-to-the-generated-diagram\n      comment: \"The diagram the reporter wrote. adr:0009\"\n      severity: error\n      select: {{ kind: type, where: {{ resideInNamespaceMatching: {} }} }}\n      adhereTo: diagram.puml\n",
        serde_json::to_string(select)?
    );
    std::fs::write(dir.join("enforce.yaml"), enforce)?;
    let graph = graph.to_string_lossy();
    let out = run(
        dir,
        &[
            "cruise",
            "--config",
            "enforce.yaml",
            "--graph",
            &graph,
            "-T",
            "json",
        ],
    )?;
    assert_eq!(out.status.code(), Some(0), "{graph}: {}", text(&out.stderr));
    let result: Value = serde_json::from_slice(&out.stdout)?;
    Ok(result["summary"]["violations"]
        .as_array()
        .cloned()
        .unwrap_or_default())
}

fn assert_adhered(violations: &[Value], graph: &Path, form: &str) {
    assert!(
        violations.is_empty(),
        "{} {form}: {} violations, the first {}",
        graph.display(),
        violations.len(),
        serde_json::to_string_pretty(&violations.first()).unwrap_or_default()
    );
}

#[test]
fn the_generated_diagram_is_adhered_to_on_every_committed_dotnet_graph()
-> Result<(), Box<dyn Error>> {
    let dir = std::env::temp_dir().join(format!("rb-cli-plantuml-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    let mut drawn = 0;
    for graph in graphs()? {
        let top = top_segment(&graph)?;
        let slices =
            format!("options:\n  reporterOptions:\n    plantuml:\n      Matching: \"{top}.(*)\"\n");
        for (form, config) in [("namespaces", String::new()), ("slices", slices.clone())] {
            let diagram = generate(&dir, &graph, &config, form)?;
            assert!(
                diagram.starts_with("@startuml\n\nhide stereotype\n\n"),
                "{diagram}"
            );
            let select = described(&diagram)?;
            assert_adhered(&enforce(&dir, &graph, &select)?, &graph, form);
            drawn += diagram.lines().filter(|l| l.contains(" --> ")).count();
            if form == "slices" {
                // The components IncludeDependenciesToOther adds are targets only: the types of
                // the slices adhere to the fuller diagram as well.
                let other = format!("{slices}      IncludeDependenciesToOther: true\n");
                let fuller = generate(&dir, &graph, &other, form)?;
                assert!(fuller.len() >= diagram.len());
                assert_adhered(&enforce(&dir, &graph, &select)?, &graph, "slices, to other");
            }
        }
    }
    assert!(
        drawn > 50,
        "the diagrams draw arrows ({drawn}), so the rules are not vacuous"
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn a_drawn_arrow_that_is_removed_is_a_violation() -> Result<(), Box<dyn Error>> {
    let dir = std::env::temp_dir().join(format!("rb-cli-plantuml-cut-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    let graph = conformance().join("archunitnet/graphs/TestAssembly.json");
    let diagram = generate(&dir, &graph, "", "namespaces")?;
    let select = described(&diagram)?;
    assert_adhered(&enforce(&dir, &graph, &select)?, &graph, "namespaces");
    // Drop every arrow: each type with a dependency on another component now fails.
    let cut: String = diagram
        .split_inclusive('\n')
        .filter(|l| !l.contains(" --> "))
        .collect();
    std::fs::write(dir.join("diagram.puml"), cut)?;
    let violations = enforce(&dir, &graph, &select)?;
    assert!(violations.len() > 5, "{violations:?}");
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn from_on_fmt_and_the_refusals() -> Result<(), Box<dyn Error>> {
    let dir = std::env::temp_dir().join(format!("rb-cli-plantuml-fmt-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    let graph = conformance().join("archunitnet/graphs/TestAssembly.json");
    let graph = graph.to_string_lossy();
    let fmt = |extra: &[&str]| run(&dir, &[&["fmt", &graph, "-T", "plantuml"], extra].concat());
    let types = fmt(&["--from", "types"])?;
    assert_eq!(types.status.code(), Some(0), "{}", text(&types.stderr));
    let types = text(&types.stdout);
    assert!(
        types.starts_with(rb_rules::plantuml_export::HEADER),
        "from types is ArchUnitNET's text: {types}"
    );
    assert!(types.contains("class \"TestAssembly.Slices.Slice1.Slice1Class\" {\n}\n"));
    assert!(types.contains(
        "[TestAssembly.Slices.Slice1.Slice1Class] --|> [TestAssembly.Slices.Slice2.Slice2Class]\n"
    ));
    // Without --from, a graph of .NET types is drawn by namespace; the output is deterministic.
    let first = fmt(&[])?;
    let second = fmt(&[])?;
    assert_eq!(first.stdout, second.stdout);
    assert!(
        text(&first.stdout).contains(
            "[TestAssembly.Slices.Slice1] <<^TestAssembly\\.Slices\\.Slice1(?:\\.(?:[^\\x2ES]"
        ),
        "{}",
        text(&first.stdout)
    );
    // The provenance values of fmt's --from still work, and give the default drawing.
    let provenance = fmt(&["--from", "rulebearing"])?;
    assert_eq!(provenance.stdout, first.stdout);
    for (args, message) in [
        (&["--from", "classes"][..], "--from `classes`"),
        (&["--from", "slices"][..], "Matching"),
    ] {
        let out = fmt(args)?;
        // An option that cannot be honoured is an invalid configuration (ADR-0008).
        assert_eq!(out.status.code(), Some(3), "{args:?}");
        assert!(text(&out.stderr).contains(message), "{}", text(&out.stderr));
    }
    let bad = run(
        &dir,
        &[
            "cruise", "--graph", &graph, "-T", "plantuml", "--from", "classes",
        ],
    )?;
    assert_eq!(
        bad.status.code(),
        Some(3),
        "an unknown --from is a usage error"
    );
    assert!(
        text(&bad.stderr).contains("namespaces"),
        "{}",
        text(&bad.stderr)
    );
    std::fs::write(
        dir.join("c4.yaml"),
        "options:\n  reporterOptions:\n    plantuml:\n      C4Style: true\n      Typo: 1\n",
    )?;
    let out = run(
        &dir,
        &[
            "cruise", "--config", "c4.yaml", "--graph", &graph, "-T", "plantuml",
        ],
    )?;
    assert_eq!(out.status.code(), Some(3));
    assert!(
        text(&out.stderr).contains("unknown key `Typo`"),
        "{}",
        text(&out.stderr)
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}
