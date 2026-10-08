//! Every `git` this crate starts, from one constructor.
//!
//! - Architecture: [`docs/architecture.md#security-posture`](../../../docs/architecture.md#security-posture)
//! - Decision: [ADR-0025](../../../docs/adr/0025-ci-and-supply-chain-hardening.md) (the opt-in
//!   git hooks that run the test suite)
//! - Plan: [Wave 3](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md)
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

use std::process::Command;

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

#[cfg(test)]
mod tests {
    use super::*;

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
