// The docs site's command line: `build`, `check`, `serve` and `licences`. site/build.sh runs it
// after compiling the wasm module and the page scripts; CI runs the same script.
//
//   node site/dist/node/cli.js build --out target/site --wasm target/site-wasm --web site/dist/web [--schema-ref vX.Y.Z]
//   node site/dist/node/cli.js check target/site
//   node site/dist/node/cli.js serve target/site [--port 8080]
//   node site/dist/node/cli.js licences
//
// Architecture: docs/architecture.md#distribution. Decision: docs/adr/0072-the-docs-site-builds-from-docs-and-serves-the-released-schemas.md.
// Plan: docs/plans/pending/0004-wave-4-reach.md, Step 3. Requirements: FR-REACH-01, NFR-DOC-01.

import { readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { buildSite } from './build.js';
import { checkSite } from './linkcheck.js';
import type { BrokenLink } from './linkcheck.js';
import { allowedLicences, checkLicences } from './licences.js';
import { latestReleaseTag, localTags } from './schemas.js';
import { serve, urlOf } from './serve.js';

/** Where a command writes: injectable so the tests read what was printed. */
export interface Io {
  readonly out: (text: string) => void;
  readonly err: (text: string) => void;
}

const USAGE = `usage:
  cli.js build --out <dir> --wasm <dir> --web <dir> [--repo <dir>] [--schema-ref <tag>] [--commit <sha>]
  cli.js check <dir>
  cli.js serve <dir> [--port <n>]
  cli.js licences [--repo <dir>]
`;

/** Splits `--name value` pairs from positional arguments. */
export function parseArgs(args: readonly string[]): {
  positional: string[];
  flags: Map<string, string>;
} {
  const positional: string[] = [];
  const flags = new Map<string, string>();
  for (let i = 0; i < args.length; i += 1) {
    const arg = args[i] ?? '';
    if (arg.startsWith('--')) {
      const value = args[i + 1];
      if (value === undefined || value.startsWith('--')) {
        throw new Error(`${arg} needs a value`);
      }
      flags.set(arg.slice(2), value);
      i += 1;
    } else {
      positional.push(arg);
    }
  }
  return { positional, flags };
}

function required(flags: Map<string, string>, name: string): string {
  const value = flags.get(name);
  if (value === undefined) {
    throw new Error(`--${name} is required`);
  }
  return value;
}

function reportBroken(broken: readonly BrokenLink[], io: Io): void {
  for (const b of broken) {
    io.err(`${b.page}: ${b.reason} -> ${b.target}\n`);
  }
}

/** Runs one command and returns its exit code; `serve` resolves once the server is listening. */
export async function main(args: readonly string[], io: Io): Promise<number> {
  const [command, ...rest] = args;
  try {
    const { positional, flags } = parseArgs(rest);
    const repo = resolve(flags.get('repo') ?? '.');
    switch (command) {
      case 'build': {
        const schemaRef = flags.get('schema-ref') ?? latestReleaseTag(localTags(repo));
        if (schemaRef === undefined) {
          io.err('no release tag vX.Y.Z to read the schemas from; pass --schema-ref\n');
          return 2;
        }
        const commit = flags.get('commit');
        const result = buildSite({
          repo,
          out: resolve(required(flags, 'out')),
          wasmDir: resolve(required(flags, 'wasm')),
          webDir: resolve(required(flags, 'web')),
          schemaRef,
          ...(commit === undefined ? {} : { commit }),
        });
        for (const b of result.brokenSources) {
          io.err(`${b}\n`);
        }
        reportBroken(result.brokenSite, io);
        const broken = result.brokenSources.length + result.brokenSite.length;
        io.out(
          `site: ${String(result.pages.length)} pages, schemas from ${schemaRef}, ${String(broken)} broken link(s)\n`,
        );
        return broken === 0 ? 0 : 1;
      }
      case 'check': {
        const dir = positional[0];
        if (dir === undefined) {
          throw new Error('check needs the built site folder');
        }
        const broken = checkSite(resolve(dir));
        reportBroken(broken, io);
        io.out(`site: ${String(broken.length)} broken link(s)\n`);
        return broken.length === 0 ? 0 : 1;
      }
      case 'serve': {
        const dir = positional[0];
        if (dir === undefined) {
          throw new Error('serve needs the built site folder');
        }
        const server = await serve(resolve(dir), Number(flags.get('port') ?? '8080'));
        io.out(`serving ${dir} at ${urlOf(server)}\n`);
        return 0;
      }
      case 'licences': {
        const allowed = allowedLicences(readFileSync(join(repo, 'deny.toml'), 'utf8'));
        const findings = checkLicences(
          readFileSync(join(repo, 'package-lock.json'), 'utf8'),
          allowed,
        );
        for (const f of findings) {
          io.err(`${f.path}: ${f.licence} is not on deny.toml's allow-list\n`);
        }
        io.out(`licences: ${String(findings.length)} package(s) outside the allow-list\n`);
        return findings.length === 0 ? 0 : 1;
      }
      default:
        io.err(USAGE);
        return 2;
    }
  } catch (e) {
    io.err(`${e instanceof Error ? e.message : String(e)}\n`);
    return 2;
  }
}
