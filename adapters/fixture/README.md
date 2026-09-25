# adapters/fixture

The one repository every test adapter runs against, so the Python and TypeScript adapters prove the same thing: that the failure message of each rule is the `junit` reporter's message for that rule ([crates/rb-report/src/junit.rs](../../crates/rb-report/src/junit.rs), [Wave 2 plan § 1.5](../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#15-interfaces-and-contracts-this-wave-freezes), [Step 14](../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#214-step-14-test-adapters-and-wrappers-2h)).

The Python files under `py/` are extractor input, not code: they exist to produce edges and one badly named class, and are excluded from `ruff` and `mypy` in the root [pyproject.toml](../../pyproject.toml).

| Rule | Outcome under `rulebearing cruise` | Branch of the message it proves |
| --- | --- | --- |
| `handlers-not-to-util` | seven error violations | the `fix`, the first five violations with id, from, to, line and column, then `... and 2 more` |
| `api-not-to-util` | one warn violation | passes; the finding is output, not a failure |
| `nothing-matches` | vacuous | fails with the liveness reason ([ADR-0007](../../docs/adr/0007-vacuous-rules-fail-by-default.md)) |
| `util-is-a-leaf` | no violation | passes |
| `classes-are-pascal-case` | one element violation | the declaration's line and column from `code.types` |

A second configuration, [unnamed.yaml](unnamed.yaml), runs over the same files: three anonymous dependency rules (each named `unnamed`) and an element rule also named `unnamed`, which the `junit` reporter lists as `unnamed`, `unnamed#2`, `unnamed#3` and `unnamed#4`, each with only its own violations ([crates/rb-report/src/catalog.rs](../../crates/rb-report/src/catalog.rs)). It also sets `options.outputTo`, which the adapters must neither read from nor write, because they always pass `--output-to -`.

The run exits 2, because a vacuous rule makes the run untrustworthy ([ADR-0008](../../docs/adr/0008-exit-code-contract.md)); the JSON is still written, and the adapters read it.

```sh
rulebearing cruise -T json  --no-progress   # what the adapters read, from this directory
rulebearing cruise -T junit --no-progress   # what their messages are compared with
rulebearing cruise -T junit --output-to - --no-progress --config unnamed.yaml
```
