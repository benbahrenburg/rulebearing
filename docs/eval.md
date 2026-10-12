# The `fix`-text eval

A rule's `fix` is the sentence an agent reads when the rule fires ([agents.md](agents.md#the-agent-reporter)). This page scores those sentences: a benchmark of violations, each handed to an agent with nothing but the `agent` report, and a check of whether the agent cleared the rule without widening it ([design § The agentic engineering hat](artifacts/design.md#the-agentic-engineering-hat-turn-two); [plan 0004, Step 10](plans/pending/0004-wave-4-reach.md#23-steps-for-sub-wave-4c-fix---plan-and-the-eval-harness-funded-only-by-the--12-rule); [ADR-0077](adr/0077-fix-plan-candidates-are-verified-and-the-eval-harness-scores-fix-text.md); [FR-REACH-03](prd.md#fr-reach-03)). It measures the agent as much as the text, so a `fix` is not called bad on one configuration's result; the table is a record, not a gate.

## The benchmark

| Suite | Repository | The violations | Configuration |
| --- | --- | --- | --- |
| `mutations` | dependency-cruiser at the [manifest](../testbeds/manifest.yaml) SHA, with [the mutation branch](../conformance/dependency-cruiser/mutations/README.md) applied | the twelve, one per rule shape; each case keeps only its own folder under `src/mutation/`, so the agent sees one violation | [eval/cases/mutations/rulebearing.yaml](../eval/cases/mutations/rulebearing.yaml): the branch's rules with a `fix` each |
| `phongnguyend` | phongnguyend/Practical.CleanArchitecture at the manifest SHA, built | one per imported NetArchTest rule (five): a type declared in the wrong layer's namespace and used from its own layer, so the layer depends on it | [eval/cases/phongnguyend/rulebearing.yaml](../eval/cases/phongnguyend/rulebearing.yaml): what `import archunit` writes, with a `fix` each |
| `monaco` | onebeyond/monaco at the manifest SHA, built | one per imported ArchUnitNET rule (six): a command no longer sealed, a validator no longer sealed, a handler renamed, an entity configuration made public, a value object given a setter, an enumeration declared in the Application layer | [eval/cases/monaco/rulebearing.yaml](../eval/cases/monaco/rulebearing.yaml): what `import archunit --graph` writes, with a `fix` each |

The two .NET oracles' own tests pass at their pinned SHAs, so their imported rules report nothing ([testbeds/results](../testbeds/results/)); each .NET case is a patch beside its suite that puts one violation in, the way a pull request would.

Each suite is a file under [eval/cases/](../eval/cases/) naming the repository, the SHA, the patches and the cases, each by rule, ends and stable id ([ADR-0015](adr/0015-stable-violation-id.md)). The harness refuses a case whose repository does not hold exactly that violation.

## How a case runs

1. The repository is cloned from the testbed checkout (or fetched at the SHA), the patches applied, and the state committed. A compiled language is built.
2. The agent's configuration is installed and committed. The one that ships is Claude Code with what `rulebearing hooks install --claude-code` writes: the session brief, `impact` before each edit and the `Stop` hook that hands back the findings ([agents.md](agents.md#the-hooks)). Only the repository's settings are loaded, and the agent may edit files and run `rulebearing`, `git`, `node`, `dotnet build` and the file commands.
3. The agent gets a fixed sentence and `rulebearing cruise --output-type agent` restricted to the case's rule, nothing else ([eval/run.py](../eval/run.py), `PROMPT`), and runs under a budget.
4. The repository is built again and scored from the binary's own answers:

| Score | Means |
| --- | --- |
| `cleared` | `cruise` reports no violation of the case's rule |
| `widened` | a rule's definition in `rules --json` changed, a finding was softened to `ignore` by a known violation, or `config lint` reports a finding it did not before |
| `budgetRaised` | a ratchet budget's `ceiling` went up |

Every row records the agent, the model the agent reported, the budget, the turns, the cost and the tokens. The results are committed under `eval/results/` as `<date>-<agent>.json`, and the tables below are written from them by `python3 eval/report.py --write docs/eval.md`; CI checks they are current.

```sh
cargo build --release -p rb-cli
python3 eval/run.py --check                                  # prepare every case, no agent
python3 eval/run.py --agent claude-code --budget-usd 3       # the benchmark; writes eval/results/
python3 eval/report.py --write docs/eval.md
```

## Results

<!-- eval-results:start -->
### 2026-10-12, claude-code: mutations

Results file: [2026-10-12-claude-code-mutations.json](https://github.com/benbahrenburg/rulebearing/blob/main/eval/results/2026-10-12-claude-code-mutations.json). Models: `claude-opus-5[1m]`. Budget per case: `usd:3.00`. Binary: `rulebearing 0.1.3`. Suites: `mutations` (sverweij/dependency-cruiser at `7c22858071cc`).

12 cases: 12 cleared, 12 cleared without widening a rule or raising a budget, 0 widened, 0 raised a budget. Cost: 7.50 USD.

| Case | Fix | Model | Turns | Cleared | Widened | Budget raised | Cost (USD) | Note |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `m01-forbidden/RB-491004cb` | Do not import src/cli from src/mutation/01-forbidden. Have the caller pass in what the module needs, or move the shared code out of src/cli into src/utl and import it from there. | claude-opus-5[1m] | 27 | yes | no | no | 1.49 | - |
| `m02-path-not/RB-6a461ebc` | Import only from this folder and from src/utl. Move what you need into src/utl, or have the caller pass it in. | claude-opus-5[1m] | 13 | yes | no | no | 0.57 | - |
| `m03-fence/RB-716a32ca` | Sibling folders under src/mutation/03-fence never import each other, nor the folder above them. Move the shared code out of src/mutation/03-fence, for example into src/utl, and import it from there. | claude-opus-5[1m] | 11 | yes | no | no | 0.64 | - |
| `m04-circular/RB-20fb7af9` | Break the cycle: one of the two modules must stop importing the other. Move what both need into a third module that imports neither. | claude-opus-5[1m] | 17 | yes | no | no | 0.58 | - |
| `m05-orphan/RB-4075c6bb` | Every module is imported by another or imports one. Import this module where it is used, or delete it if nothing needs it. | claude-opus-5[1m] | 10 | yes | no | no | 0.33 | - |
| `m06-dependency-types-not/RB-8110b593` | Modules here import local files only, never a Node built-in or a package. Take the value the built-in computes as a parameter from the caller. | claude-opus-5[1m] | 16 | yes | no | no | 0.59 | - |
| `m07-reachable/RB-5e89dfa4` | entry.mjs must not reach forbidden.mjs by any path, including through middle.mjs. Remove the import on the path, for example middle.mjs's import of forbidden.mjs. | claude-opus-5[1m] | 13 | yes | no | no | 0.50 | - |
| `m08-required/RB-36e145bc` | Every module in src/mutation/08-required uses the bus. Import src/utl/bus.mjs and use it. | claude-opus-5[1m] | 13 | yes | no | no | 0.47 | - |
| `m09-dependents/RB-5c3d6cf9` | A shared module has at least two dependents. Import it from a second module that uses it, or fold it into its one user. | claude-opus-5[1m] | 14 | yes | no | no | 0.55 | - |
| `not-in-allowed/RB-1a2adcad` | - | claude-opus-5[1m] | 17 | yes | no | no | 0.63 | - |
| `m11-via/RB-2c1087cc` | No cycle may run through hub.mjs. The hub must not import back into the modules that import it; move what it needs into a module neither imports. | claude-opus-5[1m] | 13 | yes | no | no | 0.54 | - |
| `m12-could-not-resolve/RB-30b0e427` | Every import names a file that exists. Correct the path, or create the module it names. | claude-opus-5[1m] | 15 | yes | no | no | 0.60 | - |

### 2026-10-12, claude-code: monaco, phongnguyend

Results file: [2026-10-12-claude-code-dotnet.json](https://github.com/benbahrenburg/rulebearing/blob/main/eval/results/2026-10-12-claude-code-dotnet.json). Models: `claude-opus-5[1m]`. Budget per case: `usd:3.00`. Binary: `rulebearing 0.1.3`. Suites: `monaco` (onebeyond/monaco at `5700abab4d73`), `phongnguyend` (phongnguyend/Practical.CleanArchitecture at `033acecd44b6`).

11 cases: 11 cleared, 11 cleared without widening a rule or raising a budget, 0 widened, 0 raised a budget. Cost: 11.53 USD.

| Case | Fix | Model | Turns | Cleared | Widened | Budget raised | Cost (USD) | Note |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `application-should-not-have-dependency-on-infrastructure/RB-bbe785aa` | ClassifiedAds.Application must not depend on any type in the ClassifiedAds.Infrastructure namespace. Declare what the application needs in ClassifiedAds.Application, as an interface or a plain type in an Application namespace, and implement or configure it in ClassifiedAds.Infrastructure. | claude-opus-5[1m] | 19 | yes | no | no | 0.82 | - |
| `application-should-not-have-dependency-on-persistence/RB-735b1734` | ClassifiedAds.Application must not depend on any type in the ClassifiedAds.Persistence namespace. Declare what the application needs in ClassifiedAds.Application, as an interface or a plain type in an Application namespace, and implement it in ClassifiedAds.Persistence. | claude-opus-5[1m] | 20 | yes | no | no | 0.86 | - |
| `domain-should-not-have-dependency-on-application/RB-8cc5e45e` | ClassifiedAds.Domain must not depend on any type in the ClassifiedAds.Application namespace. Keep the type the domain uses in a ClassifiedAds.Domain namespace, or have the application pass the values in. | claude-opus-5[1m] | 19 | yes | no | no | 0.86 | - |
| `domain-should-not-have-dependency-on-infrastructure/RB-03c3a1f7` | ClassifiedAds.Domain must not depend on any type in the ClassifiedAds.Infrastructure namespace. Keep the type the domain uses in a ClassifiedAds.Domain namespace, and let Infrastructure map to and from it. | claude-opus-5[1m] | 17 | yes | no | no | 0.72 | - |
| `domain-should-not-have-dependency-on-persistence/RB-47c58ac8` | ClassifiedAds.Domain must not depend on any type in the ClassifiedAds.Persistence namespace. Keep the type the domain uses in a ClassifiedAds.Domain namespace, and let Persistence map to and from it. | claude-opus-5[1m] | 17 | yes | no | no | 0.84 | - |
| `commands-and-queries-are-records-public-nested-sealed-and-named-command-query/RB-c9f13594` | A command or query is a public sealed record named Command or Query, nested in its feature class. Make it a public sealed record with that name inside the feature. | claude-opus-5[1m] | 43 | yes | no | no | 1.95 | - |
| `entity-configurations-exist-in-application-layer-and-be-internal-and-sealed/RB-83791715` | An entity type configuration lives in the Application assembly, under Persistence/EntityConfigurations, as an internal sealed class. Make it internal sealed. | claude-opus-5[1m] | 24 | yes | no | no | 1.35 | - |
| `enumerations-exist-only-in-domain-layer-and-are-immutable/RB-81cd5bec` | An Enumeration subclass lives in the Domain assembly, under Model, and is immutable. Move it into Monaco.Template.Backend.Domain and keep its values as static readonly fields. | claude-opus-5[1m] | 33 | yes | no | no | 1.28 | - |
| `handlers-exist-in-application-are-named-handler-and-are-nested-sealed-and-internal/RB-fe0f8f8c` | A request handler lives in the Application assembly as an internal sealed class named Handler, nested in its feature class. Rename it Handler, make it internal sealed and keep it inside the feature. | claude-opus-5[1m] | 28 | yes | no | no | 1.20 | - |
| `validators-exist-in-application-are-sealed-internal-and-nested/RB-e974893b` | A validator lives in the Application assembly as an internal sealed class nested in its feature class. Make it internal sealed and keep it inside the feature. | claude-opus-5[1m] | 18 | yes | no | no | 0.74 | - |
| `value-objects-exist-only-in-domain-layer-and-are-immutable/RB-7fbe8e41` | A value object lives in the Domain assembly and is immutable: no public or internal setters, no public fields. Set its properties in the constructor only, and return a new instance to change one. | claude-opus-5[1m] | 21 | yes | no | no | 0.91 | - |
<!-- eval-results:end -->
