// The addon and the binary the tests run: RULEBEARING_ADDON and RULEBEARING_BINARY when set (CI
// builds them first), else debug builds of rb-node and rb-cli, built here with cargo. The addon is
// copied to `rulebearing.node`, the name Node loads it by and the platform packages ship it under.
// Plan: docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md, Step 22.
import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import type { TestProject } from 'vitest/node';

declare module 'vitest' {
  export interface ProvidedContext {
    binary: string;
  }
}

const REPOSITORY = fileURLToPath(new URL('../../..', import.meta.url));

/** The cdylib cargo writes for rb-node on this platform. */
function library(): string {
  switch (process.platform) {
    case 'win32':
      return 'rb_node.dll';
    case 'darwin':
      return 'librb_node.dylib';
    default:
      return 'librb_node.so';
  }
}

function given(name: string): string | undefined {
  const value = process.env[name];
  if (value === undefined || value === '') {
    return undefined;
  }
  if (!existsSync(value)) {
    throw new Error(`${name} is ${value}, which does not exist`);
  }
  return value;
}

export default function setup(project: TestProject): void {
  let binary = given('RULEBEARING_BINARY');
  let addon = given('RULEBEARING_ADDON');
  const target = process.env.CARGO_TARGET_DIR ?? join(REPOSITORY, 'target');
  if (binary === undefined || addon === undefined) {
    execFileSync('cargo', ['build', '--quiet', '-p', 'rb-cli', '-p', 'rb-node'], {
      cwd: REPOSITORY,
      stdio: 'inherit',
    });
  }
  binary ??= join(
    target,
    'debug',
    process.platform === 'win32' ? 'rulebearing.exe' : 'rulebearing',
  );
  if (addon === undefined) {
    addon = join(target, 'debug', 'rulebearing.node');
    copyFileSync(join(target, 'debug', library()), addon);
    // The forked workers inherit the environment the API's loader reads.
    process.env.RULEBEARING_ADDON = addon;
  }
  for (const file of [binary, addon]) {
    if (!existsSync(file)) {
      throw new Error(`the tests need ${file}`);
    }
  }
  project.provide('binary', binary);
}
