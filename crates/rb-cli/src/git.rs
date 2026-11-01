//! Every `git` this crate starts, from one constructor.
//!
//! - Architecture: [`docs/architecture.md#security-posture`](../../../docs/architecture.md#security-posture)
//! - Decision: [ADR-0025](../../../docs/adr/0025-ci-and-supply-chain-hardening.md) (the opt-in
//!   git hooks that run the test suite)
//! - Plan: [Wave 3](../../../docs/plans/implemented/0003-wave-3-operations-surface-inner-loop.md)
//!
//! Each caller sets the folder git runs in and means the repository at that folder. Git exports
//! the variables of `git rev-parse --local-env-vars` to a hook, and from a linked worktree
//! `GIT_DIR` is an absolute path to that worktree's repository. A unit test that inherits them
//! and runs `git init`, `git commit` or `git worktree add` in a scratch folder writes to that
//! repository instead: on 2026-10-08 a pre-push hook run from a worktree set `core.bare` and
//! committed to the pushed branch. Under `cfg(test)` the constructor removes those variables,
//! so a unit test only ever reaches its own scratch repository. A release build passes them on
//! unchanged, because a hook that runs `rulebearing` (a pre-commit hook under `git commit -a`,
//! with its temporary index) means them.
//!
//! [`Snapshot`] is the one `git status` of a run. Listing the untracked files walks the whole
//! worktree, and on a large one that walk is most of a warm run: `--affected` needs the
//! untracked files, the cache's change detection needs them, and the entry it writes records
//! them with the added files. A run takes the status once and each of the three reads it
//! ([NFR-PERF-02](../../../docs/prd.md#nfr-perf-02), the Stop hook's p95;
//! [Performance model](../../../docs/architecture.md#performance-model)).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, OnceLock};

/// The variables git sets for a hook (`git rev-parse --local-env-vars`), as the integration
/// tests list them.
#[cfg(test)]
pub(crate) const HOOK_VARIABLES: [&str; 15] = [
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

/// A `git` command, without the hook's repository variables when built for unit tests.
pub(crate) fn command() -> Command {
    #[cfg_attr(
        not(test),
        expect(unused_mut, reason = "only a test build removes variables")
    )]
    let mut command = Command::new("git");
    #[cfg(test)]
    for name in HOOK_VARIABLES {
        command.env_remove(name);
    }
    command
}

/// The arguments of the status a [`Snapshot`] takes: every untracked file listed (not its
/// folder), no rename pairing, NUL-terminated entries whose paths are never quoted.
pub const STATUS_ARGS: [&str; 5] = [
    "status",
    "--porcelain=v1",
    "-z",
    "--untracked-files=all",
    "--no-renames",
];

/// One entry of `git status --porcelain=v1 -z --no-renames`: the index and worktree letters and
/// the path, relative to the worktree root, as git wrote its bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The index's letter (`?` for an untracked file, `A` for one added to the index).
    pub staged: u8,
    /// The worktree's letter.
    pub unstaged: u8,
    /// The path, relative to the worktree root.
    pub path: Vec<u8>,
}

impl Entry {
    /// Whether git lists the file as untracked (`??`).
    pub fn is_untracked(&self) -> bool {
        self.staged == b'?' && self.unstaged == b'?'
    }
}

/// The entries of [`STATUS_ARGS`]' output, in git's order. Without renames every entry is
/// `XY PATH` and a NUL; a field shorter than that is no entry and is skipped.
pub fn parse_status(output: &[u8]) -> Vec<Entry> {
    output
        .split(|b| *b == 0)
        .filter_map(|field| match field {
            [staged, unstaged, b' ', path @ ..] if !path.is_empty() => Some(Entry {
                staged: *staged,
                unstaged: *unstaged,
                path: path.to_vec(),
            }),
            _ => None,
        })
        .collect()
}

/// The `git status` of the worktree a run is in, taken the first time a step asks and shared by
/// every step of the run after it (`--affected`, the cache's change detection, the presence set
/// the entry records). A clone is the same snapshot. A run makes a new one, so a long-lived
/// process (`guard --watch`) never answers from an old status.
///
/// Taking it once is never less safe than taking it at each step: the cache records what was
/// present when the status was taken, before the extraction began, so a file that appears later
/// is not recorded and the next run sees it as added.
#[derive(Debug, Clone)]
pub struct Snapshot(Arc<Taken>);

#[derive(Debug)]
struct Taken {
    cwd: PathBuf,
    root: OnceLock<PathBuf>,
    status: OnceLock<Option<Vec<Entry>>>,
}

impl Snapshot {
    /// The snapshot of the worktree holding `cwd`; nothing runs until a step asks.
    pub fn new(cwd: &Path) -> Self {
        Self(Arc::new(Taken {
            cwd: cwd.to_path_buf(),
            root: OnceLock::new(),
            status: OnceLock::new(),
        }))
    }

    /// Takes the status on a thread of its own now, so it runs beside whatever the run does
    /// before it first asks; that step then waits for it, or finds it taken. A run that never
    /// asks has spent a `git status`, nothing else.
    pub fn prefetch(&self) {
        let snapshot = self.clone();
        // Detached: a step that asks waits on the cell, not on the thread.
        drop(std::thread::spawn(move || {
            let _ = snapshot.status_of(snapshot.root());
        }));
    }

    /// The worktree root, as [`crate::cache::key::worktree_root`] names it.
    pub fn root(&self) -> &Path {
        self.0
            .root
            .get_or_init(|| crate::cache::key::worktree_root(&self.0.cwd))
    }

    /// The status of the worktree at `root`: `None` when `root` is not this snapshot's worktree
    /// or git cannot answer, and the caller then asks git itself.
    pub fn status_of(&self, root: &Path) -> Option<&[Entry]> {
        if self.root() != root {
            return None;
        }
        self.0
            .status
            .get_or_init(|| {
                command()
                    .args(STATUS_ARGS)
                    .current_dir(self.root())
                    .output()
                    .ok()
                    .filter(|o| o.status.success())
                    .map(|o| parse_status(&o.stdout))
            })
            .as_deref()
    }

    /// The untracked files of the worktree at `root`, in git's order, as [`Self::status_of`].
    pub fn untracked_of(&self, root: &Path) -> Option<Vec<&[u8]>> {
        self.status_of(root).map(untracked)
    }

    /// The untracked files of the worktree holding `cwd`, when this is that folder's snapshot;
    /// `None` for another folder, or when git cannot answer.
    pub fn untracked_from(&self, cwd: &Path) -> Option<Vec<&[u8]>> {
        if self.0.cwd != cwd {
            return None;
        }
        self.untracked_of(self.root())
    }
}

/// The paths of the untracked entries, in git's order.
fn untracked(entries: &[Entry]) -> Vec<&[u8]> {
    entries
        .iter()
        .filter(|e| e.is_untracked())
        .map(|e| e.path.as_slice())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// A repository holding `tracked.ts`, added and not committed.
    fn repository(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rb-git-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(std::fs::create_dir_all(&dir).is_ok());
        assert!(std::fs::write(dir.join("tracked.ts"), "x\n").is_ok());
        for args in [&["init", "--quiet"][..], &["add", "tracked.ts"][..]] {
            let done = command().args(args).current_dir(&dir).status();
            assert!(done.is_ok_and(|s| s.success()));
        }
        dir
    }

    fn untracked_names(snapshot: &Snapshot) -> Vec<String> {
        snapshot
            .untracked_of(snapshot.root())
            .unwrap_or_default()
            .into_iter()
            .map(|p| String::from_utf8_lossy(p).into_owned())
            .collect()
    }

    #[test]
    fn status_entries_are_read_field_by_field() {
        let parsed = parse_status(b"?? a b.ts\0A  c.ts\0 M d/e.ts\0\0x\0?? \0");
        let entry = |staged, unstaged, path: &str| Entry {
            staged,
            unstaged,
            path: path.as_bytes().to_vec(),
        };
        assert_eq!(
            parsed,
            vec![
                entry(b'?', b'?', "a b.ts"),
                entry(b'A', b' ', "c.ts"),
                entry(b' ', b'M', "d/e.ts"),
            ],
            "a field too short to be an entry is skipped"
        );
        assert!(parsed[0].is_untracked());
        assert!(!parsed[1].is_untracked());
        assert!(
            !Entry {
                staged: b'?',
                unstaged: b' ',
                path: Vec::new()
            }
            .is_untracked()
        );
        assert!(
            !Entry {
                staged: b' ',
                unstaged: b'?',
                path: Vec::new()
            }
            .is_untracked()
        );
    }

    proptest! {
        #[test]
        fn what_git_writes_is_what_is_read(
            entries in proptest::collection::vec(
                (proptest::sample::select(b" AMDRTU?!".to_vec()), "[^\0]{1,12}"),
                0..8,
            ),
        ) {
            let mut output = Vec::new();
            for (letter, path) in &entries {
                output.extend_from_slice(&[*letter, b'M', b' ']);
                output.extend_from_slice(path.as_bytes());
                output.push(0);
            }
            let parsed = parse_status(&output);
            prop_assert_eq!(parsed.len(), entries.len());
            for (entry, (letter, path)) in parsed.iter().zip(&entries) {
                prop_assert_eq!(entry.staged, *letter);
                prop_assert_eq!(entry.unstaged, b'M');
                prop_assert_eq!(&entry.path, path.as_bytes());
            }
        }
    }

    #[test]
    fn a_snapshot_lists_each_untracked_file_as_ls_files_does() {
        let dir = repository("lists");
        assert!(std::fs::create_dir_all(dir.join("new/deep")).is_ok());
        for file in ["new/a.ts", "new/deep/b b.ts", "é.ts", "ignored.log"] {
            assert!(std::fs::write(dir.join(file), "x\n").is_ok());
        }
        assert!(std::fs::write(dir.join(".gitignore"), "*.log\n").is_ok());
        let snapshot = Snapshot::new(&dir);
        let mut listed = untracked_names(&snapshot);
        listed.sort();
        let others = command()
            .args(["ls-files", "--others", "--exclude-standard", "-z"])
            .current_dir(&dir)
            .output();
        let others = others.map(|o| o.stdout).unwrap_or_default();
        let mut expected: Vec<String> = others
            .split(|b| *b == 0)
            .filter(|f| !f.is_empty())
            .map(|p| String::from_utf8_lossy(p).into_owned())
            .collect();
        expected.sort();
        assert_eq!(listed, expected);
        assert!(listed.contains(&"new/deep/b b.ts".to_owned()));
        assert!(!listed.contains(&"ignored.log".to_owned()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_run_takes_the_status_once_and_a_new_run_takes_it_again() {
        let dir = repository("once");
        let snapshot = Snapshot::new(&dir);
        assert_eq!(untracked_names(&snapshot), Vec::<String>::new());
        assert!(std::fs::write(dir.join("later.ts"), "x\n").is_ok());
        assert_eq!(
            untracked_names(&snapshot.clone()),
            Vec::<String>::new(),
            "a clone is the same snapshot, and the file came after it"
        );
        assert_eq!(untracked_names(&Snapshot::new(&dir)), vec!["later.ts"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_prefetched_status_is_the_one_a_step_reads() {
        let dir = repository("prefetch");
        assert!(std::fs::write(dir.join("before.ts"), "x\n").is_ok());
        let snapshot = Snapshot::new(&dir);
        snapshot.prefetch();
        // The step waits for the status the thread is taking, or takes it first; either way one
        // status answers, and it holds the file written before the run.
        assert_eq!(untracked_names(&snapshot), vec!["before.ts"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_snapshot_answers_only_for_its_own_worktree_and_folder() {
        let dir = repository("own");
        let other = repository("other");
        let snapshot = Snapshot::new(&dir);
        let root = snapshot.root().to_path_buf();
        assert!(snapshot.status_of(&root).is_some());
        assert!(snapshot.untracked_of(&root).is_some());
        assert!(snapshot.untracked_from(&dir).is_some());
        assert!(snapshot.status_of(&other).is_none());
        assert!(snapshot.untracked_of(&other).is_none());
        assert!(snapshot.untracked_from(&other).is_none());
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&other);
    }

    #[test]
    fn outside_a_repository_a_snapshot_has_no_answer() {
        let dir = std::env::temp_dir().join(format!("rb-git-none-{}", std::process::id()));
        assert!(std::fs::create_dir_all(&dir).is_ok());
        // GIT_CEILING is not needed: the temporary folder is outside any repository here, and if
        // it were inside one the snapshot would be that repository's, which `status_of` reports.
        let snapshot = Snapshot::new(&dir.join("does-not-exist"));
        assert!(snapshot.status_of(snapshot.root()).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_test_build_runs_git_without_the_hook_s_variables() {
        let command = command();
        assert_eq!(command.get_program(), "git");
        let removed: Vec<_> = command
            .get_envs()
            .filter(|(_, value)| value.is_none())
            .map(|(name, _)| name.to_string_lossy().into_owned())
            .collect();
        for name in HOOK_VARIABLES {
            assert!(removed.iter().any(|r| r == name), "{name} is passed on");
        }
        assert!(
            command.get_envs().all(|(_, value)| value.is_none()),
            "nothing is set, only removed"
        );
    }
}
