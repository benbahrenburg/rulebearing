//! Two worktrees of one repository at different commits keep separate cache entries, and a
//! question asked in one is answered from its own graph.
//!
//! - Plan: [Wave 2, Step 13](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#213-step-13-worktree-aware-cache-and-the-eslint-plugin-2g)
//!   (the two-worktree integration test)
//! - Source: [design § The agentic engineering hat](../../../docs/artifacts/design.md#the-agentic-engineering-hat-turn-two)
//!   ("parallel agents in separate worktrees do not invalidate each other's caches or share stale
//!   graphs")
//! - Requirement: [FR-CLI-05](../../../docs/prd.md#fr-cli-05)
//!
//! At the first commit `src/web/view.ts` imports the domain, so a new import from the domain to the
//! web layer would close a cycle; at the second it does not. The linked worktree sits at the first
//! commit and asks first, filling its cache; the main worktree, at the second, must still answer
//! `yes`, which it cannot do from the other's graph.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

type Result<T = ()> = std::result::Result<T, Box<dyn Error>>;

const BIN: &str = env!("CARGO_BIN_EXE_rulebearing");

const CONFIG: &str = "forbidden:
  - name: no-cycles
    severity: error
    comment: \"adr:0010\"
    from: {}
    to: { circular: true }
";

/// The variables git sets for a hook (`git rev-parse --local-env-vars`). Run from a `pre-push`
/// hook, they would point every git call below, and the binary's own, at this repository
/// instead of the scratch one, so each command here runs without them.
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

/// `program`, run in `dir` without the inherited repository variables.
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

fn can_import(dir: &Path) -> Result<Output> {
    Ok(isolated(BIN, dir)
        .args(["can-import", "src/domain/model.ts", "src/web/view.ts"])
        .output()?)
}

/// The cache entries under a worktree, by folder name.
fn entries(dir: &Path) -> Result<Vec<String>> {
    let mut names: Vec<String> = std::fs::read_dir(dir.join(".graph/cache"))?
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    Ok(names)
}

fn recorded_head(dir: &Path, entry: &str) -> Result<String> {
    let text = std::fs::read_to_string(dir.join(".graph/cache").join(entry).join("key.json"))?;
    let value: serde_json::Value = serde_json::from_str(&text)?;
    Ok(value["head"].as_str().unwrap_or_default().to_owned())
}

fn repository() -> Result<(PathBuf, PathBuf, String, String)> {
    let base = std::env::temp_dir().join(format!("rb-cli-worktrees-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let main = base.join("main");
    let linked = base.join("linked");
    std::fs::create_dir_all(&main)?;
    git(&main, &["init", "--quiet"])?;
    write(&main, ".gitignore", ".graph/\n")?;
    write(&main, "rulebearing.yaml", CONFIG)?;
    write(&main, "src/domain/model.ts", "export const d = 1;\n")?;
    write(
        &main,
        "src/web/view.ts",
        "import { d } from \"../domain/model\";\nexport const w = d;\n",
    )?;
    git(&main, &["add", "."])?;
    git(
        &main,
        &["commit", "--quiet", "-m", "web imports the domain"],
    )?;
    let first = git(&main, &["rev-parse", "HEAD"])?;
    write(&main, "src/web/view.ts", "export const w = 1;\n")?;
    git(&main, &["commit", "--quiet", "-am", "web stands alone"])?;
    let second = git(&main, &["rev-parse", "HEAD"])?;
    let linked_arg = linked.to_string_lossy().into_owned();
    git(
        &main,
        &[
            "worktree",
            "add",
            "--quiet",
            "--detach",
            &linked_arg,
            &first,
        ],
    )?;
    Ok((main, linked, first, second))
}

#[test]
fn two_worktrees_at_different_heads_keep_separate_graphs() -> Result {
    let (main, linked, first, second) = repository()?;
    assert!(
        linked.join(".git").is_file(),
        "a linked worktree's .git is a file"
    );

    let old = can_import(&linked)?;
    assert_eq!(
        old.status.code(),
        Some(1),
        "at the first commit the import closes a cycle: {}{}",
        String::from_utf8_lossy(&old.stdout),
        String::from_utf8_lossy(&old.stderr)
    );
    assert!(String::from_utf8_lossy(&old.stdout).contains("rule: no-cycles"));

    let new = can_import(&main)?;
    assert_eq!(
        new.status.code(),
        Some(0),
        "the main worktree answers from its own graph: {}",
        String::from_utf8_lossy(&new.stdout)
    );

    let (linked_entries, main_entries) = (entries(&linked)?, entries(&main)?);
    assert_eq!(linked_entries.len(), 1);
    assert_eq!(main_entries.len(), 1);
    assert_ne!(linked_entries, main_entries, "different cache directories");
    for name in linked_entries.iter().chain(&main_entries) {
        assert_eq!(name.len(), 16);
        assert!(name.bytes().all(|b| b.is_ascii_hexdigit()));
    }
    // HEAD came from the files, through the linked worktree's `gitdir` for the second.
    assert_eq!(recorded_head(&linked, &linked_entries[0])?, first);
    assert_eq!(recorded_head(&main, &main_entries[0])?, second);

    // Asked again, each answers the same from its own entry, and neither grows another.
    assert_eq!(can_import(&linked)?.status.code(), Some(1));
    assert_eq!(can_import(&main)?.status.code(), Some(0));
    assert_eq!(entries(&linked)?, linked_entries);
    assert_eq!(entries(&main)?, main_entries);

    // A new commit is a new HEAD: a new entry, extracted afresh, never the stale graph.
    write(
        &main,
        "src/web/view.ts",
        "import { d } from \"../domain/model\";\nexport const w = d;\n",
    )?;
    git(
        &main,
        &["commit", "--quiet", "-am", "web imports the domain again"],
    )?;
    assert_eq!(can_import(&main)?.status.code(), Some(1));
    assert_eq!(entries(&main)?.len(), 2);

    // After `git pack-refs`, HEAD still resolves from the files alone.
    git(&main, &["pack-refs", "--all"])?;
    assert_eq!(can_import(&main)?.status.code(), Some(1));
    assert_eq!(entries(&main)?.len(), 2, "the same HEAD, the same entry");

    // An uncommitted edit is a new key too: the stale graph of the same HEAD is not read.
    write(&main, "src/web/view.ts", "export const w = 1;\n")?;
    assert_eq!(
        can_import(&main)?.status.code(),
        Some(0),
        "the edit removed the edge the committed graph still has"
    );
    assert_eq!(entries(&main)?.len(), 3);

    let _ = git(
        &main,
        &["worktree", "remove", "--force", &linked.to_string_lossy()],
    );
    if let Some(base) = main.parent() {
        let _ = std::fs::remove_dir_all(base);
    }
    Ok(())
}

/// The reviewer's case: `a` imports `b` in the commit; `b` imports `c` only in the working tree.
/// Asked before and after the edit, `can-import c a` must follow the files, not the commit.
#[test]
fn an_uncommitted_edit_is_never_answered_from_the_committed_graph() -> Result {
    let dir = std::env::temp_dir().join(format!("rb-cli-stale-edit-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    git(&dir, &["init", "--quiet"])?;
    write(&dir, ".gitignore", ".graph/\n")?;
    write(&dir, "rulebearing.yaml", CONFIG)?;
    write(
        &dir,
        "src/a.ts",
        "import { b } from \"./b\";\nexport const a = b;\n",
    )?;
    write(&dir, "src/b.ts", "export const b = 1;\n")?;
    write(&dir, "src/c.ts", "export const c = 1;\n")?;
    git(&dir, &["add", "."])?;
    git(&dir, &["commit", "--quiet", "-m", "a imports b"])?;
    let ask = || -> Result<Output> {
        Ok(isolated(BIN, &dir)
            .args(["can-import", "src/c.ts", "src/a.ts"])
            .output()?)
    };
    assert_eq!(ask()?.status.code(), Some(0), "no cycle at the commit");
    write(
        &dir,
        "src/b.ts",
        "import { c } from \"./c\";\nexport const b = c;\n",
    )?;
    let after = ask()?;
    assert_eq!(
        after.status.code(),
        Some(1),
        "c -> a -> b -> c closes a cycle once b imports c: {}",
        String::from_utf8_lossy(&after.stdout)
    );
    assert!(String::from_utf8_lossy(&after.stdout).contains("rule: no-cycles"));
    // An untracked file counts as well: a new d imported by c.
    write(&dir, "src/d.ts", "export const d = 1;\n")?;
    assert_eq!(ask()?.status.code(), Some(1));
    let names = entries(&dir)?;
    assert_eq!(
        names.len(),
        3,
        "one entry per state of the files: {names:?}"
    );
    let key: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(
        dir.join(".graph/cache").join(&names[0]).join("key.json"),
    )?)?;
    for field in [
        "root",
        "head",
        "configHash",
        "configFiles",
        "version",
        "inputs",
    ] {
        assert!(key.get(field).is_some(), "key.json has {field}: {key}");
    }
    assert_eq!(key["version"], env!("CARGO_PKG_VERSION"));
    // Asked again without a change, the entry is read, not written again.
    assert_eq!(ask()?.status.code(), Some(1));
    assert_eq!(entries(&dir)?.len(), 3);
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}
