// The playground's file gathering: the files a visitor chooses (a folder, single files, or a drop)
// read in the browser into the map of paths to source text that `cruise_ts` and `cruise_py` take.
// Paths are made relative to the chosen folder; binary files, files over the size limit, files
// past the total limit and version-control folders are skipped with their reason. Nothing is
// uploaded: the bytes go from the browser's file API to the module and nowhere else.
//
// Architecture: docs/architecture.md#security-posture (the playground runs entirely client-side).
// Decisions: docs/adr/0073-extractors-read-files-through-rb-model.md (the extractors read the
// files from memory, under /playground; a path that climbs out is refused),
// docs/adr/0072-the-docs-site-builds-from-docs-and-serves-the-released-schemas.md.
// Plan: docs/plans/pending/0004-wave-4-reach.md, Step 3; the exports in § 1.6. Requirement: FR-REACH-01.

/** The largest file read, in bytes: a source file larger than this is generated or vendored. */
export const MAX_FILE_BYTES = 1024 * 1024;

/** The most bytes read in all; files past it are skipped. */
export const MAX_TOTAL_BYTES = 32 * 1024 * 1024;

/** The most files read; files past it are skipped. */
export const MAX_FILES = 20_000;

/** Folders whose contents are history, not source, and are never read. */
export const SKIPPED_FOLDERS: readonly string[] = ['.git', '.hg', '.svn'];

/** A file as the browser hands it over; a DOM `File` is one. */
export interface PickedFile {
  readonly name: string;
  /** The path from the chosen folder, its name first, for a folder pick; empty otherwise. */
  readonly webkitRelativePath: string;
  readonly size: number;
  arrayBuffer(): Promise<ArrayBuffer>;
}

/** A file and the path it was found at. */
export interface Found {
  readonly path: string;
  readonly file: PickedFile;
}

/** Why a file was not read. */
export type SkipReason = 'binary' | 'too large' | 'over the total limit' | 'version control';

/** A file that was not read, and why. */
export interface Skipped {
  readonly path: string;
  readonly reason: SkipReason;
}

/** What gathering produced: the files to hand to the module and the ones left out. */
export interface FileSet {
  /** Paths to source text, in path order. */
  readonly files: Map<string, string>;
  readonly skipped: readonly Skipped[];
  /** The bytes read. */
  readonly bytes: number;
}

/**
 * A path as the module takes it: `/`-separated, without a leading `/` or `./`, without empty or
 * `.` segments. A `..` segment is kept, so the module refuses the path by name.
 */
export function normalisePath(path: string): string {
  return path
    .replaceAll('\\', '/')
    .split('/')
    .filter((segment) => segment !== '' && segment !== '.')
    .join('/');
}

/** The path of a picked file: its path inside the chosen folder, or its name. */
export function pickedPath(file: PickedFile): string {
  return normalisePath(file.webkitRelativePath === '' ? file.name : file.webkitRelativePath);
}

/**
 * The paths made relative to the chosen folder: when every path starts with the same folder (one
 * folder chosen or dropped), that folder is removed; otherwise the paths are kept as found.
 */
export function relativeTo(found: readonly Found[]): Found[] {
  const normalised = found.map((f) => ({ path: normalisePath(f.path), file: f.file }));
  const first = normalised[0]?.path.split('/')[0];
  const shared =
    first !== undefined &&
    normalised.every((f) => f.path.startsWith(`${first}/`) && f.path.length > first.length + 1);
  if (!shared) {
    return normalised;
  }
  return normalised.map((f) => ({ path: f.path.slice(first.length + 1), file: f.file }));
}

function inSkippedFolder(path: string): boolean {
  return path
    .split('/')
    .slice(0, -1)
    .some((segment) => SKIPPED_FOLDERS.includes(segment));
}

const decoder = new TextDecoder('utf-8', { fatal: true });

/** The text of `bytes`, or `undefined` when they are not text: a NUL byte, or not UTF-8. */
export function textOf(bytes: Uint8Array): string | undefined {
  if (bytes.includes(0)) {
    return undefined;
  }
  try {
    return decoder.decode(bytes);
  } catch {
    return undefined;
  }
}

/** The limits a gathering applies; the defaults are the exported constants. */
export interface Limits {
  readonly fileBytes: number;
  readonly totalBytes: number;
  readonly files: number;
}

/** The limits the page applies. */
export const LIMITS: Limits = {
  fileBytes: MAX_FILE_BYTES,
  totalBytes: MAX_TOTAL_BYTES,
  files: MAX_FILES,
};

/** Reads `found` (paths already relative) into a {@link FileSet}, in path order. */
export async function readFiles(
  found: readonly Found[],
  limits: Limits = LIMITS,
): Promise<FileSet> {
  const sorted = [...found].sort((a, b) => (a.path < b.path ? -1 : a.path > b.path ? 1 : 0));
  const files = new Map<string, string>();
  const skipped: Skipped[] = [];
  let bytes = 0;
  for (const { path, file } of sorted) {
    if (inSkippedFolder(path)) {
      skipped.push({ path, reason: 'version control' });
    } else if (file.size > limits.fileBytes) {
      skipped.push({ path, reason: 'too large' });
    } else if (bytes + file.size > limits.totalBytes || files.size >= limits.files) {
      skipped.push({ path, reason: 'over the total limit' });
    } else {
      const text = textOf(new Uint8Array(await file.arrayBuffer()));
      if (text === undefined) {
        skipped.push({ path, reason: 'binary' });
      } else {
        files.set(path, text);
        bytes += file.size;
      }
    }
  }
  return { files, skipped, bytes };
}

/** The files of a folder or file pick, read. */
export async function readPicked(picked: readonly PickedFile[], limits?: Limits): Promise<FileSet> {
  return readFiles(relativeTo(picked.map((file) => ({ path: pickedPath(file), file }))), limits);
}

/** A dropped entry, as `DataTransferItem.webkitGetAsEntry()` returns it. */
export interface DroppedEntry {
  readonly isFile: boolean;
  readonly isDirectory: boolean;
  /** The path from the drop, starting with `/`. */
  readonly fullPath: string;
}

/** A dropped file entry. */
export interface DroppedFile extends DroppedEntry {
  file(success: (file: PickedFile) => void, failure: (error: unknown) => void): void;
}

/** A dropped folder entry. */
export interface DroppedFolder extends DroppedEntry {
  createReader(): {
    readEntries(
      success: (entries: DroppedEntry[]) => void,
      failure: (error: unknown) => void,
    ): void;
  };
}

function isDroppedFile(entry: DroppedEntry): entry is DroppedFile {
  return entry.isFile && 'file' in entry;
}

function isDroppedFolder(entry: DroppedEntry): entry is DroppedFolder {
  return entry.isDirectory && 'createReader' in entry;
}

/** Every entry of a dropped folder: `readEntries` hands them over in batches until an empty one. */
async function children(folder: DroppedFolder): Promise<DroppedEntry[]> {
  const reader = folder.createReader();
  const all: DroppedEntry[] = [];
  for (;;) {
    const batch = await new Promise<DroppedEntry[]>((done, fail) => {
      reader.readEntries(done, fail);
    });
    if (batch.length === 0) {
      return all;
    }
    all.push(...batch);
  }
}

/** Every file under the dropped entries, by its path from the drop. Version-control folders are not walked. */
export async function walkDropped(entries: readonly DroppedEntry[]): Promise<Found[]> {
  const found: Found[] = [];
  const pending = [...entries];
  for (let entry = pending.shift(); entry !== undefined; entry = pending.shift()) {
    if (isDroppedFile(entry)) {
      const file = await new Promise<PickedFile>((done, fail) => {
        entry.file(done, fail);
      });
      found.push({ path: normalisePath(entry.fullPath), file });
    } else if (isDroppedFolder(entry)) {
      const name = normalisePath(entry.fullPath).split('/').pop() ?? '';
      if (SKIPPED_FOLDERS.includes(name)) {
        continue;
      }
      pending.push(...(await children(entry)));
    }
  }
  return found;
}

/** The files of a drop, read. */
export async function readDropped(
  entries: readonly DroppedEntry[],
  limits?: Limits,
): Promise<FileSet> {
  return readFiles(relativeTo(await walkDropped(entries)), limits);
}

/** The language a cruise extracts. */
export type Language = 'typescript' | 'python';

const TYPESCRIPT = /\.(?:[cm]?[jt]s|[jt]sx)$/u;
const PYTHON = /\.pyi?$/u;
/** Folders that hold installed packages, not the project's own sources. */
const INSTALLED = new Set(['node_modules', '.venv', 'venv', 'site-packages', '__pypackages__']);

/** How many of the project's own sources each language has; installed packages are not counted. */
export function countSources(paths: Iterable<string>): Record<Language, number> {
  const counts: Record<Language, number> = { typescript: 0, python: 0 };
  for (const path of paths) {
    if (path.split('/').some((segment) => INSTALLED.has(segment))) {
      continue;
    }
    if (TYPESCRIPT.test(path)) {
      counts.typescript += 1;
    } else if (PYTHON.test(path)) {
      counts.python += 1;
    }
  }
  return counts;
}

/**
 * The language of the files: the one with more of the project's own sources. A tie, including
 * none of either, is an error that asks the visitor to pick.
 */
export function inferLanguage(paths: Iterable<string>): Language {
  const { typescript, python } = countSources(paths);
  if (typescript > python) {
    return 'typescript';
  }
  if (python > typescript) {
    return 'python';
  }
  throw new Error(
    `the language cannot be inferred: the files hold ${String(typescript)} TypeScript or JavaScript and ${String(python)} Python source(s); pick the language`,
  );
}

/** The configuration files at the root of a chosen folder the page loads, in order of preference. */
export const CONFIG_FILES: readonly string[] = [
  'rulebearing.yaml',
  'rulebearing.yml',
  'rulebearing.json',
  '.dependency-cruiser.json',
];

/** The first configuration file at the root of the files, if any. */
export function configIn(files: ReadonlyMap<string, string>): string | undefined {
  return CONFIG_FILES.find((name) => files.has(name));
}

const SHOWN = 10;

/** One line saying what was read and what was skipped, the first few skipped files named. */
export function describeFiles(set: FileSet): string {
  const kib = (set.bytes / 1024).toFixed(1);
  const read = `${String(set.files.size)} file(s) read, ${kib} KiB, kept in this browser.`;
  if (set.skipped.length === 0) {
    return read;
  }
  const named = set.skipped.slice(0, SHOWN).map((s) => `${s.path} (${s.reason})`);
  const more = set.skipped.length > SHOWN ? `, and ${String(set.skipped.length - SHOWN)} more` : '';
  return `${read} Skipped ${String(set.skipped.length)}: ${named.join(', ')}${more}.`;
}
