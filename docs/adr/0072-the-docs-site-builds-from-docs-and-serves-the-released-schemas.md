# ADR-0072: The docs site is generated from `docs/` by a small Node generator, and one Pages deployment serves it with the released schemas

- **Status:** Accepted (2026-10-11, by the owner: "Accept ADRs 0067, 0072 and 0073"; proposed 2026-10-11)
- **Date:** 2026-10-11
- **Derives from:** [plan 0004, Step 3](../plans/pending/0004-wave-4-reach.md#21-steps-for-sub-wave-4a-docs-site-playground-cookbook-live-tables) ("a theme that embeds the playground component, pages generated from `docs/` ... and the schema at `/schema/v1.json` (the `$schema` URL the config names). A link checker runs in the build. Deployed to GitHub Pages from `main`."), its [§ 1.9](../plans/pending/0004-wave-4-reach.md#19-dependencies) (the generator row) and [§ 1.11](../plans/pending/0004-wave-4-reach.md#111-compliance-and-licence-review) (a content security policy); [ADR-0019](0019-mit-licence.md); [ADR-0039](0039-plans-and-design-kept-local.md); [ADR-0067](0067-the-playground-facade-and-an-optional-javascript-engine.md)
- **Amends:** the delivery of the schemas to Pages that `release.yml`'s `schema-pages` job made since [plan 0001](../plans/implemented/0001-wave-1-typescript-parity.md) Step 22: the job is removed and `docs.yml` deploys instead. The schema URLs and what they serve are unchanged.
- **Constrains:** `site/`, `.github/workflows/docs.yml`, `.github/workflows/release.yml`, the `site` job of `.github/workflows/ci.yml`, the root `package.json` (`markdown-it`, `playwright`), [docs/release.md](../release.md)
- **Requirements:** [FR-REACH-01](../prd.md#fr-reach-01), [NFR-DOC-01](../prd.md#nfr-doc-01)

## Context

Plan 0004 asks for a static docs site built from `docs/`, a playground component in its theme that loads the `rb-wasm` module, the JSON schemas at the `$id` URLs they carry, a link check over the build, and deployment to GitHub Pages from `main`. The plan's dependency table names `mdbook` or `zola` as candidates.

Four facts shape the choice:

- **Anchors.** The sources are already held to a link check ([ADR-0023](0023-documentation-link-and-lint-gates.md)). It computes a heading's anchor with its own slug rule, which differs from GitHub's in small ways: no de-duplication, and markup is not stripped. A link the check accepts must resolve on the site, so the site's anchors must come from the same rule.
- **Links out of the site.** Most documents link source files, crates and workflows that are not pages, and many link `docs/plans/` and `docs/artifacts/`, which git ignores ([ADR-0039](0039-plans-and-design-kept-local.md)). Each link must be rewritten: to a page, to the file on GitHub, or, for a local-only folder, to text, since there is nothing published to point at.
- **The page script.** The playground is TypeScript that loads a `wasm-bindgen` module. The repository's TypeScript is linted with `typescript-eslint` strict type-checked, formatted with Prettier and tested with vitest at the 70% floor, all on Node 22.
- **One Pages site.** A repository has one Pages site. Until now `release.yml`'s `schema-pages` job deployed a site holding only `schema/`, from each `vX.Y.Z` tag, so that the URLs serve the released contract and not work in progress on `main`. A site deployed from `main` replaces whatever was deployed before it, so the two deployments would overwrite each other.

## Decision

**A small generator in TypeScript, under `site/`, on `markdown-it`.**

| Part | What it does |
| --- | --- |
| `site/src/pages.ts` | Each tracked (`git ls-files`) Markdown file under `README.md`, `CONTRIBUTING.md`, `SECURITY.md`, `docs/`, `schema/README.md` and `cookbook/README.md` becomes a page; `README.md` becomes its folder's `index.html`. A file under `docs/plans/` or `docs/artifacts/` is never a page, even in a checkout that has them. |
| `site/src/slug.ts` | The link check's slug rule, ported, with its examples and properties as tests. |
| `site/src/links.ts` | Rewrites each relative link: to a page, to a served file (the schemas), to `github.com/.../blob/<commit>/<path>` for any other tracked file, or to plain text for a local-only folder. A link that resolves to nothing fails the build. |
| `site/src/render.ts`, `layout.ts` | Render and lay out each page. Raw HTML is kept only for comments, line breaks and explicit anchors, and is otherwise escaped. Mermaid diagrams are shown as their source with a link to GitHub's rendering, because rendering them here would mean shipping Mermaid, whose dependency tree includes `elkjs` under EPL-2.0. |
| `site/src/linkcheck.ts` | Checks the built site: every relative `href`, `src` and `data-module` must name a file it holds, and every fragment an `id` on that page. |
| `site/web/` | The playground's page script and the stylesheet. |
| `site/build.sh` | Compiles `rb-wasm` for `wasm32-unknown-unknown`, binds it with `wasm-bindgen --target web`, as the `wasm` CI job does, compiles the generator and runs it. |

Every page carries a content security policy: scripts from the site only, no inline script, no `eval`, and `wasm-unsafe-eval` so the module can compile. Plan 0004 § 1.11 asks for this.

**The playground takes YAML and JSON only, and says so.** It runs `evaluate` over a pasted configuration and cruise result, with any output type `rb-report` has except `x-dot-webpage`, which needs GraphViz's `dot` from the `PATH` ([ADR-0053](0053-x-dot-webpage-draws-with-graphviz-dot.md)). The page states that JavaScript configurations and .NET assemblies are not accepted ([ADR-0067](0067-the-playground-facade-and-an-optional-javascript-engine.md)). It wires only the exports `rb-wasm` has on `main` when the site is built: `evaluate` and `version`.

**One deployment, from `main`, carrying the schemas of the latest release.** `.github/workflows/docs.yml` builds the site:

- on a pull request that touches its inputs: build, link check, licence check and browser test, with no deploy;
- on a push to `main`, a manual run on `main`, and the completion of a `release.yml` run started by a `vX.Y.Z` tag push: the same, then deploy.

The build reads `schema/v1.json` and `schema/config-v1.json` from the tag of the latest published release (`gh release view`), not from the checkout. It refuses a file whose `$id` is not the URL it is served at. A `workflow_run` runs in the default branch's context, so a release rebuilds the site from `main` with the new schemas and deploys it through the same `main`-only deployment. `release.yml`'s `schema-pages` job is removed, so nothing else writes the site. Only the deploy job holds `pages: write` and `id-token: write`.

**The npm packages are licence-checked against the same list as the crates.** The site adds two root dev dependencies:

- `markdown-it`, MIT;
- `playwright`, Apache-2.0, pinned, for the browser test that plan 0004 § 1.8 names. `vitest` drives it through Playwright's library API, so there is one test runner.

`node site/dist/node/main.js licences` walks both packages' trees in `package-lock.json` and checks each licence against `deny.toml`'s `allow` list. It has one exception, scoped and with its reason: `argparse`, Python-2.0, which is `markdown-it`'s command-line parser and is never loaded by the library entry the site imports. The check runs in the `site` CI job and in `docs.yml`. No npm code ships to the browser: the site serves only the compiled page script and the `rb-wasm` module.

## Consequences

- The schema URLs keep serving the released contract. A release now redeploys the whole site from `main`, and a docs change on `main` redeploys with the schemas of the latest release.
- The first deploy from `main` replaces the schema-only site. `schema/v1.json` and `schema/config-v1.json` stay at the same URLs, beside the pages.
- The `github-pages` environment needs only `main` admitted. The rule that also admits `v*` tags can be removed, but it does not have to be.
- A broken link in a source fails the build. So does a broken link or anchor in the built site, such as a page link whose target is not built.
- The published site holds no plan and no exported design. Links to them render as text with a tooltip naming ADR-0039, so a reader sees the reference and knows why it is not a link.
- Mermaid diagrams render on GitHub, not on the site. If a renderer with a licence on the allow-list becomes available, adding it changes only `render.ts`.
- The generator and the page script are about 1,400 lines of TypeScript, comments included, with their own tests at the coverage floor. A plugin model is not needed, because the cookbook pages (Step 5) and the coverage tables (Step 4) are generated by the same code.

## Alternatives considered

| Option | Why not |
| --- | --- |
| `mdbook` (MPL-2.0, Rust) | Needs a `SUMMARY.md` listing every page, so pages would be listed by hand or by a preprocessor written for the purpose. Its anchors follow its own slug rule, so the link-check rule would need a preprocessor too, and so would link rewriting. The playground's TypeScript would sit outside the Rust tool's build. |
| `zola` (MIT, Rust) | Wants content in its own tree with front matter, so `docs/` would be copied and rewritten at build time, which is the generator's job again. Its anchors differ from the link check's in the same way. |
| VitePress, Astro, Eleventy (MIT, Node) | Each brings a large dependency tree (Vite, Vue or its own renderer), a slug and link pipeline to override plugin by plugin, and inline scripts in its output that a no-inline-script policy would need hashes for. |
| Keep `schema-pages` and add a second, docs-only deployment | One Pages site per repository: each deployment replaces the other, so the docs would vanish at every release and the schemas would vanish at every docs change. |
| Serve the schemas from the checkout on `main` | The URL would serve unreleased work in progress, which [docs/release.md](../release.md) rules out. |
| Render Mermaid with the `mermaid` package | Its dependency tree includes `elkjs` (EPL-2.0), which is outside the allow-list. |
