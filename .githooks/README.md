# Git hooks

Opt in once per clone:

```sh
git config core.hooksPath .githooks
```

| Hook | Runs | Typical time |
| --- | --- | --- |
| `pre-commit` | `cargo fmt --check` and the documentation link check | under a second |
| `pre-push` | `cargo lint` and the full test suite | tens of seconds |

Both start by clearing the repository variables git exports to a hook (`git rev-parse --local-env-vars`). From a linked worktree `GIT_DIR` points at this repository, and a test that runs git in a scratch folder would otherwise act on it; the unit tests also start every `git` without them ([`crates/rb-cli/src/git.rs`](../crates/rb-cli/src/git.rs)).

Both are conveniences. They can be skipped with `--no-verify`, which is why the same checks are required in [CI](../.github/workflows/ci.yml); see [ADR-0025](../docs/adr/0025-ci-and-supply-chain-hardening.md).
