# Git hooks

Opt in once per clone:

```sh
git config core.hooksPath .githooks
```

| Hook | Runs | Typical time |
| --- | --- | --- |
| `pre-commit` | `cargo fmt --check` and the documentation link check | under a second |
| `pre-push` | `cargo lint` and the full test suite | tens of seconds |

Both are conveniences. They can be skipped with `--no-verify`, which is why the same checks are required in [CI](../.github/workflows/ci.yml); see [ADR-0025](../docs/adr/0025-ci-and-supply-chain-hardening.md).
