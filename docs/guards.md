# Guards

Recipes for the guards a team writes by hand, each one a fixture that CI runs with the same commands you run. Copy a block, change the paths, and run `rulebearing test` and `rulebearing config lint` over it.

This page holds the budgets and exceptions section so far. The remaining sections arrive with their fixtures ([plan 0005](plans/pending/0005-guard-catalogue.md)). Every block is a copy of `cookbook/guards/<slug>/rulebearing.yaml`, and each fixture's README says what its graph holds and what each failure exits with ([cookbook/README.md](../cookbook/README.md)).

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
