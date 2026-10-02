//! The sidecar inside the extractor: the walk hands each CoffeeScript file it reaches to
//! dependency-cruiser and continues from its answer, and incremental extraction re-runs the
//! sidecar for a changed file only, with the full extraction's result.
//!
//! - Plan: [Wave 3, Step 10](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#22-steps-for-sub-wave-3b-the-remaining-reporters-and-the-sidecar),
//!   [Step 2](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#21-steps-for-sub-wave-3a-cache---affected-diff---exit-code-mode-strict)
//!   (incremental equals full)
//! - Decision: [ADR-0017](../../../docs/adr/0017-coffeescript-livescript-sidecar.md)
//! - Requirement: [FR-EXT-TS-05](../../../docs/prd.md#fr-ext-ts-05)
//!
//! The dependency-cruiser here is a stand-in written into the scratch folder's `node_modules`:
//! dependency-cruiser's `package.json` shape, an `allExtensions` export, and a `depcruise` command
//! that reads `# dep <specifier>` lines and logs each call. The tests need Node; without it they
//! print why and pass.

use std::path::{Path, PathBuf};
use std::process::Command;

use rb_extract_ts::{extract_incremental, extract_with, prepare};
use rb_model::{ExtractError, ExtractRequest, Extraction, SidecarRuntime, TypeScriptOptions};
use serde_json::Value;

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

const TREE: &[(&str, &str)] = &[
    (
        "src/index.js",
        "import legacy from \"./legacy.coffee\";\nexport default legacy;\n",
    ),
    (
        "src/legacy.coffee",
        "# dep ./helper.js\n# dep ./more.coffee\nmodule.exports = 1\n",
    ),
    ("src/more.coffee", "# dep ./util.js\n"),
    (
        "src/helper.js",
        "import util from \"./util.js\";\nexport default util;\n",
    ),
    ("src/util.js", "export default 3;\n"),
    (
        "node_modules/dependency-cruiser/package.json",
        "{\"name\": \"dependency-cruiser\", \"version\": \"18.2.0\", \"bin\": {\"depcruise\": \"bin/depcruise.mjs\"}, \"exports\": {\".\": {\"import\": \"./index.mjs\"}}}\n",
    ),
    (
        "node_modules/dependency-cruiser/index.mjs",
        "export const allExtensions = [{ extension: \".coffee\", available: true }];\n",
    ),
    ("node_modules/dependency-cruiser/bin/depcruise.mjs", COMMAND),
];

const COMMAND: &str = r##"import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
const args = process.argv.slice(2);
// Every file follows `--`, as the sidecar passes them; none is taken from before it.
const files = args.includes("--")
  ? args.slice(args.indexOf("--") + 1).map((f) => path.posix.normalize(f))
  : [];
// As a file path on every platform: a URL's pathname is `/C:/...` on Windows.
const here = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
fs.appendFileSync(path.join(here, "calls.log"), JSON.stringify(files) + "\n");
const modules = new Map();
const visit = (file) => {
  if (modules.has(file) || !file.endsWith(".coffee")) return;
  const dependencies = fs.readFileSync(file, "utf8").split("\n")
    .filter((l) => l.startsWith("# dep ")).map((l) => l.slice(6).trim())
    .map((spec) => ({ module: spec, moduleSystem: "cjs", dynamic: false, exoticallyRequired: false,
      dependencyTypes: ["local", "require"], resolved: path.posix.join(path.posix.dirname(file), spec),
      coreModule: false, followable: true, couldNotResolve: false, matchesDoNotFollow: false,
      circular: false, valid: true }));
  modules.set(file, { source: file, dependencies, valid: true });
  dependencies.forEach((d) => visit(d.resolved));
};
files.forEach(visit);
process.stdout.write(JSON.stringify({ modules: [...modules.values()], summary: {} }));
"##;

fn tree(name: &str) -> Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("rb-ts-sc-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (file, text) in TREE {
        let path = dir.join(file);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, text)?;
    }
    Ok(rb_model::without_verbatim(&dir.canonicalize()?))
}

fn calls(dir: &Path) -> Vec<Value> {
    std::fs::read_to_string(dir.join("node_modules/dependency-cruiser/calls.log"))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

fn node_available() -> bool {
    Command::new("node")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn sidecar_on() -> TypeScriptOptions {
    TypeScriptOptions {
        sidecar: Some(SidecarRuntime::Node),
        ..TypeScriptOptions::default()
    }
}

fn extract(dir: &Path, options: &TypeScriptOptions, keep: bool) -> Result<Extraction> {
    let (mut settings, config) = prepare(options, dir)?;
    settings.keep_file_states = keep;
    Ok(extract_with(
        &[PathBuf::from("src/index.js")],
        &settings,
        &config,
    )?)
}

fn text(extraction: &Extraction) -> String {
    let mut extraction = extraction.clone();
    extraction.files.clear();
    serde_json::to_string(&extraction).unwrap_or_default()
}

#[test]
fn the_walk_continues_from_the_sidecar_s_answers() -> Result {
    if !node_available() {
        println!("skipped: no node on the path");
        return Ok(());
    }
    let dir = tree("walk")?;
    let full = extract(&dir, &sidecar_on(), false)?;
    let sources: Vec<&str> = full.modules.iter().map(|m| m.source.as_str()).collect();
    assert_eq!(
        sources,
        [
            "src/index.js",
            "src/legacy.coffee",
            "src/helper.js",
            "src/util.js",
            "src/more.coffee"
        ],
        "the walk's depth-first order, each file once"
    );
    for module in &full.modules {
        let by_sidecar = module.source.ends_with(".coffee");
        for dependency in &module.dependencies {
            assert_eq!(
                dependency.sidecar,
                by_sidecar.then_some(true),
                "{}",
                module.source
            );
            assert_eq!(dependency.line.is_none(), by_sidecar, "{}", module.source);
        }
    }
    let receipt = full.sidecar.as_ref();
    assert_eq!(
        receipt.map(|r| (r.version.as_str(), r.files)),
        Some(("18.2.0", 2))
    );
    assert!(full.warnings.is_empty(), "{:?}", full.warnings);
    // One spawn: the stand-in follows legacy.coffee to more.coffee, whose answer is taken too.
    assert_eq!(calls(&dir), [serde_json::json!(["src/legacy.coffee"])]);
    let again = extract(&dir, &sidecar_on(), false)?;
    assert_eq!(text(&again), text(&full), "deterministic");
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn incremental_extraction_re_runs_the_sidecar_for_a_changed_file_only() -> Result {
    if !node_available() {
        println!("skipped: no node on the path");
        return Ok(());
    }
    let dir = tree("incremental")?;
    let full = extract(&dir, &sidecar_on(), true)?;
    assert_eq!(
        full.files.len(),
        5,
        "every file read keeps its state, the sidecar's too"
    );
    let spawned = calls(&dir).len();
    let read: Vec<PathBuf> = full.files.keys().map(PathBuf::from).collect();
    let incremental = |changed: Vec<PathBuf>| -> Result<Extraction> {
        let (settings, config) = prepare(&sidecar_on(), &dir)?;
        let unchanged = read
            .iter()
            .filter(|f| !changed.contains(f))
            .cloned()
            .collect();
        let request = ExtractRequest {
            changed,
            unchanged,
            previous: full.clone(),
            walk_unchanged: false,
        };
        Ok(extract_incremental(
            &[PathBuf::from("src/index.js")],
            &settings,
            &config,
            &request,
        )?)
    };
    assert_eq!(text(&incremental(Vec::new())?), text(&full));
    assert_eq!(calls(&dir).len(), spawned, "nothing changed: no spawn");
    for file in &read {
        assert_eq!(
            text(&incremental(vec![file.clone()])?),
            text(&full),
            "{file:?}"
        );
    }
    let made = calls(&dir);
    assert_eq!(
        made[spawned..],
        [
            serde_json::json!(["src/legacy.coffee"]),
            serde_json::json!(["src/more.coffee"])
        ],
        "each CoffeeScript file changed on its own goes to the sidecar alone; a JavaScript one never"
    );
    // The receipt's version is kept when nothing is spawned, and read from the installed package
    // when the earlier extraction did not record it.
    let mut unrecorded = full.clone();
    unrecorded.sidecar = None;
    let (settings, config) = prepare(&sidecar_on(), &dir)?;
    let request = ExtractRequest {
        changed: Vec::new(),
        unchanged: read.clone(),
        previous: unrecorded,
        walk_unchanged: false,
    };
    let recovered = extract_incremental(
        &[PathBuf::from("src/index.js")],
        &settings,
        &config,
        &request,
    )?;
    assert_eq!(recovered.sidecar, full.sidecar);
    // Without the sidecar, an earlier sidecar result is never reused: the file stops the run.
    let (settings, config) = prepare(&TypeScriptOptions::default(), &dir)?;
    let request = ExtractRequest {
        changed: Vec::new(),
        unchanged: read,
        previous: full.clone(),
        walk_unchanged: false,
    };
    let refused = extract_incremental(
        &[PathBuf::from("src/index.js")],
        &settings,
        &config,
        &request,
    );
    let Err(ExtractError::UnsupportedFile { path, reason }) = refused else {
        unreachable!("{refused:?}");
    };
    assert_eq!(path, PathBuf::from("src/legacy.coffee"));
    assert!(
        reason.starts_with("unsupported-file-needs-sidecar"),
        "{reason}"
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn a_file_the_sidecar_does_not_answer_is_named() -> Result {
    if !node_available() {
        println!("skipped: no node on the path");
        return Ok(());
    }
    let dir = tree("unanswered")?;
    // The stand-in answers `.coffee` files only.
    std::fs::write(dir.join("src/other.litcoffee"), "    x = 1\n")?;
    let (settings, config) = prepare(&sidecar_on(), &dir)?;
    let result = extract_with(&[PathBuf::from("src/other.litcoffee")], &settings, &config);
    let Err(ExtractError::UnsupportedFile { path, reason }) = result else {
        unreachable!("{result:?}");
    };
    assert_eq!(path, PathBuf::from("src/other.litcoffee"));
    assert!(
        reason.contains("--sidecar node: dependency-cruiser did not extract src/other.litcoffee"),
        "{reason}"
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}
