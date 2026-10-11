// The two JSON schemas the site serves at the `$id` they carry, read from a release tag rather than
// from the checkout: the site deploys from `main`, but the schema URLs hold the released contract.
//
// Architecture: docs/architecture.md#the-graph-document. Decisions: docs/adr/0072-the-docs-site-builds-from-docs-and-serves-the-released-schemas.md,
// docs/adr/0004-graph-document-is-cruise-result-superset.md. Plan: docs/plans/pending/0004-wave-4-reach.md,
// Step 3 ("the schema at `/schema/v1.json`"). Requirements: FR-REACH-01, NFR-DOC-01.

import { execFileSync } from 'node:child_process';

/** The URL the schemas are published under; each file's `$id` must be this plus its name. */
export const SCHEMA_BASE = 'https://benbahrenburg.github.io/rulebearing/schema/';

/** The schema files the site serves, by name under `schema/`. */
export const SCHEMA_FILES: readonly string[] = ['v1.json', 'config-v1.json'];

const RELEASE_TAG = /^v(\d+)\.(\d+)\.(\d+)$/u;

/**
 * The highest release tag (`vX.Y.Z`, no pre-release suffix) among `tags`, by version, or
 * `undefined` when there is none.
 */
export function latestReleaseTag(tags: readonly string[]): string | undefined {
  let best: { tag: string; key: readonly number[] } | undefined;
  for (const raw of tags) {
    const tag = raw.trim();
    const match = RELEASE_TAG.exec(tag);
    if (match === null) {
      continue;
    }
    const key = [Number(match[1]), Number(match[2]), Number(match[3])];
    if (best === undefined || compareVersions(key, best.key) > 0) {
      best = { tag, key };
    }
  }
  return best?.tag;
}

/** Orders two `[major, minor, patch]` triples: negative, zero or positive. */
export function compareVersions(a: readonly number[], b: readonly number[]): number {
  for (let i = 0; i < 3; i += 1) {
    const diff = (a[i] ?? 0) - (b[i] ?? 0);
    if (diff !== 0) {
      return diff;
    }
  }
  return 0;
}

/** Why a schema could not be served. */
export class SchemaError extends Error {
  override name = 'SchemaError';
}

/**
 * Checks that `text` is the schema `name` and carries the `$id` it is served at, and returns it
 * unchanged.
 */
export function checkSchema(name: string, text: string, ref: string): string {
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch (e) {
    throw new SchemaError(`schema/${name} at ${ref} is not JSON: ${String(e)}`);
  }
  const id =
    typeof parsed === 'object' && parsed !== null && '$id' in parsed ? parsed.$id : undefined;
  if (id !== SCHEMA_BASE + name) {
    throw new SchemaError(
      `schema/${name} at ${ref} has $id ${JSON.stringify(id)}, not ${SCHEMA_BASE + name}`,
    );
  }
  return text;
}

/** Reads each schema from `ref` in the repository at `repo`, checked. */
export function readSchemas(repo: string, ref: string): Map<string, string> {
  const schemas = new Map<string, string>();
  for (const name of SCHEMA_FILES) {
    let text: string;
    try {
      text = execFileSync('git', ['show', `${ref}:schema/${name}`], {
        cwd: repo,
        encoding: 'utf8',
        maxBuffer: 64 * 1024 * 1024,
        stdio: ['ignore', 'pipe', 'pipe'],
      });
    } catch (e) {
      throw new SchemaError(`schema/${name} cannot be read at ${ref}: ${String(e)}`);
    }
    schemas.set(name, checkSchema(name, text, ref));
  }
  return schemas;
}

/** The release tags in the repository at `repo`. */
export function localTags(repo: string): string[] {
  return execFileSync('git', ['tag', '--list', 'v*'], { cwd: repo, encoding: 'utf8' })
    .split('\n')
    .filter((t) => t !== '');
}
