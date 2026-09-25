# dependency-cruiser 18.2.0 coverage

Every attribute in the published `configuration` and `cruise-result` schemas, every command-line flag and every output type of dependency-cruiser 18.2.0 (the current release, pinned for the conformance suite), with what Rulebearing does for it. **Parity** means the same name and the same semantics, proven by dependency-cruiser's own tests; **Parity+** means it also works for .NET and Python; **Sidecar** means handled by spawning dependency-cruiser; **Kept** means TypeScript-only by nature and unchanged. Nothing is dropped. Sources: [configuration.schema.json](https://github.com/sverweij/dependency-cruiser/blob/v18.2.0/src/schema/configuration.schema.json), [cruise-result.schema.json](https://github.com/sverweij/dependency-cruiser/blob/v18.2.0/src/schema/cruise-result.schema.json), [rules-reference.md](https://github.com/sverweij/dependency-cruiser/blob/v18.2.0/doc/rules-reference.md), [options-reference.md](https://github.com/sverweij/dependency-cruiser/blob/v18.2.0/doc/options-reference.md), [cli.md](https://github.com/sverweij/dependency-cruiser/blob/v18.2.0/doc/cli.md), [api.md](https://github.com/sverweij/dependency-cruiser/blob/v18.2.0/doc/api.md).

## Rules

| Attribute | Applies to | Rulebearing | Wave |
| --- | --- | --- | --- |
| `forbidden[]` (regular) with `name`, `comment`, `severity`, `scope`, `from`, `to` | all | Parity+ | 1 |
| `forbidden[]` reachability variant (`to.reachable`) | all | Parity+ | 1 |
| `forbidden[]` dependents variant (`module.numberOfDependentsLessThan` / `MoreThan`) | all | Parity+ | 1 |
| `allowed[]`, `allowedSeverity` | all | Parity+ | 1 |
| `required[]` with `module` and `to.reachable` | all | Parity+ | 1 |
| `scope: module` / `scope: folder` (folder-level `circular`, `moreUnstable`) | all | Parity+ | 1 |
| `extends` (string or array; file, npm package, bundled presets `recommended`, `recommended-strict`, `recommended-warn-only`) | all | Parity | 1 |
| `$0` to `$9` group matching | all | Parity+ | 1 |
| `severity`: `error`, `warn`, `info`, `ignore` | all | Parity | 1 |
| `from.path`, `from.pathNot`, `from.orphan` | all | Parity+ | 1 |
| `to.path`, `to.pathNot` | all | Parity+ | 1 |
| `to.circular`, `to.via`, `to.viaOnly`, `to.viaNot`, `to.viaSomeNot` | all | Parity+ | 1 |
| `to.dependencyTypes`, `to.dependencyTypesNot` | all (per-language vocabulary) | Parity+ | 1 |
| `to.dynamic` | all | Parity+ (.NET: literal `Assembly.Load` / `Type.GetType` / `Activator.CreateInstance`; Python: literal `importlib.import_module`) | 1 |
| `to.exoticallyRequired`, `to.exoticRequire`, `to.exoticRequireNot` | TS/JS | Kept | 1 |
| `to.license`, `to.licenseNot` | TS/JS (package.json); .NET (NuGet `.nuspec` licence); Python (installed `METADATA`) | Parity+ | 2 |
| `to.moreThanOneDependencyType` | TS/JS | Kept | 1 |
| `to.moreUnstable` | all | Parity+ | 2 |
| `to.preCompilationOnly` | TS | Kept | 1 |
| `to.couldNotResolve` | all | Parity+ | 1 |
| `to.ancestor` | all | Parity+ | 1 |
| `to.reachable` (reachability forbidden and required) | all | Parity+ | 1 |
| `module.path`, `module.pathNot` | all | Parity+ | 1 |

## Options

| Option | Rulebearing | Wave |
| --- | --- | --- |
| `affected` (git revision) | Parity+; for .NET, changed `.cs` files map through the PDB to affected types | 3 |
| `babelConfig.fileName` | Parity for what it affects: syntax needs no Babel (`oxc` parses it); `babel-plugin-module-resolver` aliases are read from the config | 1 |
| `baseDir` | Parity | 1 |
| `builtInModules` (`add`, `override`) | Parity; per Node version list bundled | 1 |
| `cache` (`folder`, `strategy: metadata` / `content`, `compress`) | Parity; .NET keys on assembly and PDB hashes | 3 |
| `collapse` (digit or regex) | Parity+ | 2 |
| `combinedDependencies` | Parity (monorepo package.json walk-up) | 1 |
| `detectJSDocImports` | Parity, implemented on the comment table rather than by switching parser | 1 |
| `detectProcessBuiltinModuleCalls` | Parity | 1 |
| `doNotFollow.path`, `doNotFollow.dependencyTypes` | Parity+ | 1 |
| `enhancedResolveOptions`: `exportsFields`, `conditionNames`, `extensions`, `mainFields`, `mainFiles`, `aliasFields`, `cachedInputFileSystem.cacheDuration`, other enhanced-resolve keys | Parity through `oxc_resolver`, which is the Rust port of enhanced-resolve; `cacheDuration` accepted and ignored (the resolver caches per run) | 1 |
| `exclude.path`, `exclude.dynamic` | Parity+ | 1 |
| `exoticRequireStrings` | Kept | 1 |
| `experimentalStats` | Parity (`experimentalStats` on modules and dependencies) | 2 |
| `externalModuleResolutionStrategy`: `node_modules` / `yarn-pnp` | Parity (`oxc_resolver` supports PnP) | 1 |
| `extraExtensionsToScan` | Parity | 1 |
| `focus.path`, `focus.depth` | Parity+ | 1 |
| `forceDeriveDependents` | Parity | 1 |
| `highlight.path` | Parity+ | 2 |
| `includeOnly.path` | Parity+ | 1 |
| `knownViolations[]` | Parity+, plus optional `expires`, `owner`, `reason` per entry | 2 |
| `maxDepth` | Parity+ | 1 |
| `metrics` | Parity+ (module, folder, and for .NET project instability) | 2 |
| `moduleSystems`: `cjs`, `es6`, `amd`, `tsd` | Parity | 1 |
| `parser`: `acorn` / `swc` / `tsc` | Accepted; one parser (`oxc`) satisfies all three, and the value is recorded in `optionsUsed` for the report | 1 |
| `prefix`, `suffix` | Parity | 1 |
| `preserveSymlinks` | Parity | 1 |
| `progress`: `none`, `cli-feedback`, `performance-log`, `ndjson` | Parity | 1 |
| `reaches.path` | Parity+ | 1 |
| `reporterOptions.anon.wordlist` | Parity | 3 |
| `reporterOptions.archi` / `dot` / `ddot` / `flat`: `collapsePattern`, `filters` (`exclude`, `focus`, `includeOnly`, `reaches`), `showMetrics`, `theme` (`graph`, `node`, `edge`, `modules[]`, `dependencies[]`, `replace`) | Parity+ | 2 |
| `reporterOptions.err` / `err-long` / `err-html`: `showAliasedModulesUnresolved`, `showExternalModulesUnresolved` | Parity | 1 |
| `reporterOptions.markdown` (18 keys: `title`, `showTitle`, `showSummary`, `showSummaryHeader`, `summaryHeader`, `showStatsSummary`, `showRulesSummary`, `includeIgnoredInSummary`, `showDetails`, `showDetailsHeader`, `detailsHeader`, `includeIgnoredInDetails`, `collapseDetails`, `collapsedMessage`, `noViolationsMessage`, `showFooter`, `showAliasedModulesUnresolved`, `showExternalModulesUnresolved`) | Parity | 3 |
| `reporterOptions.mermaid.minify` | Parity | 2 |
| `reporterOptions.metrics`: `hideFolders`, `hideModules`, `orderBy` | Parity | 2 |
| `reporterOptions.text.highlightFocused` | Parity | 1 |
| `skipAnalysisNotInRules` | Parity | 1 |
| `tsConfig.fileName` (`extends`, `paths`, `baseUrl`, project references) | Parity through `oxc_resolver`'s tsconfig support | 1 |
| `tsPreCompilationDeps`: `true` / `false` / `"specify"` | Parity; `specify` records both the pre- and post-compilation dependency sets and marks `pre-compilation-only` | 1 |
| `webpackConfig.fileName`, `env`, `arguments` | Parity; the config is evaluated in the embedded JavaScript engine and its `resolve` block read; `--webpack-config-json` for a pre-evaluated copy | 2 |

## Command line

| Flag or command | Rulebearing | Wave |
| --- | --- | --- |
| positional files, directories and globs | Parity+ | 1 |
| `--config` / `--validate`, `--no-config` | Parity; also `--config-format`, `--config -` (stdin), `--config-via-node` | 1 |
| `--init` (`oneshot` presets) | Parity, with presets per language | 2 |
| `--info` (supported transpilers and extensions) | Parity, per extractor | 1 |
| `--output-type`, `--output-to` | Parity+ | 1 |
| `--include-only`, `--focus`, `--focus-depth`, `--reaches`, `--highlight`, `--collapse`, `--exclude`, `--do-not-follow`, `--max-depth`, `--module-systems`, `--prefix` | Parity+ | 1 to 2 |
| `--affected [revision]` | Parity+ | 3 |
| `--ts-pre-compilation-deps`, `--ts-config`, `--webpack-config`, `--preserve-symlinks` | Parity | 1 to 2 |
| `--metrics`, `--no-metrics` | Parity+ | 2 |
| `--ignore-known [file]`, `--no-ignore-known` | Parity+ | 2 |
| `--cache [folder]`, `--cache-strategy`, `--no-cache` | Parity+ | 3 |
| `--progress [type]`, `--no-progress` | Parity | 1 |
| `--version`, `--help` | Parity | 1 |
| exit code = number of error-severity violations | Parity, plus 2 for an untrustworthy run and 3 for an invalid config | 1 |
| `depcruise-fmt` (`-f`, `-T`, `-I`, `-F`, `-x`, `-S`, `-e`, `-p`, `--highlight`) | `rulebearing fmt`, same flags | 1 |
| `depcruise-baseline` (`--baseline-mode`: `full` / `shrink-only` / `format`) | `rulebearing baseline`, same modes | 2 |
| `depcruise-wrap-stream-in-html` | `rulebearing wrap-html` | 3 |

## Output types

| Type | Rulebearing | Wave |
| --- | --- | --- |
| `err`, `err-long` | Parity+ | 1 |
| `err-html` | Parity | 2 |
| `json` | Parity+; additive fields only, validates against the 18.2.0 `cruise-result` schema when extensions are stripped with `--strict-schema` | 1 |
| `text`, `csv` | Parity+ | 1 |
| `teamcity`, `azure-devops` | Parity+ | 1 |
| `dot`, `ddot`, `cdot` / `archi`, `fdot` / `flat` | Parity+ | 2 |
| `x-dot-webpage` | Parity | 3 |
| `mermaid`, `d2` | Parity+ | 2 |
| `html` (matrix) | Parity | 3 |
| `markdown` | Parity | 3 |
| `anon` | Parity | 3 |
| `baseline` | Parity+ | 2 |
| `metrics` | Parity+ | 2 |
| `null` | Parity | 1 |
| `plugin:<path>` (custom JavaScript reporter) | Parity through the embedded engine, receiving the cruise result object | 3 |
| New: `sarif`, `github-annotations`, `junit`, `trx`, `agent`, `plantuml` | Additions | 1 to 3 |

## Result document (`cruise-result` schema)

| Field | Rulebearing |
| --- | --- |
| `modules[]`: `source`, `dependencies[]`, `dependents[]`, `orphan`, `valid`, `rules[]`, `reachable[]`, `reaches[]`, `instability`, `couldNotResolve`, `coreModule`, `followable`, `matchesDoNotFollow`, `matchesFocus`, `matchesReaches`, `matchesHighlight`, `consolidated`, `checksum`, `license`, `dependencyTypes`, `experimentalStats` | Parity+; added `language`, `project`, `namespaces`, `attribution` |
| `modules[].dependencies[]`: `module`, `resolved`, `moduleSystem`, `dependencyTypes`, `dynamic`, `exoticallyRequired`, `exoticRequire`, `followable`, `coreModule`, `couldNotResolve`, `matchesDoNotFollow`, `circular`, `cycle[]`, `valid`, `rules[]`, `preCompilationOnly`, `typeOnly`, `protocol`, `mimeType`, `license`, `instability` | Parity+; added `line`, `column`, `dependencyKind`, `member` |
| `folders[]`: `name`, `moduleCount`, `dependencies[]`, `dependents[]`, `afferentCouplings`, `efferentCouplings`, `instability`, `experimentalStats` | Parity+ |
| `summary`: `violations[]` (`from`, `to`, `rule`, `type`, `cycle`, `via`, `metrics`, `unresolvedTo`, `comment`), `error`, `warn`, `info`, `ignore`, `totalCruised`, `totalDependenciesCruised`, `optionsUsed`, `ruleSetUsed`, `environment` | Parity+; added `inspected` (files, assemblies, modules per language) and `vacuousRules[]` |
| `revisionData` (`SHA1`, `changes[]`) | Parity |
| New `code` section: `types[]`, `members[]`, `attributes[]`, `calls[]` | Addition; the layer element, slice and diagram rules read |

## Dependency types and module systems

| Group | Values | Rulebearing |
| --- | --- | --- |
| Aliases | `aliased`, `aliased-subpath-import`, `aliased-tsconfig`, `aliased-tsconfig-base-url`, `aliased-tsconfig-paths`, `aliased-webpack`, `aliased-workspace` | Parity (`oxc_resolver` reports which alias table resolved the specifier) |
| Module forms | `import`, `export`, `require`, `import-equals`, `dynamic-import`, `amd-define`, `amd-require`, `amd-exotic-require`, `exotic-require`, `jsdoc`, `jsdoc-bracket-import`, `jsdoc-import-tag`, `triple-slash-amd-dependency`, `triple-slash-directive`, `triple-slash-file-reference`, `triple-slash-type-reference`, `process-get-builtin-module` | Parity |
| Type-level | `type-only`, `type-import`, `pre-compilation-only` | Parity (TS); `type-only` reused for Python `TYPE_CHECKING` |
| npm | `npm`, `npm-dev`, `npm-peer`, `npm-optional`, `npm-bundled`, `npm-no-pkg`, `npm-unknown`, `deprecated` | Parity |
| Other | `core`, `local`, `localmodule`, `undetermined`, `unknown` | Parity |
| Module systems | `cjs`, `es6`, `amd`, `tsd` | Parity; .NET reports `clr`, Python `py` |

## Extraction and resolution

| Capability | Rulebearing | Wave |
| --- | --- | --- |
| JavaScript (`.js`, `.mjs`, `.cjs`, `.jsx`), TypeScript (`.ts`, `.tsx`, `.mts`, `.cts`, `.d.ts`), decorators, stage-3 syntax | `oxc_parser` | 1 |
| Vue single-file components (`.vue`), Svelte (`.svelte`) | script-block splitter, then `oxc` | 2 |
| CoffeeScript, LiveScript (`.coffee`, `.litcoffee`, `.ls`, `.cjsx`, `.csx`) | Sidecar: `--sidecar node` spawns dependency-cruiser for those files and merges the edges | 3 |
| Babel-transformed syntax, `.babel` files | `oxc` parses the syntax; module-resolver aliases read from the Babel config | 1 |
| Markdown code fences (`.md`, via `extraExtensionsToScan`) | Parity | 2 |
| Resolution: node resolution, `exports` and `imports` fields, `conditionNames`, `mainFields`, `aliasFields`, tsconfig `paths` / `baseUrl` / `extends` / references, webpack `resolve.alias` and `modules`, workspaces, symlinks, Yarn PnP | `oxc_resolver` | 1 to 2 |
| npm classification from the nearest `package.json`, licence and `deprecated` from the installed package | Parity | 1 to 2 |
| Core module detection per Node version, `node:` protocol, `process.getBuiltinModule` | Parity | 1 |
| Unsupported-transpiler failure mode: dependency-cruiser cruises zero modules and exits 0 | Exit code 2 with a named reason; `the separate toolchain-version guard a heavy user has to bolt on becomes unnecessary` | 1 |

## Programmatic API

| dependency-cruiser | Rulebearing |
| --- | --- |
| `cruise(files, options, resolveOptions, transpileOptions)` | Rust `rulebearing::cruise`; Node `rulebearing` exports `cruise()` with the same signature through napi-rs |
| `format(result, options)` | `lb::format`; Node `format()` |
| `extractDepcruiseConfig`, `extractTSConfig`, `extractWebpackResolveConfig`, `extractBabelConfig` | same names in the Node binding |
| `getAvailableTranspilers`, `allExtensions` | same names, per extractor |
