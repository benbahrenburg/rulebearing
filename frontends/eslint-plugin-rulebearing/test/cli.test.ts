// The binary through the npm wrapper, and the object `can-import --json` prints (plan 0002, Step 13).
import { chmodSync, cpSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { inject } from 'vitest';
import { CliError, canImport, locateBinary, parseAnswer, warmCache } from '../src/cli.js';
import { fixText, describe as line, message } from '../src/message.js';

const binary = inject('binary');
const FIXTURE = fileURLToPath(new URL('fixture', import.meta.url));
let dir: string;

beforeAll(() => {
  dir = mkdtempSync(join(tmpdir(), 'rb-plugin-cli-'));
  cpSync(FIXTURE, dir, { recursive: true });
});

afterAll(() => {
  rmSync(dir, { recursive: true, force: true });
});

describe('locateBinary', () => {
  it('finds the binary RULEBEARING_BINARY names, as the npm launcher does', () => {
    expect(locateBinary({ RULEBEARING_BINARY: binary })).toBe(binary);
  });

  it('says what is missing, in the launcher words', () => {
    expect(() => locateBinary({ RULEBEARING_BINARY: join(dir, 'absent') })).toThrow(CliError);
    expect(() => locateBinary({ RULEBEARING_BINARY: join(dir, 'absent') })).toThrow(
      /RULEBEARING_BINARY is set to/,
    );
  });
});

describe('canImport', () => {
  const invocation = () => ({ binary, cwd: dir, config: 'rulebearing.yaml' });

  it('answers no with the deciding rule, its id and fix, and yes with nothing', () => {
    warmCache(invocation(), 'src/ui/view.ts');
    const no = canImport(invocation(), 'src/ui/view.ts', 'src/db/query.ts');
    expect(no).toEqual({
      verdict: 'no',
      from: 'src/ui/view.ts',
      to: 'src/db/query.ts',
      violations: [
        {
          name: 'ui-not-to-db',
          severity: 'error',
          id: 'RB-ecea7134',
          comment: 'The UI reaches data through services.',
          fix: 'Import from src/services instead of src/db.',
        },
      ],
      warnings: [],
    });
    const yes = canImport(invocation(), 'src/ui/view.ts', 'src/services/api.ts');
    expect(yes.verdict).toBe('yes');
    expect(yes.violations).toEqual([]);
    const warned = canImport(invocation(), 'src/services/api.ts', 'src/ui/format.ts');
    expect(warned.verdict).toBe('yes');
    expect(warned.warnings.map((w) => w.name)).toEqual(['services-not-to-ui']);
  });

  it('turns an exit other than 0 or 1 into the binary own reason', () => {
    expect(() =>
      canImport(invocation(), 'src/ui/view.ts', 'node_modules/nothing/index.js'),
    ).toThrow(/is not in the graph/);
  });

  it('names a binary that cannot start, and one that prints no reason', () => {
    expect(() => canImport({ binary: join(dir, 'absent'), cwd: dir }, 'a', 'b')).toThrow(
      /could not start/,
    );
    if (process.platform !== 'win32') {
      const silent = join(dir, 'silent.sh');
      writeFileSync(silent, '#!/bin/sh\nexit 5\n');
      chmodSync(silent, 0o755);
      expect(() => canImport({ binary: silent, cwd: dir }, 'a', 'b')).toThrow(/^exit 5$/);
      const talker = join(dir, 'talker.sh');
      writeFileSync(talker, '#!/bin/sh\necho hello\nexit 1\n');
      chmodSync(talker, 0o755);
      expect(() => canImport({ binary: talker, cwd: dir, graph: 'g.json' }, 'a', 'b')).toThrow(
        /not JSON: hello/,
      );
    }
  });
});

describe('parseAnswer', () => {
  const ok = { verdict: 'yes', from: 'a', to: 'b', violations: [], warnings: [] };
  it.each([
    [{ ...ok, verdict: 'maybe' }, /no verdict, from or to/],
    [{ ...ok, from: 1 }, /no verdict, from or to/],
    [{ ...ok, to: null }, /no verdict, from or to/],
    [[], /no verdict, from or to/],
    [{ ...ok, violations: {} }, /`violations` is not a list/],
    [{ ...ok, warnings: [{ name: 'w', severity: 'warn' }] }, /entry of `warnings` has no name/],
    [{ ...ok, warnings: ['x'] }, /entry of `warnings` has no name/],
  ])('refuses %j', (value, reason) => {
    expect(() => parseAnswer(JSON.stringify(value))).toThrow(reason);
  });

  it('keeps comment and fix only when they are text', () => {
    const answer = parseAnswer(
      JSON.stringify({
        ...ok,
        verdict: 'no',
        violations: [{ name: 'r', severity: 'error', id: 'RB-1', comment: 3, fix: 'Do it' }],
      }),
    );
    expect(answer.violations).toEqual([{ name: 'r', severity: 'error', id: 'RB-1', fix: 'Do it' }]);
  });
});

describe('message', () => {
  const finding = { name: 'r', severity: 'error', id: 'RB-00000001', fix: 'Move it.' };
  it('is the rule, the fix, then the junit line', () => {
    expect(message(finding, 'a.ts', 'b.ts', { line: 3, column: 7 })).toBe(
      'r: Move it.\nRB-00000001 a.ts -> b.ts (line 3, column 7)',
    );
    expect(line(finding, 'a.ts', 'b.ts', { line: 1, column: 1 })).toBe(
      'RB-00000001 a.ts -> b.ts (line 1, column 1)',
    );
  });

  it('words a rule without a fix as junit does', () => {
    expect(fixText({ name: 'bare', severity: 'error', id: 'RB-2' })).toBe(
      '1 violation(s) of `bare`',
    );
  });
});
