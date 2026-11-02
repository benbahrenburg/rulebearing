# Guards

Recipes for the guards a team writes by hand, each one a fixture that CI runs with the same commands you run. Copy a block, change the paths, and run the commands under [Prove the rule](#prove-the-rule).

This page holds the recipes that have a fixture today. More arrive with their fixtures ([plan 0005](plans/pending/0005-guard-catalogue.md)). Every YAML block below is a byte copy of `cookbook/guards/<slug>/rulebearing.yaml`, or of one rule of it, and `cargo xtask lint` fails when one differs ([ADR-0023](adr/0023-documentation-link-and-lint-gates.md)). Each fixture's README says what its graph holds and what each failure exits with ([cookbook/README.md](../cookbook/README.md)).

The file's overall shape, and every key, is in [config.md](config.md).

| Exit code | Meaning ([ADR-0008](adr/0008-exit-code-contract.md)) |
| --- | --- |
| 0 to 255 | The count of errors from a gating reporter: error-severity violations, expired rules and baseline entries, and exceeded ratchets ([ADR-0030](adr/0030-the-reporter-decides-the-error-count-exit.md)) |
| 2 | Also: the run cannot be trusted, such as a vacuous rule or a missing budget |
| 3 | The configuration is invalid, such as an unknown key, a missing decision token or a name in `allowEmpty` that is no rule |

## Anatomy of a rule

Every family shares the same metadata. This is the rule from [`no-circular`](../cookbook/guards/no-circular/README.md):

```yaml guard=no-circular#no-circular
      - name: no-circular
        comment: "A module on a cycle cannot be changed, tested or moved on its own. plan:0005-guard-catalogue"
        fix: "Break the cycle: move the shared code into a module both sides import, or invert one edge behind an interface. Type-only imports do not count."
        severity: error
        from: {}
        to:
          circular: true
          viaOnly: { dependencyTypesNot: [type-only] }
        examples:
          forbidden: ["src/order/order.ts -> src/order/order.ts"]
          allowed: ["src/main.ts -> src/order/order.ts"]
```

| Field | What it does |
| --- | --- |
| `name` | Stable and kebab-case. The baseline and the violation id key on it |
| `comment` | The why, with a decision token, `adr:NNNN` or `plan:<slug>`. `--require-comment-token` makes a rule without one exit 3 |
| `fix` | The imperative to follow when the rule fires |
| `severity` | `error` counts toward the exit code; `warn` and `info` are reported; `ignore` is silent |
| `from`, `to` | Matchers on the resolved path and on the module's and the edge's facts |
| `examples` | Edges `rulebearing test` builds into a synthetic graph, so a rule's pattern is tested like code |

`owner`, `expires` and `allowEmpty` appear in [Budgets and exceptions](#budgets-and-exceptions).

## Guard catalogue

| You want to guard | Rule shape | Proven by |
| --- | --- | --- |
| No import cycles, ignoring type-only edges | `to.circular` with `viaOnly` | [`no-circular`](../cookbook/guards/no-circular/README.md) |
| A count that may only fall | `rules.ratchets` with a budget file | [`ratchet-routes-via-service`](../cookbook/guards/ratchet-routes-via-service/README.md) |
| Accept today's findings, with a deadline | `options.knownViolations` with `expires` | [`baseline-known-violations`](../cookbook/guards/baseline-known-violations/README.md) |
| A temporary rule | `expires` and `owner` | [`expiring-exception-rule`](../cookbook/guards/expiring-exception-rule/README.md) |
| A named exception list kept out of the regex | `defines` from JSON, `${name}` | [`defines-exception-list`](../cookbook/guards/defines-exception-list/README.md) |
| A rule ahead of its first match | `allowEmpty` on the rule | [`allow-empty-by-rule`](../cookbook/guards/allow-empty-by-rule/README.md) |
| The same, for a rule in a dependency-cruiser file | the top-level `allowEmpty` list | [`allow-empty-by-list`](../cookbook/guards/allow-empty-by-list/README.md) |

## Code quality guards

Facts about the graph itself.

### No cycles, and type-only edges do not count

`viaOnly` requires every edge of the cycle to be other than type-only, so a cycle closed only through `import type` does not fire. Written as `to: { circular: true, dependencyTypesNot: [type-only] }`, the filter applies only to the reported edge. Proven by [`no-circular`](../cookbook/guards/no-circular/README.md).

```yaml guard=no-circular
rules:
  dependencies:
    forbidden:
      - name: no-circular
        comment: "A module on a cycle cannot be changed, tested or moved on its own. plan:0005-guard-catalogue"
        fix: "Break the cycle: move the shared code into a module both sides import, or invert one edge behind an interface. Type-only imports do not count."
        severity: error
        from: {}
        to:
          circular: true
          viaOnly: { dependencyTypesNot: [type-only] }
        examples:
          forbidden: ["src/order/order.ts -> src/order/order.ts"]
          allowed: ["src/main.ts -> src/order/order.ts"]
```

## Budgets and exceptions

The guards that keep the other guards honest over time. Each refuses the cheapest path to green: a ceiling cannot be raised, a baseline entry expires, and an exception has a date and an owner.

### A ratchet: a count that may only fall

Counts the direct edges that match `from` and `to`, with `$1` captures, and compares the count with the ceiling in the budget file, `{ "ceiling": 2 }`. An exceeded ratchet is one error and a missing budget exits 2 ([ADR-0029](adr/0029-ratchets-enforced-by-cruise-and-reported-in-the-summary.md)). `rulebearing count --write` lowers the ceiling to the current count and refuses to raise it. Proven by [`ratchet-routes-via-service`](../cookbook/guards/ratchet-routes-via-service/README.md).

```yaml guard=ratchet-routes-via-service
rules:
  ratchets:
    - name: routes-via-service
      comment: "Route files reach the database through the service layer; the direct edges are being retired. adr:0029"
      fix: "Move the query into apps/<app>/src/server/services and call it from the route."
      owner: "@web-platform"
      from: { path: "^apps/([^/]+)/src/app/.*/(page|route)\\.tsx?$" }
      to: { path: "^apps/$1/src/(server|domain)/" }
      budget: budget.json
```

### A baseline: accept today's findings, each with a deadline

A known finding is reported at severity `ignore`. An entry past its date stops applying, so its finding is an error again and the expired entry is one more ([ADR-0031](adr/0031-a-saved-result-carries-what-the-exit-code-counts.md)). An entry in dependency-cruiser's shape needs `type`, because dependency-cruiser's matching compares it. An id survives an unrelated edit ([ADR-0015](adr/0015-stable-violation-id.md)). Proven by [`baseline-known-violations`](../cookbook/guards/baseline-known-violations/README.md).

```yaml guard=baseline-known-violations
rules:
  dependencies:
    forbidden:
      - name: ui-never-imports-server
        comment: "Server code in a page ships secrets and database clients to the browser. plan:0005-guard-catalogue"
        fix: "Call the server code through a route or a server action; do not import it from a page."
        severity: error
        from: { path: "^apps/[^/]+/src/app/" }
        to: { path: "^apps/[^/]+/src/server/" }
        examples:
          forbidden: ["apps/web/src/app/orders/page.tsx -> apps/web/src/server/db.ts"]
          allowed: ["apps/web/src/app/orders/page.tsx -> apps/web/src/lib/format.ts"]
options:
  knownViolations:
    - id: RB-40dba9c6
      rule: { name: ui-never-imports-server, severity: error }
      expires: 2099-12-31
      owner: "@orders-team"
    - from: apps/web/src/app/users/page.tsx
      to: apps/web/src/server/auth.ts
      type: dependency
      rule: { name: ui-never-imports-server, severity: error }
```

### A temporary rule

`expires` puts a date on a rule and `owner` says who answers for it. The day after the date, the rule is one error from a gating reporter ([ADR-0030](adr/0030-the-reporter-decides-the-error-count-exit.md)). Proven by [`expiring-exception-rule`](../cookbook/guards/expiring-exception-rule/README.md).

```yaml guard=expiring-exception-rule
rules:
  dependencies:
    forbidden:
      - name: no-legacy-http-client
        comment: "The legacy client is being retired; this rule goes when its last caller does. plan:0005-guard-catalogue"
        fix: "Use packages/http instead of packages/legacy-http."
        severity: error
        owner: "@web-platform"
        expires: 2099-12-31
        from: { path: "^packages/", pathNot: "^packages/legacy-http/" }
        to: { path: "^packages/legacy-http/" }
        examples:
          forbidden: ["packages/orders/src/index.ts -> packages/legacy-http/src/index.ts"]
          allowed: ["packages/orders/src/index.ts -> packages/http/src/index.ts"]
```

### An exception list kept out of the regex

`defines` keeps the list in a JSON file with its own owner and review. `rulebearing config expand` prints the substituted alternation. A file that does not exist is a configuration error, exit 3. Proven by [`defines-exception-list`](../cookbook/guards/defines-exception-list/README.md).

```yaml guard=defines-exception-list
defines:
  migratingPackages: { fromJson: eng/migration.json, select: "packages[*]", joinWith: "|" }
rules:
  dependencies:
    forbidden:
      - name: no-legacy-http-client-except-migrating
        comment: "The legacy client is retired; the packages still migrating are listed in JSON with an owner's review. plan:0005-guard-catalogue"
        fix: "Use packages/http; add the package to eng/migration.json only with the owner's review."
        severity: error
        owner: "@web-platform"
        from: { path: "^packages/", pathNot: ["^packages/(${migratingPackages})/", "^packages/legacy-http/"] }
        to: { path: "^packages/legacy-http/" }
        examples:
          forbidden: ["packages/search/src/index.ts -> packages/legacy-http/src/index.ts"]
          allowed: ["packages/billing/src/index.ts -> packages/legacy-http/src/index.ts"]
```

### A rule ahead of its first match

`allowEmpty: true` excuses one rule from liveness. The same rule without it is vacuous and exits 2 ([ADR-0007](adr/0007-vacuous-rules-fail-by-default.md)). Proven by [`allow-empty-by-rule`](../cookbook/guards/allow-empty-by-rule/README.md).

```yaml guard=allow-empty-by-rule
rules:
  dependencies:
    forbidden:
      - name: plugins-stay-apart
        comment: "Plugins are independent; the first one lands next sprint, so this rule matches nothing yet. plan:0005-guard-catalogue"
        fix: "Talk to another plugin through the host API."
        severity: error
        from: { path: "^plugins/([^/]+)/" }
        to: { path: "^plugins/([^/]+)/", pathNot: "^plugins/$1/" }
        allowEmpty: true
        examples:
          forbidden: ["plugins/a/index.ts -> plugins/b/index.ts"]
          allowed: ["plugins/a/index.ts -> plugins/a/util.ts"]
```

### The same excuse for a rule in a dependency-cruiser file

dependency-cruiser's schema refuses `allowEmpty` on a rule, so the native file names it in a top-level list. A name that is no rule exits 3, so an exception cannot outlive its rule ([ADR-0032](adr/0032-liveness-follows-the-configuration-format.md)). The native entry adds `fix` and `examples`, which the dependency-cruiser file cannot hold either. Proven by [`allow-empty-by-list`](../cookbook/guards/allow-empty-by-list/README.md).

```yaml guard=allow-empty-by-list
extends: ./.dependency-cruiser.json
allowEmpty: [plugins-stay-apart]
rules:
  dependencies:
    forbidden:
      - name: plugins-stay-apart
        fix: "Talk to another plugin through the host API."
        examples:
          forbidden: ["plugins/a/index.ts -> plugins/b/index.ts"]
          allowed: ["plugins/a/index.ts -> plugins/a/util.ts"]
```

## Prove the rule

A rule is code. These commands are its tests, its linter and its documentation.

| Command | Answers |
| --- | --- |
| `rulebearing test` | Does each rule flag its `examples.forbidden` edges and pass its `examples.allowed` ones? |
| `rulebearing config lint --graph <graph>` | Can a rule never match? Is it shadowed by an earlier one? Is a `fix` missing, or a restatement of the name? With `--require-comment-token`, is a decision token missing? |
| `rulebearing explain <rule>` | The rule in a sentence, its reason and fix, and the first edges it matched |
| `rulebearing rules --json` | Every rule with its family, severity and how many modules each side matched |
| `rulebearing cruise --require-comment-token -T err-long` | The gate itself. Every finding prints the rule's `fix` |

## Before you merge a rule

- The `comment` says why and carries `adr:NNNN` or `plan:<slug>`.
- The `fix` is an imperative someone can follow without opening the rule.
- `examples.forbidden` and `examples.allowed` are present and `rulebearing test` is green. A ratchet has no `examples`; `rulebearing count` shows what it counts.
- `rulebearing rules --json` shows a match on the `from` side, or the rule has `allowEmpty` with the reason in its `comment`.
- `rulebearing config lint` reports nothing.
- A temporary rule or baseline entry has `owner` and `expires`.
- No ratchet ceiling was raised, and no `pathNot` was widened to get green without a decision recorded.
- The rule uses `pathNot` rather than lookaround, and captures rather than duplicated literals.
