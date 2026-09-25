# pytest-rulebearing

Each Rulebearing architecture rule as a test in the pytest run: one item per rule, failing with the rule's `fix` and its first violations. Source, documentation and releases: https://github.com/benbahrenburg/rulebearing

```sh
pip install pytest-rulebearing        # installs the rulebearing wheel for this platform too
pytest --rulebearing                  # or set rulebearing = true, below
```

```toml
[tool.pytest.ini_options]
rulebearing = true
rulebearing_config = "rulebearing.yaml"   # optional; the binary finds one by default
rulebearing_args = ["src"]                # optional; what to cruise
```

## What it does

When `--rulebearing` is passed or `rulebearing = true` is set, the plugin runs `rulebearing cruise --output-type json` once, during collection, and adds one item per rule, named `rulebearing::<rule>` and marked `rulebearing` (so `-m rulebearing` and `-k` select them). It never evaluates a rule: the binary decides, the plugin reports ([ADR-0010](../../../docs/adr/0010-crate-layout-and-extractor-boundary.md) rule 4).

The failure message is exactly the message the `junit` reporter writes for that rule ([crates/rb-report/src/junit.rs](../../../crates/rb-report/src/junit.rs), [Wave 2 plan § 1.5](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#15-interfaces-and-contracts-this-wave-freezes)):

| Rule outcome | Item | Message |
| --- | --- | --- |
| error-severity violations | fails | the `fix` (or `N violation(s) of <rule>`), then the first five violations as `id from -> to (line L, column C)`, then `... and N more` |
| vacuous ([ADR-0007](../../../docs/adr/0007-vacuous-rules-fail-by-default.md)) | fails | ``rule `<name>` is vacuous: its <side> side matched nothing, so it checks nothing (ADR-0007)`` |
| expired rule or known violation, a ratchet over its ceiling or without a budget | fails | the junit error or failure text |
| warn, info or known findings only | passes | the findings in a `rulebearing` report section |
| no violation | passes | |

A run that writes no result (the binary is missing, or the configuration is invalid) is one failing item, `rulebearing::cruise`, whose message is the command, its exit code and its stderr.

| Option | Ini key | Meaning |
| --- | --- | --- |
| `--rulebearing` | `rulebearing` | turn the plugin on |
| `--rulebearing-config FILE` | `rulebearing_config` | `--config` |
| `--rulebearing-graph FILE` | `rulebearing_graph` | `--graph`: read a graph document instead of extracting |
| `--rulebearing-binary FILE` | `rulebearing_binary` | the binary; default `RULEBEARING_BINARY`, then the one the `rulebearing` wheel carries |
| `--rulebearing-arg ARG` (repeatable) | `rulebearing_args` | further arguments, such as directories to cruise |

Paths on the command line are relative to the invocation directory; paths in the ini file are relative to the ini file, which is also where the binary runs.

## Developing

The plugin is linted and type-checked by the root configuration through `cargo xtask lint` ([ADR-0023](../../../docs/adr/0023-documentation-link-and-lint-gates.md)) and holds the 70% line floor of [ADR-0018](../../../docs/adr/0018-test-coverage-threshold.md). The tests run against the shared fixture [adapters/fixture](../../fixture/README.md) with the binary built from the checkout; `tests/test_junit_equality.py` is the proof that every message equals `rulebearing cruise -T junit`'s.

```sh
cargo build --release -p rb-cli
pip install pytest pytest-cov hatchling packaging
pytest adapters/python --cov=adapters/python          # from the repository root
```

Plan: [Wave 2, Step 14](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#214-step-14-test-adapters-and-wrappers-2h). Release: [docs/release.md](../../../docs/release.md).
