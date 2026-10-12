# Architecture questions

One page per architecture question: the rule that answers it in `rulebearing.yaml`, the same rule in dependency-cruiser's format as `rulebearing config convert` writes it, the edge that breaks it in a small repository, and the fix. Each page opens a playground with that repository and rule loaded, so the rule can be run and changed in the browser.

- Plan: [plan 0004, Step 5](../../docs/plans/pending/0004-wave-4-reach.md#21-steps-for-sub-wave-4a-docs-site-playground-cookbook-live-tables)
- Source: [design § The developer relations hat](../../docs/artifacts/design.md#the-developer-relations-hat-the-first-ten-minutes-and-the-brownfield-repo) ("one page per architecture question"), [design § import-linter contracts](../../docs/artifacts/design.md#import-linter-contracts-for-the-python-teams-who-know-them)
- Decision: [ADR-0074](../../docs/adr/0074-the-cookbook-question-pages-are-fixtures-beside-the-guard-catalogue.md)
- Requirement: [FR-REACH-01](../../docs/prd.md#fr-reach-01)

## The questions

| Question | Language | Answered by | import-linter contract |
| --- | --- | --- | --- |
| [Features must not know each other](features-must-not-know-each-other/README.md) | TypeScript | a `forbidden` rule with a `$1` capture | |
| [Controllers only through services](controllers-only-through-services/README.md) | TypeScript | a `forbidden` rule naming what a controller may import | |
| [Nothing imports the producer](nothing-imports-the-producer/README.md) | TypeScript | a `forbidden` rule with `from.pathNot` | |
| [Layers only import downward](layers-only-import-downward/README.md) | Python | a `forbidden` rule per layer, `reachable: true` | `layers` |
| [Packages do not import each other](packages-do-not-import-each-other/README.md) | Python | a `forbidden` rule with a `$1` capture, `reachable: true` | `independence` |
| [The domain does no I/O](domain-does-no-io/README.md) | Python | a `forbidden` rule, `reachable: true` | `forbidden` |
| [Only the repositories touch the database](only-the-repositories-touch-the-database/README.md) | Python | a `forbidden` rule with `from.pathNot` | `protected` |
| [Sibling packages form no cycle](sibling-packages-form-no-cycle/README.md) | Python | a `circular` rule with `scope: folder` | `acyclic_siblings` |

The first three are the questions the design names. The other five are the contract kinds `rulebearing import import-linter` translates, each with the contract, the rules the import writes and the violations they find, beside the rule a person would write.

## A fixture

`<slug>/`, where the slug is the question in kebab-case:

| File | Holds |
| --- | --- |
| `README.md` | the page: the question, the rule, the failing edge and the fix, with a `<!-- cookbook:<part> -->` line where the site puts each generated part |
| `rulebearing.yaml` | the rules, each with `comment` (ending in a decision token), `fix`, `severity` and `examples` |
| `repo/` | a small repository whose files break the rule at least once; the playground loads it |
| `expected.json` | `summary.violations` and `summary.vacuousRules` of `rulebearing cruise --config ../rulebearing.yaml -T json` in `repo/` |
| `converted/.dependency-cruiser.json`, `converted/dropped.txt` | `rulebearing config convert rulebearing.yaml --to dependency-cruiser` and its report of what it dropped |
| `repo/.importlinter` | for an import-linter page, the contract |
| `imported/rulebearing.yaml`, `imported/expected.json` | for an import-linter page, what `rulebearing import import-linter` writes for the contract, and what those rules find in `repo/` |

The markers are `rule`, `converted`, `violations` and `playground` on every page, and `import-linter`, `imported` and `imported-violations` on a page whose repository holds an `.importlinter`. The site fills them from the files above, so the page shows only what the binary produced.

## Running

```sh
cargo build --release
cookbook/questions/run.sh layers-only-import-downward   # one fixture
cookbook/questions/run.sh --all                         # every fixture, one line each
```

[`run.sh`](run.sh) lists the commands it runs and what it compares. It runs in the `guards` CI job and in `site/build.sh`, before the site is generated, so a page that has drifted from the binary fails both.

## Adding a question

1. Write `README.md` with its markers, `rulebearing.yaml` and `repo/`, and link the page from the table above; the site build fails on a page this index does not link.
2. Run `RB_UPDATE_SNAPSHOTS=1 cookbook/questions/run.sh <slug>` to write `expected.json`, `converted/` and, for an import-linter page, `imported/`.
3. Read what it wrote. The violations must be the ones the README describes, and nothing else.
4. Run `cookbook/questions/run.sh <slug>` without the variable.
