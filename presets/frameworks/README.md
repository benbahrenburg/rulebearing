# Framework presets

Five bundled rule sets, one per framework or architecture style, that a configuration can `extends` ([design § The developer relations hat](../../docs/artifacts/design.md#the-developer-relations-hat-the-first-ten-minutes-and-the-brownfield-repo): "off by default, each a documented opinion"; [plan 0003, Step 11](../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#23-steps-for-sub-wave-3c-presets-lifecycle-fields-snapshot-and-changelog); [FR-REACH-04](../../docs/prd.md#fr-reach-04), [FR-CLI-08](../../docs/prd.md#fr-cli-08)).

Each is an opinion, not a default. Nothing extends one unless a configuration names it, `rulebearing init` proposes one only when `--preset` names it, and none is part of `rulebearing:recommended`. A preset states the architecture its framework is usually written in, conservatively: every rule reads module paths only, so it holds for any language whose folders follow the convention, and a boundary the graph cannot see (a `"use client"` directive, a runtime import) is left out rather than guessed. No pattern nests a quantifier, so extending a preset adds no safe-regex warning to a run.

```yaml
extends: [rulebearing:typescript, rulebearing:recommended, rulebearing:nextjs]
```

```sh
rulebearing init --preset nextjs              # the languages found, plus the Next.js opinion
rulebearing init --preset python,fastapi      # Python named, plus the FastAPI opinion
rulebearing cruise --init --preset clean-architecture
```

`init` treats a framework preset's rules as it treats its own: a rule whose `from` side matches nothing in the repository is left out with a `severity: ignore` entry naming it (delete the entry to turn the rule on once the folder exists), and every finding the code has today is baselined, so the configuration it writes passes on its first run.

Every rule carries a `comment` with the decision token `plan:rulebearing-<preset>`, which `--require-comment-token` accepts and which points at the section below; a `fix`; a `severity`; and `examples` of the edges it forbids and allows, which `rulebearing test` runs. A rule is changed in a repository by naming it: a rule of the extending file with the same name replaces the attributes it sets ([config.md § Presets](../../docs/config.md#presets)).

| Preset | Rules | For |
| --- | --- | --- |
| [`rulebearing:nextjs`](nextjs.yaml) | 3 | Next.js, App Router and Pages Router |
| [`rulebearing:clean-architecture`](clean-architecture.yaml) | 3 | a .NET solution or a TypeScript or Python tree in Domain, Application, Infrastructure and Presentation layers |
| [`rulebearing:django`](django.yaml) | 3 | Django projects |
| [`rulebearing:fastapi`](fastapi.yaml) | 4 | FastAPI services in routers, services and repositories |
| [`rulebearing:vertical-slices`](vertical-slices.yaml) | 2 | features (slices) that are independent of each other, with a shared kernel |

## `rulebearing:nextjs`

Next.js loads route files by their path. The opinion: a route is an entry, not a library, and shared code never depends on a route. A type-only import (`import type`) is erased at build time and is never a finding.

| Rule | Severity | Forbids |
| --- | --- | --- |
| `nextjs-no-import-of-route-entries` | error | importing an App Router special file (`page`, `layout`, `route`, `template`, `default`, `loading`, `error`, `global-error`, `not-found`) from any module but a test or a story |
| `nextjs-no-import-of-api-routes` | error | importing a handler under `pages/api/` or `app/api/` from outside those folders |
| `nextjs-shared-code-not-to-routes` | error | `components/`, `hooks/` and `lib/` outside `app/` and `pages/` importing from `app/` or `pages/`; the same folders inside a route folder are colocated with the route and are part of it (`app/lib/data.ts -> app/lib/definitions.ts` is allowed) |

Not checked: whether a component imports server code. Under the App Router a component is a Server Component unless it or a parent says `"use client"`, and a Server Component importing the database module is the recommended pattern; under the Pages Router every component is bundled for the browser. Which one a component is depends on the directive, which the graph does not record, so a path rule would be wrong for one router or the other. The preset leaves it to Next.js's own build error and to the `server-only` package.

## `rulebearing:clean-architecture`

The dependency rule: source code depends only inwards, Domain <- Application <- Infrastructure and Presentation. A layer is a path segment named for it, alone (`src/Domain/`, `src/domain/`) or as the last dotted part of a project folder (`src/Shop.Domain/`), so the preset reads a .NET solution and a TypeScript or Python tree alike. A test project (`tests/Domain.UnitTests/`) is not a layer. Presentation is `Web`, `WebUI`, `WebApi`, `Api`, `API`, `Presentation` or `UI` (or with a lower-case first letter); it may depend on Infrastructure, which it composes at start-up. An npm package under `node_modules/` is never a layer, whatever its path (`@opentelemetry/api`, `undici/lib/web/`).

| Rule | Severity | Forbids |
| --- | --- | --- |
| `clean-domain-depends-on-nothing-outer` | error | Domain importing Application, Infrastructure or Presentation |
| `clean-application-not-to-outer-layers` | error | Application importing Infrastructure or Presentation |
| `clean-infrastructure-not-to-presentation` | error | Infrastructure importing Presentation |

A module's layer is the last layer segment of its path: `apps/api/src/domain/` is Domain, since the app folder `apps/api/` comes first (a Turborepo or Nx layout), and `src/Web/Infrastructure/` (jasontaylordev/CleanArchitecture's endpoint plumbing) is Infrastructure, whose files import each other freely. Each rule's `pathNot` leaves out a path with a later layer segment of another kind. The cost: a folder named for a layer inside another layer (`src/Domain/Api/`) is that layer; rename it, or override the rule's `pathNot`. On .NET, `rulebearing init` also proposes namespace-based layer rules from the built assemblies ([cli.md § Commands](../../docs/cli.md#commands)); the two agree on a solution whose folders and namespaces name the same layers.

## `rulebearing:django`

Django's file names say what a module is. The opinion: models are the innermost layer, views are reached through URL configurations, and migrations are Django's.

| Rule | Severity | Forbids |
| --- | --- | --- |
| `django-models-are-innermost` | error | `models.py` or `models/` importing `views`, `forms`, `admin`, `serializers` or `urls` |
| `django-views-only-from-urls` | error | importing `views.py` or `views/` from anything but `urls`, another view, a test or `conftest.py` |
| `django-no-import-of-migrations` | error | importing a `migrations/` module from outside `migrations/` |

Not fenced: one app's use of another app's models. Foreign keys across apps are ordinary Django, and which apps may know each other is a project's decision; write it as a rule of your own.

## `rulebearing:fastapi`

The layering most FastAPI services use: routers (`routers/`, `router.py`, `routes/`, `endpoints/`) handle HTTP and call services; services (`services/`, `service.py`) hold the logic and call repositories; repositories (`repositories/`, `repository.py`, `crud/`, `crud.py`) talk to the database; models and schemas are data every layer shares.

| Rule | Severity | Forbids |
| --- | --- | --- |
| `fastapi-services-not-to-routers` | error | a service importing a router |
| `fastapi-repositories-not-to-services-or-routers` | error | a repository importing a service or a router |
| `fastapi-routers-through-services` | warn | a router importing a repository directly |
| `fastapi-models-and-schemas-are-innermost` | error | `models` or `schemas` importing a router, a service or a repository |

`fastapi-routers-through-services` is a warning: a small service that calls its CRUD functions from the router is common and deliberate, and the rule says where the logic would go once it grows.

## `rulebearing:vertical-slices`

A feature, or slice, owns its request, handler, validation and data access, and is added, changed and deleted on its own. A slice is a folder directly under `features/`, `Features/`, `slices/` or `Slices/`, anywhere in the tree; the shared kernel is `shared/`, `Shared/`, `common/`, `Common/`, `kernel/`, `Kernel/`, `SharedKernel/` or `shared-kernel/` outside the slices. A slice's own `shared/` or `common/` folder is part of the slice (`src/features/cart/shared/util.ts -> src/features/cart/cart.ts` is allowed).

| Rule | Severity | Forbids |
| --- | --- | --- |
| `slices-are-independent` | error | a slice importing another slice |
| `slices-shared-kernel-not-to-slices` | error | the shared kernel importing a slice |

`rulebearing init` proposes `features-are-independent` by itself when it finds `src/features/` with two or more features; this preset is the same opinion for any slice folder, in any language, with the shared kernel fenced too.
