// The binary the agreement tests run: RULEBEARING_BINARY when set (CI builds it first), else a
// debug build of rb-cli, built here with cargo when it is missing or stale.
// Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 13.
import { execFileSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import type { TestProject } from 'vitest/node';

declare module 'vitest' {
  export interface ProvidedContext {
    binary: string;
  }
}

const REPOSITORY = fileURLToPath(new URL('../../..', import.meta.url));

export default function setup(project: TestProject): void {
  const override = process.env.RULEBEARING_BINARY;
  if (override !== undefined && override !== '') {
    if (!existsSync(override)) {
      throw new Error(`RULEBEARING_BINARY is ${override}, which does not exist`);
    }
    project.provide('binary', override);
    return;
  }
  execFileSync('cargo', ['build', '--quiet', '-p', 'rb-cli'], {
    cwd: REPOSITORY,
    stdio: 'inherit',
  });
  const target = process.env.CARGO_TARGET_DIR ?? join(REPOSITORY, 'target');
  const binary = join(
    target,
    'debug',
    process.platform === 'win32' ? 'rulebearing.exe' : 'rulebearing',
  );
  if (!existsSync(binary)) {
    throw new Error(`cargo build -p rb-cli did not produce ${binary}`);
  }
  project.provide('binary', binary);
}
