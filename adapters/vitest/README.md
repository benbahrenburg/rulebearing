# adapters/vitest: `rulebearing/vitest`

Each Rulebearing architecture rule as a vitest test, and a reporter that prints the rule results as one block. It is published inside the `rulebearing` npm package as the subpath `rulebearing/vitest`, not as a package of its own: the `@rulebearing` scope is not held ([ADR-0020](../../docs/adr/0020-single-name-across-registries.md)), and the tests run the binary that package already installs ([architecture § Distribution](../../docs/architecture.md#distribution)).

```sh
npm install --save-dev rulebearing vitest
```

```ts
// architecture.test.ts
import { defineArchitectureTests } from 'rulebearing/vitest';

defineArchitectureTests({ config: 'rulebearing.yaml', args: ['src'] });
```

```ts
// vitest.config.ts (optional: the rule block at the end of the run)
import { defineConfig } from 'vitest/config';
import { RulebearingReporter } from 'rulebearing/vitest';

export default defineConfig({ test: { reporters: ['default', new RulebearingReporter()] } });
```

## What it does

`defineArchitectureTests(options)` runs `rulebearing cruise --output-type json` once, while vitest collects the file, and registers a `describe('rulebearing')` with one `test` per rule, in the order the `junit` reporter lists them. It never evaluates a rule ([ADR-0010](../../docs/adr/0010-crate-layout-and-extractor-boundary.md) rule 4). A failing rule throws `ArchitectureRuleError`, whose message is exactly the `junit` reporter's message for that rule ([crates/rb-report/src/junit.rs](../../crates/rb-report/src/junit.rs), [Wave 2 plan § 1.5](../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#15-interfaces-and-contracts-this-wave-freezes)): the `fix`, the first five violations with id, from, to, line and column, then `... and N more`. A vacuous rule fails with the liveness reason ([ADR-0007](../../docs/adr/0007-vacuous-rules-fail-by-default.md)); warn, info and known findings pass and ride along as metadata. A run that writes no result registers one failing test, `cruise`, with the command, its exit code and its stderr.

| Option | Meaning |
| --- | --- |
| `config` | `--config`; unset, the binary finds the configuration |
| `graph` | `--graph`: read a graph document instead of extracting |
| `args` | further arguments, such as the directories to cruise |
| `cwd` | where the binary runs; the process's working directory by default |
| `binary` | the binary; default `RULEBEARING_BINARY`, then the `rulebearing` package's own launcher |
| `suite` | the `describe` name; `rulebearing` by default |

Each rule test carries `task.meta.rulebearing` (`rule`, `family`, `severity`, `message`, `output`). `RulebearingReporter` reads it and, at the end of the run, writes to standard error how many rules ran and failed, each failure's message and each passing rule's warnings. Other tests are not its concern.

## Developing

The sources compile into the npm wrapper's `dist/vitest/` (`npm run build` in [wrappers/npm](../../wrappers/npm/README.md) builds both), are linted by the root configuration through `cargo xtask lint` ([ADR-0023](../../docs/adr/0023-documentation-link-and-lint-gates.md)), and hold the 70% line floor of [ADR-0018](../../docs/adr/0018-test-coverage-threshold.md) in `vitest.config.ts`. The tests run against the shared fixture [adapters/fixture](../fixture/README.md) with the binary built from the checkout: `test/junit.test.ts` compares every rule test with `rulebearing cruise -T junit`, and `test/e2e.test.ts` runs a real vitest over the fixture with the reporter.

```sh
cargo build --release -p rb-cli
npm ci                                           # at the repository root
(cd adapters/vitest && npm test)                 # vitest run --coverage
```

Plan: [Wave 2, Step 14](../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#214-step-14-test-adapters-and-wrappers-2h). Release: [docs/release.md](../../docs/release.md).
