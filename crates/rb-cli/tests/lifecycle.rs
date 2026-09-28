//! The rule lifecycle end to end: `snapshot` on committed fixture graphs, `changelog` between
//! them, `rules --unused` over committed snapshots, and the lifecycle fields in `cruise` and
//! `config lint`.
//!
//! - Plan: [Wave 3, Steps 12 and 13](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#23-steps-for-sub-wave-3c-presets-lifecycle-fields-snapshot-and-changelog)
//!   (`--unused` over three fixture snapshots; the insufficient-history case; a committed fixture
//!   pair and the expected Markdown output, byte-compared)
//! - Contract: [Wave 3 plan § 1.5](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#15-interfaces-and-contracts-this-wave-freezes)
//!   (the snapshot's shape)
//! - Decision: [ADR-0007](../../../docs/adr/0007-vacuous-rules-fail-by-default.md) (a deprecated
//!   rule that matches nothing still fails)
//! - Requirement: [FR-CLI-07](../../../docs/prd.md#fr-cli-07)
//!
//! `tests/fixtures/lifecycle/graph-1.0.0.json` and `graph-1.1.0.json` are real cruise results of
//! the two releases [`tree`] writes, with the working-directory path written as `.` and a fixed
//! `revisionData` added, so the snapshots record a commit. The first test proves they still are
//! what `cruise -T json` writes. The `expected-*` files are the snapshots and the changelog
//! renderings. `RB_UPDATE_SNAPSHOTS=1` regenerates all of them, a reviewable diff. The three
//! snapshots under `unused/` are written by hand, as a repository's history would hold them.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

type Result<T = ()> = std::result::Result<T, Box<dyn Error>>;

const BIN: &str = env!("CARGO_BIN_EXE_rulebearing");

/// The configuration at release 1.0.0: `old-rule` exists, `no-legacy-http` is live.
const CONFIG_1: &str = r#"rules:
  layers:
    - name: app-layers
      comment: "adr:0010"
      fix: Depend downwards only, the ui on the domain and never the reverse
      layers: ["^src/ui/", "^src/domain/"]
      allowEmpty: true
  slices:
    - name: features
      comment: "adr:0011"
      fix: Share code between features through src/shared
      matching: "src/features/(**)//"
      should: notDependOnEachOther
      severity: warn
  ratchets:
    - name: ui-to-db
      from: { path: "^src/ui/" }
      to: { path: "^src/db/" }
      budget: budgets/ui-to-db.json
  dependencies:
    forbidden:
      - name: no-legacy-http
        comment: "adr:0012"
        since: "1.0.0"
        severity: warn
        from: { path: "^src/" }
        to: { path: "^src/legacy/" }
        allowEmpty: true
      - name: old-rule
        severity: info
        from: { path: "^src/" }
        to: { path: "^src/old/" }
        allowEmpty: true
"#;

/// The configuration at release 1.1.0: `old-rule` is gone, `no-legacy-http` is deprecated.
const CONFIG_2: &str = r#"rules:
  layers:
    - name: app-layers
      comment: "adr:0010"
      fix: Depend downwards only, the ui on the domain and never the reverse
      layers: ["^src/ui/", "^src/domain/"]
      allowEmpty: true
  slices:
    - name: features
      comment: "adr:0011"
      fix: Share code between features through src/shared
      matching: "src/features/(**)//"
      should: notDependOnEachOther
      severity: warn
  ratchets:
    - name: ui-to-db
      from: { path: "^src/ui/" }
      to: { path: "^src/db/" }
      budget: budgets/ui-to-db.json
  dependencies:
    forbidden:
      - name: no-legacy-http
        comment: "adr:0012"
        since: "1.0.0"
        deprecated: "1.1.0"
        replacedBy: ui-through-services
        severity: warn
        from: { path: "^src/" }
        to: { path: "^src/legacy/" }
        allowEmpty: true
      - name: ui-through-services
        comment: "adr:0012"
        since: "1.1.0"
        severity: warn
        from: { path: "^src/ui/" }
        to: { path: "^src/db/" }
        allowEmpty: true
"#;

/// The variables git sets for a hook; each command here runs without them (see `diff.rs`).
const GIT_LOCAL_ENV: [&str; 15] = [
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_CONFIG",
    "GIT_CONFIG_PARAMETERS",
    "GIT_CONFIG_COUNT",
    "GIT_OBJECT_DIRECTORY",
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_IMPLICIT_WORK_TREE",
    "GIT_GRAFT_FILE",
    "GIT_INDEX_FILE",
    "GIT_NO_REPLACE_OBJECTS",
    "GIT_REPLACE_REF_BASE",
    "GIT_PREFIX",
    "GIT_SHALLOW_FILE",
    "GIT_COMMON_DIR",
];

/// A command in `dir` that sees no enclosing repository unless `dir` is one: git stops looking
/// at the folder above it.
fn isolated(program: &str, dir: &Path) -> Command {
    let mut command = Command::new(program);
    command.current_dir(dir);
    for name in GIT_LOCAL_ENV {
        command.env_remove(name);
    }
    if let Some(parent) = dir.parent() {
        command.env("GIT_CEILING_DIRECTORIES", parent);
    }
    command
}

fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let output = isolated("git", dir)
        .args([
            "-c",
            "user.name=rulebearing",
            "-c",
            "user.email=rulebearing@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn write(dir: &Path, file: &str, text: &str) -> Result {
    let path = dir.join(file);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, text)?;
    Ok(())
}

fn run(dir: &Path, args: &[&str]) -> Result<Output> {
    Ok(isolated(BIN, dir).args(args).output()?)
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/lifecycle")
}

fn updating() -> bool {
    std::env::var_os("RB_UPDATE_SNAPSHOTS").is_some()
}

/// Compares `actual` with the committed `file`, or writes it under `RB_UPDATE_SNAPSHOTS`.
fn expect(file: &str, actual: &str) -> Result {
    let path = fixtures().join(file);
    if updating() {
        std::fs::create_dir_all(fixtures())?;
        std::fs::write(&path, actual)?;
        return Ok(());
    }
    let expected = std::fs::read_to_string(&path)?;
    assert_eq!(
        actual, expected,
        "{file} changed; if that was intended, regenerate with RB_UPDATE_SNAPSHOTS=1 and explain the change"
    );
    Ok(())
}

/// A fresh folder.
fn folder(name: &str) -> Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("rb-cli-lifecycle-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    // Canonical, as the process's working directory is (`/var` is a link on macOS).
    Ok(dir.canonicalize()?)
}

/// The tree of `release` (`1.0.0` or `1.1.0`) with its configuration, in a fresh folder.
fn tree(release: &str) -> Result<PathBuf> {
    let dir = folder(&format!("tree-{release}"))?;
    let second = release == "1.1.0";
    write(
        &dir,
        "rulebearing.yaml",
        if second { CONFIG_2 } else { CONFIG_1 },
    )?;
    write(&dir, "budgets/ui-to-db.json", "{ \"ceiling\": 2 }\n")?;
    write(&dir, "src/db/store.ts", "export const store = 1;\n")?;
    write(&dir, "src/features/b/y.ts", "export const y = 1;\n")?;
    write(
        &dir,
        "src/ui/view.ts",
        "import { store } from \"../db/store\";\nimport { model } from \"../domain/model\";\nexport const view = [store, model];\n",
    )?;
    if second {
        write(&dir, "src/ui/format.ts", "export const format = 1;\n")?;
        write(
            &dir,
            "src/domain/model.ts",
            "import { format } from \"../ui/format\";\nexport const model = format;\n",
        )?;
        write(
            &dir,
            "src/features/a/x.ts",
            "import { y } from \"../b/y\";\nexport const x = y;\n",
        )?;
        write(
            &dir,
            "src/main.ts",
            "import { view } from \"./ui/view\";\nimport { x } from \"./features/a/x\";\nconsole.log(view, x);\n",
        )?;
    } else {
        write(&dir, "src/domain/model.ts", "export const model = 1;\n")?;
        write(&dir, "src/features/a/x.ts", "export const x = 1;\n")?;
        write(&dir, "src/legacy/http.ts", "export const http = 1;\n")?;
        write(
            &dir,
            "src/ui/list.ts",
            "import { store } from \"../db/store\";\nimport { http } from \"../legacy/http\";\nexport const list = [store, http];\n",
        )?;
        write(
            &dir,
            "src/main.ts",
            "import { view } from \"./ui/view\";\nimport { list } from \"./ui/list\";\nimport { x } from \"./features/a/x\";\nimport { y } from \"./features/b/y\";\nconsole.log(view, list, x, y);\n",
        )?;
    }
    Ok(dir)
}

/// `cruise -T json` of `release`, with the working-directory path written as `.` and the fixed
/// `revisionData` the fixture carries.
fn cruise(release: &str) -> Result<String> {
    let dir = tree(release)?;
    let out = run(&dir, &["cruise", "-T", "json"])?;
    assert!(
        matches!(out.status.code(), Some(0 | 1)),
        "{release}: {}",
        text(&out.stderr)
    );
    let mut value: Value = serde_json::from_slice(&out.stdout)?;
    if let Some(options) = value
        .get_mut("summary")
        .and_then(|s| s.get_mut("optionsUsed"))
        .and_then(Value::as_object_mut)
    {
        options.insert("baseDir".into(), Value::String(".".into()));
    }
    let sha = if release == "1.1.0" { "2" } else { "1" }.repeat(40);
    if let Some(root) = value.as_object_mut() {
        root.insert(
            "revisionData".into(),
            serde_json::json!({ "SHA1": sha, "changes": [] }),
        );
    }
    let mut json = serde_json::to_string_pretty(&value)?;
    json.push('\n');
    let _ = std::fs::remove_dir_all(&dir);
    Ok(json)
}

const RELEASES: [&str; 2] = ["1.0.0", "1.1.0"];

#[test]
fn the_fixture_graphs_are_what_cruise_writes() -> Result {
    for release in RELEASES {
        expect(&format!("graph-{release}.json"), &cruise(release)?)?;
    }
    Ok(())
}

/// A folder holding the configuration of `release` and its budget.
fn project(name: &str, config: &str) -> Result<PathBuf> {
    let dir = folder(name)?;
    write(&dir, "rulebearing.yaml", config)?;
    write(&dir, "budgets/ui-to-db.json", "{ \"ceiling\": 2 }\n")?;
    Ok(dir)
}

/// `snapshot --version <release> --graph <fixture>` in `dir`.
fn snapshot(dir: &Path, release: &str) -> Result<Output> {
    let graph = fixtures().join(format!("graph-{release}.json"));
    let graph = graph.to_string_lossy();
    run(dir, &["snapshot", "--version", release, "--graph", &graph])
}

/// Both releases snapshotted in one folder, each under its own configuration, as a repository
/// does it release by release; the folder ends with the 1.1.0 configuration.
fn history(name: &str) -> Result<PathBuf> {
    let dir = project(name, CONFIG_1)?;
    let first = snapshot(&dir, "1.0.0")?;
    assert_eq!(first.status.code(), Some(0), "{}", text(&first.stderr));
    assert_eq!(
        text(&first.stdout),
        "rulebearing snapshot: wrote .graph/snapshots/1.0.0.json and .graph/snapshots/1.0.0.cruise.json\n"
    );
    write(&dir, "rulebearing.yaml", CONFIG_2)?;
    let second = snapshot(&dir, "1.1.0")?;
    assert_eq!(second.status.code(), Some(0), "{}", text(&second.stderr));
    Ok(dir)
}

#[test]
fn snapshots_and_the_changelog_are_byte_stable() -> Result {
    let dir = history("stable")?;
    let snapshots = dir.join(".graph/snapshots");
    for release in RELEASES {
        let written = std::fs::read_to_string(snapshots.join(format!("{release}.json")))?;
        expect(&format!("expected-{release}.json"), &written)?;
        assert!(snapshots.join(format!("{release}.cruise.json")).is_file());
    }
    let markdown = run(&dir, &["changelog", "--since", "1.0.0"])?;
    assert_eq!(
        markdown.status.code(),
        Some(0),
        "{}",
        text(&markdown.stderr)
    );
    expect("expected-changelog.md", &text(&markdown.stdout))?;
    let json = run(
        &dir,
        &[
            "changelog",
            "--since",
            "1.0.0",
            "--to",
            "1.1.0",
            "-T",
            "json",
        ],
    )?;
    assert_eq!(json.status.code(), Some(0), "{}", text(&json.stderr));
    expect("expected-changelog.json", &text(&json.stdout))?;

    // Determinism: the same inputs, the same bytes; a snapshot written again is identical.
    let again = run(&dir, &["changelog", "--since", "1.0.0", "--to", "1.1.0"])?;
    assert_eq!(again.stdout, markdown.stdout);
    let before = std::fs::read(snapshots.join("1.1.0.json"))?;
    let rewritten = snapshot(&dir, "1.1.0")?;
    assert_eq!(rewritten.status.code(), Some(0));
    assert_eq!(std::fs::read(snapshots.join("1.1.0.json"))?, before);

    // `-f` writes the file instead of stdout.
    let to_file = run(&dir, &["changelog", "--since", "1.0.0", "-f", "CHANGES.md"])?;
    assert_eq!(to_file.status.code(), Some(0));
    assert!(to_file.stdout.is_empty());
    assert_eq!(std::fs::read(dir.join("CHANGES.md"))?, markdown.stdout);
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn without_the_cruise_results_the_changelog_says_so() -> Result {
    let dir = history("no-cruise")?;
    std::fs::remove_file(dir.join(".graph/snapshots/1.0.0.cruise.json"))?;
    let out = run(&dir, &["changelog", "--since", "1.0.0"])?;
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let stdout = text(&out.stdout);
    assert!(
        stdout.contains(
            "## New edges across boundaries\n\nNot available: the edges need the cruise results `rulebearing snapshot` writes beside both snapshots, and `.graph/snapshots/1.0.0.cruise.json` is missing.\n"
        ),
        "{stdout}"
    );
    assert!(
        stdout.contains("## Ratchets that fell\n\n| Ratchet |"),
        "{stdout}"
    );
    let json = run(&dir, &["changelog", "--since", "1.0.0", "-T", "json"])?;
    let value: Value = serde_json::from_slice(&json.stdout)?;
    assert_eq!(value["newEdgesAcrossBoundaries"], Value::Null);
    write(&dir, ".graph/snapshots/1.0.0.cruise.json", "not json")?;
    let broken = run(&dir, &["changelog", "--since", "1.0.0"])?;
    assert_eq!(broken.status.code(), Some(2));
    assert!(text(&broken.stderr).contains("1.0.0.cruise.json is not a cruise result"));
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn changelog_and_snapshot_errors_are_named() -> Result {
    let dir = history("errors")?;
    let unknown = run(&dir, &["changelog", "--since", "0.9.0"])?;
    assert_eq!(unknown.status.code(), Some(2));
    assert!(
        text(&unknown.stderr).contains(
            "no snapshot of `0.9.0` under .graph/snapshots (it has 1.0.0, 1.1.0); write it with `rulebearing snapshot --version 0.9.0` at that release"
        ),
        "{}",
        text(&unknown.stderr)
    );
    let unknown_to = run(&dir, &["changelog", "--since", "1.0.0", "--to", "2.0.0"])?;
    assert_eq!(unknown_to.status.code(), Some(2));
    let output_type = run(&dir, &["changelog", "--since", "1.0.0", "-T", "html"])?;
    assert_eq!(output_type.status.code(), Some(3));
    assert!(text(&output_type.stderr).contains("use markdown or json"));
    let no_since = run(&dir, &["changelog"])?;
    assert_eq!(no_since.status.code(), Some(3));

    let bad_version = run(&dir, &["snapshot", "--version", "release/1"])?;
    assert_eq!(bad_version.status.code(), Some(3));
    assert!(text(&bad_version.stderr).contains("may hold only letters"));
    // Not a repository, so there is no tag to default to.
    let no_version = run(&dir, &["snapshot", "--graph", "none.json"])?;
    assert_eq!(no_version.status.code(), Some(3));
    assert!(
        text(&no_version.stderr).contains("pass --version"),
        "{}",
        text(&no_version.stderr)
    );
    let no_graph = run(
        &dir,
        &["snapshot", "--version", "3.0.0", "--graph", "none.json"],
    )?;
    assert_eq!(no_graph.status.code(), Some(2));
    assert!(text(&no_graph.stderr).contains("cannot read the graph"));
    write(&dir, "rulebearing.yaml", "rules: [\n")?;
    let bad_config = run(&dir, &["changelog", "--since", "1.0.0"])?;
    assert_eq!(bad_config.status.code(), Some(3));
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn the_version_defaults_to_the_tag_at_head_and_the_sha_to_head() -> Result {
    let dir = project("tagged", CONFIG_2)?;
    git(&dir, &["init", "-q"])?;
    git(&dir, &["add", "-A"])?;
    git(&dir, &["commit", "-q", "-m", "release"])?;
    git(&dir, &["tag", "v2.0.0-rc.1"])?;
    git(&dir, &["tag", "v2.0.0"])?;
    let head = git(&dir, &["rev-parse", "HEAD"])?;
    // The graph without its recorded commit, so the sha comes from HEAD.
    let mut graph: Value = serde_json::from_str(&std::fs::read_to_string(
        fixtures().join("graph-1.1.0.json"),
    )?)?;
    if let Some(root) = graph.as_object_mut() {
        root.remove("revisionData");
    }
    write(&dir, "graph.json", &serde_json::to_string(&graph)?)?;
    let out = run(&dir, &["snapshot", "--graph", "graph.json"])?;
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let written: Value = serde_json::from_str(&std::fs::read_to_string(
        dir.join(".graph/snapshots/v2.0.0.json"),
    )?)?;
    assert_eq!(written["version"], "v2.0.0");
    assert_eq!(written["sha"], Value::String(head));
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

/// The configuration the three `unused/` snapshots were taken under.
const UNUSED_CONFIG: &str = r#"rules:
  dependencies:
    forbidden:
      - { name: idle, severity: warn, deprecated: "1.1.0", replacedBy: busy, from: { path: "^nothing/" }, to: {}, allowEmpty: true }
      - { name: busy, severity: warn, from: { path: "^src/" }, to: { path: "^src/db/" } }
      - { name: idle-lately, severity: warn, from: { path: "^src/old/" }, to: {}, allowEmpty: true }
      - { name: added-later, severity: warn, from: { path: "^src/new/" }, to: {}, allowEmpty: true }
"#;

/// A folder holding the `unused/` snapshots and [`UNUSED_CONFIG`].
fn unused_history(name: &str) -> Result<PathBuf> {
    let dir = project(name, UNUSED_CONFIG)?;
    let snapshots = dir.join(".graph/snapshots");
    std::fs::create_dir_all(&snapshots)?;
    for entry in std::fs::read_dir(fixtures().join("unused"))? {
        let path = entry?.path();
        if let Some(file) = path.file_name() {
            std::fs::copy(&path, snapshots.join(file))?;
        }
    }
    Ok(dir)
}

#[test]
fn unused_rules_are_those_that_matched_nothing_in_each_of_the_last_releases() -> Result {
    let dir = unused_history("unused")?;
    let out = run(&dir, &["rules", "--unused"])?;
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(
        text(&out.stdout),
        "unused in each of 1.9.0, 1.10.0, 1.11.0:\n  idle (deprecated 1.1.0) (replaced by busy)\n1 rule(s); each still fails cruise as vacuous unless it has allowEmpty: delete it, or mark it deprecated with replacedBy\n"
    );
    // The last two releases by version, not by file name (`1.9.0.json` sorts last as text):
    // `idle-lately` matched something in 1.9.0 and nothing since, and `added-later` arrived in
    // 1.10.0.
    let two = run(&dir, &["rules", "--unused", "--releases", "2"])?;
    assert_eq!(
        text(&two.stdout),
        "unused in each of 1.10.0, 1.11.0:\n  idle (deprecated 1.1.0) (replaced by busy)\n  idle-lately\n  added-later\n3 rule(s); each still fails cruise as vacuous unless it has allowEmpty: delete it, or mark it deprecated with replacedBy\n"
    );
    let json = run(&dir, &["rules", "--unused", "--json"])?;
    let value: Value = serde_json::from_slice(&json.stdout)?;
    assert_eq!(
        value["releases"],
        serde_json::json!(["1.9.0", "1.10.0", "1.11.0"])
    );
    assert_eq!(value["insufficientHistory"], false);
    assert_eq!(value["unused"][0]["name"], "idle");
    assert_eq!(value["unused"][0]["deprecated"], "1.1.0");
    assert_eq!(value["unused"][0]["replacedBy"], "busy");
    assert_eq!(value["unused"].as_array().map(Vec::len), Some(1));
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn fewer_snapshots_than_releases_is_insufficient_history() -> Result {
    let dir = unused_history("insufficient")?;
    let out = run(&dir, &["rules", "--unused", "--releases", "4"])?;
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(
        text(&out.stdout),
        "insufficient history: 3 snapshot(s) under .graph/snapshots, and --releases asks for 4; write one per release with `rulebearing snapshot`\n"
    );
    let json = run(&dir, &["rules", "--unused", "--releases", "4", "--json"])?;
    let value: Value = serde_json::from_slice(&json.stdout)?;
    assert_eq!(value["insufficientHistory"], true);
    assert_eq!(value["unused"], serde_json::json!([]));
    let empty = project("insufficient-empty", UNUSED_CONFIG)?;
    let none = run(&empty, &["rules", "--unused"])?;
    assert_eq!(none.status.code(), Some(0));
    assert!(text(&none.stdout).starts_with("insufficient history: 0 snapshot(s)"));
    for bad in [
        &["rules", "--unused", "--graph", "g.json"][..],
        &["rules", "--unused", "src"][..],
        &["rules", "--releases", "2"][..],
        &["rules", "--unused", "--releases", "0"][..],
    ] {
        let refused = run(&dir, bad)?;
        assert_eq!(
            refused.status.code(),
            Some(3),
            "{bad:?}: {}",
            text(&refused.stderr)
        );
    }
    write(&dir, ".graph/snapshots/9.9.9.json", "{")?;
    let broken = run(&dir, &["rules", "--unused"])?;
    assert_eq!(broken.status.code(), Some(2));
    assert!(text(&broken.stderr).contains("9.9.9.json is not a snapshot"));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&empty);
    Ok(())
}

#[test]
fn a_deprecated_rule_that_matches_nothing_still_fails() -> Result {
    let rule = |allow: &str| {
        format!(
            "rules:\n  dependencies:\n    forbidden:\n      - {{ name: retired, severity: error, deprecated: \"1.0.0\", replacedBy: other, from: {{ path: \"^nothing/\" }}, to: {{}}{allow} }}\n      - {{ name: other, severity: error, from: {{ path: \"^src/\" }}, to: {{ path: \"^lib/\" }}, allowEmpty: true }}\n"
        )
    };
    let dir = project("liveness", &rule(""))?;
    write(&dir, "src/a.ts", "export const a = 1;\n")?;
    let out = run(&dir, &["cruise", "src"])?;
    assert_eq!(out.status.code(), Some(2), "{}", text(&out.stdout));
    assert!(
        text(&out.stderr).contains("retired"),
        "{}",
        text(&out.stderr)
    );
    write(&dir, "rulebearing.yaml", &rule(", allowEmpty: true"))?;
    let allowed = run(&dir, &["cruise", "src"])?;
    assert_eq!(allowed.status.code(), Some(0), "{}", text(&allowed.stderr));
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn config_lint_and_rules_json_carry_the_lifecycle_fields() -> Result {
    let dir = project(
        "lint",
        "rules:\n  dependencies:\n    forbidden:\n      - { name: old, fix: Import the gateway instead, since: \"2.0.0\", deprecated: \"1.0.0\", replacedBy: gone, from: { path: \"^src/\" }, to: {} }\n",
    )?;
    let lint = run(&dir, &["config", "lint"])?;
    assert_eq!(
        text(&lint.stdout),
        "replaced-by-unknown old: `replacedBy` names `gone`, which is no rule, shorthand or ratchet of this configuration; add that rule or correct the name\nsince-after-deprecated old: `since` 2.0.0 is later than `deprecated` 1.0.0; a rule is deprecated after it arrives, so correct one of the two\nconfig lint: 2 finding(s)\n"
    );
    assert_eq!(lint.status.code(), Some(2));
    write(&dir, "src/a.ts", "export const a = 1;\n")?;
    let rules = run(&dir, &["rules", "--json", "src"])?;
    assert_eq!(rules.status.code(), Some(0), "{}", text(&rules.stderr));
    let value: Value = serde_json::from_slice(&rules.stdout)?;
    assert_eq!(value["rules"][0]["since"], "2.0.0");
    assert_eq!(value["rules"][0]["deprecated"], "1.0.0");
    assert_eq!(value["rules"][0]["replacedBy"], "gone");
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn a_snapshot_describes_the_tree_at_its_commit_not_a_saved_graph() -> Result {
    let dir = project("fresh", CONFIG_2)?;
    write(&dir, ".gitignore", ".graph/\n")?;
    write(&dir, "src/a.ts", "export const a = 1;\n")?;
    git(&dir, &["init", "-q"])?;
    git(&dir, &["add", "-A"])?;
    git(&dir, &["commit", "-q", "-m", "A"])?;
    // A result saved at commit A, as `cruise -T json -f .graph/cruise.json` leaves it.
    let saved = run(
        &dir,
        &["cruise", "src", "-T", "json", "-f", ".graph/cruise.json"],
    )?;
    assert!(
        dir.join(".graph/cruise.json").is_file(),
        "{}",
        text(&saved.stderr)
    );
    write(
        &dir,
        "src/b.ts",
        "import { a } from \"./a\";\nexport const b = a;\n",
    )?;
    write(
        &dir,
        "src/c.ts",
        "import { b } from \"./b\";\nexport const c = b;\n",
    )?;
    git(&dir, &["add", "-A"])?;
    git(&dir, &["commit", "-q", "-m", "B"])?;
    git(&dir, &["tag", "v2.0.0"])?;
    let head = git(&dir, &["rev-parse", "HEAD"])?;
    let out = run(&dir, &["snapshot", "src"])?;
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let written: Value = serde_json::from_str(&std::fs::read_to_string(
        dir.join(".graph/snapshots/v2.0.0.json"),
    )?)?;
    assert_eq!(written["sha"], Value::String(head));
    assert_eq!(
        written["counts"]["modules"], 3,
        "the tree at B, not the result saved at A"
    );
    assert_eq!(written["counts"]["dependencies"], 2);
    // `--graph` given explicitly is what the caller vouches for, and is read as it is.
    let explicit = run(
        &dir,
        &[
            "snapshot",
            "--version",
            "old",
            "--graph",
            ".graph/cruise.json",
        ],
    )?;
    assert_eq!(
        explicit.status.code(),
        Some(0),
        "{}",
        text(&explicit.stderr)
    );
    let old: Value = serde_json::from_str(&std::fs::read_to_string(
        dir.join(".graph/snapshots/old.json"),
    )?)?;
    assert_eq!(old["counts"]["modules"], 1);
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn shorthands_and_ratchets_carry_the_lifecycle_fields_into_expand_and_changelog() -> Result {
    let retiring = CONFIG_2
        .replace(
            "      layers: [\"^src/ui/\", \"^src/domain/\"]\n",
            "      layers: [\"^src/ui/\", \"^src/domain/\"]\n      since: \"1.0.0\"\n      deprecated: \"1.1.0\"\n      replacedBy: features\n",
        )
        .replace(
            "      budget: budgets/ui-to-db.json\n",
            "      budget: budgets/ui-to-db.json\n      deprecated: \"v1.1.0\"\n      replacedBy: ui-through-services\n",
        );
    let dir = history("shorthands")?;
    write(&dir, "rulebearing.yaml", &retiring)?;
    let expanded = run(&dir, &["config", "expand", "rulebearing.yaml"])?;
    assert_eq!(
        expanded.status.code(),
        Some(0),
        "{}",
        text(&expanded.stderr)
    );
    let expanded = text(&expanded.stdout);
    assert!(
        expanded.contains("app-layers:2-to-1") && expanded.contains("replacedBy: features"),
        "{expanded}"
    );
    let lint = run(&dir, &["config", "lint"])?;
    assert!(
        !text(&lint.stdout).contains("replaced-by"),
        "every replacedBy names something: {}",
        text(&lint.stdout)
    );
    let json = run(&dir, &["changelog", "--since", "1.0.0", "-T", "json"])?;
    assert_eq!(json.status.code(), Some(0), "{}", text(&json.stderr));
    let value: Value = serde_json::from_slice(&json.stdout)?;
    let retired: Vec<(&str, &str)> = value["retiredRules"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|r| {
                    (
                        r["name"].as_str().unwrap_or_default(),
                        r["deprecated"].as_str().unwrap_or_default(),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    assert_eq!(
        retired,
        [
            ("app-layers", "1.1.0"),
            ("no-legacy-http", "1.1.0"),
            ("old-rule", ""),
            ("ui-to-db", "v1.1.0"),
        ],
        "the layers entry once, and the ratchet whose release is spelt with a v"
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn a_snapshot_must_record_the_version_its_file_is_named_for() -> Result {
    let dir = history("renamed")?;
    let snapshots = dir.join(".graph/snapshots");
    std::fs::copy(snapshots.join("1.0.0.json"), snapshots.join("0.9.0.json"))?;
    let out = run(&dir, &["changelog", "--since", "1.0.0"])?;
    assert_eq!(out.status.code(), Some(2));
    assert!(
        text(&out.stderr)
            .contains("0.9.0.json records the version `1.0.0` but is named for `0.9.0`"),
        "{}",
        text(&out.stderr)
    );
    std::fs::remove_file(snapshots.join("0.9.0.json"))?;
    let escaping = std::fs::read_to_string(snapshots.join("1.0.0.json"))?
        .replace("\"version\": \"1.0.0\"", "\"version\": \"../../x\"");
    write(&dir, ".graph/snapshots/x.json", &escaping)?;
    for args in [
        &["changelog", "--since", "../../x"][..],
        &["rules", "--unused"][..],
    ] {
        let refused = run(&dir, args)?;
        assert_eq!(refused.status.code(), Some(2), "{args:?}");
        assert!(
            text(&refused.stderr).contains("records a version that cannot name a snapshot"),
            "{args:?}: {}",
            text(&refused.stderr)
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}
