//! Incremental extraction after real edits: a copy of the code-layer tree is extracted in full,
//! one file is edited, and the incremental run over the earlier extraction must equal a full
//! extraction of the edited tree byte for byte, the file states, the walk and the linked code
//! layer included.
//!
//! - Plan: [Wave 3, Step 16](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#24-steps-for-sub-wave-3d---mode-source-guard---watch-the-2-s-proof)
//!   (`guard --watch` re-checks a saved file through the incremental entry),
//!   [Wave 3, Step 2](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#21-steps-for-sub-wave-3a-cache---affected-diff---exit-code-mode-strict)
//!   (incremental extraction equals full extraction)
//! - Requirement: [NFR-PERF-03](../../../docs/prd.md#nfr-perf-03)
//!
//! The edits cover each way [`rb_extract_ts::incremental::replay`] builds its result: a comment
//! (the earlier code layer stands), a new class and call (the layer is relinked), lines inserted
//! above the imports (every position moves), an import of a file the earlier walk did not reach
//! (read for the first time) and imports removed (files the walk no longer reaches are dropped).
//! Each runs with the walk taken from the earlier extraction and with the inputs walked again,
//! with the code layer on and off, and chained, each edit's incremental result the next one's earlier extraction, as `guard --watch`
//! chains them.

use std::error::Error;
use std::path::{Path, PathBuf};

use rb_extract_ts::{extract_incremental_from, extract_with, prepare};
use rb_model::{ExtractRequest, Extraction, TypeScriptOptions};

fn copy(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn tree(name: &str) -> Result<PathBuf, Box<dyn Error>> {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/codelayer");
    let dir = std::env::temp_dir().join(format!("rb-incremental-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    copy(&fixture, &dir)?;
    // Canonical, as the module sources the resolver writes are.
    Ok(rb_extract_ts::resolve::simplified(&dir.canonicalize()?))
}

fn full(cwd: &Path, roots: &[PathBuf], code_layer: bool) -> Result<Extraction, Box<dyn Error>> {
    let (mut settings, config) = prepare(&TypeScriptOptions::default(), cwd)?;
    settings.code_layer = code_layer;
    settings.keep_file_states = true;
    settings.keep_walk = true;
    Ok(extract_with(roots, &settings, &config)?)
}

fn incremental(
    cwd: &Path,
    roots: &[PathBuf],
    previous: Extraction,
    changed: &str,
    (walk_unchanged, code_layer): (bool, bool),
) -> Result<Extraction, Box<dyn Error>> {
    let (mut settings, config) = prepare(&TypeScriptOptions::default(), cwd)?;
    settings.code_layer = code_layer;
    settings.keep_file_states = true;
    settings.keep_walk = true;
    let unchanged = previous
        .files
        .keys()
        .filter(|f| *f != changed)
        .map(PathBuf::from)
        .collect();
    let request = ExtractRequest {
        changed: vec![PathBuf::from(changed)],
        unchanged,
        previous,
        walk_unchanged,
    };
    Ok(extract_incremental_from(
        roots, &settings, &config, request,
    )?)
}

const MAIN: &str = "src/main.ts";

/// Each edit of `src/main.ts`, whole.
fn edits() -> Vec<(&'static str, String)> {
    let original = "import { Widget } from './ui/widget';\nimport { Store } from './ui/store';\nimport './legacy/plugin';\n\nexport const start = (): Widget => new Widget('main', new Store());\n";
    vec![
        ("a comment", format!("{original}// edited\n")),
        (
            "a class and a call",
            format!(
                "{original}export class Runner extends Widget {{\n  go(): void {{ this.render(); new Store().save(); }}\n}}\n"
            ),
        ),
        (
            "lines above the imports",
            format!("\n\n// header\n{original}"),
        ),
        (
            "an import of a file not reached before",
            format!(
                "import {{ Base }} from './model/base';\n{original}export const b: Base | undefined = undefined;\n"
            ),
        ),
        (
            "imports removed",
            "export const start = (): number => 1;\n".to_owned(),
        ),
        ("the original back", original.to_owned()),
    ]
}

fn compare(roots: &[PathBuf], name: &str) -> Result<(), Box<dyn Error>> {
    for (walk_unchanged, code_layer) in [(true, true), (false, true), (true, false)] {
        let how = (walk_unchanged, code_layer);
        let cwd = tree(&format!("{name}-{walk_unchanged}-{code_layer}"))?;
        let mut chained = full(&cwd, roots, code_layer)?;
        assert!(chained.walk.is_some() && !chained.files.is_empty());
        for (what, text) in edits() {
            let before = full(&cwd, roots, code_layer)?;
            std::fs::write(cwd.join(MAIN), &text)?;
            let expected = serde_json::to_string(&full(&cwd, roots, code_layer)?)?;
            let alone = incremental(&cwd, roots, before, MAIN, how)?;
            assert_eq!(serde_json::to_string(&alone)?, expected, "{what}, {how:?}");
            chained = incremental(&cwd, roots, chained, MAIN, how)?;
            assert_eq!(
                serde_json::to_string(&chained)?,
                expected,
                "{what}, chained, {how:?}"
            );
        }
        let _ = std::fs::remove_dir_all(&cwd);
    }
    Ok(())
}

#[test]
fn an_edit_under_a_walked_folder_equals_a_full_extraction() -> Result<(), Box<dyn Error>> {
    compare(&[PathBuf::from("src")], "folder")
}

#[test]
fn an_edit_of_the_one_initial_file_equals_a_full_extraction() -> Result<(), Box<dyn Error>> {
    compare(&[PathBuf::from(MAIN)], "file")
}

/// The walk kept is what walking the inputs finds: the initial sources in order and every folder
/// listed, and a request that promises it unchanged starts from it without listing a folder, so a
/// file added without the promise being withdrawn is not seen (the caller's guarantee is load
/// bearing, which `guard --watch` meets by watching the folders).
#[test]
fn the_kept_walk_is_the_walk_and_is_taken_as_promised() -> Result<(), Box<dyn Error>> {
    let cwd = tree("walk")?;
    let roots = [PathBuf::from("src")];
    let first = full(&cwd, &roots, true)?;
    let walk = first.walk.clone().ok_or("no walk kept")?;
    assert_eq!(
        walk.sources,
        [
            "src/legacy/plugin.js",
            "src/main.ts",
            "src/model/base.ts",
            "src/model/decorators.ts",
            "src/model/index.ts",
            "src/ui/store.ts",
            "src/ui/widget.tsx"
        ]
    );
    let folders: Vec<String> = walk
        .folders
        .iter()
        .map(|f| {
            f.strip_prefix(&cwd)
                .unwrap_or(f)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    assert_eq!(folders, ["src", "src/legacy", "src/model", "src/ui"]);
    std::fs::write(cwd.join("src/added.ts"), "export const added = 1;\n")?;
    let promised = incremental(&cwd, &roots, first.clone(), MAIN, (true, true))?;
    assert!(!promised.modules.iter().any(|m| m.source == "src/added.ts"));
    let walked = incremental(&cwd, &roots, first, MAIN, (false, true))?;
    assert!(walked.modules.iter().any(|m| m.source == "src/added.ts"));
    assert_eq!(
        serde_json::to_string(&walked)?,
        serde_json::to_string(&full(&cwd, &roots, true)?)?
    );
    // Without `keep_walk` nothing is kept.
    let (settings, config) = prepare(&TypeScriptOptions::default(), &cwd)?;
    assert!(extract_with(&roots, &settings, &config)?.walk.is_none());
    let _ = std::fs::remove_dir_all(&cwd);
    Ok(())
}

/// Under `maxDepth` a file met at the limit was not read, so when an edit brings it nearer it is
/// read in full: every file is read, as [`rb_extract_ts::reuse_refused`] says, and the result is
/// the full extraction's.
#[test]
fn under_max_depth_a_file_brought_nearer_is_read_in_full() -> Result<(), Box<dyn Error>> {
    let dir = std::env::temp_dir().join(format!("rb-incremental-depth-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src"))?;
    for (file, text) in [
        ("a", "import { x } from './x';\nexport const a = x;\n"),
        ("x", "import { b } from './b';\nexport const x = b;\n"),
        ("b", "import { c } from './c';\nexport const b = c;\n"),
        ("c", "export const c = 1;\n"),
    ] {
        std::fs::write(dir.join(format!("src/{file}.ts")), text)?;
    }
    let cwd = rb_extract_ts::resolve::simplified(&dir.canonicalize()?);
    let options: TypeScriptOptions = serde_json::from_str(r#"{"maxDepth": 2}"#)?;
    let roots = [PathBuf::from("src/a.ts")];
    let run = || -> Result<_, Box<dyn Error>> {
        let (mut settings, config) = prepare(&options, &cwd)?;
        settings.keep_file_states = true;
        Ok((settings, config))
    };
    let (settings, config) = run()?;
    assert!(rb_extract_ts::reuse_refused(&settings).is_some_and(|r| r.contains("maxDepth")));
    let before = extract_with(&roots, &settings, &config)?;
    // `src/b.ts` was met at the limit: no dependencies.
    let at_limit = before.modules.iter().find(|m| m.source == "src/b.ts");
    assert!(at_limit.is_some_and(|m| m.dependencies.is_empty()));
    std::fs::write(
        cwd.join("src/a.ts"),
        "import { b } from './b';\nexport const a = b;\n",
    )?;
    let (settings, config) = run()?;
    let expected = serde_json::to_string(&extract_with(&roots, &settings, &config)?)?;
    let unchanged = before
        .files
        .keys()
        .filter(|f| *f != "src/a.ts")
        .map(PathBuf::from)
        .collect();
    let request = ExtractRequest {
        changed: vec![PathBuf::from("src/a.ts")],
        unchanged,
        previous: before,
        walk_unchanged: false,
    };
    let (settings, config) = run()?;
    let again = extract_incremental_from(&roots, &settings, &config, request)?;
    assert_eq!(serde_json::to_string(&again)?, expected);
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}
