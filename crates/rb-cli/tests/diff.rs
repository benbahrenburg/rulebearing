//! `diff`, end to end: the committed fixture pair and its three renderings, `--base` against a
//! two-commit repository, and the error paths.
//!
//! - Plan: [Wave 3, Step 4](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#21-steps-for-sub-wave-3a-cache---affected-diff---exit-code-mode-strict)
//!   (a fixture pair with one added edge, one removed edge, one new violation, one resolved
//!   violation and one ratchet that fell; `--base` against a two-commit fixture repository; the
//!   `markdown` rendering committed as the expected output for the wave 4 pull-request comment)
//! - Contract: [Wave 3 plan § 1.5](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#15-interfaces-and-contracts-this-wave-freezes)
//! - Decisions: [ADR-0015](../../../docs/adr/0015-stable-violation-id.md),
//!   [ADR-0008](../../../docs/adr/0008-exit-code-contract.md)
//! - Requirement: [FR-CLI-01](../../../docs/prd.md#fr-cli-01)
//!
//! The fixture pair in `tests/fixtures/diff/` is two real cruise results of the tree [`tree`]
//! writes: `old.json` before and `new.json` after `src/routes/b.ts` swaps its import of the store
//! for one of the web layer. The first test proves they still are what `cruise -T json` writes
//! (the working-directory path in `optionsUsed.baseDir` written as `.`); the `expected.*` files
//! are the renderings. `RB_UPDATE_SNAPSHOTS=1` regenerates all five, a reviewable diff.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

type Result<T = ()> = std::result::Result<T, Box<dyn Error>>;

const BIN: &str = env!("CARGO_BIN_EXE_rulebearing");

const CONFIG: &str = r#"forbidden:
  - name: routes-not-to-db
    severity: error
    comment: "Routes reach the store through a service (adr:0010)"
    fix: Call the store through src/services instead of importing src/db
    from: { path: "^src/routes/" }
    to: { path: "^src/db/" }
  - name: routes-not-to-web
    severity: error
    comment: "Routes return data; the web layer renders it (adr:0010)"
    fix: Return data from the route and render it in src/web
    from: { path: "^src/routes/" }
    to: { path: "^src/web/" }
rules:
  ratchets:
    - name: routes-via-service
      from: { path: "^src/routes/" }
      to: { path: "^src/db/" }
      budget: budgets/routes-via-service.json
"#;

/// `src/routes/b.ts` before: it reads the store, as `src/routes/a.ts` does.
const B_BEFORE: &str = "import { store } from \"../db/store\";\nexport const b = store;\n";
/// `src/routes/b.ts` after: it renders a view instead.
const B_AFTER: &str = "import { view } from \"../web/view\";\nexport const b = view;\n";

/// The variables git sets for a hook; each command here runs without them, so a test run from a
/// `pre-push` hook does not reach this repository (see `cache_worktree.rs`).
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

fn isolated(program: &str, dir: &Path) -> Command {
    let mut command = Command::new(program);
    command.current_dir(dir);
    for name in GIT_LOCAL_ENV {
        command.env_remove(name);
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

/// The tree before the change, in a fresh folder.
fn tree(name: &str) -> Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("rb-cli-diff-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    write(&dir, ".gitignore", ".graph/\n")?;
    write(&dir, "rulebearing.yaml", CONFIG)?;
    write(
        &dir,
        "budgets/routes-via-service.json",
        "{ \"ceiling\": 2 }\n",
    )?;
    write(&dir, "src/db/store.ts", "export const store = 1;\n")?;
    write(&dir, "src/web/view.ts", "export const view = 1;\n")?;
    write(
        &dir,
        "src/routes/a.ts",
        "import { store } from \"../db/store\";\nexport const a = store;\n",
    )?;
    write(&dir, "src/routes/b.ts", B_BEFORE)?;
    write(
        &dir,
        "src/main.ts",
        "import { a } from \"./routes/a\";\nimport { b } from \"./routes/b\";\nconsole.log(a, b);\n",
    )?;
    Ok(dir)
}

fn run(dir: &Path, args: &[&str]) -> Result<Output> {
    Ok(isolated(BIN, dir).args(args).output()?)
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/diff")
}

fn updating() -> bool {
    std::env::var_os("RB_UPDATE_SNAPSHOTS").is_some()
}

/// Compares `actual` with the committed `file`, or writes it under `RB_UPDATE_SNAPSHOTS`.
fn snapshot(file: &str, actual: &str) -> Result {
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

/// `cruise -T json` in `dir`, with the working-directory path written as `.`.
fn cruise(dir: &Path) -> Result<String> {
    let out = run(dir, &["cruise", "-T", "json"])?;
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let mut value: Value = serde_json::from_slice(&out.stdout)?;
    if let Some(options) = value
        .get_mut("summary")
        .and_then(|s| s.get_mut("optionsUsed"))
        .and_then(Value::as_object_mut)
    {
        options.insert("baseDir".into(), Value::String(".".into()));
    }
    let mut json = serde_json::to_string_pretty(&value)?;
    json.push('\n');
    Ok(json)
}

const RENDERINGS: [(&str, &str); 3] = [
    ("json", "expected.json"),
    ("markdown", "expected.md"),
    ("agent", "expected-agent.txt"),
];

#[test]
fn the_fixture_pair_is_what_cruise_writes() -> Result {
    let dir = tree("pair")?;
    let old = cruise(&dir)?;
    write(&dir, "src/routes/b.ts", B_AFTER)?;
    let new = cruise(&dir)?;
    snapshot("old.json", &old)?;
    snapshot("new.json", &new)?;
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn the_three_renderings_are_committed_and_deterministic() -> Result {
    let dir = fixtures();
    for (output_type, file) in RENDERINGS {
        let out = run(&dir, &["diff", "old.json", "new.json", "-T", output_type])?;
        assert_eq!(
            out.status.code(),
            Some(0),
            "{output_type}: {}",
            text(&out.stderr)
        );
        assert!(out.stderr.is_empty(), "{}", text(&out.stderr));
        let again = run(&dir, &["diff", "old.json", "new.json", "-T", output_type])?;
        assert_eq!(out.stdout, again.stdout, "{output_type}: two runs differ");
        snapshot(file, &text(&out.stdout))?;
    }
    Ok(())
}

#[test]
fn the_fixture_pair_has_one_of_each_change() -> Result {
    let out = run(&fixtures(), &["diff", "old.json", "new.json"])?;
    let diff: Value = serde_json::from_slice(&out.stdout)?;
    let count = |key: &str| diff[key].as_array().map_or(0, Vec::len);
    for key in [
        "addedEdges",
        "removedEdges",
        "newViolations",
        "resolvedViolations",
        "ratchets",
    ] {
        assert_eq!(count(key), 1, "{key}: {diff}");
    }
    assert_eq!(diff["addedEdges"][0]["from"], "src/routes/b.ts");
    assert_eq!(diff["addedEdges"][0]["to"], "src/web/view.ts");
    assert_eq!(diff["removedEdges"][0]["to"], "src/db/store.ts");
    assert_eq!(diff["newViolations"][0]["rule"], "routes-not-to-web");
    assert_eq!(
        diff["newViolations"][0]["fix"],
        "Return data from the route and render it in src/web"
    );
    assert!(
        diff["newViolations"][0]["id"]
            .as_str()
            .is_some_and(|id| id.starts_with("RB-"))
    );
    assert_eq!(diff["resolvedViolations"][0]["rule"], "routes-not-to-db");
    assert_eq!(
        diff["ratchets"][0],
        serde_json::json!({ "name": "routes-via-service", "before": 2, "after": 1 })
    );
    assert!(diff.get("base").is_none() && diff.get("head").is_none());

    // Swapped, added and removed swap and nothing else is new.
    let back = run(&fixtures(), &["diff", "new.json", "old.json"])?;
    let back: Value = serde_json::from_slice(&back.stdout)?;
    assert_eq!(back["addedEdges"][0]["to"], "src/db/store.ts");
    assert_eq!(back["newViolations"][0]["rule"], "routes-not-to-db");
    assert_eq!(back["ratchets"][0]["after"], 2);

    // A result against itself is empty.
    let same = run(
        &fixtures(),
        &["diff", "new.json", "new.json", "-T", "agent"],
    )?;
    assert_eq!(
        text(&same.stdout),
        "diff: 0 new violations, 0 resolved; 0 edges added, 0 removed; 0 ratchets changed\n"
    );
    Ok(())
}

#[test]
fn exit_code_counts_new_error_violations_and_output_to_writes_a_file() -> Result {
    let dir = fixtures();
    let gated = run(&dir, &["diff", "old.json", "new.json", "--exit-code"])?;
    assert_eq!(
        gated.status.code(),
        Some(1),
        "one new error-severity violation"
    );
    let reverse = run(&dir, &["diff", "new.json", "old.json", "-e", "-T", "agent"])?;
    assert_eq!(reverse.status.code(), Some(1));
    let same = run(&dir, &["diff", "old.json", "old.json", "-e"])?;
    assert_eq!(same.status.code(), Some(0));

    let out_dir = std::env::temp_dir().join(format!("rb-cli-diff-out-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out_dir);
    let target = out_dir.join("nested/diff.md");
    let target_arg = target.to_string_lossy().into_owned();
    let old = dir.join("old.json").to_string_lossy().into_owned();
    let new = dir.join("new.json").to_string_lossy().into_owned();
    let written = run(
        &dir,
        &["diff", &old, &new, "-T", "markdown", "-f", &target_arg],
    )?;
    assert_eq!(written.status.code(), Some(0), "{}", text(&written.stderr));
    assert!(written.stdout.is_empty());
    assert_eq!(
        std::fs::read_to_string(&target)?,
        std::fs::read_to_string(dir.join("expected.md"))?
    );
    let _ = std::fs::remove_dir_all(&out_dir);
    Ok(())
}

#[test]
fn unreadable_inputs_and_wrong_command_lines_are_named() -> Result {
    let dir = fixtures();
    let missing = run(&dir, &["diff", "old.json", "no-such.json"])?;
    assert_eq!(missing.status.code(), Some(2));
    assert!(
        text(&missing.stderr).contains("cannot read")
            && text(&missing.stderr).contains("no-such.json"),
        "{}",
        text(&missing.stderr)
    );

    let scratch = std::env::temp_dir().join(format!("rb-cli-diff-bad-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    write(&scratch, "not-a-result.json", "{\"hello\": 1}\n")?;
    let bad = scratch
        .join("not-a-result.json")
        .to_string_lossy()
        .into_owned();
    let old = dir.join("old.json").to_string_lossy().into_owned();
    let foreign = run(&dir, &["diff", &old, &bad])?;
    assert_eq!(foreign.status.code(), Some(2));
    assert!(
        text(&foreign.stderr).contains("is not a cruise result"),
        "{}",
        text(&foreign.stderr)
    );

    for (args, needle) in [
        (vec!["diff", "old.json"], "give two cruise results"),
        (
            vec!["diff", "old.json", "new.json", "extra.json"],
            "give two cruise results",
        ),
        (vec!["diff"], "give two cruise results"),
        (
            vec!["diff", "old.json", "new.json", "-c", "rulebearing.yaml"],
            "apply to --base only",
        ),
        (
            vec!["diff", "old.json", "new.json", "--no-cache"],
            "apply to --base only",
        ),
        (
            vec!["diff", "old.json", "new.json", "-T", "err"],
            "not an output type of diff",
        ),
    ] {
        let out = run(&dir, &args)?;
        assert_eq!(out.status.code(), Some(3), "{args:?}");
        assert!(
            text(&out.stderr).contains(needle),
            "{args:?}: {}",
            text(&out.stderr)
        );
    }

    // --base outside a repository.
    let outside = run(&scratch, &["diff", "--base", "main"])?;
    assert_eq!(outside.status.code(), Some(2));
    assert!(
        text(&outside.stderr).contains("not inside a git repository"),
        "{}",
        text(&outside.stderr)
    );
    let _ = std::fs::remove_dir_all(&scratch);
    Ok(())
}

/// A repository with the tree committed before and after the change.
fn repository(name: &str) -> Result<(PathBuf, String, String)> {
    let dir = tree(name)?;
    git(&dir, &["init", "--quiet"])?;
    git(&dir, &["add", "."])?;
    git(&dir, &["commit", "--quiet", "-m", "routes read the store"])?;
    let first = git(&dir, &["rev-parse", "HEAD"])?;
    write(&dir, "src/routes/b.ts", B_AFTER)?;
    git(&dir, &["commit", "--quiet", "-am", "b renders a view"])?;
    let second = git(&dir, &["rev-parse", "HEAD"])?;
    Ok((dir, first, second))
}

fn entries(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir.join(".graph/cache"))
        .map(|r| {
            r.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// The diff without `base` and `head`, pretty-printed as `diff` prints it.
fn without_sides(stdout: &[u8]) -> Result<String> {
    let mut value: Value = serde_json::from_slice(stdout)?;
    if let Some(map) = value.as_object_mut() {
        // `retain` keeps the order; `remove` on an order-preserving map swaps the last key in.
        map.retain(|key, _| key != "base" && key != "head");
    }
    let mut json = serde_json::to_string_pretty(&value)?;
    json.push('\n');
    Ok(json)
}

#[test]
fn base_compares_a_revision_with_the_working_tree() -> Result {
    let (dir, first, second) = repository("base")?;
    let worktrees_before = git(&dir, &["worktree", "list", "--porcelain"])?;

    let out = run(&dir, &["diff", "--base", "HEAD~1"])?;
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let value: Value = serde_json::from_slice(&out.stdout)?;
    assert_eq!(
        value["base"],
        serde_json::json!({ "revision": "HEAD~1", "sha": first })
    );
    assert_eq!(value["head"], serde_json::json!({ "sha": second }));
    // The same diff as the committed fixture pair, which is the same change cruised by hand.
    assert_eq!(
        without_sides(&out.stdout)?,
        std::fs::read_to_string(fixtures().join("expected.json"))?
    );

    // The checkout is gone, the working tree is as it was, and the base graph is cached.
    assert_eq!(
        git(&dir, &["worktree", "list", "--porcelain"])?,
        worktrees_before
    );
    assert_eq!(git(&dir, &["status", "--porcelain"])?, "");
    let cached = entries(&dir);
    assert_eq!(cached.len(), 1, "{cached:?}");
    let key: Value = serde_json::from_str(&std::fs::read_to_string(
        dir.join(".graph/cache").join(&cached[0]).join("key.json"),
    )?)?;
    assert_eq!(key["head"], first.as_str());

    // Asked again, the answer is the same and no second entry is written.
    let again = run(&dir, &["diff", "--base", "HEAD~1"])?;
    assert_eq!(again.stdout, out.stdout, "two runs differ");
    assert_eq!(entries(&dir), cached);

    // The entry is what the next run reads: with `src/routes/b.ts` stripped of its import in the
    // cached graph, nothing is removed any more. `--no-cache` checks the commit out again, gives
    // the true answer, and leaves the entry as it found it.
    let graph_file = dir.join(".graph/cache").join(&cached[0]).join("graph.json");
    let mut graph: Value = serde_json::from_str(&std::fs::read_to_string(&graph_file)?)?;
    for module in graph["modules"].as_array_mut().into_iter().flatten() {
        if module["source"] == "src/routes/b.ts" {
            module["dependencies"] = Value::Array(Vec::new());
        }
    }
    let tampered = serde_json::to_string(&graph)?;
    std::fs::write(&graph_file, &tampered)?;
    let from_cache = run(&dir, &["diff", "--base", "HEAD~1"])?;
    let from_cache: Value = serde_json::from_slice(&from_cache.stdout)?;
    assert_eq!(from_cache["removedEdges"], serde_json::json!([]));
    let no_cache = run(&dir, &["diff", "--base", &first, "--no-cache"])?;
    assert_eq!(
        no_cache.status.code(),
        Some(0),
        "{}",
        text(&no_cache.stderr)
    );
    assert_eq!(
        without_sides(&no_cache.stdout)?,
        without_sides(&out.stdout)?
    );
    assert_eq!(entries(&dir), cached, "--no-cache writes nothing");
    assert_eq!(std::fs::read_to_string(&graph_file)?, tampered);

    // A corrupt entry is a miss: the base is checked out again and the entry rewritten.
    std::fs::write(&graph_file, "{ half")?;
    let rebuilt = run(&dir, &["diff", "--base", "HEAD~1"])?;
    assert_eq!(rebuilt.stdout, out.stdout);
    assert_ne!(std::fs::read_to_string(&graph_file)?, "{ half");

    // The markdown rendering is the committed one, with the two commits named.
    let markdown = run(&dir, &["diff", "--base", "HEAD~1", "-T", "markdown"])?;
    let expected = std::fs::read_to_string(fixtures().join("expected.md"))?.replacen(
        "## Architecture diff\n\n",
        &format!("## Architecture diff\n\nBase `HEAD~1` at `{first}`, head `{second}`.\n\n"),
        1,
    );
    assert_eq!(text(&markdown.stdout), expected);

    // Against its own commit a clean tree has no diff; an uncommitted edit is the head side.
    let clean = run(&dir, &["diff", "--base", "HEAD", "-T", "agent"])?;
    assert_eq!(
        text(&clean.stdout),
        "diff: 0 new violations, 0 resolved; 0 edges added, 0 removed; 0 ratchets changed\n"
    );
    write(&dir, "src/routes/b.ts", B_BEFORE)?;
    let edited = run(&dir, &["diff", "--base", "HEAD", "-e"])?;
    assert_eq!(
        edited.status.code(),
        Some(1),
        "the edit brings routes-not-to-db back"
    );
    let edited: Value = serde_json::from_slice(&edited.stdout)?;
    assert_eq!(edited["newViolations"][0]["rule"], "routes-not-to-db");
    assert_eq!(edited["resolvedViolations"][0]["rule"], "routes-not-to-web");
    assert_eq!(edited["ratchets"][0]["before"], 1);
    assert_eq!(edited["ratchets"][0]["after"], 2);
    assert_eq!(
        git(&dir, &["worktree", "list", "--porcelain"])?,
        worktrees_before
    );

    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn base_names_an_unknown_revision_and_an_empty_or_missing_base_adds_everything() -> Result {
    let (dir, _, _) = repository("unknown")?;
    for reference in ["no-such-branch", "-x"] {
        let out = run(&dir, &["diff", &format!("--base={reference}")])?;
        assert_eq!(out.status.code(), Some(2), "{reference}");
        assert!(
            text(&out.stderr).contains(&format!("unknown revision `{reference}`")),
            "{}",
            text(&out.stderr)
        );
    }

    // A commit with no source at all is an empty base: everything in the working tree is added.
    git(&dir, &["checkout", "--quiet", "--orphan", "empty"])?;
    git(&dir, &["rm", "-r", "--quiet", "--cached", "src"])?;
    git(&dir, &["commit", "--quiet", "-m", "no source"])?;
    let empty = git(&dir, &["rev-parse", "HEAD"])?;
    git(&dir, &["checkout", "--quiet", "--force", "main"])?;
    let out = run(&dir, &["diff", "--base", "empty", "-e"])?;
    let value: Value = serde_json::from_slice(&out.stdout)?;
    assert_eq!(
        out.status.code(),
        Some(2),
        "two new error violations: {}",
        text(&out.stderr)
    );
    assert_eq!(value["base"]["sha"], empty.as_str());
    assert_eq!(value["addedEdges"].as_array().map(Vec::len), Some(4));
    assert_eq!(value["removedEdges"], serde_json::json!([]));
    assert_eq!(value["newViolations"].as_array().map(Vec::len), Some(2));
    assert_eq!(
        value["ratchets"],
        serde_json::json!([{ "name": "routes-via-service", "before": 0, "after": 1 }])
    );
    assert_eq!(
        git(&dir, &["worktree", "list", "--porcelain"])?
            .matches("worktree ")
            .count(),
        1
    );

    // A path the change adds is absent from the base: its edges are all added, not an error.
    write(
        &dir,
        "src/jobs/nightly.ts",
        "import { store } from \"../db/store\";\nexport const n = store;\n",
    )?;
    git(&dir, &["add", "src/jobs"])?;
    git(&dir, &["commit", "--quiet", "-m", "a nightly job"])?;
    let added = run(&dir, &["diff", "--base", "HEAD~1", "src/jobs"])?;
    assert_eq!(added.status.code(), Some(0), "{}", text(&added.stderr));
    let added: Value = serde_json::from_slice(&added.stdout)?;
    assert_eq!(
        added["addedEdges"],
        serde_json::json!([{ "from": "src/jobs/nightly.ts", "to": "src/db/store.ts", "line": 1, "column": 1 }])
    );
    assert_eq!(added["removedEdges"], serde_json::json!([]));
    // A path present on both sides next to it is still compared.
    let both = run(
        &dir,
        &["diff", "--base", "HEAD~1", "src/jobs", "src/routes"],
    )?;
    assert_eq!(both.status.code(), Some(0), "{}", text(&both.stderr));
    let both: Value = serde_json::from_slice(&both.stdout)?;
    assert_eq!(both["addedEdges"], added["addedEdges"]);

    // An invalid configuration is exit 3 before anything is checked out.
    write(&dir, "broken.yaml", "forbidden: 7\n")?;
    let broken = run(&dir, &["diff", "--base", "HEAD", "-c", "broken.yaml"])?;
    assert_eq!(broken.status.code(), Some(3), "{}", text(&broken.stderr));
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

/// `fixtures/diff/upstream-new.json` is dependency-cruiser 18.2.0's own result for the tree after
/// the change, written by the pinned upstream binary
/// (`node conformance/dependency-cruiser/upstream/dependency-cruiser/bin/dependency-cruise.mjs
/// --config .dependency-cruiser.json -T json src`, with the two forbidden rules of [`CONFIG`] in
/// that file and `baseDir` written as `.`). It carries no ids and no `dependencyKind`, so it must
/// still match Rulebearing's result of the same tree finding for finding.
#[test]
fn a_dependency_cruiser_result_matches_rulebearings_of_the_same_tree() -> Result {
    let dir = fixtures();
    for (old, new) in [
        ("upstream-new.json", "new.json"),
        ("new.json", "upstream-new.json"),
    ] {
        let out = run(&dir, &["diff", old, new, "-e"])?;
        assert_eq!(
            out.status.code(),
            Some(0),
            "{old} {new}: {}",
            text(&out.stdout)
        );
        let value: Value = serde_json::from_slice(&out.stdout)?;
        for key in [
            "addedEdges",
            "removedEdges",
            "newViolations",
            "resolvedViolations",
        ] {
            assert_eq!(value[key], serde_json::json!([]), "{old} {new}: {key}");
        }
    }
    // Across the change, the upstream result shows the same one new and one resolved violation.
    let out = run(&dir, &["diff", "old.json", "upstream-new.json"])?;
    let value: Value = serde_json::from_slice(&out.stdout)?;
    let rules = |key: &str| -> Vec<String> {
        value[key]
            .as_array()
            .into_iter()
            .flatten()
            .map(|f| f["rule"].as_str().unwrap_or_default().to_owned())
            .collect()
    };
    assert_eq!(rules("newViolations"), ["routes-not-to-web"]);
    assert_eq!(rules("resolvedViolations"), ["routes-not-to-db"]);
    Ok(())
}

#[test]
fn exit_code_mode_strict_shifts_the_count() -> Result {
    let dir = fixtures();
    let mode = |old: &str, new: &str, extra: &[&str]| -> Result<Option<i32>> {
        let mut args = vec!["diff", old, new];
        args.extend_from_slice(extra);
        Ok(run(&dir, &args)?.status.code())
    };
    let strict = ["-e", "--exit-code-mode", "strict"];
    assert_eq!(mode("old.json", "new.json", &strict)?, Some(11), "10 + one");
    assert_eq!(
        mode(
            "old.json",
            "new.json",
            &["-e", "--exit-code-mode", "default"]
        )?,
        Some(1)
    );
    assert_eq!(mode("old.json", "old.json", &strict)?, Some(0));
    let without = run(
        &dir,
        &["diff", "old.json", "new.json", "--exit-code-mode", "strict"],
    )?;
    assert_eq!(without.status.code(), Some(3), "the mode needs --exit-code");
    assert!(
        text(&without.stderr).contains("--exit-code"),
        "{}",
        text(&without.stderr)
    );
    Ok(())
}

/// A `post-checkout` planted where the first release pointed `core.hooksPath` (a fixed folder
/// under the temporary directory), in the temporary directory itself and in the repository's own
/// hooks never runs: `git worktree add` runs with an empty folder of the process's own.
#[cfg(unix)]
#[test]
fn base_runs_no_git_hook() -> Result {
    use std::os::unix::fs::PermissionsExt as _;
    let (dir, _, _) = repository("hooks")?;
    let tmp = std::env::temp_dir().join(format!("rb-cli-diff-hooks-tmp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let markers = tmp.join("markers");
    std::fs::create_dir_all(&markers)?;
    let plant = |folder: &Path, name: &str| -> Result {
        std::fs::create_dir_all(folder)?;
        let hook = folder.join("post-checkout");
        let marker = markers.join(name);
        std::fs::write(
            &hook,
            format!("#!/bin/sh\necho ran > '{}'\n", marker.display()),
        )?;
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755))?;
        Ok(())
    };
    plant(&tmp.join("rulebearing-no-git-hooks"), "old-path")?;
    plant(&tmp, "tmpdir")?;
    plant(&dir.join(".git/hooks"), "repository")?;

    let out = isolated(BIN, &dir)
        .env("TMPDIR", &tmp)
        .args(["diff", "--base", "HEAD~1", "--no-cache"])
        .output()?;
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let ran: Vec<String> = std::fs::read_dir(&markers)?
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(ran.is_empty(), "hooks ran: {ran:?}");
    let leftovers: Vec<String> = std::fs::read_dir(&tmp)?
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("rulebearing-no-hooks-") || n.starts_with("rulebearing-diff-"))
        .collect();
    assert!(leftovers.is_empty(), "left behind: {leftovers:?}");

    // The planted hook is live: git pointed at the old folder runs it, so the test can see one.
    let probe = tmp.join("probe");
    let probe_arg = probe.to_string_lossy().into_owned();
    let hooks = format!(
        "core.hooksPath={}",
        tmp.join("rulebearing-no-git-hooks").display()
    );
    let status = isolated("git", &dir)
        .args(["-c", &hooks, "worktree", "add", "--quiet", "--detach"])
        .args([probe_arg.as_str(), "HEAD"])
        .status()?;
    assert!(status.success());
    assert!(
        markers.join("old-path").is_file(),
        "the probe must trip the hook"
    );
    git(&dir, &["worktree", "remove", "--force", &probe_arg])?;
    let _ = std::fs::remove_dir_all(&tmp);
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}
