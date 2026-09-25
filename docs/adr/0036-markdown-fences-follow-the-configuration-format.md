# ADR-0036: Markdown fences are read for a native configuration; a dependency-cruiser one keeps upstream's `extraExtensionsToScan`

- **Status:** Accepted (2026-09-24, by the maintainer)
- **Date:** 2026-09-24
- **Derives from:** [design § Why](../artifacts/design.md#why) ("superset, precisely; nothing is dropped"), [ADR-0009](0009-conformance-suites-as-specification.md) (the upstream suites are the specification), [ADR-0032](0032-liveness-follows-the-configuration-format.md) (a behaviour that would break a drop-in follows the configuration's format)
- **Constrains:** `crates/rb-extract-ts` (`md.rs`, `Settings::markdown_fences`), `crates/rb-cli` (which configurations turn it on)
- **Implemented by:** [Wave 2 plan, Step 9](../plans/pending/0002-wave-2-dotnet-python-element-rules.md#29-step-9-presets---init-presets-vue-svelte-markdown-webpackconfig-collapse-highlight-experimentalstats-2d)
- **Requirements:** [FR-EXT-TS-04](../prd.md#fr-ext-ts-04)

## Context

[FR-EXT-TS-04](../prd.md#fr-ext-ts-04) says Markdown code fences MUST be scanned when `.md` is listed in `extraExtensionsToScan`, and the coverage tab's row "Markdown code fences (`.md`, via `extraExtensionsToScan`)" calls that Parity. dependency-cruiser 18.2.0 does the opposite. Its options reference says it "will take special care not to even _read_" a file whose extension is in `extraExtensionsToScan`, and its own test ("does not parse files matching extensions in the extraExtensionsToScan array", recorded in gate 1 layer 1) proves it. A repository lists `.md` there so that orphan and reachability rules see its Markdown files as modules with no dependencies. Reading their fences would give those modules edges: a README whose example imports `src/index.ts` would stop being an orphan and would make `src/index.ts` reachable, and rules would report differently from dependency-cruiser on the same configuration. The wave 1 fixture `tests/options/extra-extensions` asserts upstream's behaviour for exactly this case.

## Decision

- A `.dependency-cruiser.*` configuration keeps upstream's behaviour: a listed extension, `.md` included, is never read.
- A `rulebearing.*` configuration that lists `.md` in `extraExtensionsToScan` has the JavaScript and TypeScript fences of its Markdown files extracted (tags `js`, `ts`, `jsx`, `tsx`, `javascript`, `typescript`), each fence parsed on its own with its tag selecting the syntax, and every dependency's `line` and `column` pointing into the Markdown file. This is the requirement, as a native addition.
- The extractor carries the switch as `Settings::markdown_fences`, off by default; `rb-cli` turns it on from the root configuration's format, as it chooses the liveness mode ([ADR-0032](0032-liveness-follows-the-configuration-format.md)). No configuration key is added.

## Consequences

- A dependency-cruiser configuration cruises to the same graph under both tools, and gate 1 layer 1 stays at its recorded expectation.
- A native configuration gets the requirement. Documentation examples become edges that rules see, which is what a repository that asks for them wants: a README that imports a module that no longer exists fails `not-to-unresolvable`.
- The coverage tab's row is Parity+ rather than Parity. The tab is regenerated from the design document, not edited, so the plan's status table records this ADR as the evidence.

## Alternatives considered

- **Read fences whenever `.md` is listed.** Rejected: it changes the graph of every dependency-cruiser repository that lists `.md`, the drop-in promise the design makes.
- **Never read fences.** Rejected: it drops a MUST of the requirements.
- **A new key (`languages.typescript.markdownFences`).** Rejected: the requirement already names the switch (`.md` in `extraExtensionsToScan`), and a second key would have to agree with it.
