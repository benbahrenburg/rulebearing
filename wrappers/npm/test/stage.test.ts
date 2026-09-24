// Staging the release packages (plan 0001, Step 19): archives in, seven package folders out, the
// version stamped everywhere, and the published manifest kept in step with the release matrix.
import { execFileSync } from 'node:child_process';
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { PLATFORM_PACKAGES } from '../src/platforms.js';
import {
  MAIN_PACKAGE,
  StageError,
  archiveName,
  mainManifest,
  normaliseVersion,
  platformManifest,
  stage,
  stageCli,
} from '../src/stage.js';

const packageDir = join(import.meta.dirname, '..');
const unix = process.platform !== 'win32';

let scratch: string;
let dist: string;
let out: string;
let source: string;

function readJson(path: string): Record<string, unknown> {
  return JSON.parse(readFileSync(path, 'utf8')) as Record<string, unknown>;
}

/** Writes `rulebearing-<target>.tar.gz` holding the binary, LICENSE and README.md, as release.yml does. */
function archive(target: string, binary: string, withBinary = true): void {
  const content = join(scratch, `content-${target}`);
  mkdirSync(content, { recursive: true });
  if (withBinary) {
    writeFileSync(join(content, binary), `binary for ${target}\n`);
  }
  writeFileSync(join(content, 'LICENSE'), 'MIT from the archive\n');
  writeFileSync(join(content, 'README.md'), 'readme\n');
  execFileSync('tar', ['-czf', join(dist, archiveName(target)), '-C', content, '.']);
}

/** A copy of the wrapper's manifest with the files it lists, so staging never needs a build. */
function fakeSource(): void {
  mkdirSync(join(source, 'bin'), { recursive: true });
  mkdirSync(join(source, 'dist'), { recursive: true });
  writeFileSync(
    join(source, 'package.json'),
    readFileSync(join(packageDir, 'package.json'), 'utf8'),
  );
  writeFileSync(join(source, 'README.md'), '# rulebearing\n');
  writeFileSync(join(source, 'bin', 'rulebearing.js'), '// launcher\n');
  writeFileSync(join(source, 'dist', 'launcher.js'), '// compiled\n');
  writeFileSync(join(source, 'dist', 'platforms.js'), '// compiled\n');
  // The declarations the package exports to eslint-plugin-rulebearing (plan 0002, Step 13).
  writeFileSync(join(source, 'dist', 'launcher.d.ts'), '// declared\n');
  writeFileSync(join(source, 'dist', 'platforms.d.ts'), '// declared\n');
  // The licence is read from the repository root, two levels above the package.
  writeFileSync(join(scratch, 'LICENSE'), 'MIT from the repository\n');
}

beforeEach(() => {
  scratch = mkdtempSync(join(tmpdir(), 'rulebearing-stage-test-'));
  dist = join(scratch, 'dist');
  out = join(scratch, 'out');
  source = join(scratch, 'wrappers', 'npm');
  mkdirSync(dist, { recursive: true });
  mkdirSync(source, { recursive: true });
  fakeSource();
  vi.spyOn(console, 'warn').mockImplementation(() => undefined);
});

afterEach(() => {
  rmSync(scratch, { recursive: true, force: true });
  vi.restoreAllMocks();
});

describe('normaliseVersion', () => {
  it.each([
    ['0.1.0', '0.1.0'],
    ['v0.1.0', '0.1.0'],
    ['1.2.3-rc.1', '1.2.3-rc.1'],
    ['v1.2.3-rc.1+build.5', '1.2.3-rc.1+build.5'],
  ])('accepts %s as %s', (input, expected) => {
    expect(normaliseVersion(input)).toBe(expected);
  });

  it.each(['', 'v', '1.2', 'latest', '1.2.3.4', 'v1.2.3 ', '01.2.x'])('rejects %j', (input) => {
    expect(() => normaliseVersion(input)).toThrow(StageError);
  });
});

describe('platformManifest', () => {
  it.each(PLATFORM_PACKAGES.map((p) => [p.name, p] as const))(
    '%s carries npm install filters for its platform only',
    (_name, pkg) => {
      const manifest = platformManifest(pkg, '9.8.7');
      expect(manifest.name).toBe(pkg.name);
      expect(manifest.version).toBe('9.8.7');
      expect(manifest.os).toEqual([pkg.os]);
      expect(manifest.cpu).toEqual([pkg.cpu]);
      expect(manifest.libc).toEqual(pkg.libc === undefined ? undefined : [pkg.libc]);
      expect(manifest.files).toEqual(['bin/']);
      expect(manifest).not.toHaveProperty('bin');
      expect(manifest).not.toHaveProperty('scripts');
    },
  );
});

describe('mainManifest', () => {
  it('stamps the version everywhere and drops build-only fields', () => {
    const manifest = mainManifest(readJson(join(packageDir, 'package.json')), '2.0.0');
    expect(manifest.version).toBe('2.0.0');
    expect(manifest.optionalDependencies).toEqual(
      Object.fromEntries(PLATFORM_PACKAGES.map((p) => [p.name, '2.0.0'])),
    );
    expect(manifest).not.toHaveProperty('scripts');
    expect(manifest).not.toHaveProperty('devDependencies');
    expect(manifest.bin).toEqual({ rulebearing: 'bin/rulebearing.js' });
  });
});

describe('the committed package.json', () => {
  const manifest = readJson(join(packageDir, 'package.json'));

  it('lists the six platform packages at its own version and no other dependency', () => {
    expect(manifest.optionalDependencies).toEqual(
      Object.fromEntries(PLATFORM_PACKAGES.map((p) => [p.name, manifest.version])),
    );
    expect(manifest).not.toHaveProperty('dependencies');
    expect(manifest.scripts).not.toHaveProperty('postinstall');
    expect(manifest.scripts).not.toHaveProperty('install');
    expect(manifest.scripts).not.toHaveProperty('preinstall');
  });

  it('carries the workspace version from Cargo.toml', () => {
    const cargo = readFileSync(join(packageDir, '..', '..', 'Cargo.toml'), 'utf8');
    const version = /\[workspace\.package\][^[]*?\nversion = "([^"]+)"/.exec(cargo)?.[1];
    expect(manifest.version).toBe(version);
  });

  it('matches the targets release.yml builds', () => {
    const workflow = readFileSync(
      join(packageDir, '..', '..', '.github', 'workflows', 'release.yml'),
      'utf8',
    );
    const targets = [...workflow.matchAll(/target: ([a-z0-9_-]+) \}/g)].map((m) => m[1]);
    expect(new Set(targets)).toEqual(new Set(PLATFORM_PACKAGES.map((p) => p.target)));
  });
});

describe('stage', () => {
  it('writes every platform package and the main package from a full set of archives', () => {
    for (const pkg of PLATFORM_PACKAGES) {
      archive(pkg.target, pkg.binary);
    }
    const staged = stage({ distDir: dist, version: 'v0.2.0', outDir: out, packageDir: source });
    expect(staged.map((s) => s.name)).toEqual([
      ...PLATFORM_PACKAGES.map((p) => p.name),
      MAIN_PACKAGE,
    ]);

    for (const pkg of PLATFORM_PACKAGES) {
      const dir = join(out, pkg.name);
      const binary = join(dir, 'bin', pkg.binary);
      expect(readFileSync(binary, 'utf8')).toBe(`binary for ${pkg.target}\n`);
      if (unix && !pkg.binary.endsWith('.exe')) {
        expect(statSync(binary).mode & 0o777).toBe(0o755);
      }
      expect(readJson(join(dir, 'package.json'))).toEqual(platformManifest(pkg, '0.2.0'));
      expect(readFileSync(join(dir, 'LICENSE'), 'utf8')).toBe('MIT from the archive\n');
      expect(readFileSync(join(dir, 'README.md'), 'utf8')).toContain(pkg.target);
    }

    const main = join(out, MAIN_PACKAGE);
    const manifest = readJson(join(main, 'package.json'));
    expect(manifest.version).toBe('0.2.0');
    expect(manifest).not.toHaveProperty('scripts');
    for (const file of [
      'bin/rulebearing.js',
      'dist/launcher.js',
      'dist/launcher.d.ts',
      'dist/platforms.js',
      'dist/platforms.d.ts',
      'README.md',
    ]) {
      expect(existsSync(join(main, file))).toBe(true);
    }
    expect(readFileSync(join(main, 'LICENSE'), 'utf8')).toBe('MIT from the repository\n');
    // The source tree is not modified.
    expect(readJson(join(source, 'package.json')).version).toBe(
      readJson(join(packageDir, 'package.json')).version,
    );
  });

  it('is deterministic: staging twice writes the same manifests', () => {
    archive('aarch64-apple-darwin', 'rulebearing');
    const once = () => {
      stage({ distDir: dist, version: '1.0.0', outDir: out, partial: true, packageDir: source });
      return [
        readFileSync(join(out, 'rulebearing-cli-darwin-arm64', 'package.json'), 'utf8'),
        readFileSync(join(out, MAIN_PACKAGE, 'package.json'), 'utf8'),
      ];
    };
    expect(once()).toEqual(once());
  });

  it('fails on a missing archive unless partial', () => {
    archive('x86_64-unknown-linux-gnu', 'rulebearing');
    expect(() =>
      stage({ distDir: dist, version: '1.0.0', outDir: out, packageDir: source }),
    ).toThrow(/missing .*rulebearing-x86_64-unknown-linux-musl\.tar\.gz/);

    const staged = stage({
      distDir: dist,
      version: '1.0.0',
      outDir: out,
      partial: true,
      packageDir: source,
    });
    expect(staged.map((s) => s.name)).toEqual(['rulebearing-cli-linux-x64-gnu', MAIN_PACKAGE]);
    // The main package still lists all six, so every platform resolves once published.
    expect(
      Object.keys(readJson(join(out, MAIN_PACKAGE, 'package.json')).optionalDependencies as object),
    ).toHaveLength(6);
  });

  it('fails when there is no archive at all, even partial', () => {
    expect(() =>
      stage({ distDir: dist, version: '1.0.0', outDir: out, partial: true, packageDir: source }),
    ).toThrow(StageError);
  });

  it('fails on an archive without the binary', () => {
    archive('x86_64-pc-windows-msvc', 'rulebearing.exe', false);
    expect(() =>
      stage({ distDir: dist, version: '1.0.0', outDir: out, partial: true, packageDir: source }),
    ).toThrow(/does not contain rulebearing\.exe/);
  });

  it('fails when the launcher has not been built', () => {
    archive('aarch64-apple-darwin', 'rulebearing');
    rmSync(join(source, 'dist'), { recursive: true });
    expect(() =>
      stage({ distDir: dist, version: '1.0.0', outDir: out, partial: true, packageDir: source }),
    ).toThrow(/npm run build/);
  });

  it('fails when the manifest has no files list', () => {
    archive('aarch64-apple-darwin', 'rulebearing');
    writeFileSync(join(source, 'package.json'), '{"name":"rulebearing"}');
    expect(() =>
      stage({ distDir: dist, version: '1.0.0', outDir: out, partial: true, packageDir: source }),
    ).toThrow(/"files"/);
    writeFileSync(join(source, 'package.json'), '[]');
    expect(() =>
      stage({ distDir: dist, version: '1.0.0', outDir: out, partial: true, packageDir: source }),
    ).toThrow(/not a JSON object/);
  });

  it('defaults to this package as the source of the main package', () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    // No archives: the default path is resolved before any archive is read.
    expect(stageCli([dist, '1.0.0', out, '--partial'])).toBe(1);
    expect(error.mock.calls[0]?.[0]).toContain('no rulebearing-<target>.tar.gz archives');
  });
});

describe('stageCli', () => {
  it('exits 2 with the usage line on bad arguments', () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    expect(stageCli([])).toBe(2);
    expect(stageCli([dist, '1.0.0'])).toBe(2);
    expect(stageCli([dist, '1.0.0', out, 'extra'])).toBe(2);
    expect(error).toHaveBeenCalledTimes(3);
    expect(error.mock.calls[0]?.[0]).toMatch(/^usage: /);
  });

  it('exits 1 with the reason when staging fails', () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    expect(stageCli([dist, 'not-a-version', out])).toBe(1);
    expect(error.mock.calls[0]?.[0]).toBe('stage: not a semantic version: not-a-version');
  });

  it('exits 0 and names each staged folder', () => {
    const pkg = PLATFORM_PACKAGES[0]!;
    archive(pkg.target, pkg.binary);
    const warn = vi.mocked(console.warn);
    expect(stageCli([dist, '1.0.0', out, '--partial'], source)).toBe(0);
    expect(warn.mock.calls.map((c) => String(c[0]))).toEqual([
      ...PLATFORM_PACKAGES.slice(1).map(
        (p) => expect.stringContaining(`skipping ${p.name}`) as string,
      ),
      `stage: ${pkg.name} -> ${join(out, pkg.name)}`,
      `stage: ${MAIN_PACKAGE} -> ${join(out, MAIN_PACKAGE)}`,
    ]);
  });
});
