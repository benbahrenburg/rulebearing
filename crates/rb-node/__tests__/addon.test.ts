// Where the API finds the addon: the override, else `rulebearing.node` in the platform package
// beside its binary, else one line saying why there is none.
// Plan: docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md, Step 22.
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterAll, describe, expect, it } from 'vitest';
import {
  ADDON_FILE,
  ADDON_OVERRIDE,
  loadAddon,
  resolveAddon,
} from '../../../wrappers/npm/src/api/addon.js';

const MAC = { platform: 'darwin', arch: 'arm64', libc: undefined } as const;
const folder = mkdtempSync(join(tmpdir(), 'rb-node-addon-'));

afterAll(() => {
  rmSync(folder, { recursive: true, force: true });
});

describe('resolveAddon', () => {
  it('uses the override when it exists, and says so when it does not', () => {
    const file = join(folder, 'mine.node');
    writeFileSync(file, '');
    expect(resolveAddon(MAC, { [ADDON_OVERRIDE]: file })).toEqual({ kind: 'binary', path: file });
    const missing = resolveAddon(MAC, { [ADDON_OVERRIDE]: join(folder, 'none.node') });
    expect(missing.kind).toBe('missing');
    expect(missing.kind === 'missing' && missing.message).toMatch(/RULEBEARING_ADDON is set/);
  });

  it('finds rulebearing.node in the platform package', () => {
    const pkg = join(folder, 'rulebearing-cli-darwin-arm64');
    mkdirSync(pkg, { recursive: true });
    writeFileSync(join(pkg, 'package.json'), '{}');
    const resolve = (): string => join(pkg, 'package.json');
    const absent = resolveAddon(MAC, {}, resolve, () => '1.2.3');
    expect(absent.kind === 'missing' && absent.message).toMatch(/is installed but .* is missing/);
    writeFileSync(join(pkg, ADDON_FILE), '');
    expect(resolveAddon(MAC, {}, resolve)).toEqual({ kind: 'binary', path: join(pkg, ADDON_FILE) });
  });

  it('names the package to install, and the hosts there is no addon for', () => {
    const notInstalled = resolveAddon(
      MAC,
      {},
      () => {
        throw new Error('not found');
      },
      () => '1.2.3',
    );
    expect(notInstalled.kind === 'missing' && notInstalled.message).toMatch(
      /npm install rulebearing-cli-darwin-arm64@1\.2\.3/,
    );
    const unknown = resolveAddon({ platform: 'aix', arch: 'ppc64', libc: undefined }, {});
    expect(unknown.kind === 'missing' && unknown.message).toMatch(
      /no prebuilt addon for aix ppc64/,
    );
  });
});

describe('loadAddon', () => {
  it('throws the reason when there is no addon, and loads the one found', () => {
    expect(() => loadAddon({ kind: 'missing', message: 'no addon here' })).toThrow('no addon here');
    const loaded = loadAddon({ kind: 'binary', path: '/x/rulebearing.node' }, (path) => ({ path }));
    expect(loaded).toEqual({ path: '/x/rulebearing.node' });
  });
});
