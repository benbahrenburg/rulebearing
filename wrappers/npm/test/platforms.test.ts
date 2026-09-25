// The platform to package table (plan 0001, Step 19): all six release targets and the hosts no
// target covers.
import { describe, expect, it } from 'vitest';
import { PLATFORM_PACKAGES, platformPackageFor } from '../src/platforms.js';
import type { Libc } from '../src/platforms.js';

describe('platformPackageFor', () => {
  const supported: [string, string, Libc | undefined, string, string][] = [
    ['darwin', 'arm64', undefined, 'rulebearing-cli-darwin-arm64', 'aarch64-apple-darwin'],
    ['darwin', 'x64', undefined, 'rulebearing-cli-darwin-x64', 'x86_64-apple-darwin'],
    ['linux', 'x64', 'glibc', 'rulebearing-cli-linux-x64-gnu', 'x86_64-unknown-linux-gnu'],
    ['linux', 'x64', 'musl', 'rulebearing-cli-linux-x64-musl', 'x86_64-unknown-linux-musl'],
    ['linux', 'arm64', 'glibc', 'rulebearing-cli-linux-arm64-gnu', 'aarch64-unknown-linux-gnu'],
    ['win32', 'x64', undefined, 'rulebearing-cli-win32-x64-msvc', 'x86_64-pc-windows-msvc'],
  ];

  it.each(supported)('%s %s %s is %s', (platform, arch, libc, name, target) => {
    const pkg = platformPackageFor(platform, arch, libc);
    expect(pkg?.name).toBe(name);
    expect(pkg?.target).toBe(target);
  });

  const unsupported: [string, string, Libc | undefined][] = [
    ['linux', 'arm64', 'musl'],
    ['linux', 'x64', undefined],
    ['linux', 'ia32', 'glibc'],
    ['win32', 'arm64', undefined],
    ['win32', 'ia32', undefined],
    ['darwin', 'ia32', undefined],
    ['freebsd', 'x64', undefined],
    ['sunos', 'x64', undefined],
  ];

  it.each(unsupported)('%s %s %s has no package', (platform, arch, libc) => {
    expect(platformPackageFor(platform, arch, libc)).toBeUndefined();
  });

  it('covers exactly the six release targets, each once', () => {
    expect(PLATFORM_PACKAGES).toHaveLength(6);
    expect(new Set(PLATFORM_PACKAGES.map((p) => p.name)).size).toBe(6);
    expect(new Set(PLATFORM_PACKAGES.map((p) => p.target)).size).toBe(6);
    for (const pkg of PLATFORM_PACKAGES) {
      expect(pkg.name).toMatch(/^rulebearing-cli-/);
      expect(pkg.libc !== undefined).toBe(pkg.os === 'linux');
      expect(pkg.binary).toBe(pkg.os === 'win32' ? 'rulebearing.exe' : 'rulebearing');
    }
  });
});
