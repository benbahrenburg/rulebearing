//! The playground's `cruise_ts`, `cruise_py` and `explain` against the command line's own output,
//! and their refusals.
//!
//! - Plan: [Wave 4, Step 2](../../../docs/plans/pending/0004-wave-4-reach.md#21-steps-for-sub-wave-4a-docs-site-playground-cookbook-live-tables)
//!   ("the `cruise_ts` and `cruise_py` fixtures")
//! - Decision: [ADR-0073](../../../docs/adr/0073-extractors-read-files-through-rb-model.md)
//! - Requirement: [FR-REACH-01](../../../docs/prd.md#fr-reach-01)
//!
//! Each fixture is a small repository (`repo/`) and what `rulebearing cruise -T <type>` and
//! `rulebearing explain <rule>` print there (`expected/`), which `rb-cli`'s
//! `tests/playground_fixtures.rs` holds the command line to on every run. The playground, handed
//! the same files as a map of paths to text, must print the same bytes.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use rb_wasm::cruise::{Extractor, cruise, file_tree, receipt};
use rb_wasm::playground::{Now, PlaygroundError, explain};

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Every reporter the command line's test compares (it names the four it leaves out).
const COMPARED: &[&str] = &[
    "err",
    "err-long",
    "err-html",
    "json",
    "text",
    "csv",
    "teamcity",
    "azure-devops",
    "agent",
    "null",
    "dot",
    "ddot",
    "archi",
    "cdot",
    "flat",
    "fdot",
    "mermaid",
    "baseline",
    "metrics",
    "junit",
    "trx",
    "html",
    "markdown",
    "plantuml",
];

/// The command line's `SOURCE_DATE_EPOCH` in the fixtures' outputs: 2026-10-10T12:00:00Z.
fn now() -> Now {
    Now {
        today: NaiveDate::from_ymd_opt(2026, 10, 10).unwrap_or_default(),
        timestamp: "2026-10-10T12:00:00.000".to_owned(),
    }
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// Every file under `dir`, by its path from `dir`, as the page hands a dropped folder over.
fn dropped(dir: &Path) -> Result<BTreeMap<String, String>> {
    let mut files = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(folder) = stack.pop() {
        for entry in std::fs::read_dir(&folder)? {
            let path = entry?.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let name = path.strip_prefix(dir)?.to_string_lossy().replace('\\', "/");
                files.insert(name, std::fs::read_to_string(&path)?);
            }
        }
    }
    Ok(files)
}

fn read(path: &Path) -> Result<String> {
    Ok(std::fs::read_to_string(path)?)
}

fn equals_the_command_line(name: &str, extractor: Extractor) -> Result {
    let root = fixture(name);
    let files = dropped(&root.join("repo"))?;
    let config = read(&root.join("repo/rulebearing.yaml"))?;
    for output_type in COMPARED {
        let printed = cruise(extractor, &files, &config, output_type, &now())?;
        let expected = read(&root.join(format!("expected/{output_type}.out")))?;
        assert_eq!(printed, expected, "{name}: {output_type}");
    }
    Ok(())
}

#[test]
fn cruise_ts_prints_what_the_command_line_prints() -> Result {
    equals_the_command_line("typescript", Extractor::TypeScript)
}

#[test]
fn cruise_py_prints_what_the_command_line_prints() -> Result {
    equals_the_command_line("python", Extractor::Python)
}

#[test]
fn explain_prints_what_the_command_line_prints() -> Result {
    for (name, rules) in [
        ("typescript", ["no-circular", "no-unresolvable"]),
        ("python", ["no-circular", "ui-not-to-models"]),
    ] {
        let root = fixture(name);
        let config = read(&root.join("repo/rulebearing.yaml"))?;
        let graph = read(&root.join("expected/json.out"))?;
        for rule in rules {
            let printed = explain(&config, &graph, rule, &now())?;
            let expected = read(&root.join(format!("expected/explain-{rule}.out")))?;
            assert_eq!(printed, expected, "{name}: {rule}");
        }
    }
    Ok(())
}

#[test]
fn the_same_files_give_the_same_bytes() -> Result {
    let root = fixture("typescript");
    let files = dropped(&root.join("repo"))?;
    let config = read(&root.join("repo/rulebearing.yaml"))?;
    let once = cruise(Extractor::TypeScript, &files, &config, "json", &now())?;
    let again = cruise(Extractor::TypeScript, &files, &config, "json", &now())?;
    assert_eq!(once, again);
    Ok(())
}

fn refused(result: std::result::Result<String, PlaygroundError>) -> String {
    result.err().map(|e| e.to_string()).unwrap_or_default()
}

#[test]
fn what_needs_the_disk_or_a_process_is_refused_by_name() {
    let files = BTreeMap::from([("src/a.ts".to_owned(), "import './b';\n".to_owned())]);
    let ts = |config: &str| {
        refused(cruise(
            Extractor::TypeScript,
            &files,
            config,
            "json",
            &now(),
        ))
    };
    let pnp = ts(
        "languages:\n  typescript:\n    externalModuleResolutionStrategy: yarn-pnp\nforbidden: []\n",
    );
    assert!(
        pnp.starts_with("the files could not be extracted: ")
            && pnp.contains("languages.typescript.externalModuleResolutionStrategy")
            && pnp.contains("Yarn Plug'n'Play"),
        "{pnp}"
    );
    let ratchet = ts(
        "rules:\n  ratchets:\n    - name: r\n      from: { path: '^src/' }\n      to: { path: '^lib/' }\n      budget: budgets/r.json\n",
    );
    assert_eq!(
        ratchet,
        "the configuration did not load: ratchet `r` counts against its budget file \
         budgets/r.json, which the playground does not read; run the command line"
    );
    let coffee = BTreeMap::from([("src/a.coffee".to_owned(), "x = 1\n".to_owned())]);
    let needs = refused(cruise(
        Extractor::TypeScript,
        &coffee,
        "forbidden: []\n",
        "json",
        &now(),
    ));
    assert!(needs.contains("unsupported-file-needs-sidecar"), "{needs}");
}

#[test]
fn what_cannot_be_extracted_or_reported_is_named() {
    let empty = BTreeMap::new();
    let none = refused(cruise(
        Extractor::Python,
        &empty,
        "forbidden: []\n",
        "json",
        &now(),
    ));
    assert!(
        none.starts_with("the files could not be extracted: "),
        "{none}"
    );
    let only_text = BTreeMap::from([("README.md".to_owned(), "# x\n".to_owned())]);
    let nothing = refused(cruise(
        Extractor::TypeScript,
        &only_text,
        "forbidden: []\n",
        "err",
        &now(),
    ));
    assert!(
        nothing.starts_with("the files could not be extracted: "),
        "{nothing}"
    );
    let reporter = refused(cruise(
        Extractor::TypeScript,
        &only_text,
        "forbidden: []\n",
        "nope",
        &now(),
    ));
    assert!(reporter.starts_with("no output type `nope`"), "{reporter}");
    let config = refused(cruise(
        Extractor::TypeScript,
        &only_text,
        "extends: ./base.yaml\n",
        "json",
        &now(),
    ));
    assert!(
        config.starts_with("the configuration did not load: "),
        "{config}"
    );
}

#[test]
fn explain_names_a_rule_it_does_not_have_and_a_graph_it_cannot_read() -> Result {
    let root = fixture("typescript");
    let config = read(&root.join("repo/rulebearing.yaml"))?;
    let graph = read(&root.join("expected/json.out"))?;
    let missing = refused(explain(&config, &graph, "nope", &now()));
    assert_eq!(
        missing,
        "no rule `nope`; the rules are no-circular, no-unresolvable, ui-not-from-order, \
         no-dev-deps-in-src"
    );
    let not_a_graph = refused(explain(&config, "[]", "no-circular", &now()));
    assert!(
        not_a_graph.starts_with("the graph is not a cruise result: "),
        "{not_a_graph}"
    );
    let bad_config = refused(explain("extends: ./x.yaml\n", &graph, "x", &now()));
    assert!(bad_config.starts_with("the configuration did not load: "));
    Ok(())
}

#[test]
fn a_dropped_folder_is_a_tree_under_the_root() -> Result {
    let files = BTreeMap::from([
        ("./src/a.ts".to_owned(), "a".to_owned()),
        ("/src/b.ts".to_owned(), "b".to_owned()),
        ("lib\\c.ts".to_owned(), "c".to_owned()),
        ("src/x/../d.ts".to_owned(), "d".to_owned()),
    ]);
    let tree = file_tree(&files)?;
    let paths: Vec<&Path> = tree.paths().collect();
    assert_eq!(
        paths,
        [
            Path::new("/playground/lib/c.ts"),
            Path::new("/playground/src/a.ts"),
            Path::new("/playground/src/b.ts"),
            Path::new("/playground/src/d.ts"),
        ]
    );
    for outside in ["../etc/passwd", "src/../../x.ts", ".", ""] {
        let files = BTreeMap::from([(outside.to_owned(), String::new())]);
        let refused = file_tree(&files).err().map(|e| e.to_string());
        assert_eq!(
            refused,
            Some(format!(
                "the files cannot be read: `{outside}` is not a file inside the dropped folder; \
                 give paths relative to it"
            )),
            "{outside}"
        );
    }
    let clash = BTreeMap::from([
        ("src".to_owned(), String::new()),
        ("src/a.ts".to_owned(), String::new()),
    ]);
    let refused = file_tree(&clash).err().map(|e| e.to_string());
    // The path in the platform's spelling: `\playground\src` on Windows.
    let src = rb_model::files::lexical(&Path::new(rb_wasm::playground::ROOT).join("src"));
    assert_eq!(
        refused,
        Some(format!(
            "the files cannot be read: {}: is both a file and a folder of the file tree",
            src.display()
        ))
    );
    Ok(())
}

#[test]
fn the_receipt_counts_each_language_an_extractor_did_not_count() {
    use rb_model::{GraphDocument, Inspected, Language, Module, Receipt};
    let module = |source: &str, language: Language| Module {
        language: Some(language),
        ..Module::new(source)
    };
    let mut document = GraphDocument {
        modules: vec![
            module("a.ts", Language::Typescript),
            module("b.ts", Language::Typescript),
            module("c.py", Language::Python),
            Module::new("no-language"),
        ],
        ..GraphDocument::default()
    };
    document.summary.inspected = Some(Inspected::from([(
        Language::Python,
        Receipt::counts(7, 0, 7),
    )]));
    let inspected = receipt(&document);
    assert_eq!(inspected.len(), 2);
    let ts = inspected.get(&Language::Typescript);
    assert_eq!(ts.map(|r| (r.files, r.modules)), Some((2, 2)));
    let py = inspected.get(&Language::Python);
    assert_eq!(
        py.map(|r| (r.files, r.modules)),
        Some((7, 7)),
        "the extractor's own receipt is kept"
    );
}
