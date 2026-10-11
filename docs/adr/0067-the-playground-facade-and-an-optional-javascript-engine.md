# ADR-0067: The playground's wasm facade, and the JavaScript engine as an optional feature

- **Status:** Proposed (2026-10-10)
- **Date:** 2026-10-10
- **Derives from:** [plan 0004, Step 2](../plans/pending/0004-wave-4-reach.md#21-steps-for-sub-wave-4a-docs-site-playground-cookbook-live-tables) and its decision rule in [§ 1.7](../plans/pending/0004-wave-4-reach.md#17-decisions-applied-and-decisions-this-wave-must-make) ("Every crate that compiles to `wasm32-unknown-unknown` without a patch; one that does not (the candidates are `rquickjs` and any `notify` or process code in `rb-cli`) is feature-gated out of `rb-wasm`, and the playground page says which inputs it therefore cannot take"); [ADR-0002](0002-rust-as-implementation-language.md) (the playground as the consequence of Rust)
- **Amends:** [ADR-0010](0010-crate-layout-and-extractor-boundary.md)'s crate table, by one row. ADR-0010's rules are unchanged.
- **Constrains:** `crates/rb-wasm`, `crates/rb-config` (`js-config` feature), the workspace's `rb-config` dependency, `crates/rb-cli/tests/crate_boundary.rs`
- **Requirements:** [FR-REACH-01](../prd.md#fr-reach-01), [FR-CFG-03](../prd.md#fr-cfg-03)

## Context

Plan 0004 adds a browser playground: the library crates compiled to `wasm32-unknown-unknown` behind a thin facade, `rb-wasm`, whose exports take and return strings. ADR-0010's table names every crate and the crates it may depend on, and the crate-boundary test holds the workspace to it. `rb-wasm` is not in the table.

Every library crate builds for the target. One links badly: `rquickjs-sys` 0.14, the QuickJS engine behind JavaScript configurations and `plugin:` reporters ([ADR-0006](0006-embedded-quickjs-config-evaluator.md)). Its wasm bindings declare `JS_DupValue` and `JS_FreeContext` as functions, but QuickJS's C source emits neither as a symbol for that target, so the module fails to link. Fixing that means patching the dependency, which the plan's rule says not to do.

## Decision

**`rb-wasm` joins ADR-0010's table as a facade.** It may depend on `rb-model`, `rb-config`, `rb-rules`, `rb-report` and `rb-ingest`, and no crate depends on it. [ADR-0073](0073-extractors-read-files-through-rb-model.md) adds `rb-extract-ts` and `rb-extract-python` to the row, for `cruise_ts` and `cruise_py`. It never links `rb-cli`. The logic it needs from `rb-cli`'s `cruise --graph` path moves into `rb-rules`, so both call the same code:
- `reset`, which clears an earlier evaluation from a saved graph;
- `report_format`, the report's filters and collapse.

**QuickJS becomes `rb-config`'s `js-config` feature,** on by default in `rb-config` and asked for explicitly by `rb-cli`.
- The workspace's `rb-config` dependency turns default features off, so that `rb-rules` does not pull the engine into the playground.
- Without the feature, every type, constant and policy of the JavaScript module stays. Evaluating a JavaScript configuration is `JsError::Unavailable`, exit 3, naming the file and the way out: "write it as YAML or JSON".
- JSON5, which the engine reads, is refused in the same words.
- `rb_config::js::JS_ENGINE` says which build is running.

The playground page states that it takes YAML and JSON configurations only.

## Consequences

- The command line is unchanged: `rb-cli` always has the engine, and its tests, the sandbox-escape tests among them, run with it.
- `cargo test -p rb-config --no-default-features` runs in the `wasm` CI job, so the build without the engine is tested, and its refusals have tests of their own.
- Tested on its own, `rb-wasm` has no engine, as in the browser. Tested beside `rb-cli`, Cargo's feature unification gives it one. Its test of a JavaScript configuration asserts whichever build it is in.
- When a later `rquickjs` release links for the target, turning `js-config` on in `rb-wasm` brings JavaScript configurations to the playground with no other change.

## Alternatives considered

| Option | Why not |
| --- | --- |
| Patch `rquickjs-sys` with the two missing symbols | A patched dependency is what the plan's rule rules out, and the sandbox is a security boundary ([ADR-0006](0006-embedded-quickjs-config-evaluator.md)) that should run exactly the engine the command line runs. |
| Evaluate JavaScript configurations with the browser's own engine | Breaks the rule that a configuration runs only in the sandbox, never a page-level `eval` (plan 0004 § 1.7, ADR-0006). |
| Leave `rb-wasm` out of the crate table and the test | The table is how the dependency direction is enforced. A crate outside it is unguarded. |
