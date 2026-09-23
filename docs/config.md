# Configuration

Rulebearing reads two configuration formats into one model: dependency-cruiser's, unchanged, and its own native format, which is a superset. Decisions: [ADR-0005](adr/0005-native-config-superset-and-compat.md) (the two formats), [ADR-0006](adr/0006-embedded-quickjs-config-evaluator.md) (JavaScript in a sandbox), [ADR-0016](adr/0016-linear-time-regex-and-strict-compat.md) and [ADR-0028](adr/0028-backreferences-by-instantiation-on-the-linear-engine.md) (regular expressions). Source: [design § Configuration](artifacts/design.md#configuration-a-native-format-and-dependency-cruisers-as-it-is). The rule language itself is in [rules.md](rules.md).

## Finding the file

`--config FILE` names it; `--config -` reads stdin; `--no-config` runs without one. With `--config` and no value, or with no flag at all on the commands that need a configuration, the first of these found in the working directory is used:

| Order | Name | Format |
| --- | --- | --- |
| 1 | `rulebearing.yaml`, `.yml`, `.json`, `.jsonc`, `.toml` | native |
| 2 | `.dependency-cruiser.js`, `.cjs`, `.mjs`, `.json`, `.yaml`, `.yml` | dependency-cruiser |

`--config-format native` or `--config-format dependency-cruiser` settles a name that says neither.

## dependency-cruiser's format, as it is

A `.dependency-cruiser.js` that works with dependency-cruiser 18.2.0 works here without edits: `forbidden`, `allowed`, `allowedSeverity`, `required`, `options` and `extends`, with every key, attribute and option of the [coverage tab](artifacts/dependency-cruiser-18.2.0-coverage.md). Unknown keys are refused, as dependency-cruiser refuses them.

**JavaScript configurations** run in an embedded QuickJS sandbox with no filesystem, network or process access, and with CommonJS and ES module syntax. `require` and `import` resolve only inside the repository, and the pure `path` and `url` modules are provided ([ADR-0027](adr/0027-pure-path-and-url-modules-in-the-config-sandbox.md)). A configuration that reads files, such as one that lists a folder to build a pattern, is refused with a message naming `--config-via-node`, which evaluates it with the local Node instead.

**`extends`** takes a file, an npm package, or one of the bundled presets:

| Preset | What it is |
| --- | --- |
| `dependency-cruiser/configs/recommended`, `recommended-strict`, `recommended-warn-only` | dependency-cruiser's own, vendored with their licence ([presets/dependency-cruiser](../presets/dependency-cruiser/)) |
| `rulebearing:recommended` | Six rules, every one with a `fix` ([presets/rulebearing/recommended.yaml](../presets/rulebearing/recommended.yaml)) |
| `rulebearing:typescript` | The recommended rules plus the TypeScript settings a monorepo usually needs: type-only imports kept as edges, and export maps resolved as Node and bundlers resolve them ([presets/rulebearing/typescript.yaml](../presets/rulebearing/typescript.yaml)) |

Rules merge by name, as dependency-cruiser merges them: a rule in the extending file with the name of an extended rule replaces the attributes it names.

## The native format

```yaml
$schema: https://benbahrenburg.github.io/rulebearing/schema/config-v1.json
extends: rulebearing:recommended
defines:
  legacyApps: { fromJson: eng/legacy-apps.json, select: "apps[*].name" }
languages:
  typescript:
    tsConfig: { fileName: tsconfig.json }
    tsPreCompilationDeps: true
rules:
  dependencies:
    forbidden:
      - name: no-app-to-app
        comment: "An app never imports another app. adr:0003"
        fix: "Move the shared code into a package under packages/ and import it from both apps."
        severity: error
        from: { path: "^apps/([^/]+)/", pathNot: "^apps/(${legacyApps})/" }
        to: { path: "^apps/([^/]+)/", pathNot: "^apps/$1/" }
        examples:
          forbidden: ["apps/web/src/a.ts -> apps/admin/src/b.ts"]
          allowed: ["apps/web/src/a.ts -> apps/web/src/b.ts"]
  layers:
    - name: clean
      layers: ["^src/web/", "^src/application/", "^src/domain/"]
  independence:
    - name: features
      pattern: "^src/features/([^/]+)/"
  ratchets:
    - name: routes-via-service
      from: { path: "^apps/([^/]+)/src/app/.*/(page|route)\\.tsx?$" }
      to: { path: "^apps/$1/src/(server|domain)/" }
      budget: eng/routes-via-service-budget.json
options:
  doNotFollow: { path: node_modules }
```

The top level holds `$schema`, `extends`, `defines`, `languages`, `options`, `rules`, and dependency-cruiser's `forbidden`, `allowed`, `allowedSeverity` and `required`, which are aliases for `rules.dependencies.*`. [`schema/config-v1.json`](../schema/config-v1.json) is generated from the model and checked by a test, so an editor with YAML schema support completes and validates the file.

| Key | Holds |
| --- | --- |
| `languages.typescript` | The per-language options: `tsConfig`, `tsPreCompilationDeps`, `babelConfig`, `webpackConfig`, `enhancedResolveOptions`, `moduleSystems`, `parser`, `baseDir` and the rest of the TypeScript extractor's options. dependency-cruiser's flat option names are accepted at the top of `options` too, as aliases |
| `rules.dependencies` | dependency-cruiser's rule set: `forbidden`, `allowed`, `allowedSeverity`, `required` ([rules.md](rules.md)) |
| `rules.layers` | Paths from the highest layer to the lowest; each lower layer gets one `forbidden` rule per higher layer, named `<name>:<lower>-to-<higher>` |
| `rules.independence` | A pattern with one capturing group; a module in one group may not import a module in another |
| `rules.ratchets` | A count of direct edges that may only fall, against a budget file `{ "ceiling": n }` ([ADR-0029](adr/0029-ratchets-enforced-by-cruise-and-reported-in-the-summary.md)) |
| `defines.<name>` | A value read from a JSON file (`fromJson`), selected with dot-separated keys, `[*]` and `[n]` (`select`), escaped, and joined with `joinWith` (default `\|`); `${name}` in any pattern is replaced by it |
| `options` | Everything else of dependency-cruiser's `options`, including `knownViolations` |
| `allowEmpty` | Rules and ratchets allowed to match nothing, by name (`allowed[N]` for the Nth `allowed` entry), including rules from an extended dependency-cruiser file, which cannot carry the key itself. A name that is no rule is an error, so an exception cannot outlive its rule ([ADR-0032](adr/0032-liveness-follows-the-configuration-format.md)) |

`rulebearing config expand FILE` prints a native file with `defines` substituted and the shorthands expanded into the rules they stand for.

## Rule metadata

Every rule, in either format, may carry five fields dependency-cruiser does not have. They are what `explain`, `err-long` and the `agent` reporter print ([design § Rule metadata](artifacts/design.md#rule-metadata-that-says-what-to-do)).

| Field | Meaning |
| --- | --- |
| `fix` | The imperative to follow when the rule fires |
| `examples` | `forbidden` and `allowed` edges, `from -> to`, that `rulebearing test` checks against the rule |
| `owner` | Who answers for the rule |
| `expires` | A date after which the rule fails the run: for a temporary exception |
| `allowEmpty` | Opt the rule out of liveness: it may match nothing without failing the run ([ADR-0007](adr/0007-vacuous-rules-fail-by-default.md)). dependency-cruiser's schema refuses it on a rule, so a rule in a `.dependency-cruiser.*` file is named in the native file's top-level `allowEmpty` list instead ([ADR-0032](adr/0032-liveness-follows-the-configuration-format.md)) |

A decision token in `comment`, `adr:NNNN` or `plan:<slug>`, links the rule to the decision behind it. `--require-comment-token` makes a rule without one a configuration error (exit 3).

## Regular expressions

Patterns are JavaScript regular expressions, run on the linear-time `regex` crate through a translation table. Backreferences are supported ([ADR-0028](adr/0028-backreferences-by-instantiation-on-the-linear-engine.md)); lookaround is refused with a message. `$1` in a `to` pattern takes the first capture of `from.path`, escaped so it matches only itself. `--strict-compat` refuses what dependency-cruiser would refuse, including the nested quantifiers its safe-regex check rejects, and any Rulebearing addition, so a file that passes it also runs under dependency-cruiser.

## Between the formats

| Command | Does |
| --- | --- |
| `rulebearing config convert FILE` | dependency-cruiser to native, losslessly |
| `rulebearing config convert FILE --to dependency-cruiser` | native to dependency-cruiser, and prints what it dropped: ratchets, shorthands (expanded instead), `defines` (substituted), `languages` other than TypeScript, `fix`, `examples` and the other metadata |
| `rulebearing config expand FILE` | the native file with nothing hidden |
| `rulebearing config lint` | rules that can never match (with `--graph`), rules shadowed by an earlier rule, overlapping `allowed` entries, an `allowed` list that admits everything, a severity below `error` on a rule with no violations, rules without a `fix` or whose `fix` restates the name, and missing decision tokens under `--require-comment-token` |
