# ADR-0074: The cookbook's question pages are fixtures beside the guard catalogue, and the site renders them from what the binary produced

- **Status:** Proposed (2026-10-11)
- **Date:** 2026-10-11
- **Derives from:** [plan 0004, Step 5](../plans/pending/0004-wave-4-reach.md#21-steps-for-sub-wave-4a-docs-site-playground-cookbook-live-tables) ("`site/cookbook/<question>.md` ... Each page's rules live in `site/cookbook/fixtures/<question>/rulebearing.yaml` with `examples`, and the docs build runs `rulebearing test` and `config convert` over them so the two formats cannot drift. The playground on each page is preloaded with the page's fixture."), its [§ 1.7](../plans/pending/0004-wave-4-reach.md#17-decisions-applied-and-decisions-this-wave-must-make) (the cookbook page format); [plan 0005](../plans/pending/0005-guard-catalogue.md) (the guard catalogue under `cookbook/guards/`); [ADR-0072](0072-the-docs-site-builds-from-docs-and-serves-the-released-schemas.md) (the generator)
- **Amends:** the paths plan 0004 Step 5 names. The pages and their fixtures live under `cookbook/questions/<question>/`, not `site/cookbook/`.
- **Constrains:** `cookbook/questions/`, `site/src/cookbook.ts`, `site/src/build.ts`, `site/build.sh`, the `guards` job of `.github/workflows/ci.yml`, `.github/workflows/docs.yml`
- **Requirements:** [FR-REACH-01](../prd.md#fr-reach-01)

## Context

Plan 0004 asks for one cookbook page per architecture question, each with the rule in both configuration formats, the edge that breaks it and the fix, with the rules in a fixture that the docs build runs through `rulebearing test` and `config convert`, and a playground preloaded with the fixture. It places the pages and fixtures under `site/`.

Three facts shape where they go and how a page is kept honest:

- **There is already a cookbook.** Plan 0005's guard catalogue keeps its recipes under `cookbook/guards/<slug>/`, each a fixture that `cookbook/guards/run.sh` holds to the binary in the `guards` CI job, with the dependency-cruiser conversion committed beside it as `converted/`. A second cookbook under `site/` would be a second fixture layout and a second place a reader looks.
- **A page shows output.** The dependency-cruiser form of a rule and the violations a repository produces are what `config convert` and `cruise` print. Typed into a page by hand, they drift the first time the converter or the engine changes.
- **The playground extracts source.** `cruise_ts` and `cruise_py` take a repository's files (ADR-0073), so a page's playground can load a repository rather than a graph, and the violations on the page can come from the same files.

## Decision

**One cookbook, two halves.** The question pages live in `cookbook/questions/<question>/`, beside `cookbook/guards/`. Each is a fixture:

| File | Holds |
| --- | --- |
| `README.md` | the page's prose, with a `<!-- cookbook:<part> -->` line where each generated part goes |
| `rulebearing.yaml` | the rules, each with `fix`, a decision token and `examples` |
| `repo/` | a small repository that breaks the rules at least once |
| `expected.json` | the violations `cruise` finds in `repo/` |
| `converted/` | `config convert --to dependency-cruiser` and its report of what it dropped |
| `imported/` | for an import-linter question, what `import import-linter` writes for `repo/.importlinter`, and its violations |

**The binary writes every generated file, and a runner holds them to it.** `cookbook/questions/run.sh` runs `rulebearing test` (no rule without examples), `config lint --require-comment-token --strict-compat`, `config convert`, `cruise` in `repo/` and, for an import-linter question, `import import-linter` and a `cruise` with what it wrote, and byte-compares each with the committed file; `RB_UPDATE_SNAPSHOTS=1` rewrites them. It also requires at least one violation, and that the imported rules find the same edges as the page's rules unless the import writes slice rules, whose findings name slices rather than modules. It runs in the `guards` CI job beside the guard runner, and in `site/build.sh` before the site is generated, so neither a pull request nor a deployment can carry a page that disagrees with the binary.

**The site renders a page from the committed files and nothing else.** `site/src/cookbook.ts` replaces each marker with the file it stands for (the YAML, the converted JSON and the dropped report, the violations as a table, the contract and the imported rules) and with a playground whose embedded example is the fixture's `repo/` and `rulebearing.yaml`, loaded as soon as the module is ready. A page that lacks a marker its fixture calls for, or carries one it has no content for, fails the build, as does a question the index `cookbook/questions/README.md` does not link. The generator needs no binary: it reads the files the runner checked.

## Consequences

- One fixture layout family and one place to look: `cookbook/`. The site lists both halves under a Cookbook group.
- On GitHub a question page reads as a README that links its files; on the site the same page shows them inline.
- A change to the converter, the engine or the importer that alters a page fails the `guards` job on that pull request. Regenerating the fixture touches `cookbook/`, which starts the docs build, so the deployed page follows.
- The docs build compiles `rb-cli` for the host as well as `rb-wasm`, which lengthens it by that compile.
- A cycle between two folders takes two edges, and `rulebearing test` builds one edge per example, so a folder-scoped cycle rule carries `allowed` examples only; its forbidden side is the cycle in `repo/`, held by `expected.json`.
- `rb-wasm`'s tests run `cruise_ts` and `cruise_py` over every question's `repo/` and compare the violations with `expected.json`, and the browser test runs a question page's preloaded playground, so the playground and the page agree with the command line.

## Alternatives considered

| Option | Why not |
| --- | --- |
| `site/cookbook/<question>.md` with fixtures under `site/cookbook/fixtures/`, as the plan's text has it | A second cookbook beside `cookbook/guards/`, with its own layout, its own runner and a source tree that mixes documents with the generator's code. |
| Run `config convert` inside the generator and embed its output | The generator would need the binary in every test and every build, and the committed fixture would no longer show a reviewer what changed. The runner gives the same guarantee with a diff. |
| Hand-write the converted form and the violations in the page, with a test that compares them | Two copies of the same text, one of them in prose; the generated copy is the one that is right. |
| A graph per question, as the guard catalogue has | The playground could only evaluate a pasted graph. A repository lets a reader see the import that breaks the rule and run the extraction too. |
