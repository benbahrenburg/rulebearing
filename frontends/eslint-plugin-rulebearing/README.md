# eslint-plugin-rulebearing

One ESLint rule, `rulebearing/boundaries`, that reports an import the rulebearing gate would fail, at the import, with the rule's name, its `fix` and the violation id. Agents run ESLint on every edit, so a boundary violation is fixed before the cruise runs ([design § Two front-ends that will matter more than the MCP server](../../docs/artifacts/design.md#two-front-ends-that-will-matter-more-than-the-mcp-server), [ADR-0021](../../docs/adr/0021-agent-surface-cli-first.md), [FR-DIST-04](../../docs/prd.md#fr-dist-04), [plan 0002, Step 13](../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#213-step-13-worktree-aware-cache-and-the-eslint-plugin-2g)).

```sh
npm install --save-dev eslint-plugin-rulebearing
```

```javascript
// eslint.config.js
import rulebearing from 'eslint-plugin-rulebearing';

export default [{ plugins: { rulebearing }, rules: { 'rulebearing/boundaries': 'error' } }];
```

The plugin depends on the [`rulebearing`](../../wrappers/npm/README.md) package at the same version and runs the binary that package installs; `RULEBEARING_BINARY` points both at a local build.

## What it reports

A violation reads exactly as the gate's `junit` failure message words the same edge, with the rule's name in front:

```text
ui-not-to-db: Import from src/services instead of src/db.
RB-ecea7134 src/ui/view.ts -> src/db/query.ts (line 1, column 1)
```

The first line is the rule and its `fix` (a rule without one reads ``1 violation(s) of `<rule>` ``, as in `junit`); the second is the stable violation id ([ADR-0015](../../docs/adr/0015-stable-violation-id.md)), the edge and where the import sits. The id is the one the cruise, a baseline and SARIF give the edge, so an agent can cite it or look it up with `rulebearing explain`.

When the binary cannot answer (it is not installed, the configuration is invalid, the graph cannot be read), the import is reported as `rulebearing could not answer: <reason>` rather than passing silently.

## How it answers

| Step | What happens |
| --- | --- |
| Import forms | `import ... from`, `export * from`, `export { } from`, `import()` and `require()` with a string (or a template literal without expressions) |
| Resolution | Off the cached graph's `modules[]`, with no second resolver: the `resolved` path of the dependency the graph records for that specifier in this file; else, for a relative specifier the graph has not recorded yet, the module among `modules[].source` it names by extension or `index` file; else, for a relative specifier no module matches (a file created since the graph was extracted), the first of the same candidates that is a file on disk; else, for a bare specifier, what the graph resolves it to elsewhere. A bare specifier the graph cannot name is left to the gate |
| The graph | The `graph` option when given; otherwise the newest entry under `.graph/cache/` whose `key.json` names this worktree root (a Windows `//?/` prefix and the drive letter's case do not count), its current `HEAD`, and a configuration whose files re-hash to the recorded `configHash` (and include the `config` option's file when given). With none, `rulebearing impact <file> --json` writes one first (a miss extracts) |
| The question | `rulebearing can-import <from> <to> --json`, in ESLint's working folder. `no` is reported; `yes` is not |
| Memoisation | One question per distinct target per file per lint run; the graph is read once per change |

The answer is the gate's own, from the same configuration and the same graph: the plugin evaluates no rule itself. The binary's cache key also follows uncommitted edits and the build's version, so `can-import` answers from the files as they are; an import added since the graph was cached is resolved by the relative-path step (or off the disk) and asked as a new edge, as `can-import` answers before the import exists ([agents.md](../../docs/agents.md#questions-before-the-import-is-written)).

## Options

| Option | Default | Meaning |
| --- | --- | --- |
| `config` | the binary's search in the working folder | `--config` for the binary: the rules file |
| `graph` | the worktree-aware cache | A graph document (such as `.graph/cruise.json` from `rulebearing cruise -T json -f .graph/cruise.json`) to resolve against and answer from |
| `severity` | `error` | The lowest rule severity reported: `error` reports what fails the gate; `warn` and `info` add the rules that only warn or inform |

```javascript
rules: {
  'rulebearing/boundaries': ['error', { config: 'rulebearing.yaml', severity: 'warn' }],
}
```

## Developing this package

TypeScript under `src/` (strict, ESM, Node 22), compiled to `dist/`, linted by the root configuration through `cargo xtask lint` ([ADR-0023](../../docs/adr/0023-documentation-link-and-lint-gates.md)). In this repository `rulebearing` is the npm wrapper's source (the root `tsconfig.json` and `vitest.config.ts` map it), and the build reads the wrapper's declarations, so build the wrapper first. The tests hold the 70% line floor of [ADR-0018](../../docs/adr/0018-test-coverage-threshold.md), set in `vitest.config.ts`.

```sh
npm ci                                                 # once, at the repository root
(cd wrappers/npm && npm run build)                     # the wrapper's declarations
npm run build                                          # src/ to dist/
RULEBEARING_BINARY=../../target/debug/rulebearing npm test   # vitest run --coverage
```

Without `RULEBEARING_BINARY` the test setup runs `cargo build -p rb-cli` and uses `target/debug/rulebearing`. The agreement test copies `test/fixture/` to a temporary folder, runs `rulebearing cruise -T junit` over it and asserts that every message the rule reports equals the junit failure message of the same edge, for each import form, from the cache and from a saved graph, and for an import written after the graph was cached. The package is published from [release.yml](../../.github/workflows/release.yml) at the release version.
