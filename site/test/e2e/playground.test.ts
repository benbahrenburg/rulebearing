// The playground in a browser: the built site (site/build.sh) served locally, the playground page
// opened in headless Chromium, and each export driven through the page.
//
// - `evaluate` on the committed guard fixture the page opens with: the report must find what the
//   command line found (the fixture's expected.json, which the `guards` job byte-compares against
//   the binary).
// - `cruise_ts` and `cruise_py` on `rb-wasm`'s fixture repositories, loaded from the page's
//   embedded examples and, once each, chosen as a folder through the folder picker: the report
//   must be byte for byte what `rulebearing cruise -T <type>` printed there (the fixtures'
//   `expected/`, which rb-cli's tests/playground_fixtures.rs holds the command line to). The
//   reporters compared print no timestamp, so the browser's clock does not enter.
// - `explain` over the cruise result of either mode: byte for byte the command line's
//   `expected/explain-<rule>.out`.
// - The module's refusals shown verbatim.
// - A cookbook question page (plan 0004, Step 5; docs/adr/0074): its playground preloaded with the
//   page's repository and rule, and Run finding the violations the page shows.
//
// Every run is timed against the 500 ms target.
//
// RB_PLAYGROUND_FIXTURE=<dir> times another fixture of the same shape (rulebearing.yaml or
// rulebearing.json, graph.json, expected.json) for `evaluate` instead; plan 0004's mutation
// fixture is one, once its graph is written from a dependency-cruiser checkout
// (conformance/dependency-cruiser/mutations).
//
// Plan: docs/plans/pending/0004-wave-4-reach.md, Step 3 and § 1.8 ("evaluate the wave 1 mutation
// fixture in the browser in under 500 ms after load"; "Playwright test in the docs build").
// Decisions: docs/adr/0072-the-docs-site-builds-from-docs-and-serves-the-released-schemas.md,
// docs/adr/0073-extractors-read-files-through-rb-model.md. Requirement: FR-REACH-01.

import { existsSync, readFileSync } from 'node:fs';
import type { Server } from 'node:http';
import { join, resolve } from 'node:path';
import { chromium } from 'playwright';
import type { Browser, Locator, Page } from 'playwright';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { serve, urlOf } from '../../src/serve.js';

const root = resolve(import.meta.dirname, '../../..');
const site = resolve(process.env.RB_SITE_DIR ?? join(root, 'target/site'));
const fixture = resolve(
  process.env.RB_PLAYGROUND_FIXTURE ?? join(root, 'cookbook/guards/no-circular'),
);
const repos = join(root, 'crates/rb-wasm/tests/fixtures');
const LIMIT_MS = 500;

/** The reporters compared byte for byte: the JSON and two text reporters, none with a timestamp. */
const COMPARED = ['json', 'err-long', 'text'] as const;

/** The rules explained, per fixture, as rb-wasm's own tests explain them. */
const EXPLAINED = {
  typescript: ['no-circular', 'no-unresolvable'],
  python: ['no-circular', 'ui-not-to-models'],
} as const;

interface Expected {
  violations: unknown[];
  vacuousRules: unknown[];
}

function configOf(dir: string): string {
  const yaml = join(dir, 'rulebearing.yaml');
  return readFileSync(existsSync(yaml) ? yaml : join(dir, 'rulebearing.json'), 'utf8');
}

function expectedOf(name: string, file: string): string {
  return readFileSync(join(repos, name, 'expected', file), 'utf8');
}

async function setValue(textarea: Locator, value: string): Promise<void> {
  await textarea.evaluate((element, text) => {
    (element as unknown as { value: string }).value = text;
  }, value);
}

let server: Server;
let browser: Browser;
const timings: string[] = [];

beforeAll(async () => {
  if (!existsSync(join(site, 'playground.html'))) {
    throw new Error(`${site} holds no built site: run site/build.sh first`);
  }
  server = await serve(site, 0);
  browser = await chromium.launch();
});

afterAll(async () => {
  console.log(`playground timings in Chromium:\n  ${timings.join('\n  ')}`);
  await browser.close();
  await new Promise((done) => server.close(done));
});

/** The playground page, loaded and ready, and the errors it logs. */
async function open(): Promise<{ page: Page; component: Locator; errors: string[] }> {
  const page = await browser.newPage();
  const errors: string[] = [];
  page.on('pageerror', (e) => errors.push(e.message));
  page.on('console', (m) => {
    if (m.type() === 'error') {
      errors.push(m.text());
    }
  });
  await page.goto(`${urlOf(server)}playground.html`);
  const component = page.locator('[data-playground]');
  await expect.poll(() => component.getAttribute('data-state')).toBe('ready');
  return { page, component, errors };
}

/** Clicks `button`, waits for the run, and returns its output after checking the time. */
async function ran(component: Locator, button: string, what: string): Promise<string> {
  // Each run replaces the state; clearing it first makes the wait see this run, not the last.
  await component.evaluate((element) => {
    (element as unknown as { removeAttribute: (name: string) => void }).removeAttribute(
      'data-state',
    );
  });
  await component.locator(button).click();
  await expect.poll(() => component.getAttribute('data-state')).not.toBeNull();
  const state = await component.getAttribute('data-state');
  const status = (await component.locator('[data-status]').textContent()) ?? '';
  expect(state, `${what}: ${status}`).toBe('ok');
  const elapsed = Number(await component.getAttribute('data-elapsed'));
  const action = (await component.getAttribute('data-action')) ?? '';
  timings.push(`${what}: ${action} ${elapsed.toFixed(1)} ms`);
  expect(elapsed, what).toBeLessThan(LIMIT_MS);
  return (await component.locator('[data-output]').textContent()) ?? '';
}

describe('the playground page', () => {
  it('evaluates the fixture as the command line does, within the time target', async () => {
    const { component, errors } = await open();
    // Set as a paste would; typing a graph of a megabyte or more key by key takes minutes.
    await setValue(component.locator('textarea[name="config"]'), configOf(fixture));
    await setValue(
      component.locator('textarea[name="graph"]'),
      readFileSync(join(fixture, 'graph.json'), 'utf8'),
    );
    await component.locator('select[name="output-type"]').selectOption('json');
    const output = await ran(component, '[data-run]', `evaluate on ${fixture}`);
    const result = JSON.parse(output) as {
      summary: { violations: unknown; vacuousRules?: unknown };
    };
    const expected = JSON.parse(readFileSync(join(fixture, 'expected.json'), 'utf8')) as Expected;
    expect(result.summary.violations).toEqual(expected.violations);
    expect(result.summary.vacuousRules ?? []).toEqual(expected.vacuousRules);
    expect(await component.getAttribute('data-action')).toBe('evaluate');
    expect(errors).toEqual([]);
  });

  for (const name of ['typescript', 'python'] as const) {
    const exported = name === 'typescript' ? 'cruise_ts' : 'cruise_py';

    it(`runs ${exported} on the ${name} example as the command line does`, async () => {
      const { component, errors } = await open();
      await component.locator(`button[data-example="${name}"]`).click();
      expect(await component.locator('[data-panel="files"]').isVisible()).toBe(true);
      expect(await component.locator('[data-panel="graph"]').isVisible()).toBe(false);
      for (const outputType of COMPARED) {
        await component.locator('select[name="output-type"]').selectOption(outputType);
        const output = await ran(component, '[data-run]', `${name} example, ${outputType}`);
        expect(await component.getAttribute('data-action')).toBe(exported);
        expect(output, `${name}: ${outputType}`).toBe(expectedOf(name, `${outputType}.out`));
      }
      for (const rule of EXPLAINED[name]) {
        await component.locator('input[name="rule"]').fill(rule);
        const output = await ran(component, '[data-explain]', `${name} example, explain ${rule}`);
        expect(output, `${name}: explain ${rule}`).toBe(expectedOf(name, `explain-${rule}.out`));
      }
      expect(errors).toEqual([]);
    });

    it(`runs ${exported} on the ${name} repository chosen as a folder`, async () => {
      const { component, errors } = await open();
      await component.locator('select[name="mode"]').selectOption('files');
      await component.locator('input[name="folder"]').setInputFiles(join(repos, name, 'repo'));
      await expect
        .poll(() => component.locator('[data-files-note]').textContent())
        .toMatch(
          /file\(s\) read, .* KiB, kept in this browser\. The configuration is rulebearing\.yaml from the folder\./u,
        );
      expect(await component.locator('textarea[name="config"]').inputValue()).toBe(
        readFileSync(join(repos, name, 'repo/rulebearing.yaml'), 'utf8'),
      );
      // Left on "Infer from the files".
      expect(await component.locator('select[name="language"]').inputValue()).toBe('auto');
      await component.locator('select[name="output-type"]').selectOption('json');
      const output = await ran(component, '[data-run]', `${name} folder, json`);
      expect(await component.getAttribute('data-action')).toBe(exported);
      expect(output).toBe(expectedOf(name, 'json.out'));
      expect(errors).toEqual([]);
    });

    it(`explains over a pasted ${name} cruise result as the command line does`, async () => {
      const { component } = await open();
      await setValue(
        component.locator('textarea[name="config"]'),
        readFileSync(join(repos, name, 'repo/rulebearing.yaml'), 'utf8'),
      );
      await setValue(component.locator('textarea[name="graph"]'), expectedOf(name, 'json.out'));
      const rule = EXPLAINED[name][1];
      await component.locator('input[name="rule"]').fill(rule);
      const output = await ran(component, '[data-explain]', `${name} pasted, explain ${rule}`);
      expect(output).toBe(expectedOf(name, `explain-${rule}.out`));
    });
  }

  // A cookbook question page opens with its own repository and rule loaded; Run must find the
  // violations the page shows (expected.json, which cookbook/questions/run.sh holds the command
  // line to). One TypeScript and one Python page.
  for (const [slug, exported] of [
    ['features-must-not-know-each-other', 'cruise_ts'],
    ['layers-only-import-downward', 'cruise_py'],
  ] as const) {
    it(`runs the preloaded playground of the cookbook page ${slug}`, async () => {
      const dir = join(root, 'cookbook/questions', slug);
      const page = await browser.newPage();
      const errors: string[] = [];
      page.on('pageerror', (e) => errors.push(e.message));
      await page.goto(`${urlOf(server)}cookbook/questions/${slug}/index.html`);
      const component = page.locator('[data-playground]');
      await expect.poll(() => component.getAttribute('data-state')).toBe('ready');
      expect(await component.locator('[data-status]').textContent()).toBe(
        `Loaded the ${slug} example.`,
      );
      expect(await component.locator('select[name="mode"]').inputValue()).toBe('files');
      expect(await component.locator('textarea[name="config"]').inputValue()).toBe(
        readFileSync(join(dir, 'rulebearing.yaml'), 'utf8'),
      );
      const shown = await page.locator('table.cookbook-findings').first().textContent();
      const expected = JSON.parse(readFileSync(join(dir, 'expected.json'), 'utf8')) as Expected & {
        violations: { from: string; to: string }[];
      };
      for (const { from, to } of expected.violations) {
        expect(shown).toContain(from);
        expect(shown).toContain(to);
      }
      await component.locator('select[name="output-type"]').selectOption('json');
      const output = await ran(component, '[data-run]', `cookbook ${slug}, json`);
      expect(await component.getAttribute('data-action')).toBe(exported);
      const result = JSON.parse(output) as {
        summary: { violations: unknown; vacuousRules?: unknown };
      };
      expect(result.summary.violations).toEqual(expected.violations);
      expect(result.summary.vacuousRules ?? []).toEqual(expected.vacuousRules);
      expect(errors).toEqual([]);
    });
  }

  it('shows the reason a JavaScript configuration is refused', async () => {
    const { component } = await open();
    await component.locator('textarea[name="config"]').fill('module.exports = {};');
    await component.locator('[data-run]').click();
    await expect.poll(() => component.getAttribute('data-state')).toBe('error');
    expect(await component.locator('[data-status]').textContent()).toMatch(
      /this build has no JavaScript engine/u,
    );
  });

  it('shows what needs the disk or a process refused, verbatim', async () => {
    const { component } = await open();
    await component.locator('button[data-example="typescript"]').click();
    await setValue(
      component.locator('textarea[name="config"]'),
      'languages:\n  typescript:\n    externalModuleResolutionStrategy: yarn-pnp\n',
    );
    await component.locator('[data-run]').click();
    await expect.poll(() => component.getAttribute('data-state')).toBe('error');
    expect(await component.locator('[data-status]').textContent()).toMatch(
      /^the files could not be extracted: .*Yarn Plug'n'Play cannot be used/u,
    );
    await component.locator('button[data-example="no-circular"]').click();
    await component.locator('input[name="rule"]').fill('nope');
    await component.locator('[data-explain]').click();
    await expect.poll(() => component.getAttribute('data-state')).toBe('error');
    expect(await component.locator('[data-status]').textContent()).toMatch(/no rule `nope`/u);
  });
});
