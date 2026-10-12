// The cookbook's question pages: which tracked files are question fixtures, how a fixture is read,
// that a README carries exactly the markers its fixture calls for, what each marker becomes, and
// that every committed fixture renders. Plan: docs/plans/pending/0004-wave-4-reach.md, Step 5;
// docs/adr/0074-the-cookbook-question-pages-are-fixtures-beside-the-guard-catalogue.md.
import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import {
  IMPORT_LINTER_MARKERS,
  MARKERS,
  QUESTIONS_ROOT,
  asFindings,
  fileBlock,
  fillQuestion,
  findingsTable,
  firstRule,
  questionExample,
  questionSlug,
  questionSlugs,
  readQuestion,
  unlisted,
  withPlaceholders,
} from '../src/cookbook.js';
import type { Findings, Question } from '../src/cookbook.js';
import { trackedFolders } from '../src/links.js';
import type { LinkContext } from '../src/links.js';
import { pagePath } from '../src/pages.js';
import { createRenderer, renderPage } from '../src/render.js';

const realRepo = resolve(import.meta.dirname, '../..');

const FINDINGS: Findings = {
  violations: [
    {
      from: 'shop/domain/order.py',
      to: 'shop/web/formatting.py',
      type: 'reachability',
      rule: { name: 'domain-is-the-lowest-layer', severity: 'error' },
      via: [{ name: 'shop/domain/pricing.py' }, { name: 'shop/web/formatting.py' }],
      fix: 'Move it <down>.',
    },
    {
      from: 'shop/domain/pricing.py',
      to: 'shop/web/formatting.py',
      type: 'reachability',
      rule: { name: 'domain-is-the-lowest-layer', severity: 'error' },
      fix: 'Move it <down>.',
    },
    {
      from: 'shop/billing',
      to: 'shop/orders',
      type: 'cycle',
      rule: { name: 'no-cycle', severity: 'warn' },
      cycle: [{ name: 'shop/orders' }, { name: 'shop/billing' }],
    },
  ],
  vacuousRules: [],
};

function readme(markers: readonly string[]): string {
  return `# A question\n\nWhy.\n\n${markers.map((m) => `<!-- cookbook:${m} -->\n`).join('\n')}`;
}

function question(overrides: Partial<Question> = {}): Question {
  return {
    slug: 'a-question',
    dir: `${QUESTIONS_ROOT}a-question`,
    readme: readme(MARKERS),
    config: 'rules: {}\n',
    converted: '{ "forbidden": [] }\n',
    dropped: 'dropped 1 item(s)\n',
    expected: FINDINGS,
    files: { 'src/a.ts': "import './b';\n", 'package.json': '{}\n' },
    language: 'typescript',
    ...overrides,
  };
}

describe('which files are question pages', () => {
  it.each([
    ['cookbook/questions/domain-does-no-io/README.md', 'domain-does-no-io'],
    ['cookbook/questions/README.md', undefined],
    ['cookbook/questions/domain-does-no-io/repo/README.md', undefined],
    ['cookbook/questions//README.md', undefined],
    ['cookbook/guards/no-circular/README.md', undefined],
    ['docs/cli.md', undefined],
  ])('%s is the page of %s', (source, slug) => {
    expect(questionSlug(source)).toBe(slug);
  });

  it('needs a README and a rulebearing.yaml, and sorts', () => {
    expect(
      questionSlugs([
        'cookbook/questions/b/README.md',
        'cookbook/questions/b/rulebearing.yaml',
        'cookbook/questions/a/README.md',
        'cookbook/questions/a/rulebearing.yaml',
        'cookbook/questions/no-config/README.md',
        'cookbook/questions/README.md',
      ]),
    ).toEqual(['a', 'b']);
  });
});

describe('asFindings', () => {
  it('reads an expected.json', () => {
    expect(asFindings(FINDINGS, 'x')).toEqual(FINDINGS);
  });

  it.each([
    [null, 'x is not an object'],
    [{ violations: [] }, 'x has no violations and vacuousRules arrays'],
    [{ violations: [{ from: 'a' }], vacuousRules: [] }, 'x: a violation without from, to and rule'],
    [
      { violations: [{ from: 'a', to: 'b', rule: { name: 'r' } }], vacuousRules: [] },
      'x: a violation without from, to and rule',
    ],
    [{ violations: [null], vacuousRules: [] }, 'x: a violation without from, to and rule'],
  ])('refuses %j', (value, message) => {
    expect(() => asFindings(value, 'x')).toThrow(message);
  });
});

describe('the markers', () => {
  it('become placeholder paragraphs when each appears once', () => {
    const text = withPlaceholders(question());
    expect(text).not.toContain('<!-- cookbook:');
    expect(text).toContain('RBCOOKBOOKMARKERRULE');
    expect(text).toContain('RBCOOKBOOKMARKERPLAYGROUND');
  });

  it('are required once each, and only those the fixture has content for', () => {
    expect(() => withPlaceholders(question({ readme: readme(['rule', 'rule']) }))).toThrow(
      /<!-- cookbook:rule --> appears 2 times, not once; <!-- cookbook:converted --> appears 0 times/u,
    );
    expect(() => withPlaceholders(question({ readme: readme([...MARKERS, 'imported']) }))).toThrow(
      /<!-- cookbook:imported --> is not a marker this page has content for/u,
    );
  });

  it('include the import-linter ones when the repository holds a contract', () => {
    const importLinter = {
      contract: '[importlinter]\n',
      imported: 'rules: {}\n',
      expected: FINDINGS,
    };
    expect(() => withPlaceholders(question({ importLinter }))).toThrow(
      /<!-- cookbook:import-linter --> appears 0 times/u,
    );
    expect(
      withPlaceholders(
        question({ importLinter, readme: readme([...MARKERS, ...IMPORT_LINTER_MARKERS]) }),
      ),
    ).toContain('RBCOOKBOOKMARKERIMPORTED_VIOLATIONS');
  });
});

describe('what the markers become', () => {
  it('shows a file under its name, escaped', () => {
    expect(fileBlock('a.yaml', 'x: "<y>"\n', 'yaml')).toBe(
      '<figure class="cookbook-file"><figcaption><code>a.yaml</code></figcaption><pre><code class="language-yaml">x: &quot;&lt;y&gt;&quot;\n</code></pre></figure>\n',
    );
  });

  it('tabulates the findings with the chain or cycle, and each fix once', () => {
    const table = findingsTable(FINDINGS);
    expect(table).toContain(
      '<td><code>shop/domain/pricing.py → shop/web/formatting.py</code></td>',
    );
    expect(table).toContain('<td><code>shop/orders → shop/billing</code></td>');
    expect(table).toContain('<td>warn</td><td><code>no-cycle</code></td>');
    expect(table.match(/fix: /gu)).toHaveLength(1);
    expect(table).toContain('Move it &lt;down&gt;.');
    expect(findingsTable({ violations: [], vacuousRules: [] })).not.toContain('cookbook-fixes');
    const slice = findingsTable({
      violations: [
        {
          from: 'billing',
          to: 'orders',
          type: 'slice',
          rule: { name: 'acyclic:shop', severity: 'error' },
          via: [{ name: 'a.py -> b.py' }, { name: 'b.py -> a.py' }],
        },
      ],
      vacuousRules: [],
    });
    expect(slice).toContain('<code>a.py -&gt; b.py; b.py -&gt; a.py</code>');
  });

  it('presets the playground to the first rule broken, in the source-files mode', () => {
    const q = question();
    expect(firstRule(q)).toBe('domain-is-the-lowest-layer');
    expect(firstRule(question({ expected: { violations: [], vacuousRules: [] } }))).toBe('');
    expect(questionExample(q)).toEqual({
      id: 'a-question',
      title: "This page's repository",
      data: {
        id: 'a-question',
        mode: 'files',
        config: q.config,
        outputType: 'err-long',
        rule: 'domain-is-the-lowest-layer',
        language: 'typescript',
        files: q.files,
      },
    });
  });
});

describe('a rendered page', () => {
  const context: LinkContext = {
    pages: new Set(),
    tracked: new Set(),
    served: new Map(),
    repoUrl: 'https://example.com/r',
    commit: 'c0ffee',
  };
  const render = (q: Question): string =>
    renderPage(createRenderer(), `${q.dir}/README.md`, withPlaceholders(q), context, new Set())
      .html;

  it('holds every generated part where its marker was', () => {
    const importLinter = {
      contract: '[importlinter]\nroot_package = shop\n',
      imported: '# imported\n',
      expected: {
        violations: [FINDINGS.violations[2]].flatMap((v) => (v ? [v] : [])),
        vacuousRules: [],
      },
    };
    const q = question({ importLinter, readme: readme([...MARKERS, ...IMPORT_LINTER_MARKERS]) });
    const path = pagePath(`${q.dir}/README.md`);
    const html = fillQuestion(render(q), q, path);
    expect(html).not.toContain('RBCOOKBOOKMARKER');
    expect(html).toContain('<code>rulebearing.yaml</code>');
    expect(html).toContain('<code>.dependency-cruiser.json</code>');
    expect(html).toContain('dropped 1 item(s)');
    expect(html).toContain('<code>.importlinter</code>');
    expect(html).toContain('root_package = shop');
    expect(html).toContain('<code>rulebearing import import-linter</code>');
    expect(html.match(/class="cookbook-findings"/gu)).toHaveLength(2);
    expect(html).toContain('data-preload="a-question"');
    expect(html).toContain('data-module="../../../playground/rb_wasm.js"');
    expect(html).toContain('data-example-data="a-question"');
  });

  it('needs each marker on a line of its own', () => {
    const q = question({
      readme: `# Q\n\nText.\n<!-- cookbook:rule -->\n\n${MARKERS.slice(1)
        .map((m) => `<!-- cookbook:${m} -->\n`)
        .join('\n')}`,
    });
    expect(() => fillQuestion(render(q), q, 'x.html')).toThrow(
      /<!-- cookbook:rule --> must stand alone between blank lines/u,
    );
  });
});

describe('the index', () => {
  it('names the questions it does not link', () => {
    const index =
      '| [A](a/README.md) | x |\n[B](./b/) and [C](c/README.md#why) and [plan](../../docs/x.md)\n';
    expect(unlisted(index, ['a', 'b', 'c', 'd'])).toEqual(['d']);
  });
});

describe('readQuestion', () => {
  let repo: string;

  function write(path: string, text: string): void {
    const full = join(repo, path);
    mkdirSync(dirname(full), { recursive: true });
    writeFileSync(full, text);
  }

  beforeEach(() => {
    repo = mkdtempSync(join(tmpdir(), 'rb-cookbook-'));
  });

  afterEach(() => {
    rmSync(repo, { recursive: true, force: true });
  });

  it('reads a fixture, its repository and the language of its files', () => {
    const dir = `${QUESTIONS_ROOT}q`;
    const files: Record<string, string> = {
      [`${dir}/README.md`]: '# Q\n',
      [`${dir}/rulebearing.yaml`]: 'rules: {}\n',
      [`${dir}/converted/.dependency-cruiser.json`]: '{}\n',
      [`${dir}/converted/dropped.txt`]: '',
      [`${dir}/expected.json`]: JSON.stringify(FINDINGS),
      [`${dir}/repo/shop/a.py`]: 'import shop.b\n',
      [`${dir}/repo/.importlinter`]: '[importlinter]\n',
      [`${dir}/imported/rulebearing.yaml`]: '# imported\n',
      [`${dir}/imported/expected.json`]: JSON.stringify(FINDINGS),
    };
    for (const [path, text] of Object.entries(files)) {
      write(path, text);
    }
    const q = readQuestion(repo, Object.keys(files), 'q');
    expect(q.language).toBe('python');
    expect(Object.keys(q.files)).toEqual(['.importlinter', 'shop/a.py']);
    expect(q.importLinter?.contract).toBe('[importlinter]\n');
    expect(q.importLinter?.expected).toEqual(FINDINGS);
    expect(q.expected).toEqual(FINDINGS);

    const tracked = Object.keys(files).filter((f) => !f.includes('/imported/'));
    expect(() => readQuestion(repo, tracked, 'q')).toThrow(
      /cookbook\/questions\/q\/imported\/rulebearing.yaml is not tracked/u,
    );
    expect(() =>
      readQuestion(
        repo,
        Object.keys(files).filter((f) => !f.includes('/repo/')),
        'q',
      ),
    ).toThrow(/repo\/ holds no tracked file/u);
  });
});

describe('the committed question fixtures', () => {
  const tracked = execFileSync('git', ['ls-files', '-z'], { cwd: realRepo, encoding: 'utf8' })
    .split('\0')
    .filter((f) => f !== '');
  const slugs = questionSlugs(tracked);

  it('cover the design questions and every import-linter contract kind, all linked', () => {
    expect(slugs).toEqual(
      expect.arrayContaining([
        'features-must-not-know-each-other',
        'controllers-only-through-services',
        'nothing-imports-the-producer',
      ]),
    );
    const contracts = slugs
      .map((slug) => readQuestion(realRepo, tracked, slug).importLinter?.contract ?? '')
      .join('\n');
    for (const kind of ['layers', 'independence', 'forbidden', 'protected', 'acyclic_siblings']) {
      expect(contracts).toMatch(new RegExp(`^type = ${kind}$`, 'mu'));
    }
    const index = readFileSync(join(realRepo, QUESTIONS_ROOT, 'README.md'), 'utf8');
    expect(unlisted(index, slugs)).toEqual([]);
  });

  it.each(slugs)('%s renders with every part, no broken link and a failing edge', (slug) => {
    const q = readQuestion(realRepo, tracked, slug);
    const source = `${q.dir}/README.md`;
    const pages = new Set(tracked.filter((f) => f.endsWith('.md')));
    const page = renderPage(
      createRenderer(),
      source,
      withPlaceholders(q),
      { ...context(pages, tracked) },
      trackedFolders(tracked),
    );
    expect(page.broken).toEqual([]);
    const html = fillQuestion(page.html, q, pagePath(source));
    expect(html).not.toContain('RBCOOKBOOKMARKER');
    expect(q.expected.violations.length).toBeGreaterThan(0);
    expect(html).toContain(`data-preload="${slug}"`);
  });

  function context(pages: ReadonlySet<string>, files: readonly string[]): LinkContext {
    return {
      pages,
      tracked: new Set(files),
      served: new Map(),
      repoUrl: 'https://example.com/r',
      commit: 'c0ffee',
    };
  }
});
