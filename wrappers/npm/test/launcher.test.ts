// The launcher (plan 0001, Step 19): libc detection, binary resolution and its one-line errors,
// and the pass-through of arguments, exit codes and signals to and from the binary.
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from 'vitest';
import {
  BINARY_OVERRIDE,
  EXIT_UNTRUSTWORTHY,
  describeHost,
  detectHost,
  detectLibc,
  launch,
  ownVersion,
  resolveBinary,
  resolvePackageJson,
  runBinary,
} from '../src/launcher.js';
import type { Host, LaunchEffects } from '../src/launcher.js';

const unix = process.platform !== 'win32';
let scratch: string;
let fake: string;

// A stand-in binary: records its arguments in $FAKE_OUT, then exits with $FAKE_CODE or, when
// $FAKE_SIGNAL is set, kills itself with that signal.
const FAKE_BINARY = `#!/usr/bin/env node
const { writeFileSync } = require('node:fs');
if (process.env.FAKE_OUT) writeFileSync(process.env.FAKE_OUT, JSON.stringify(process.argv.slice(2)));
if (process.env.FAKE_SIGNAL) process.kill(process.pid, process.env.FAKE_SIGNAL);
else process.exit(Number(process.env.FAKE_CODE ?? '0'));
`;

beforeAll(() => {
  scratch = mkdtempSync(join(tmpdir(), 'rulebearing-launcher-'));
  fake = join(scratch, 'fake-rulebearing.cjs');
  writeFileSync(fake, FAKE_BINARY);
  chmodSync(fake, 0o755);
});

afterAll(() => {
  rmSync(scratch, { recursive: true, force: true });
});

afterEach(() => {
  vi.unstubAllEnvs();
  vi.restoreAllMocks();
});

const darwin: Host = { platform: 'darwin', arch: 'arm64', libc: undefined };

function effects(): LaunchEffects & { codes: number[]; signals: string[] } {
  const codes: number[] = [];
  const signals: string[] = [];
  return {
    codes,
    signals,
    setExitCode: (code) => codes.push(code),
    kill: (signal) => signals.push(signal),
  };
}

describe('detectLibc', () => {
  it('is glibc when the report records a glibc runtime', () => {
    expect(detectLibc(() => ({ header: { glibcVersionRuntime: '2.39' } }))).toBe('glibc');
  });

  it.each([
    ['no glibc field', { header: { osName: 'Linux' } }],
    ['a non-string glibc field', { header: { glibcVersionRuntime: 2 } }],
    ['no header', {}],
    ['a null header', { header: null }],
    ['no report', undefined],
    ['a null report', null],
  ])('is musl with %s', (_label, report) => {
    expect(detectLibc(() => report)).toBe('musl');
  });

  it('reads the running process report by default', () => {
    expect(['glibc', 'musl']).toContain(detectLibc());
  });
});

describe('detectHost', () => {
  it('asks for the C library on Linux only', () => {
    const libc = vi.fn(() => 'musl' as const);
    expect(detectHost('linux', 'x64', libc)).toEqual({
      platform: 'linux',
      arch: 'x64',
      libc: 'musl',
    });
    expect(detectHost('darwin', 'arm64', libc)).toEqual(darwin);
    expect(detectHost('win32', 'x64', libc).libc).toBeUndefined();
    expect(libc).toHaveBeenCalledTimes(1);
  });

  it('defaults to the running process', () => {
    const host = detectHost();
    expect(host.platform).toBe(process.platform);
    expect(host.arch).toBe(process.arch);
  });
});

describe('describeHost', () => {
  it('names the platform, the architecture and, on Linux, the C library', () => {
    expect(describeHost(darwin)).toBe('darwin arm64');
    expect(describeHost({ platform: 'linux', arch: 'x64', libc: 'musl' })).toBe('linux x64 musl');
  });
});

describe('ownVersion', () => {
  it('is the version in package.json', () => {
    const manifest = JSON.parse(
      readFileSync(new URL('../package.json', import.meta.url), 'utf8'),
    ) as { version: string };
    expect(ownVersion()).toBe(manifest.version);
  });
});

describe('resolvePackageJson', () => {
  it('resolves an installed package and throws for a missing one', () => {
    expect(resolvePackageJson('vitest')).toMatch(/vitest[/\\]package\.json$/);
    expect(() => resolvePackageJson('rulebearing-cli-no-such-platform')).toThrow();
  });
});

describe('resolveBinary', () => {
  const neverCalled = (): string => {
    throw new Error('the resolver should not be consulted');
  };

  it('uses RULEBEARING_BINARY when it names a file', () => {
    expect(resolveBinary(darwin, { [BINARY_OVERRIDE]: fake }, neverCalled)).toEqual({
      kind: 'binary',
      path: fake,
    });
  });

  it('reports a RULEBEARING_BINARY that does not exist', () => {
    const missing = join(scratch, 'absent');
    const result = resolveBinary(darwin, { [BINARY_OVERRIDE]: missing }, neverCalled);
    expect(result).toEqual({
      kind: 'missing',
      message: expect.stringContaining(`${BINARY_OVERRIDE} is set to ${missing}`) as string,
    });
  });

  it('ignores an empty RULEBEARING_BINARY', () => {
    const result = resolveBinary(darwin, { [BINARY_OVERRIDE]: '' }, () => {
      throw new Error('not installed');
    });
    expect(result.kind).toBe('missing');
  });

  it('names the host and the supported platforms when no package covers it', () => {
    const result = resolveBinary(
      { platform: 'freebsd', arch: 'x64', libc: undefined },
      {},
      neverCalled,
    );
    expect(result.kind).toBe('missing');
    if (result.kind === 'missing') {
      expect(result.message).toContain('no prebuilt binary for freebsd x64');
      expect(result.message).toContain('macOS arm64');
      expect(result.message).toContain(BINARY_OVERRIDE);
      expect(result.message).not.toContain('\n');
    }
  });

  it('names the package to install and the detected platform when it is missing', () => {
    const result = resolveBinary(
      { platform: 'linux', arch: 'x64', libc: 'musl' },
      {},
      () => {
        throw new Error('Cannot find module');
      },
      () => '1.2.3',
    );
    expect(result).toEqual({
      kind: 'missing',
      message:
        'rulebearing: the platform package rulebearing-cli-linux-x64-musl is not installed (detected linux x64 musl). Install it with `npm install rulebearing-cli-linux-x64-musl@1.2.3`, or reinstall rulebearing without --omit=optional or --no-optional.',
    });
  });

  it('finds bin/<binary> inside the installed platform package', () => {
    const pkg = join(scratch, 'node_modules', 'rulebearing-cli-win32-x64-msvc');
    mkdirSync(join(pkg, 'bin'), { recursive: true });
    writeFileSync(join(pkg, 'package.json'), '{}');
    const host: Host = { platform: 'win32', arch: 'x64', libc: undefined };
    const resolve = (name: string): string => join(scratch, 'node_modules', name, 'package.json');

    const missing = resolveBinary(host, {}, resolve);
    expect(missing.kind).toBe('missing');
    if (missing.kind === 'missing') {
      expect(missing.message).toContain('is installed but');
    }

    writeFileSync(join(pkg, 'bin', 'rulebearing.exe'), '');
    expect(resolveBinary(host, {}, resolve)).toEqual({
      kind: 'binary',
      path: join(pkg, 'bin', 'rulebearing.exe'),
    });
  });
});

describe.skipIf(!unix)('runBinary', () => {
  it('passes every argument through unchanged and returns the exit code', async () => {
    const out = join(scratch, 'args.json');
    vi.stubEnv('FAKE_OUT', out);
    vi.stubEnv('FAKE_CODE', '7');
    const args = ['cruise', '--output-type', 'err', 'a b', '', '--', '"quoted"'];
    await expect(runBinary(fake, args)).resolves.toEqual({ code: 7 });
    expect(JSON.parse(readFileSync(out, 'utf8'))).toEqual(args);
  });

  it('returns the signal that stopped the binary', async () => {
    vi.stubEnv('FAKE_SIGNAL', 'SIGTERM');
    await expect(runBinary(fake, [])).resolves.toEqual({ signal: 'SIGTERM' });
  });

  it('is exit 2 with one message when the binary cannot start', async () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const notExecutable = join(scratch, 'not-executable');
    writeFileSync(notExecutable, '');
    chmodSync(notExecutable, 0o644);
    await expect(runBinary(notExecutable, [])).resolves.toEqual({
      code: EXIT_UNTRUSTWORTHY,
    });
    expect(error).toHaveBeenCalledOnce();
    expect(error.mock.calls[0]?.[0]).toContain(`could not start ${notExecutable}`);
  });

  it('stops forwarding signals once the binary has exited', async () => {
    const before = process.listenerCount('SIGTERM');
    await runBinary(fake, []);
    expect(process.listenerCount('SIGTERM')).toBe(before);
  });
});

describe('launch', () => {
  it('prints one error and exits 2 when the platform package is missing', async () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const fx = effects();
    await launch(
      ['--version'],
      {},
      darwin,
      () => {
        throw new Error('Cannot find module');
      },
      fx,
    );
    expect(fx.codes).toEqual([EXIT_UNTRUSTWORTHY]);
    expect(error).toHaveBeenCalledOnce();
    expect(error.mock.calls[0]?.[0]).toContain('rulebearing-cli-darwin-arm64');
    expect(error.mock.calls[0]?.[0]).toContain('detected darwin arm64');
  });

  it.skipIf(!unix)('sets the binary exit code as its own', async () => {
    vi.stubEnv('FAKE_CODE', '3');
    const fx = effects();
    await launch(['check'], { [BINARY_OVERRIDE]: fake }, darwin, resolvePackageJson, fx);
    expect(fx.codes).toEqual([3]);
    expect(fx.signals).toEqual([]);
  });

  it.skipIf(!unix)('re-raises the signal that stopped the binary', async () => {
    vi.stubEnv('FAKE_SIGNAL', 'SIGTERM');
    const fx = effects();
    await launch([], { [BINARY_OVERRIDE]: fake }, darwin, resolvePackageJson, fx);
    expect(fx.signals).toEqual(['SIGTERM']);
    expect(fx.codes).toEqual([]);
  });
});
