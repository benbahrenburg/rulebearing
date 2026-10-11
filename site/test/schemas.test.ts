// The schemas come from the release tag, carrying the `$id` they are served at.
// Plan: docs/plans/pending/0004-wave-4-reach.md, Step 3.
import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import {
  SCHEMA_BASE,
  SchemaError,
  checkSchema,
  compareVersions,
  latestReleaseTag,
  localTags,
  readSchemas,
} from '../src/schemas.js';

describe('latestReleaseTag', () => {
  it.each([
    [['v0.1.0', 'v0.1.3', 'v0.1.2'], 'v0.1.3'],
    [['v0.9.0', 'v0.10.0'], 'v0.10.0'],
    [['v1.0.0', 'v2.0.0-rc.1', 'wave-3a-complete'], 'v1.0.0'],
    [[' v1.2.3 '], 'v1.2.3'],
    [['v2.0.0', 'v10.0.0', 'v9.9.9'], 'v10.0.0'],
    [[], undefined],
    [['nightly'], undefined],
  ])('%j -> %s', (tags, expected) => {
    expect(latestReleaseTag(tags)).toBe(expected);
  });

  it('orders versions part by part', () => {
    expect(compareVersions([1, 2, 3], [1, 2, 3])).toBe(0);
    expect(compareVersions([1, 10, 0], [1, 9, 9])).toBeGreaterThan(0);
    expect(compareVersions([0, 0, 1], [0, 1, 0])).toBeLessThan(0);
  });
});

describe('checkSchema', () => {
  it('accepts the schema served at its $id', () => {
    const text = JSON.stringify({ $id: `${SCHEMA_BASE}v1.json` });
    expect(checkSchema('v1.json', text, 'v1.0.0')).toBe(text);
  });

  it('refuses a wrong $id and text that is not JSON', () => {
    expect(() => checkSchema('v1.json', '{"$id":"https://elsewhere/v1.json"}', 'v1.0.0')).toThrow(
      SchemaError,
    );
    expect(() => checkSchema('v1.json', '[]', 'v1.0.0')).toThrow(/has \$id undefined/u);
    expect(() => checkSchema('v1.json', 'nope', 'v1.0.0')).toThrow(/is not JSON/u);
  });
});

describe('readSchemas', () => {
  let repo: string;
  const git = (...args: string[]) =>
    execFileSync('git', args, { cwd: repo, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] });

  beforeAll(() => {
    repo = mkdtempSync(join(tmpdir(), 'rb-site-schemas-'));
    git('init', '-q');
    git('config', 'user.email', 'test@example.com');
    git('config', 'user.name', 'test');
    mkdirSync(join(repo, 'schema'));
    for (const name of ['v1.json', 'config-v1.json']) {
      writeFileSync(join(repo, 'schema', name), JSON.stringify({ $id: SCHEMA_BASE + name, v: 1 }));
    }
    git('add', '.');
    git('commit', '-qm', 'one');
    git('tag', 'v1.0.0');
    writeFileSync(
      join(repo, 'schema', 'v1.json'),
      JSON.stringify({ $id: SCHEMA_BASE + 'v1.json', v: 2 }),
    );
    git('commit', '-qam', 'work in progress');
  });

  afterAll(() => {
    rmSync(repo, { recursive: true, force: true });
  });

  it('reads the tag, not the work in progress', () => {
    const schemas = readSchemas(repo, 'v1.0.0');
    expect([...schemas.keys()]).toEqual(['v1.json', 'config-v1.json']);
    expect(JSON.parse(schemas.get('v1.json') ?? '')).toMatchObject({ v: 1 });
  });

  it('names a ref it cannot read', () => {
    expect(() => readSchemas(repo, 'v9.9.9')).toThrow(/cannot be read at v9.9.9/u);
  });

  it('lists the release tags', () => {
    expect(localTags(repo)).toEqual(['v1.0.0']);
  });
});
