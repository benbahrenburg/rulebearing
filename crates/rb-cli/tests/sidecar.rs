//! `cruise --sidecar node`: CoffeeScript and LiveScript through the repository's own
//! dependency-cruiser, merged into the walk.
//!
//! - Plan: [Wave 3, Step 10](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#22-steps-for-sub-wave-3b-the-remaining-reporters-and-the-sidecar)
//!   (the missing-flag case; the receipt fields), [§ 1.5](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#15-interfaces-and-contracts-this-wave-freezes)
//!   (`sidecar: true`, `summary.sidecar`, `unsupported-file-needs-sidecar`)
//! - Decisions: [ADR-0017](../../../docs/adr/0017-coffeescript-livescript-sidecar.md),
//!   [ADR-0004](../../../docs/adr/0004-graph-document-is-cruise-result-superset.md) (additive,
//!   stripped by `--strict-schema`), [ADR-0008](../../../docs/adr/0008-exit-code-contract.md)
//! - Requirements: [FR-EXT-TS-05](../../../docs/prd.md#fr-ext-ts-05), [NFR-SEC-01](../../../docs/prd.md#nfr-sec-01),
//!   [FR-CLI-05](../../../docs/prd.md#fr-cli-05) (a cached run equals a cold one)
//!
//! Most tests run a stand-in dependency-cruiser written into the scratch repository's
//! `node_modules`: a package with dependency-cruiser's `package.json` shape, an `allExtensions`
//! export and a `depcruise` command that reads `# dep <specifier>` lines. It logs each call, so a
//! test sees which files the sidecar was asked about, and the configuration it was given. The
//! real dependency-cruiser is run by the last test when a checkout is at hand
//! (`RB_LAYER1_SIDECAR`, or `conformance/dependency-cruiser/upstream/dependency-cruiser`); with
//! `RB_LAYER1_SIDECAR` set, as `conformance/dependency-cruiser/run.sh` sets it, it must run. A test
//! that needs Node and finds none prints why and passes; `cargo test` never fails for lack of Node.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

type Result<T = ()> = std::result::Result<T, Box<dyn Error>>;

const BIN: &str = env!("CARGO_BIN_EXE_rulebearing");

const CONFIG: &str = "forbidden:
  - name: not-to-unresolvable
    severity: error
    comment: \"adr:0017\"
    from: {}
    to: { couldNotResolve: true }
";

const TREE: &[(&str, &str)] = &[
    ("rulebearing.yaml", CONFIG),
    ("package.json", "{ \"name\": \"fixture\" }\n"),
    (
        "src/index.js",
        "import legacy from \"./legacy.coffee\";\nimport util from \"./util.js\";\nexport default legacy + util;\n",
    ),
    (
        "src/legacy.coffee",
        "# dep ./helper.js\n# dep ./more.coffee\nmodule.exports = 1\n",
    ),
    ("src/more.coffee", "# dep ./util.js\nmodule.exports = 2\n"),
    (
        "src/helper.js",
        "import util from \"./util.js\";\nexport default util;\n",
    ),
    ("src/util.js", "export default 3;\n"),
];

/// The stand-in's `allExtensions`: LiveScript is not available, as in a repository without
/// `livescript`.
const ENTRY: &str = "export const allExtensions = [
  { extension: \".coffee\", available: true },
  { extension: \".litcoffee\", available: true },
  { extension: \".ls\", available: false },
];
";

/// The stand-in's command: `--config FILE --output-type json FILE...`. Each file's `# dep` lines
/// are its dependencies; CoffeeScript ones are followed, as dependency-cruiser follows them, and
/// each JavaScript file reached is listed with no dependencies, as a module the merge must not
/// take. Each call is appended to `calls.log` beside the package.
const COMMAND: &str = r##"import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
const args = process.argv.slice(2);
const config = JSON.parse(fs.readFileSync(args[args.indexOf("--config") + 1], "utf8"));
// Every file follows `--`, as the sidecar passes them; none is taken from before it.
const files = args.includes("--")
  ? args.slice(args.indexOf("--") + 1).map((f) => path.posix.normalize(f))
  : [];
// As a file path on every platform: a URL's pathname is `/C:/...` on Windows.
const here = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
fs.appendFileSync(path.join(here, "calls.log"), JSON.stringify({ files, config }) + "\n");
const modules = new Map();
const visit = (file) => {
  if (modules.has(file)) return;
  if (!/\.(coffee|litcoffee|ls)$/.test(file)) {
    modules.set(file, { source: file, dependencies: [], valid: true });
    return;
  }
  const dependencies = fs
    .readFileSync(file, "utf8")
    .split("\n")
    .filter((l) => l.startsWith("# dep "))
    .map((l) => l.slice(6).trim())
    .map((spec) => ({
      module: spec,
      moduleSystem: "cjs",
      dynamic: false,
      exoticallyRequired: false,
      dependencyTypes: ["local", "require"],
      resolved: path.posix.join(path.posix.dirname(file), spec),
      coreModule: false,
      followable: true,
      couldNotResolve: false,
      matchesDoNotFollow: false,
      circular: false,
      valid: true,
    }));
  modules.set(file, { source: file, dependencies, dependents: [], orphan: false, valid: true });
  dependencies.forEach((d) => visit(d.resolved));
};
files.forEach(visit);
process.stdout.write(JSON.stringify({ modules: [...modules.values()], summary: {} }));
"##;

/// A fresh scratch folder holding `files`, canonical.
fn tree(name: &str, files: &[(&str, &str)]) -> Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("rb-cli-sc-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    for (file, text) in files {
        write(&dir, file, text)?;
    }
    Ok(rb_model::without_verbatim(&dir.canonicalize()?))
}

fn write(dir: &Path, file: &str, text: &str) -> Result {
    let path = dir.join(file);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, text)?;
    Ok(())
}

/// Installs the stand-in dependency-cruiser at `version` in `dir`.
fn install(dir: &Path, version: &str) -> Result {
    let package = "node_modules/dependency-cruiser";
    write(
        dir,
        &format!("{package}/package.json"),
        &format!(
            "{{\"name\": \"dependency-cruiser\", \"version\": \"{version}\", \"bin\": {{\"depcruise\": \"bin/depcruise.mjs\"}}, \"exports\": {{\".\": {{\"import\": \"./index.mjs\"}}}}}}\n"
        ),
    )?;
    write(dir, &format!("{package}/index.mjs"), ENTRY)?;
    write(dir, &format!("{package}/bin/depcruise.mjs"), COMMAND)
}

/// The stand-in's calls: the files each was given, and the configuration.
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

fn cruise(dir: &Path, args: &[&str]) -> Result<Output> {
    Ok(Command::new(BIN)
        .arg("cruise")
        .args(args)
        .current_dir(dir)
        .env_remove("RULEBEARING_NODE")
        .output()?)
}

fn json(output: &Output) -> Result<Value> {
    serde_json::from_slice(&output.stdout).map_err(|e| {
        format!(
            "not JSON ({e}): {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into()
    })
}

fn module<'a>(result: &'a Value, source: &str) -> &'a Value {
    result["modules"]
        .as_array()
        .and_then(|m| m.iter().find(|m| m["source"] == source))
        .unwrap_or(&Value::Null)
}

/// Without `--sidecar node`, a CoffeeScript file the walk reaches stops the run with the named
/// reason; one it does not reach does not.
#[test]
fn without_the_flag_a_coffeescript_file_stops_the_run() -> Result {
    let dir = tree("flagless", TREE)?;
    let output = cruise(&dir, &["-T", "json", "src"])?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(
        stderr.contains("src/legacy.coffee: unsupported-file-needs-sidecar")
            && stderr.contains("--sidecar node")
            && stderr.contains("ADR-0017"),
        "{stderr}"
    );
    let untouched = cruise(&dir, &["-T", "json", "src/util.js"])?;
    assert_eq!(untouched.status.code(), Some(0));
    let result = json(&untouched)?;
    assert!(
        result["summary"].get("sidecar").is_none(),
        "no sidecar, no receipt"
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

/// A `.csx` file (also the C# script extension) is never skipped, and the reason says it may be
/// a C# script and how to exclude it; excluded, the run passes.
#[test]
fn a_csx_file_is_named_as_a_possible_csharp_script() -> Result {
    let dir = tree(
        "csx",
        &[
            ("rulebearing.yaml", CONFIG),
            ("a.ts", "export const a = 1;\n"),
            (
                "scripts/build.csx",
                "#r \"nuget: Foo, 1.0\"\nConsole.WriteLine(\"hi\");\n",
            ),
        ],
    )?;
    let output = cruise(&dir, &["-T", "json", "."])?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(
        stderr.contains("scripts/build.csx: unsupported-file-needs-sidecar")
            && stderr.contains("C# script")
            && stderr.contains("options.exclude"),
        "{stderr}"
    );
    let excluded = cruise(&dir, &["-T", "json", "--exclude", "\\.csx$", "."])?;
    assert_eq!(
        excluded.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&excluded.stderr)
    );
    // A CoffeeScript file's reason has no such note.
    let coffee = tree("coffee-reason", TREE)?;
    let output = cruise(&coffee, &["-T", "json", "src"])?;
    assert!(!String::from_utf8_lossy(&output.stderr).contains("C# script"));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&coffee);
    Ok(())
}

/// Node missing: exit 2, naming what is missing and how to install it; dependency-cruiser
/// missing likewise.
#[test]
fn a_missing_node_or_dependency_cruiser_is_named() -> Result {
    let dir = tree("missing", TREE)?;
    let output = cruise(&dir, &["-T", "json", "--sidecar", "node", "src"])?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(
        stderr.contains("src/legacy.coffee: --sidecar node: dependency-cruiser is not installed")
            && stderr.contains("npm install --save-dev dependency-cruiser@18.2.0"),
        "{stderr}"
    );
    install(&dir, "18.2.0")?;
    let empty = dir.join("empty-path");
    std::fs::create_dir_all(&empty)?;
    let output = Command::new(BIN)
        .args(["cruise", "-T", "json", "--sidecar", "node", "src"])
        .current_dir(&dir)
        .env("PATH", &empty)
        .env_remove("RULEBEARING_NODE")
        .output()?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(
        stderr.contains("src/legacy.coffee: --sidecar node: could not start `node`")
            && stderr.contains("install Node 22 or later"),
        "{stderr}"
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

/// The merge: every file once, the sidecar's edges marked and the native ones not, the receipt,
/// two runs byte for byte, and `--strict-schema` stripping all of it.
#[test]
fn the_sidecar_s_edges_merge_into_the_walk() -> Result {
    if !node_available() {
        println!("skipped: no node on the path");
        return Ok(());
    }
    let dir = tree("merge", TREE)?;
    install(&dir, "18.2.0")?;
    let output = cruise(&dir, &["-T", "json", "--sidecar", "node", "src"])?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(0), "{stderr}");
    assert!(!stderr.contains("warning"), "{stderr}");
    let result = json(&output)?;
    let sources: Vec<&str> = result["modules"]
        .as_array()
        .map(|m| m.iter().filter_map(|m| m["source"].as_str()).collect())
        .unwrap_or_default();
    let mut unique = sources.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), sources.len(), "no module twice: {sources:?}");
    for source in [
        "src/index.js",
        "src/legacy.coffee",
        "src/more.coffee",
        "src/helper.js",
        "src/util.js",
    ] {
        assert!(sources.contains(&source), "{source} in {sources:?}");
    }
    let legacy = module(&result, "src/legacy.coffee");
    assert_eq!(
        legacy["dependencies"]
            .as_array()
            .map(|d| d.iter().map(|d| d["resolved"].clone()).collect::<Vec<_>>()),
        Some(vec!["src/helper.js".into(), "src/more.coffee".into()])
    );
    for dependency in legacy["dependencies"].as_array().into_iter().flatten() {
        assert_eq!(dependency["sidecar"], Value::Bool(true), "{dependency}");
        assert!(dependency.get("line").is_none(), "{dependency}");
    }
    // A JavaScript file only a CoffeeScript file reaches is read natively: its own edge, with
    // its position and no sidecar mark.
    let helper = &module(&result, "src/helper.js")["dependencies"][0];
    assert_eq!(helper["resolved"], "src/util.js");
    assert!(
        helper.get("sidecar").is_none() && helper["line"] == 1,
        "{helper}"
    );
    let index = &module(&result, "src/index.js")["dependencies"][0];
    assert!(index.get("sidecar").is_none(), "{index}");
    assert_eq!(
        result["summary"]["sidecar"],
        serde_json::json!({ "tool": "dependency-cruiser", "version": "18.2.0", "files": 2 })
    );
    assert!(result["summary"]["optionsUsed"].get("sidecar").is_none());
    // What the sidecar was given: the files the walk reached, and the options with maxDepth 0.
    let made = calls(&dir);
    assert_eq!(made.len(), 1, "one spawn for the run: {made:?}");
    assert_eq!(
        made[0]["files"],
        serde_json::json!(["src/legacy.coffee", "src/more.coffee"])
    );
    assert_eq!(made[0]["config"]["options"]["maxDepth"], 0);
    assert!(made[0]["config"].get("forbidden").is_none());
    // Deterministic: a second run is byte for byte the first.
    let again = cruise(&dir, &["-T", "json", "--sidecar", "node", "src"])?;
    assert_eq!(again.stdout, output.stdout);
    // --strict-schema strips the edge marks and the receipt.
    let strict = cruise(
        &dir,
        &["-T", "json", "--sidecar", "node", "--strict-schema", "src"],
    )?;
    let text = String::from_utf8_lossy(&strict.stdout);
    assert_eq!(strict.status.code(), Some(0));
    assert!(!text.contains("sidecar"), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

/// A version other than the pinned one runs, with a warning, and the receipt says which.
#[test]
fn another_version_is_a_warning_not_an_error() -> Result {
    if !node_available() {
        println!("skipped: no node on the path");
        return Ok(());
    }
    let dir = tree("version", TREE)?;
    install(&dir, "17.3.1")?;
    let output = cruise(&dir, &["-T", "json", "--sidecar", "node", "src"])?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(0), "{stderr}");
    assert!(
        stderr.contains("warning: the sidecar ran dependency-cruiser 17.3.1")
            && stderr.contains("18.2.0"),
        "{stderr}"
    );
    assert_eq!(json(&output)?["summary"]["sidecar"]["version"], "17.3.1");
    // A LiveScript file with no livescript beside dependency-cruiser is refused, not read as
    // JavaScript. (`.ls` is scanned only where livescript is installed, as upstream scans it, so
    // the file is named as a root.)
    write(&dir, "src/old.ls", "# dep ./util.js\n")?;
    let output = cruise(&dir, &["-T", "json", "--sidecar", "node", "src/old.ls"])?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(
        stderr.contains("cannot read .ls files")
            && stderr.contains("npm install --save-dev livescript"),
        "{stderr}"
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

/// `summary.cache` and `optionsUsed.cache` removed: what a cached run and a cold one share.
fn without_cache(output: &Output) -> Result<Value> {
    let mut value = json(output)?;
    if let Some(summary) = value.get_mut("summary").and_then(Value::as_object_mut) {
        summary.remove("cache");
        if let Some(used) = summary
            .get_mut("optionsUsed")
            .and_then(Value::as_object_mut)
        {
            used.remove("cache");
            used.remove("cacheStrategy");
        }
    }
    Ok(value)
}

/// With `--cache`: an unchanged run spawns nothing and equals the cold run; an edited
/// CoffeeScript file is extracted again by the sidecar, alone, and the result equals a cold run
/// of the edited tree.
#[test]
fn a_cached_run_re_runs_the_sidecar_for_a_changed_file_only() -> Result {
    if !node_available() {
        println!("skipped: no node on the path");
        return Ok(());
    }
    let dir = tree("cache", TREE)?;
    install(&dir, "18.2.0")?;
    let cached = [
        "-T",
        "json",
        "--sidecar",
        "node",
        "--cache",
        ".graph/sidecar-cache",
        "--cache-strategy",
        "content",
        "src",
    ];
    let cold_args = ["-T", "json", "--sidecar", "node", "src"];
    let cold = cruise(&dir, &cold_args)?;
    let first = cruise(&dir, &cached)?;
    assert_eq!(first.status.code(), Some(0));
    assert_eq!(without_cache(&first)?, without_cache(&cold)?);
    assert_eq!(
        calls(&dir).len(),
        2,
        "the cold run and the first cached run spawn"
    );
    let hit = cruise(&dir, &cached)?;
    assert_eq!(json(&hit)?["summary"]["cache"]["hit"], true);
    assert_eq!(without_cache(&hit)?, without_cache(&cold)?);
    assert_eq!(calls(&dir).len(), 2, "a hit spawns nothing");
    // Edit one CoffeeScript file: a new edge.
    write(
        &dir,
        "src/more.coffee",
        "# dep ./util.js\n# dep ./helper.js\nmodule.exports = 2\n",
    )?;
    let edited = cruise(&dir, &cached)?;
    let result = json(&edited)?;
    assert_eq!(result["summary"]["cache"]["hit"], false);
    let made = calls(&dir);
    assert_eq!(made.len(), 3);
    assert_eq!(
        made[2]["files"],
        serde_json::json!(["src/more.coffee"]),
        "only the changed file goes to the sidecar"
    );
    assert_eq!(
        module(&result, "src/more.coffee")["dependencies"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );
    let cold_edited = cruise(&dir, &cold_args)?;
    assert_eq!(without_cache(&edited)?, without_cache(&cold_edited)?);
    // A run without the flag never takes the sidecar's entry: the flag is in the key.
    let flagless = cruise(
        &dir,
        &["-T", "json", "--cache", ".graph/sidecar-cache", "src"],
    )?;
    assert_eq!(flagless.status.code(), Some(2));
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

/// The dependency-cruiser checkout the real-tool tests run, or `None` (with the reason printed)
/// when it or Node is missing; with `RB_LAYER1_SIDECAR` set, a missing one fails the test.
fn real_checkout() -> Option<PathBuf> {
    let checkout = std::env::var_os("RB_LAYER1_SIDECAR").map_or_else(
        || {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../conformance/dependency-cruiser/upstream/dependency-cruiser")
        },
        PathBuf::from,
    );
    if node_available() && checkout.join("node_modules/coffeescript").is_dir() {
        return Some(checkout);
    }
    assert!(
        std::env::var_os("RB_LAYER1_SIDECAR").is_none(),
        "RB_LAYER1_SIDECAR is set, but there is no node or no {}",
        checkout.join("node_modules/coffeescript").display()
    );
    println!(
        "skipped: needs node and a dependency-cruiser checkout with node_modules at {}",
        checkout.display()
    );
    None
}

/// Installs `checkout` as `dir`'s `node_modules/dependency-cruiser`, by a link.
fn link(checkout: &Path, dir: &Path) -> Result {
    std::fs::create_dir_all(dir.join("node_modules"))?;
    #[cfg(unix)]
    std::os::unix::fs::symlink(checkout, dir.join("node_modules/dependency-cruiser"))?;
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(checkout, dir.join("node_modules/dependency-cruiser"))?;
    Ok(())
}

/// Every file under `dir`, `node_modules` and `.graph` aside, relative and sorted.
fn listing(dir: &Path) -> Vec<String> {
    fn walk(root: &Path, at: &Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(at).into_iter().flatten().flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name == "node_modules" || name == ".graph" {
                continue;
            }
            if path.is_dir() {
                walk(root, &path, out);
            } else if let Ok(relative) = path.strip_prefix(root) {
                out.push(relative.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

/// A file whose name reads as one of dependency-cruiser's options is passed as a path: with the
/// real dependency-cruiser, files named `--output-to=pwned.coffee` and `--config=x.coffee`, and a
/// folder named `--output-to=..` holding `a.coffee`, are extracted with the right edges and no
/// file is written, in the repository or beside it.
#[test]
fn a_file_named_like_an_option_is_never_an_option() -> Result {
    let Some(checkout) = real_checkout() else {
        return Ok(());
    };
    let dir = tree(
        "dashes",
        &[
            ("rulebearing.yaml", CONFIG),
            (
                "main.js",
                "import a from \"./--output-to=pwned.coffee\";\nimport b from \"./--config=x.coffee\";\nimport c from \"./--output-to=../a.coffee\";\nexport default [a, b, c];\n",
            ),
            (
                "--output-to=pwned.coffee",
                "import u from \"./util.js\"\nexport default u\n",
            ),
            (
                "--config=x.coffee",
                "import u from \"./util.js\"\nexport default u\n",
            ),
            (
                "--output-to=../a.coffee",
                "import u from \"../util.js\"\nexport default u\n",
            ),
            ("util.js", "export default 1;\n"),
        ],
    )?;
    link(&checkout, &dir)?;
    let parent = dir.parent().map(Path::to_path_buf).unwrap_or_default();
    let beside = |name: &str| parent.join(name).exists();
    let before = listing(&dir);
    let output = cruise(&dir, &["-T", "json", "--sidecar", "node", "main.js"])?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(0), "{stderr}");
    assert_eq!(listing(&dir), before, "dependency-cruiser wrote nothing");
    assert!(
        !beside("a.coffee") && !beside("pwned.coffee"),
        "nor beside the repository"
    );
    let result = json(&output)?;
    for (source, target) in [
        ("--output-to=pwned.coffee", "util.js"),
        ("--config=x.coffee", "util.js"),
        ("--output-to=../a.coffee", "util.js"),
    ] {
        let dependencies = &module(&result, source)["dependencies"];
        assert_eq!(
            dependencies[0]["resolved"], target,
            "{source}: {dependencies}"
        );
        assert_eq!(dependencies[0]["sidecar"], true, "{source}");
    }
    assert_eq!(result["summary"]["sidecar"]["files"], 3);
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

/// The real dependency-cruiser over its own CoffeeScript fixture, through the command line,
/// when a checkout with its `node_modules` is at hand.
#[test]
fn the_real_dependency_cruiser_extracts_its_coffeescript_fixture() -> Result {
    let Some(checkout) = real_checkout() else {
        return Ok(());
    };
    let mocks = Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../conformance/dependency-cruiser/fixtures/extract/test/extract/__mocks__/coffee",
    );
    let dir = tree("real", &[("rulebearing.yaml", CONFIG)])?;
    for file in [
        "index.coffee",
        "javascriptThing.js",
        "sub/index.coffee",
        "sub/kaching.litcoffee",
        "sub/willBeReExported.coffee.md",
    ] {
        write(
            &dir,
            &format!("src/{file}"),
            &std::fs::read_to_string(mocks.join(file))?,
        )?;
    }
    link(&checkout, &dir)?;
    let output = cruise(
        &dir,
        &["-T", "json", "--sidecar", "node", "src/index.coffee"],
    )?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(0), "{stderr}");
    let result = json(&output)?;
    let index = module(&result, "src/index.coffee");
    let resolved: Vec<&str> = index["dependencies"]
        .as_array()
        .map(|d| d.iter().filter_map(|d| d["resolved"].as_str()).collect())
        .unwrap_or_default();
    assert_eq!(
        resolved,
        [
            "src/javascriptThing.js",
            "src/sub/index.coffee",
            "src/sub/kaching.litcoffee",
            "src/sub/willBeReExported.coffee.md",
            "path"
        ]
    );
    assert!(
        index["dependencies"]
            .as_array()
            .into_iter()
            .flatten()
            .all(|d| d["sidecar"] == true)
    );
    assert_eq!(result["summary"]["sidecar"]["files"], 4);
    assert_eq!(
        module(&result, "src/javascriptThing.js")["language"],
        "javascript"
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}
