# ADR-0079: YAML is parsed with serde-saphyr, with options that keep what serde_yaml meant

- **Status:** Proposed (2026-11-01)
- **Date:** 2026-11-01
- **Derives from:** [ADR-0025](0025-ci-and-supply-chain-hardening.md) (`cargo deny check licenses advisories bans sources` is a required check), [ADR-0005](0005-native-config-superset-and-compat.md) (the native format is YAML), [architecture § Technology choices](../architecture.md#technology-choices)
- **Supersedes:** nothing. It replaces the `serde_yaml` row of the serialisation choice in [architecture § Technology choices](../architecture.md#technology-choices), which cites [ADR-0004](0004-graph-document-is-cruise-result-superset.md) for the graph document only.
- **Constrains:** `Cargo.toml` (`[workspace.dependencies]`), `crates/rb-config/src/yaml.rs`, every YAML read in `rb-config`, `rb-cli` and `rb-ingest`, the YAML `config convert` and `config expand` write
- **Requirements:** [FR-CFG-01](../prd.md#fr-cfg-01), [NFR-SEC-01](../prd.md#nfr-sec-01)

## Context

Every YAML text the tool reads went through `serde_yaml` 0.9: the native configuration and dependency-cruiser's YAML configurations, the presets the binary carries, `fleet.yaml`, `edges.yaml` and OpenAPI server documents, the generated-examples editor, `init`, `propose`, and the test suites' fixtures. `config convert` and `config expand` wrote their YAML with `serde_yaml::to_string`.

On 2026-11-01 `cargo deny` started failing on `main` and on every open pull request with [RUSTSEC-2024-0449](https://rustsec.org/advisories/RUSTSEC-2024-0449): `unsafe-libyaml`, which `serde_yaml` parses with, is archived and unmaintained. It is a machine translation of libyaml's C into `unsafe` Rust, and the advisory states that no safe upgrade exists. `serde_yaml` itself has been marked deprecated since 2024. The workspace forbids `unsafe` in its own code ([CLAUDE.md § Rust conventions](../../CLAUDE.md#rust-conventions)) and parses configurations from repositories it does not control, so the parser is on the untrusted-input path [architecture § Security posture](../architecture.md#security-posture) describes.

## Decision

**YAML is parsed with `serde-saphyr` 2.0 (MIT OR Apache-2.0),** a serde front-end over a YAML parser written in safe Rust. `rb_config::yaml` holds the one set of parser options and the `from_str` and `from_slice` every crate calls. `rb-ingest`, which does not depend on `rb-config`, repeats the same options beside its two reads.

**`config convert` and `config expand` keep the bytes they wrote.** `serde-saphyr`'s serialiser folds long strings and double-quotes where `serde_yaml` single-quoted, which would have changed every committed `expanded.yaml` and converted fixture. `rb_config::yaml::to_string` writes the block layout itself (a sequence's `- ` at its key's indentation, `{}` and `[]` for empty collections, a string with a newline as a literal block chomped by its trailing newlines) and asks `serde-saphyr`, under YAML 1.2 quoting, only whether one scalar may be plain. When it may not, the scalar is single-quoted, or double-quoted with escapes when it holds a control character. A fixed-vector test pins the layout, and a property test checks that every written mapping reads back equal.

The options keep what the existing configurations, presets and fixtures meant under `serde_yaml` 0.9:

| Option | Value | What `serde_yaml` 0.9 did |
| --- | --- | --- |
| `strict_booleans` | on | Only `true` and `false` (any case) are booleans; `yes`, `no`, `on`, `off` and `y` stay strings, as YAML 1.2 has it |
| `merge_keys` | `AsOrdinary` | `<<` is an ordinary key; `serde_yaml` merged only through `Value::apply_merge`, which nothing called |
| `duplicate_keys` | `LastWins` | Into `serde_json::Value`, the last of two equal keys won |
| `reject_unsupported_tags` | on | An unknown local tag (`!foo`) was an error |
| `non_finite_float_policy` | `PassThrough` | `.inf` and `.nan` reached `serde_json::Value` as `null` |
| `with_snippet` | off | One-line errors with the line and column; the callers wrap them in their own named reason and exit code |
| `budget` | the crate's default | Bounds alias expansion, depth and document size, so an alias bomb or a self-referencing alias is an error, never a hang or a panic |

A test in `rb_config::yaml` pins each row, and property tests check that arbitrary text never panics and that what `to_string` writes reads back equal.

**What still differs**, measured against `serde_yaml` 0.9 over the same inputs:

| Input | `serde_yaml` 0.9 | Now |
| --- | --- | --- |
| Error text | `did not find expected ',' or ']' at line 2 column 1, ...` | `unclosed bracket '[' at line 1, column 4`. The exit code and the named reason around it are unchanged |
| A leading-zero or underscored number (`017`, `1_000`) | the string `"017"`, `"1_000"` | the numbers 17.0 and 1000 |
| A null or boolean mapping key (`~: a`) | the key `"~"` | an error |
| `!!timestamp` | the string | an error |
| A tab before a top-level key | an error | accepted |

None of the configurations under `conformance/`, `cookbook/`, `presets/`, `testbeds/` or the test fixtures use the input forms in the middle rows, and the conformance gates, the guard catalogue and the question pages pass unchanged. A configuration that relied on one of them now loads with a different value, or fails with exit 3 naming the file, rather than being read wrongly in silence.

## Consequences

- `cargo deny check licenses advisories bans sources` passes again with an empty `[advisories] ignore`, and no YAML is parsed by `unsafe` code.
- The YAML `config convert` and `config expand` write is unchanged byte for byte on every committed fixture (the guard catalogue's `expanded.yaml`, the question pages' converted configurations). It now depends on `rb_config::yaml::to_string`, about a hundred lines this repository owns, rather than on a serialiser's defaults.
- The parser options live in one function. A change to one of them changes what users' configurations mean, so it is a new ADR.
- `serde-saphyr` requires Rust 1.89 and the workspace's minimum is 1.97, so the `msrv` job is unaffected.

## Alternatives considered

- **`serde_yaml_ng` or `serde_yml`.** Both are forks of `serde_yaml` and parse with `unsafe-libyaml` or its fork `libyml`. The advisory names them, so the check would keep failing.
- **`noyalib`.** Pure safe Rust with serde support, but at 0.0.x its API and behaviour make no stability promise, and it has far fewer users.
- **An entry in `deny.toml`'s `[advisories] ignore`.** It would make the check pass while leaving an unmaintained `unsafe` parser on the untrusted-input path. It would also be the first exception in a list ADR-0025 keeps empty. Rejected, because the working agreement fixes the code rather than widening an allowance to get green.
- **Writing a YAML parser.** No. The subset the configurations use is small, but the inputs are arbitrary repositories' files, and a well-tested parser is the safer choice.
