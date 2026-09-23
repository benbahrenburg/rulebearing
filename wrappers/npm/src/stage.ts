// Stages the npm packages of one release from the archives release.yml builds.
//
// Architecture: docs/architecture.md#distribution. Decision: docs/adr/0020-single-name-across-registries.md.
// Plan: docs/plans/pending/0001-wave-1-typescript-parity.md, Step 19. Requirement: FR-DIST-01.
// Procedure: docs/release.md.
//
// Given a directory of `rulebearing-<target>.tar.gz` archives and a version, writes one folder per
// platform package (package.json, README.md, LICENSE and bin/<binary>, executable on Unix) and the
// main package with the version stamped into it and into its optionalDependencies. The source
// tree is never modified; `npm pack` or `npm publish` runs on the staged folders.
//
// Usage: node scripts/stage.mjs <dist-dir> <version> <out-dir> [--partial]
//   --partial  skip archives that are absent instead of failing (local checks on one host).

import { execFileSync } from 'node:child_process';
import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { PLATFORM_PACKAGES } from './platforms.js';
import type { PlatformPackage } from './platforms.js';

/** The main package's name on npm (ADR-0020). */
export const MAIN_PACKAGE = 'rulebearing';

const REPOSITORY = {
  type: 'git',
  url: 'git+https://github.com/benbahrenburg/rulebearing.git',
  directory: 'wrappers/npm',
};

/** What to stage. */
export interface StageOptions {
  /** Directory holding the `rulebearing-<target>.tar.gz` archives. */
  readonly distDir: string;
  /** The release version; a leading `v`, as in a tag name, is removed. */
  readonly version: string;
  /** Directory the package folders are written to; created if absent. */
  readonly outDir: string;
  /** Skip absent archives instead of failing. */
  readonly partial?: boolean;
  /** The main package's source directory (wrappers/npm). */
  readonly packageDir?: string;
}

/** One staged package folder. */
export interface StagedPackage {
  readonly name: string;
  readonly dir: string;
}

/** A staging failure, reported by the command line as one line and exit 1. */
export class StageError extends Error {
  override readonly name = 'StageError';
}

const SEMVER = /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/;

/** Validates a version, removing a leading `v`. */
export function normaliseVersion(version: string): string {
  const bare = version.startsWith('v') ? version.slice(1) : version;
  if (!SEMVER.test(bare)) {
    throw new StageError(`not a semantic version: ${version}`);
  }
  return bare;
}

/** The archive release.yml produces for a target. */
export function archiveName(target: string): string {
  return `rulebearing-${target}.tar.gz`;
}

function writeJson(path: string, value: unknown): void {
  writeFileSync(path, `${JSON.stringify(value, null, 2)}\n`);
}

function readJson(path: string): Record<string, unknown> {
  const value: unknown = JSON.parse(readFileSync(path, 'utf8'));
  if (typeof value !== 'object' || value === null || Array.isArray(value)) {
    throw new StageError(`${path} is not a JSON object`);
  }
  return value as Record<string, unknown>;
}

/** The package.json of a platform package. */
export function platformManifest(pkg: PlatformPackage, version: string): Record<string, unknown> {
  return {
    name: pkg.name,
    version,
    description: `The rulebearing binary for ${pkg.label}. Installed by the rulebearing package; depend on that instead.`,
    license: 'MIT',
    repository: REPOSITORY,
    homepage: 'https://github.com/benbahrenburg/rulebearing',
    os: [pkg.os],
    cpu: [pkg.cpu],
    ...(pkg.libc === undefined ? {} : { libc: [pkg.libc] }),
    files: ['bin/'],
    preferUnplugged: true,
  };
}

function platformReadme(pkg: PlatformPackage): string {
  return [
    `# ${pkg.name}`,
    '',
    `The \`rulebearing\` binary for ${pkg.label} (\`${pkg.target}\`). Install \`rulebearing\`, not this package: it lists every platform package under \`optionalDependencies\` and npm installs only the one that matches the machine.`,
    '',
    'Source, documentation and licence: https://github.com/benbahrenburg/rulebearing',
    '',
  ].join('\n');
}

function stagePlatform(
  pkg: PlatformPackage,
  archive: string,
  version: string,
  outDir: string,
  license: string,
): StagedPackage {
  const scratch = mkdtempSync(join(tmpdir(), 'rulebearing-stage-'));
  try {
    execFileSync('tar', ['-xzf', archive, '-C', scratch], { stdio: 'pipe' });
    const binary = join(scratch, pkg.binary);
    if (!existsSync(binary)) {
      throw new StageError(`${archive} does not contain ${pkg.binary}`);
    }
    const dir = join(outDir, pkg.name);
    rmSync(dir, { recursive: true, force: true });
    mkdirSync(join(dir, 'bin'), { recursive: true });
    const target = join(dir, 'bin', pkg.binary);
    copyFileSync(binary, target);
    if (!pkg.binary.endsWith('.exe')) {
      chmodSync(target, 0o755);
    }
    const archivedLicense = join(scratch, 'LICENSE');
    copyFileSync(existsSync(archivedLicense) ? archivedLicense : license, join(dir, 'LICENSE'));
    writeFileSync(join(dir, 'README.md'), platformReadme(pkg));
    writeJson(join(dir, 'package.json'), platformManifest(pkg, version));
    return { name: pkg.name, dir };
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
}

/** The main package's manifest for a release: version stamped, build-only fields removed. */
export function mainManifest(
  source: Record<string, unknown>,
  version: string,
): Record<string, unknown> {
  const rest = Object.fromEntries(
    Object.entries(source).filter(([key]) => key !== 'scripts' && key !== 'devDependencies'),
  );
  const optionalDependencies = Object.fromEntries(PLATFORM_PACKAGES.map((p) => [p.name, version]));
  return { ...rest, version, optionalDependencies };
}

function stageMain(
  packageDir: string,
  version: string,
  outDir: string,
  license: string,
): StagedPackage {
  const source = readJson(join(packageDir, 'package.json'));
  const files = source.files;
  if (!Array.isArray(files) || !files.every((f) => typeof f === 'string')) {
    throw new StageError(`${packageDir}/package.json has no "files" list`);
  }
  const dir = join(outDir, MAIN_PACKAGE);
  rmSync(dir, { recursive: true, force: true });
  for (const file of [...files, 'README.md']) {
    const from = join(packageDir, file);
    if (!existsSync(from)) {
      throw new StageError(`${from} is missing; run \`npm run build\` in ${packageDir} first`);
    }
    mkdirSync(dirname(join(dir, file)), { recursive: true });
    copyFileSync(from, join(dir, file));
  }
  copyFileSync(license, join(dir, 'LICENSE'));
  writeJson(join(dir, 'package.json'), mainManifest(source, version));
  return { name: MAIN_PACKAGE, dir };
}

/** Stages every platform package with an archive, then the main package. */
export function stage(options: StageOptions): StagedPackage[] {
  const version = normaliseVersion(options.version);
  const packageDir = options.packageDir ?? fileURLToPath(new URL('..', import.meta.url));
  const license = join(packageDir, '..', '..', 'LICENSE');
  const outDir = resolve(options.outDir);
  mkdirSync(outDir, { recursive: true });

  const staged: StagedPackage[] = [];
  for (const pkg of PLATFORM_PACKAGES) {
    const archive = join(options.distDir, archiveName(pkg.target));
    if (!existsSync(archive)) {
      if (options.partial === true) {
        console.warn(`stage: skipping ${pkg.name}, no ${archive}`);
        continue;
      }
      throw new StageError(`missing ${archive}`);
    }
    staged.push(stagePlatform(pkg, archive, version, outDir, license));
  }
  if (staged.length === 0) {
    throw new StageError(`no rulebearing-<target>.tar.gz archives in ${options.distDir}`);
  }
  staged.push(stageMain(packageDir, version, outDir, license));
  return staged;
}

const USAGE = 'usage: node scripts/stage.mjs <dist-dir> <version> <out-dir> [--partial]';

/** The command line: returns the process exit code. `packageDir` defaults to this package. */
export function stageCli(argv: readonly string[], packageDir?: string): number {
  const partial = argv.includes('--partial');
  const positional = argv.filter((a) => a !== '--partial');
  const [distDir, version, outDir] = positional;
  if (
    positional.length !== 3 ||
    distDir === undefined ||
    version === undefined ||
    outDir === undefined
  ) {
    console.error(USAGE);
    return 2;
  }
  try {
    const options: StageOptions = {
      distDir,
      version,
      outDir,
      partial,
      ...(packageDir === undefined ? {} : { packageDir }),
    };
    for (const pkg of stage(options)) {
      console.warn(`stage: ${pkg.name} -> ${pkg.dir}`);
    }
    return 0;
  } catch (error) {
    console.error(`stage: ${error instanceof Error ? error.message : String(error)}`);
    return 1;
  }
}
