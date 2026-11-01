# ADR-0081: The YAML budget scales with the text

- **Status:** Accepted (2026-11-01)
- **Date:** 2026-11-01
- **Derives from:** [ADR-0079](0079-yaml-is-parsed-with-serde-saphyr.md) (the parser and its options), [architecture § Security posture](../architecture.md#security-posture) (untrusted input ends in a named reason, never a hang or a panic)
- **Supersedes:** the `budget` row of [ADR-0079](0079-yaml-is-parsed-with-serde-saphyr.md), "the crate's default". The rest of that decision stands
- **Constrains:** `crates/rb-config/src/yaml.rs`, `from_yaml` in `crates/rb-ingest/src/declared.rs`
- **Requirements:** [FR-CFG-01](../prd.md#fr-cfg-01), [NFR-SEC-01](../prd.md#nfr-sec-01)

## Context

ADR-0079 kept `serde-saphyr`'s default budget, which allows 250,000 nodes and 1,000,000 events in a document. `serde_yaml`, which it replaced, had no such limit. The first nightly after the swap (run [41344094939](https://github.com/benbahrenburg/rulebearing/actions/runs/41344094939)) failed `init` on apache/superset, umbraco/Umbraco-CMS, zulip/zulip and getsentry/sentry with `budget breached: Nodes { nodes: 250001 }`, because `init` reads back the configuration it wrote and a large repository's known-violations list passes that limit. No committed fixture is that large, so the workspace suite passed.

## Decision

The limits that count a document's size take the larger of the crate's default and a figure from the text's length in bytes: `max_nodes` and `max_total_scalar_bytes` the length, `max_events` twice the length. Every other limit stays at the crate's default, including the alias, anchor, depth and input-size limits.

A node takes at least one byte of text and a collection's two events take at least two, so a document that uses no alias cannot breach these limits at any size. Only expanding an alias produces nodes the text does not spell out, and that is what the budget is for.

## Consequences

- A configuration of any size up to the 256 MiB input limit parses, as it did under `serde_yaml`.
- An alias bomb is bounded by its own text's size. A larger text allows proportionally more expansion, so the memory a bomb can cost grows linearly with the size of the file, not with its alias depth.
- A fixed-vector test holds the budget at the default for small text and at the length for large text. A test parses documents past the default limits, and a test proves a bomb in 5 MB of padding is still an error.
- `rb-ingest` repeats the calculation, as it repeats the other options, because it does not depend on `rb-config`.

## Alternatives considered

| Alternative | Why not |
| --- | --- |
| A larger fixed limit | Moves the same failure to a larger repository, and raises what a small bomb may cost |
| No node or event limit (`options.budget = None`) | Removes the bound on alias expansion, which is the protection ADR-0079 chose this parser's budget for |
| `init` not reading back what it wrote | Fixes one caller. A user's own large configuration, and `cruise` reading the configuration `init` wrote, fail the same way |
