// The graph the plugin resolves import specifiers against: the cached graph's `modules[]`.
//
// Source: docs/artifacts/design.md#two-front-ends-that-will-matter-more-than-the-mcp-server.
// Decision: docs/adr/0021-agent-surface-cli-first.md. Contract: docs/adr/0004-graph-document-is-cruise-result-superset.md.
// Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 13. Requirement: FR-DIST-04.
//
// There is no second resolver. A specifier resolves to the `resolved` path of the dependency the
// graph already records for it; a relative specifier the graph has not seen yet (an import the
// agent has just written) resolves to the module among `modules[].source` it names, by the file
// extensions and `index` files those sources carry, and when no module matches (a file created
// since the graph was extracted), to the first of the same candidates that is a file on disk, so
// `can-import` is still asked. A bare specifier the graph cannot name is not guessed.
//
// The cache entry read is the one `can-import` would read for the same worktree root, `HEAD`
// and configuration: `key.json` records them (crates/rb-cli/src/cache/key.rs), the plugin reads
// `HEAD` from the git files as the binary does and re-hashes the configuration files the entry
// names, and takes the newest entry that agrees. A Windows root is compared without its verbatim
// prefix (`//?/`) and with its drive letter in either case.

import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, readFileSync, readdirSync, realpathSync, statSync } from 'node:fs';
import { isAbsolute, join, posix, resolve, sep } from 'node:path';

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
 * The path `specifier`, written in `from`, resolves to in `graph`; for a relative specifier no
 * module matches, the first candidate `isFile` accepts (a graph path, relative to the working
 * folder); `undefined` when neither can name it.
 */
export function resolveSpecifier(
  graph: Graph,
  from: string,
  specifier: string,
  isFile: (path: string) => boolean = () => false,
): string | undefined {
  const recorded = graph.modules.get(from)?.dependencies.find((d) => d.module === specifier);
  if (recorded !== undefined) {
    return recorded.couldNotResolve ? undefined : recorded.resolved;
  }
  if (isRelative(specifier)) {
    const base = posix.normalize(posix.join(posix.dirname(from), specifier)).replace(/\/$/, '');
    const paths = candidates(base);
    return paths.find((path) => graph.modules.has(path)) ?? paths.find((path) => isFile(path));
  }
  return graph.bare.get(specifier);
}

/** An `isFile` for [`resolveSpecifier`]: whether `path`, relative to `cwd`, is a file. */
export function fileIn(cwd: string): (path: string) => boolean {
  return (path) => {
    try {
      return statSync(join(cwd, ...path.split('/'))).isFile();
    } catch {
      return false;
    }
  };
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
 * A path in the one form roots are compared in: `/`-separated, without a trailing `/`, without
 * the Windows verbatim prefix (`//?/C:/x` is `C:/x`, `//?/UNC/host/share` is `//host/share`), the
 * drive letter upper-case.
 */
export function comparable(path: string): string {
  let out = path.replace(/\\/g, '/');
  if (out.startsWith('//?/UNC/')) {
    out = `//${out.slice('//?/UNC/'.length)}`;
  } else if (out.startsWith('//?/')) {
    out = out.slice('//?/'.length);
  }
  out = out.replace(/^([a-zA-Z]):/, (_, drive: string) => `${drive.toUpperCase()}:`);
  return out.length > 1 ? out.replace(/\/+$/, '') : out;
}

/** Whether `root` is `here` or a folder above it, both as [`comparable`] gives them. */
export function holds(root: string, here: string): boolean {
  const [r, h] = [comparable(root), comparable(here)];
  return h === r || h.startsWith(r.endsWith('/') ? r : `${r}/`);
}

function isObjectName(text: string): boolean {
  return /^([0-9a-fA-F]{40}|[0-9a-fA-F]{64})$/.test(text);
}

function readText(path: string): string | undefined {
  try {
    return readFileSync(path, 'utf8');
  } catch {
    return undefined;
  }
}

/** The git directory of the worktree at `root`: `.git`, or the folder a `.git` file names. */
function gitDir(root: string): string | undefined {
  const dotGit = join(root, '.git');
  try {
    if (statSync(dotGit).isDirectory()) {
      return dotGit;
    }
  } catch {
    return undefined;
  }
  const named = readText(dotGit)
    ?.split('\n')
    .find((l) => l.startsWith('gitdir:'))
    ?.slice('gitdir:'.length)
    .trim();
  if (named === undefined || named === '') {
    return undefined;
  }
  return isAbsolute(named) ? named : resolve(root, named);
}

/**
 * The commit `HEAD` names in the worktree at `root`, from the files as `rb-cli` reads them (a
 * symbolic `HEAD` through loose refs, the common directory, then `packed-refs`; at most five
 * hops), else `undefined`.
 */
export function headFromFiles(root: string): string | undefined {
  const git = gitDir(root);
  if (git === undefined) {
    return undefined;
  }
  const commonText = readText(join(git, 'commondir'))?.trim();
  const common =
    commonText === undefined || commonText === ''
      ? git
      : isAbsolute(commonText)
        ? commonText
        : resolve(git, commonText);
  let content = readText(join(git, 'HEAD'));
  for (let hop = 0; hop < 5 && content !== undefined; hop += 1) {
    const text = content.trim();
    if (isObjectName(text)) {
      return text;
    }
    if (!text.startsWith('ref:')) {
      return undefined;
    }
    const name = text.slice('ref:'.length).trim();
    const loose = readText(join(git, name)) ?? readText(join(common, name));
    if (loose === undefined) {
      const packed = readText(join(common, 'packed-refs'))
        ?.split('\n')
        .filter((l) => !l.startsWith('#') && !l.startsWith('^'))
        .map((l) => l.split(' '))
        .find(([object, reference]) => reference?.trim() === name && isObjectName(object ?? ''));
      return packed?.[0];
    }
    content = loose;
  }
  return undefined;
}

/** `HEAD` as the binary resolves it: from the files, else `git rev-parse HEAD`, else empty. */
export function head(root: string): string {
  const fromFiles = headFromFiles(root);
  if (fromFiles !== undefined) {
    return fromFiles;
  }
  const result = spawnSync('git', ['rev-parse', 'HEAD'], {
    cwd: root,
    encoding: 'utf8',
    windowsHide: true,
  });
  return result.status === 0 ? result.stdout.trim() : '';
}

/**
 * SHA-256 over `(name, bytes)` pairs in name order, each field length-prefixed with a 64-bit
 * big-endian count: `hash_files` in crates/rb-cli/src/cmd/attest.rs.
 */
export function hashFiles(files: readonly (readonly [string, Buffer])[]): string {
  const hash = createHash('sha256');
  const sorted = [...files].sort(([a], [b]) => Buffer.compare(Buffer.from(a), Buffer.from(b)));
  for (const [name, bytes] of sorted) {
    for (const field of [Buffer.from(name, 'utf8'), bytes]) {
      const length = Buffer.alloc(8);
      length.writeBigUInt64BE(BigInt(field.length));
      hash.update(length);
      hash.update(field);
    }
  }
  return hash.digest('hex');
}

/** A configuration file an entry names, as a path: relative names are under the root. */
function configPath(root: string, name: string): string {
  return /^([a-zA-Z]:)?\//.test(name) || name.startsWith('//') ? name : posix.join(root, name);
}

function readBytes(path: string): Buffer {
  try {
    return readFileSync(path);
  } catch {
    return Buffer.alloc(0);
  }
}

/**
 * Whether an entry's configuration is the one on disk: its files, re-read and hashed, give its
 * `configHash`, and `config` (the rule's option, resolved) is among them when given. An entry
 * whose configuration came from standard input names no files and never agrees.
 */
export function configAgrees(
  key: Record<string, unknown>,
  root: string,
  config: string | undefined,
): boolean {
  const files = key.configFiles;
  if (
    typeof key.configHash !== 'string' ||
    !Array.isArray(files) ||
    files.length === 0 ||
    !files.every((f): f is string => typeof f === 'string')
  ) {
    return false;
  }
  const paths = files.map((f) => configPath(root, f));
  if (config !== undefined && !paths.some((p) => comparable(p) === comparable(real(config)))) {
    return false;
  }
  const hash = hashFiles(files.map((f, i) => [f, readBytes(paths[i] ?? f)] as const));
  return hash === key.configHash;
}

/**
 * The newest cache entry under `cwd` that `can-import` would read now: its worktree root holds
 * `cwd`, its `head` is the worktree's `HEAD` and its configuration is the one on disk (`config`,
 * the rule's option, resolved against `cwd`, when given). `undefined` when there is none yet, so
 * the caller asks the binary to write it.
 */
export function findCachedGraph(cwd: string, config?: string): string | undefined {
  const folder = join(cwd, CACHE_DIR);
  if (!existsSync(folder)) {
    return undefined;
  }
  const here = real(cwd);
  const configFile = config === undefined ? undefined : resolve(cwd, config);
  const heads = new Map<string, string>();
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
    if (!isRecord(key) || typeof key.root !== 'string' || typeof key.head !== 'string') {
      continue;
    }
    const root = key.root;
    if (!holds(root, here)) {
      continue;
    }
    let current = heads.get(root);
    if (current === undefined) {
      current = head(root);
      heads.set(root, current);
    }
    if (key.head !== current || !configAgrees(key, root, configFile)) {
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
