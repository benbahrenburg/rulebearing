// The six platform packages of the npm wrapper, one per release target.
//
// Architecture: docs/architecture.md#distribution. Decision: docs/adr/0020-single-name-across-registries.md
// (the `@rulebearing` scope is not held, so the platform packages are unscoped `rulebearing-cli-*`).
// Plan: docs/plans/pending/0001-wave-1-typescript-parity.md, Step 19. Requirement: FR-DIST-01.
//
// The targets are the matrix of .github/workflows/release.yml; each archive it produces is named
// `rulebearing-<target>.tar.gz`. The launcher and the staging script both read this one table.

/** The C library a Linux binary links against, spelled as npm's `libc` field spells it. */
export type Libc = 'glibc' | 'musl';

/** One platform package: the npm name, the Rust target it carries, and npm's install filters. */
export interface PlatformPackage {
  /** The npm package name. */
  readonly name: string;
  /** The Rust target triple, which is also the release archive's suffix. */
  readonly target: string;
  /** npm `os` and Node's `process.platform`. */
  readonly os: string;
  /** npm `cpu` and Node's `process.arch`. */
  readonly cpu: string;
  /** npm `libc`, Linux only. */
  readonly libc?: Libc;
  /** The executable's file name inside the package's `bin/` directory. */
  readonly binary: string;
  /** A human label for messages and the package description. */
  readonly label: string;
}

/** Every platform package, in the order release.yml lists its targets. */
export const PLATFORM_PACKAGES: readonly PlatformPackage[] = [
  {
    name: 'rulebearing-cli-linux-x64-gnu',
    target: 'x86_64-unknown-linux-gnu',
    os: 'linux',
    cpu: 'x64',
    libc: 'glibc',
    binary: 'rulebearing',
    label: 'Linux x64 (glibc)',
  },
  {
    name: 'rulebearing-cli-linux-x64-musl',
    target: 'x86_64-unknown-linux-musl',
    os: 'linux',
    cpu: 'x64',
    libc: 'musl',
    binary: 'rulebearing',
    label: 'Linux x64 (musl)',
  },
  {
    name: 'rulebearing-cli-linux-arm64-gnu',
    target: 'aarch64-unknown-linux-gnu',
    os: 'linux',
    cpu: 'arm64',
    libc: 'glibc',
    binary: 'rulebearing',
    label: 'Linux arm64 (glibc)',
  },
  {
    name: 'rulebearing-cli-darwin-arm64',
    target: 'aarch64-apple-darwin',
    os: 'darwin',
    cpu: 'arm64',
    binary: 'rulebearing',
    label: 'macOS arm64',
  },
  {
    name: 'rulebearing-cli-darwin-x64',
    target: 'x86_64-apple-darwin',
    os: 'darwin',
    cpu: 'x64',
    binary: 'rulebearing',
    label: 'macOS x64',
  },
  {
    name: 'rulebearing-cli-win32-x64-msvc',
    target: 'x86_64-pc-windows-msvc',
    os: 'win32',
    cpu: 'x64',
    binary: 'rulebearing.exe',
    label: 'Windows x64',
  },
];

/**
 * The platform package for a host, or `undefined` when no release target covers it.
 * `libc` is consulted only for packages that declare one, that is, on Linux.
 */
export function platformPackageFor(
  platform: string,
  arch: string,
  libc: Libc | undefined,
): PlatformPackage | undefined {
  return PLATFORM_PACKAGES.find(
    (p) => p.os === platform && p.cpu === arch && (p.libc === undefined || p.libc === libc),
  );
}
