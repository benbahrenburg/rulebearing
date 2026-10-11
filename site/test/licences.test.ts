// The licence check for the site's npm packages, against deny.toml's allow-list.
// Plan: docs/plans/pending/0004-wave-4-reach.md, § 1.7 and § 1.11; docs/adr/0019-mit-licence.md.
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import { EXCEPTIONS, allowedLicences, checkLicences, satisfies } from '../src/licences.js';

const root = resolve(import.meta.dirname, '../..');

describe('allowedLicences', () => {
  it("reads deny.toml's list", () => {
    const allowed = allowedLicences(readFileSync(resolve(root, 'deny.toml'), 'utf8'));
    expect(allowed).toContain('MIT');
    expect(allowed).toContain('Apache-2.0');
    expect(allowed).not.toContain('GPL-3.0');
  });

  it('refuses a file without one', () => {
    expect(() => allowedLicences('[bans]\n')).toThrow(/no \[licenses\] allow list/u);
  });
});

describe('satisfies', () => {
  const allowed = ['MIT', 'Apache-2.0'];
  it.each([
    ['MIT', true],
    ['(MPL-2.0 OR Apache-2.0)', true],
    ['MIT AND Apache-2.0', true],
    ['MIT AND GPL-3.0', false],
    ['GPL-3.0', false],
    ['(MIT OR (GPL-3.0 AND X))', false],
  ])('%s: %s', (expression, expected) => {
    expect(satisfies(expression, allowed)).toBe(expected);
  });
});

describe('checkLicences', () => {
  const lock = JSON.stringify({
    packages: {
      '': {},
      'node_modules/top': { license: 'MIT', dependencies: { mid: '1', argparse: '1' } },
      'node_modules/mid': { license: 'GPL-3.0', dependencies: { leaf: '1' } },
      'node_modules/top/node_modules/leaf': { license: 'ISC' },
      'node_modules/mid/node_modules/leaf': { license: 'EPL-2.0' },
      'node_modules/argparse': { license: 'Python-2.0' },
      'node_modules/bare': {},
    },
  });

  it('walks nested trees, honours the exceptions and names every finding', () => {
    expect(checkLicences(lock, ['MIT', 'ISC'], ['top', 'bare', 'absent'])).toEqual([
      { name: 'absent', path: 'node_modules/absent', licence: 'not installed' },
      { name: 'bare', path: 'node_modules/bare', licence: 'none declared' },
      { name: 'mid', path: 'node_modules/mid', licence: 'GPL-3.0' },
      { name: 'leaf', path: 'node_modules/mid/node_modules/leaf', licence: 'EPL-2.0' },
    ]);
    expect(Object.keys(EXCEPTIONS)).toEqual(['argparse']);
  });

  it("passes for the repository's own lock file", () => {
    const allowed = allowedLicences(readFileSync(resolve(root, 'deny.toml'), 'utf8'));
    expect(
      checkLicences(readFileSync(resolve(root, 'package-lock.json'), 'utf8'), allowed),
    ).toEqual([]);
  });
});
