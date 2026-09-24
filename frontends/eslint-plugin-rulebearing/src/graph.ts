// The graph the plugin resolves import specifiers against: the cached graph's `modules[]`.
//
// Source: docs/artifacts/design.md#two-front-ends-that-will-matter-more-than-the-mcp-server.
// Decision: docs/adr/0021-agent-surface-cli-first.md. Contract: docs/adr/0004-graph-document-is-cruise-result-superset.md.
// Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 13. Requirement: FR-DIST-04.
//
// There is no second resolver. A specifier resolves to the `resolved` path of the dependency the
// graph already records for it; a relative specifier the graph has not seen yet (an import the
// agent has just written) resolves to the module among `modules[].source` it names, by the file
// extensions and `index` files those sources carry. What the graph cannot name is not guessed.

import { existsSync, readFileSync, readdirSync, realpathSync, statSync } from 'node:fs';
import { join, posix, sep } from 'node:path';

/** The folder of worktree-aware cache entries, as `rb-cli/src/cache/key.rs` names it. */
export const CACHE_DIR = join('.graph', 'cache');
/** The graph document inside a cache entry. */
export const GRAPH_FILE = 'graph.json';
/** The inputs an entry's name hashes, written beside the graph for readers such as this one. */
export const KEY_FILE = 'key.json';

/** One edge of a module, as far as resolution reads it. */
export interface GraphDependency {
  /** The specifier as written in the source. */
  readonly module: string;
  /** The repository-relative path it resolved to. */
  readonly resolved: string;
  /** True when the extractor could not resolve it. */
  readonly couldNotResolve: boolean;
}

/** One module of the graph, as far as resolution reads it. */
export interface GraphModule {
  readonly source: string;
  readonly dependencies: readonly GraphDependency[];
}

/** The graph's modules, indexed for resolution. */
export interface Graph {
  readonly modules: ReadonlyMap<string, GraphModule>;
  /** Bare specifier to the path the graph resolves it to, from any module (first in source order). */
  readonly bare: ReadonlyMap<string, string>;
}

/** A graph document that cannot be read, named with its path. */
export class GraphError extends Error {
  override readonly name = 'GraphError';
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

/** Whether a specifier is relative to the importing file. */
export function isRelative(specifier: string): boolean {
  return (
    specifier === '.' ||
    specifier === '..' ||
    specifier.startsWith('./') ||
    specifier.startsWith('../')
  );
}

/** Reads the modules of a cruise result or cached graph document. */
export function parseGraph(text: string, name: string): Graph {
  let document: unknown;
  try {
    document = JSON.parse(text);
  } catch (error) {
    throw new GraphError(
      `${name} is not JSON: ${error instanceof Error ? error.message : String(error)}`,
    );
  }
  if (!isRecord(document) || !Array.isArray(document.modules)) {
    throw new GraphError(`${name} is not a graph document: it has no modules[]`);
  }
  const modules = new Map<string, GraphModule>();
  const bare = new Map<string, string>();
  const ordered = document.modules
    .filter(isRecord)
    .filter((m): m is Record<string, unknown> & { source: string } => typeof m.source === 'string')
    .sort((a, b) => (a.source < b.source ? -1 : a.source > b.source ? 1 : 0));
  for (const m of ordered) {
    const dependencies: GraphDependency[] = (Array.isArray(m.dependencies) ? m.dependencies : [])
      .filter(isRecord)
      .flatMap((d) =>
        typeof d.module === 'string' && typeof d.resolved === 'string'
          ? [
              {
                module: d.module,
                resolved: d.resolved,
                couldNotResolve: d.couldNotResolve === true,
              },
            ]
          : [],
      );
    modules.set(m.source, { source: m.source, dependencies });
    for (const d of dependencies) {
      if (!d.couldNotResolve && !isRelative(d.module) && !bare.has(d.module)) {
        bare.set(d.module, d.resolved);
      }
    }
  }
  return { modules, bare };
}

/** Extensions tried after a relative specifier with none, in the order a TypeScript build prefers. */
export const EXTENSIONS: readonly string[] = [
  '.ts',
  '.tsx',
  '.d.ts',
  '.mts',
  '.cts',
  '.js',
  '.jsx',
  '.mjs',
  '.cjs',
  '.json',
  '.vue',
  '.svelte',
];

/** A JavaScript extension written in a TypeScript ESM import names its TypeScript source. */
const SOURCE_FOR: Readonly<Record<string, readonly string[]>> = {
  '.js': ['.ts', '.tsx'],
  '.jsx': ['.tsx'],
  '.mjs': ['.mts'],
  '.cjs': ['.cts'],
};

/** The candidate paths a relative specifier names, most specific first. */
export function candidates(base: string): string[] {
  const out = [base];
  const dot = base.lastIndexOf('.');
  if (dot > base.lastIndexOf('/')) {
    for (const ext of SOURCE_FOR[base.slice(dot)] ?? []) {
      out.push(base.slice(0, dot) + ext);
    }
  }
  for (const ext of EXTENSIONS) {
    out.push(base + ext);
  }
  for (const ext of EXTENSIONS) {
    out.push(`${base}/index${ext}`);
  }
  return out;
}

/**
 * The path `specifier`, written in `from`, resolves to in `graph`, or `undefined` when the graph
 * cannot name it.
 */
export function resolveSpecifier(
  graph: Graph,
  from: string,
  specifier: string,
): string | undefined {
  const recorded = graph.modules.get(from)?.dependencies.find((d) => d.module === specifier);
  if (recorded !== undefined) {
    return recorded.couldNotResolve ? undefined : recorded.resolved;
  }
  if (isRelative(specifier)) {
    const base = posix.normalize(posix.join(posix.dirname(from), specifier)).replace(/\/$/, '');
    return candidates(base).find((path) => graph.modules.has(path));
  }
  return graph.bare.get(specifier);
}

/** A path as the graph writes it: relative to `cwd`, `/`-separated. */
export function graphPath(cwd: string, file: string): string {
  const relative = file.startsWith(cwd + sep) ? file.slice(cwd.length + 1) : file;
  return relative.split(sep).join('/');
}

function real(path: string): string {
  try {
    return realpathSync(path);
  } catch {
    return path;
  }
}

/**
 * The newest cache entry under `cwd` whose worktree root holds `cwd`: the graph `can-import`
 * reads, or the one it read before the last commit. `undefined` when there is none yet.
 */
export function findCachedGraph(cwd: string): string | undefined {
  const folder = join(cwd, CACHE_DIR);
  if (!existsSync(folder)) {
    return undefined;
  }
  const here = real(cwd).split(sep).join('/');
  let newest: { path: string; mtime: number } | undefined;
  for (const entry of readdirSync(folder).sort()) {
    const graph = join(folder, entry, GRAPH_FILE);
    const keyFile = join(folder, entry, KEY_FILE);
    if (!existsSync(graph) || !existsSync(keyFile)) {
      continue;
    }
    let key: unknown;
    try {
      key = JSON.parse(readFileSync(keyFile, 'utf8'));
    } catch {
      continue;
    }
    if (!isRecord(key) || typeof key.root !== 'string') {
      continue;
    }
    const root = key.root.replace(/\/$/, '');
    if (here !== root && !here.startsWith(`${root}/`)) {
      continue;
    }
    const mtime = statSync(graph).mtimeMs;
    if (newest === undefined || mtime > newest.mtime) {
      newest = { path: graph, mtime };
    }
  }
  return newest?.path;
}

const loaded = new Map<string, { mtime: number; graph: Graph }>();

/** Reads a graph document once per change of its modification time. */
export function loadGraph(path: string): Graph {
  let mtime: number;
  try {
    mtime = statSync(path).mtimeMs;
  } catch (error) {
    throw new GraphError(
      `cannot read the graph ${path}: ${error instanceof Error ? error.message : String(error)}`,
    );
  }
  const cached = loaded.get(path);
  if (cached?.mtime === mtime) {
    return cached.graph;
  }
  const graph = parseGraph(readFileSync(path, 'utf8'), path);
  loaded.set(path, { mtime, graph });
  return graph;
}
