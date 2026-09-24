// Shared by the tests of rulebearing/vitest: the locally built binary and the shared fixture.
// Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 14 (2H).
// The fixture repository is adapters/fixture (its README says what each rule proves).

import { spawnSync } from 'node:child_process';
import { cpSync, existsSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

/** The repository root. */
export const REPOSITORY = join(import.meta.dirname, '..', '..', '..');

/** adapters/fixture. */
export const FIXTURE = join(REPOSITORY, 'adapters', 'fixture');

/** The binary built from this checkout: RULEBEARING_BINARY, else target/release, else target/debug. */
export function localBinary(): string {
  const name = process.platform === 'win32' ? 'rulebearing.exe' : 'rulebearing';
  const override = process.env.RULEBEARING_BINARY;
  const candidates = [
    ...(override === undefined || override === '' ? [] : [override]),
    join(REPOSITORY, 'target', 'release', name),
    join(REPOSITORY, 'target', 'debug', name),
  ];
  const found = candidates.find((candidate) => existsSync(candidate));
  if (found === undefined) {
    throw new Error(
      'no rulebearing binary: run `cargo build --release -p rb-cli`, or set RULEBEARING_BINARY',
    );
  }
  return found;
}

/** A copy of adapters/fixture in a new temporary directory; `remove` deletes it. */
export function copyFixture(): { dir: string; remove: () => void } {
  const dir = mkdtempSync(join(tmpdir(), 'rulebearing-vitest-'));
  cpSync(FIXTURE, dir, {
    recursive: true,
    filter: (source) => !source.endsWith('README.md') && !source.includes('.graph'),
  });
  return {
    dir,
    remove: () => {
      rmSync(dir, { recursive: true, force: true });
    },
  };
}

/**
 * The junit reporter's message per test case, for the fixture copy in `dir`: the `message` of the
 * `<failure>` then of each `<error>`, joined by newlines, with XML's five entities and character
 * references decoded.
 */
export function junitMessages(binary: string, dir: string): Map<string, string> {
  const run = spawnSync(binary, ['cruise', '-T', 'junit', '--no-progress'], {
    cwd: dir,
    encoding: 'utf8',
  });
  const messages = new Map<string, string>();
  const decode = (text: string): string =>
    text
      .replace(/&#(\d+);/g, (_, code: string) => String.fromCodePoint(Number(code)))
      .replace(/&lt;/g, '<')
      .replace(/&gt;/g, '>')
      .replace(/&quot;/g, '"')
      .replace(/&apos;/g, "'")
      .replace(/&amp;/g, '&');
  for (const match of run.stdout.matchAll(
    /<testcase name="([^"]*)"[^>]*?(?:\/>|>([\s\S]*?)<\/testcase>)/g,
  )) {
    const body = match[2] ?? '';
    const parts = [...body.matchAll(/<(failure|error) [^>]*?message="([^"]*)"/g)].map((m) =>
      decode(m[2] ?? ''),
    );
    messages.set(decode(match[1] ?? ''), parts.join('\n'));
  }
  return messages;
}
