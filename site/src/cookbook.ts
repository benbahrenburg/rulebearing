// The cookbook's question pages: each fixture under cookbook/questions/<slug>/ is a page whose
// README carries the prose and marks where the generated parts go. The generated parts are the
// fixture's own files, which cookbook/questions/run.sh holds to the binary before the site is
// built: the rule (rulebearing.yaml), the dependency-cruiser form `config convert` wrote and its
// report of what it dropped, the violations `cruise` found in the fixture's repository, for an
// import-linter fixture the contract, the rules `import import-linter` wrote and their violations,
// and a playground preloaded with the repository and the rule. Nothing on a page is written twice.
//
// Architecture: docs/architecture.md#distribution. Decisions: docs/adr/0074-the-cookbook-question-pages-are-fixtures-beside-the-guard-catalogue.md,
// docs/adr/0072-the-docs-site-builds-from-docs-and-serves-the-released-schemas.md,
// docs/adr/0073-extractors-read-files-through-rb-model.md (the playground reads the files in memory).
// Plan: docs/plans/pending/0004-wave-4-reach.md, Step 5. Requirement: FR-REACH-01.

import { readFileSync } from 'node:fs';
import { join, posix } from 'node:path';
import { escapeHtml, playgroundComponent } from './layout.js';
import type { EmbeddedExample } from './layout.js';

/** The folder whose fixtures are question pages. */
export const QUESTIONS_ROOT = 'cookbook/questions/';

/** The output type a question page's playground opens with. */
export const QUESTION_OUTPUT = 'err-long';

/** The markers every question page carries, each once, in its README. */
export const MARKERS: readonly string[] = ['rule', 'converted', 'violations', 'playground'];

/** The markers a page whose repository holds an `.importlinter` carries as well, and no other. */
export const IMPORT_LINTER_MARKERS: readonly string[] = [
  'import-linter',
  'imported',
  'imported-violations',
];

const MARKER_LINE = /^<!-- cookbook:([a-z-]+) -->$/gmu;

/** The text a marker line becomes before rendering, so the rendered page holds a findable `<p>`. */
function placeholder(name: string): string {
  return `RBCOOKBOOKMARKER${name.replaceAll('-', '_').toUpperCase()}`;
}

/** One violation as `cruise -T json` prints it, in the fields a page shows. */
export interface Finding {
  readonly from: string;
  readonly to: string;
  readonly type?: string;
  readonly rule: { readonly name: string; readonly severity: string };
  readonly via?: readonly { readonly name: string }[];
  readonly cycle?: readonly { readonly name: string }[];
  readonly fix?: string;
}

/** A fixture's `expected.json`: what the page reports. */
export interface Findings {
  readonly violations: readonly Finding[];
  readonly vacuousRules: readonly unknown[];
}

/** One question page's fixture, read from the tracked files. */
export interface Question {
  readonly slug: string;
  /** The fixture's folder, repository-relative. */
  readonly dir: string;
  /** The README: the page's prose with its markers. */
  readonly readme: string;
  readonly config: string;
  /** `config convert --to dependency-cruiser`'s output. */
  readonly converted: string;
  /** What `config convert` reported it dropped. */
  readonly dropped: string;
  readonly expected: Findings;
  /** The import-linter half, when the repository holds an `.importlinter`. */
  readonly importLinter?: {
    readonly contract: string;
    readonly imported: string;
    readonly expected: Findings;
  };
  /** The repository's files, by their path inside `repo/`. */
  readonly files: Readonly<Record<string, string>>;
  readonly language: 'typescript' | 'python';
}

/** The README of the question page at `source`, or undefined when `source` is no question page. */
export function questionSlug(source: string): string | undefined {
  if (!source.startsWith(QUESTIONS_ROOT)) {
    return undefined;
  }
  const parts = source.slice(QUESTIONS_ROOT.length).split('/');
  return parts.length === 2 && parts[1] === 'README.md' && parts[0] !== '' ? parts[0] : undefined;
}

/** The question fixtures among the tracked files: a README and a rulebearing.yaml, sorted. */
export function questionSlugs(tracked: readonly string[]): string[] {
  const all = new Set(tracked);
  return tracked
    .map(questionSlug)
    .filter((slug): slug is string => slug !== undefined)
    .filter((slug) => all.has(`${QUESTIONS_ROOT}${slug}/rulebearing.yaml`))
    .sort();
}

/** Checks the JSON of an `expected.json`. */
export function asFindings(value: unknown, file: string): Findings {
  if (typeof value !== 'object' || value === null) {
    throw new Error(`${file} is not an object`);
  }
  const { violations, vacuousRules } = value as Record<string, unknown>;
  if (!Array.isArray(violations) || !Array.isArray(vacuousRules)) {
    throw new Error(`${file} has no violations and vacuousRules arrays`);
  }
  for (const v of violations as unknown[]) {
    const f = v as Partial<Finding> | null;
    if (
      typeof f?.from !== 'string' ||
      typeof f.to !== 'string' ||
      typeof f.rule?.name !== 'string' ||
      typeof f.rule.severity !== 'string'
    ) {
      throw new Error(`${file}: a violation without from, to and rule`);
    }
  }
  return { violations: violations as Finding[], vacuousRules };
}

/** Reads the fixture of question `slug` from the repository's tracked files. */
export function readQuestion(repo: string, tracked: readonly string[], slug: string): Question {
  const dir = `${QUESTIONS_ROOT}${slug}`;
  const all = new Set(tracked);
  const read = (path: string): string => {
    const full = `${dir}/${path}`;
    if (!all.has(full)) {
      throw new Error(
        `${full} is not tracked; the cookbook page ${slug} needs it (cookbook/questions/run.sh writes it)`,
      );
    }
    return readFileSync(join(repo, full), 'utf8');
  };
  const json = (path: string): Findings =>
    asFindings(JSON.parse(read(path)) as unknown, `${dir}/${path}`);

  const prefix = `${dir}/repo/`;
  const files: Record<string, string> = {};
  for (const path of tracked.filter((p) => p.startsWith(prefix)).sort()) {
    files[path.slice(prefix.length)] = readFileSync(join(repo, path), 'utf8');
  }
  const paths = Object.keys(files);
  if (paths.length === 0) {
    throw new Error(
      `${dir}/repo/ holds no tracked file; the cookbook page ${slug} needs its repository`,
    );
  }
  const language = paths.some((p) => p.endsWith('.py')) ? 'python' : 'typescript';
  const contract = files['.importlinter'];
  return {
    slug,
    dir,
    readme: read('README.md'),
    config: read('rulebearing.yaml'),
    converted: read('converted/.dependency-cruiser.json'),
    dropped: read('converted/dropped.txt'),
    expected: json('expected.json'),
    ...(contract === undefined
      ? {}
      : {
          importLinter: {
            contract,
            imported: read('imported/rulebearing.yaml'),
            expected: json('imported/expected.json'),
          },
        }),
    files,
    language,
  };
}

/**
 * The README with each marker line replaced by a placeholder paragraph, after checking that the
 * page carries exactly the markers its fixture calls for, each once.
 */
export function withPlaceholders(question: Question): string {
  const wanted = [
    ...MARKERS,
    ...(question.importLinter === undefined ? [] : IMPORT_LINTER_MARKERS),
  ];
  const found = [...question.readme.matchAll(MARKER_LINE)].map((m) => m[1] ?? '');
  const problems: string[] = [];
  for (const name of wanted) {
    const count = found.filter((f) => f === name).length;
    if (count !== 1) {
      problems.push(`<!-- cookbook:${name} --> appears ${String(count)} times, not once`);
    }
  }
  for (const name of new Set(found)) {
    if (!wanted.includes(name)) {
      problems.push(`<!-- cookbook:${name} --> is not a marker this page has content for`);
    }
  }
  if (problems.length > 0) {
    throw new Error(`${question.dir}/README.md: ${problems.join('; ')}`);
  }
  return question.readme.replaceAll(MARKER_LINE, (_line, name: string) => placeholder(name));
}

/** A file shown on the page, under its name. */
export function fileBlock(name: string, text: string, language: string): string {
  return `<figure class="cookbook-file"><figcaption><code>${escapeHtml(name)}</code></figcaption><pre><code class="language-${escapeHtml(language)}">${escapeHtml(text)}</code></pre></figure>\n`;
}

/** The modules a finding passes through: a chain's `via`, a cycle's members. */
function through(finding: Finding): string {
  const steps = (finding.via ?? finding.cycle ?? []).map((s) => s.name);
  // A slice finding's steps are module edges (`a -> b`), listed rather than chained.
  return steps.join(steps.some((s) => s.includes(' -> ')) ? '; ' : ' → ');
}

/** The findings as a table, with each rule's `fix` under it once. */
export function findingsTable(findings: Findings): string {
  const rows = findings.violations
    .map(
      (f) =>
        `<tr><td>${escapeHtml(f.rule.severity)}</td><td><code>${escapeHtml(f.rule.name)}</code></td><td><code>${escapeHtml(f.from)}</code></td><td><code>${escapeHtml(f.to)}</code></td><td>${escapeHtml(f.type ?? '')}</td><td>${through(f) === '' ? '' : `<code>${escapeHtml(through(f))}</code>`}</td></tr>`,
    )
    .join('\n');
  const fixes = new Map<string, string>();
  for (const f of findings.violations) {
    if (f.fix !== undefined && !fixes.has(f.rule.name)) {
      fixes.set(f.rule.name, f.fix);
    }
  }
  const fixList =
    fixes.size === 0
      ? ''
      : `<ul class="cookbook-fixes">${[...fixes]
          .map(([rule, fix]) => `<li><code>${escapeHtml(rule)}</code> fix: ${escapeHtml(fix)}</li>`)
          .join('')}</ul>\n`;
  return `<table class="cookbook-findings">
<thead><tr><th>Severity</th><th>Rule</th><th>From</th><th>To</th><th>Type</th><th>Through</th></tr></thead>
<tbody>
${rows}
</tbody>
</table>
${fixList}`;
}

/** The rule `explain` is preset to: the first one the page's repository breaks. */
export function firstRule(question: Question): string {
  return question.expected.violations[0]?.rule.name ?? '';
}

/** The page's repository as a playground example in the source-files mode. */
export function questionExample(question: Question): EmbeddedExample {
  return {
    id: question.slug,
    title: "This page's repository",
    data: {
      id: question.slug,
      mode: 'files',
      config: question.config,
      outputType: QUESTION_OUTPUT,
      rule: firstRule(question),
      language: question.language,
      files: question.files,
    },
  };
}

/** The HTML each marker stands for, on the page at site path `pagePath`. */
export function markerContent(question: Question, pagePath: string): Map<string, string> {
  const content = new Map<string, string>([
    ['rule', fileBlock('rulebearing.yaml', question.config, 'yaml')],
    [
      'converted',
      fileBlock('.dependency-cruiser.json', question.converted, 'json') +
        fileBlock('dropped (stderr of config convert)', question.dropped, 'text'),
    ],
    ['violations', findingsTable(question.expected)],
    [
      'playground',
      playgroundComponent(pagePath, {
        config: question.config,
        graph: '',
        outputType: QUESTION_OUTPUT,
        rule: firstRule(question),
        examples: [questionExample(question)],
        preload: question.slug,
      }),
    ],
  ]);
  const { importLinter } = question;
  if (importLinter !== undefined) {
    content.set('import-linter', fileBlock('.importlinter', importLinter.contract, 'ini'));
    content.set(
      'imported',
      fileBlock('rulebearing import import-linter', importLinter.imported, 'yaml'),
    );
    content.set('imported-violations', findingsTable(importLinter.expected));
  }
  return content;
}

/** The rendered README with each placeholder replaced by what its marker stands for. */
export function fillQuestion(html: string, question: Question, pagePath: string): string {
  let filled = html;
  for (const [name, content] of markerContent(question, pagePath)) {
    const paragraph = `<p>${placeholder(name)}</p>\n`;
    if (!filled.includes(paragraph)) {
      throw new Error(
        `${question.dir}/README.md: <!-- cookbook:${name} --> must stand alone between blank lines`,
      );
    }
    // A function, so a `$` in the content (a regular expression's anchor) is not a pattern.
    filled = filled.replace(paragraph, () => content);
  }
  return filled;
}

/** The question pages the cookbook's index does not link, so a page cannot be orphaned. */
export function unlisted(index: string, slugs: readonly string[]): string[] {
  const linked = new Set(
    [...index.matchAll(/\]\(([^)#\s]+)/gu)].map((m) =>
      posix.normalize(m[1] ?? '').replace(/\/(README\.md)?$/u, ''),
    ),
  );
  return slugs.filter((slug) => !linked.has(slug));
}
