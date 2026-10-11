// The playground in a browser: the built site (site/build.sh) served locally, the playground page
// opened in headless Chromium, and `evaluate` run on the committed guard fixture it opens with. The
// report must find what the command line found (the fixture's expected.json, which the `guards`
// job byte-compares against the binary), and the evaluation is timed against the 500 ms target.
//
// RB_PLAYGROUND_FIXTURE=<dir> times another fixture of the same shape (rulebearing.yaml or
// rulebearing.json, graph.json, expected.json) instead; plan 0004's mutation fixture is one, once
// its graph is written from a dependency-cruiser checkout (conformance/dependency-cruiser/mutations).
//
// Plan: docs/plans/pending/0004-wave-4-reach.md, Step 3 and § 1.8 ("evaluate the wave 1 mutation
// fixture in the browser in under 500 ms after load"; "Playwright test in the docs build").
// Decision: docs/adr/0072-the-docs-site-builds-from-docs-and-serves-the-released-schemas.md.
// Requirement: FR-REACH-01.

import { existsSync, readFileSync } from 'node:fs';
import type { Server } from 'node:http';
import { join, resolve } from 'node:path';
import { chromium } from 'playwright';
import type { Browser, Locator } from 'playwright';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { serve, urlOf } from '../../src/serve.js';

const root = resolve(import.meta.dirname, '../../..');
const site = resolve(process.env.RB_SITE_DIR ?? join(root, 'target/site'));
const fixture = resolve(
  process.env.RB_PLAYGROUND_FIXTURE ?? join(root, 'cookbook/guards/no-circular'),
);
const LIMIT_MS = 500;

interface Expected {
  violations: unknown[];
  vacuousRules: unknown[];
}

function configOf(dir: string): string {
  const yaml = join(dir, 'rulebearing.yaml');
  return readFileSync(existsSync(yaml) ? yaml : join(dir, 'rulebearing.json'), 'utf8');
}

async function setValue(textarea: Locator, value: string): Promise<void> {
  await textarea.evaluate((element, text) => {
    (element as unknown as { value: string }).value = text;
  }, value);
}

let server: Server;
let browser: Browser;

beforeAll(async () => {
  if (!existsSync(join(site, 'playground.html'))) {
    throw new Error(`${site} holds no built site: run site/build.sh first`);
  }
  server = await serve(site, 0);
  browser = await chromium.launch();
});

afterAll(async () => {
  await browser.close();
  await new Promise((done) => server.close(done));
});

describe('the playground page', () => {
  it('evaluates the fixture as the command line does, within the time target', async () => {
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

    // Set as a paste would; typing a graph of a megabyte or more key by key takes minutes.
    await setValue(component.locator('textarea[name="config"]'), configOf(fixture));
    await setValue(
      component.locator('textarea[name="graph"]'),
      readFileSync(join(fixture, 'graph.json'), 'utf8'),
    );
    await component.locator('select[name="output-type"]').selectOption('json');
    await component.locator('[data-run]').click();
    await expect.poll(() => component.getAttribute('data-state')).toBe('ok');

    const output = (await component.locator('[data-output]').textContent()) ?? '';
    const result = JSON.parse(output) as {
      summary: { violations: unknown; vacuousRules?: unknown };
    };
    const expected = JSON.parse(readFileSync(join(fixture, 'expected.json'), 'utf8')) as Expected;
    expect(result.summary.violations).toEqual(expected.violations);
    expect(result.summary.vacuousRules ?? []).toEqual(expected.vacuousRules);

    const elapsed = Number(await component.getAttribute('data-elapsed'));
    console.log(`playground: evaluate on ${fixture} took ${elapsed.toFixed(1)} ms in Chromium`);
    expect(elapsed).toBeLessThan(LIMIT_MS);
    expect(errors).toEqual([]);
  });

  it('shows the reason a JavaScript configuration is refused', async () => {
    const page = await browser.newPage();
    await page.goto(`${urlOf(server)}playground.html`);
    const component = page.locator('[data-playground]');
    await expect.poll(() => component.getAttribute('data-state')).toBe('ready');
    await component.locator('textarea[name="config"]').fill('module.exports = {};');
    await component.locator('[data-run]').click();
    await expect.poll(() => component.getAttribute('data-state')).toBe('error');
    expect(await component.locator('[data-status]').textContent()).toMatch(
      /this build has no JavaScript engine/u,
    );
  });
});
