# AGENTS.md

Read [CLAUDE.md](CLAUDE.md) first: it is the working agreement for this repository. The section below is generated from [rulebearing.yaml](rulebearing.yaml) and checked by the `self-check` job in [ci.yml](.github/workflows/ci.yml) with `rulebearing docs --format agents-md --verify --out AGENTS.md`; regenerate it with `rulebearing docs --format agents-md --out AGENTS.md` after changing a rule. The rules are evaluated over the crate graph (`scripts/cargo-graph.sh`), so a "file" below is a crate's `Cargo.toml`.

<!-- rulebearing:agents-md:begin -->
## Architecture rules

Generated from `rulebearing.yaml` by `rulebearing docs --format agents-md`; change the rules, not this section. Each line is one rule: what it forbids or requires, its severity, what to do when it fires, and the decision it serves. Ask `rulebearing can-import <from> <to>` before writing an import, and `rulebearing explain <rule>` when one fires.

### `crates/`

- **no-circular-crates** (forbidden, error): Files matching `crates/` may not be part of an import cycle. Fix: Move the shared type down into the crate both sides already depend on, usually rb-model. Decision: [adr:0010](docs/adr/0010-crate-layout-and-extractor-boundary.md).

### `crates/rb-<x>/`

- **engine-is-language-agnostic** (forbidden, error): Files matching `crates/rb-<x>/` may not import files matching `crates/rb-extract-`. Fix: Put the language-specific fact into the graph document at extraction time and match on it as a string. Decision: [adr:0010](docs/adr/0010-crate-layout-and-extractor-boundary.md), [adr:0014](docs/adr/0014-no-invented-cross-language-edges.md).

### `crates/rb-cli/Cargo.toml`

- **cli-links-every-extractor-behind-a-feature** (required, warn): Every file matching `crates/rb-cli/Cargo.toml` must reach a file matching `crates/rb-extract-<y>/`. Fix: Add the extractor to rb-cli's dependencies as an optional dependency behind its extract-<language> feature. Decision: [adr:0010](docs/adr/0010-crate-layout-and-extractor-boundary.md).

### `crates/rb-extract-<x>/`

- **extractors-only-read-the-model** (forbidden, error): Files matching `crates/rb-extract-<x>/` may not import files matching `crates/rb-<y>/`. Fix: Pass per-language options as a plain struct defined in rb-model instead of reading rb-config. Decision: [adr:0010](docs/adr/0010-crate-layout-and-extractor-boundary.md).

### `crates/rb-model/`

- **model-depends-on-nothing** (forbidden, error): Files matching `crates/rb-model/` may not import files matching `crates/rb-` (except `crates/rb-model/`). Fix: Move the type into rb-model, or move the logic that needs the other crate out of rb-model. Decision: [adr:0010](docs/adr/0010-crate-layout-and-extractor-boundary.md).

<!-- rulebearing:agents-md:end -->
