# Importer fixtures

Inputs for `rulebearing import`, each folder with the `expected.yaml` the importer writes for it ([plan 0002, Step 11](../../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#211-step-11-the-three-importers-and-oracle-agreement-2f)). [`tests/import.rs`](../../import.rs) byte-compares each output and runs the import-linter and ESLint outputs against a tree; [`tests/import_corpus.rs`](../../import_corpus.rs) holds the ArchUnitNET and NetArchTest round trips. Regenerate the expected files with `RB_UPDATE_SNAPSHOTS=1 cargo test -p rb-cli --test import` and review the diff.

| Folder | What it covers |
| --- | --- |
| `import-linter/forbidden` | `forbidden`: wildcards (`*`, `**`), an external module, `allow_indirect_imports`, `as_packages`, `ignore_imports` (matched and unmatched) |
| `import-linter/layers` | `layers` in `pyproject.toml`: two containers, `\|` and `:` siblings, an optional layer, `exhaustive` with an unlisted module; a custom contract type |
| `import-linter/independence` | `independence` in `setup.cfg`: literal modules pair by pair, a wildcard as the shorthand, `ignore_imports` |
| `import-linter/protected` | `protected`: the `allowed` rule, the catch-all, an empty contract |
| `import-linter/acyclic` | `acyclic_siblings`: every package below an ancestor, `skip_descendants`, a missing ancestor |
| `import-linter/own` | import-linter's own `.importlinter` and a tree with its package layout; the output equals [`testbeds/oracles/configs/seddonym__import-linter.yaml`](../../../../../testbeds/oracles/configs/seddonym__import-linter.yaml) |
| `eslint/flat` | a flat `eslint.config.mjs` importing plugins: `import/no-restricted-paths` zones (a plain path with `except`, a list, globs, `basePath`) and `boundaries/element-types` with `default: disallow` |
| `eslint/legacy` | `.eslintrc.json` with comments, `overrides`, `import-x/`, a brace glob |
| `eslint/commonjs` | `.eslintrc.cjs` with `require`, and a captured-value selector the importer refuses |
| `eslint/package` | `eslintConfig` in `package.json`, `mode: full`, and a `basePattern` the importer refuses |
| `archunit/fluent` | ArchUnitNET: a loader, a provider field, a `GetClassOfType` field, a local holding the rule, `And`/`Or`, `AndShould`/`OrShould`, `...TypesThat()`, `Because`, `WithoutRequiringPositiveResults`, `Types(true)`, a slice rule, an alias and nested types; five chains that are not imported, each with its reason |
| `archunit/netarchtest` | NetArchTest: `InAssembly`, `InCurrentDomain`, `InNamespace`, `ShouldNot`, `Or`, ordinal comparison, the README table's patterns; `MeetCustomRule`, `BeImmutable`, an expected failure and `FromFile`, not imported |
| `archunit/roundtrip` | two ArchUnitNET test files, evaluated over the gate 2 graphs of `TestAssembly` and `ArchUnitNETTests` |

A JavaScript or JSON input is committed with a `.fixture` suffix, which the test drops when it copies the folder, so the repository's own ESLint and Prettier do not read it.

## Provenance

| File | Source | Licence |
| --- | --- | --- |
| `import-linter/own/.importlinter` | [seddonym/import-linter](https://github.com/seddonym/import-linter) at the SHA in `testbeds/manifest.yaml` | BSD-2-Clause |
| `archunit/roundtrip/DependenciesToOtherAssembliesTests.cs` | ArchUnitNET 0.13.4, `ArchUnitNETTests/Fluent/Syntax/`, unchanged below its header | Apache-2.0 |
| `archunit/roundtrip/SlicesTests.cs` | ArchUnitNET 0.13.4, `ArchUnitNETTests/Fluent/Slices/`, the first test only | Apache-2.0 |

Every other file is written for these tests.
