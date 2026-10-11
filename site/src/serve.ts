// A static file server for the built site, for local preview and for the playground's browser
// test: a page cannot fetch the wasm module from a `file:` URL, and the module needs the
// `application/wasm` type to compile while streaming.
//
// Architecture: docs/architecture.md#distribution. Decision: docs/adr/0072-the-docs-site-builds-from-docs-and-serves-the-released-schemas.md.
// Plan: docs/plans/pending/0004-wave-4-reach.md, Step 3. Requirements: FR-REACH-01.

import { createReadStream, existsSync, statSync } from 'node:fs';
import { createServer } from 'node:http';
import type { Server } from 'node:http';
import { extname, join, normalize, sep } from 'node:path';

/** The media type of each file extension the site holds. */
export const MEDIA_TYPES: Readonly<Record<string, string>> = {
  '.html': 'text/html; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.wasm': 'application/wasm',
  '.svg': 'image/svg+xml',
  '.txt': 'text/plain; charset=utf-8',
};

/**
 * The file under `root` that `urlPath` names, or `undefined` when there is none or the path
 * climbs out of `root`. A folder is served by its `index.html`.
 */
export function fileFor(root: string, urlPath: string): string | undefined {
  let decoded: string;
  try {
    decoded = decodeURIComponent(urlPath.split('?')[0] ?? '');
  } catch {
    return undefined;
  }
  const full = normalize(join(root, decoded));
  if (full !== normalize(root) && !full.startsWith(normalize(root) + sep)) {
    return undefined;
  }
  if (!existsSync(full)) {
    return undefined;
  }
  if (statSync(full).isDirectory()) {
    const index = join(full, 'index.html');
    return existsSync(index) ? index : undefined;
  }
  return full;
}

/** Starts serving `root` on `127.0.0.1:port` (0 picks a free port) and resolves once listening. */
export function serve(root: string, port: number): Promise<Server> {
  const server = createServer((request, response) => {
    const file = fileFor(root, request.url ?? '/');
    if (file === undefined) {
      response.writeHead(404, { 'content-type': 'text/plain; charset=utf-8' });
      response.end('not found\n');
      return;
    }
    response.writeHead(200, {
      'content-type': MEDIA_TYPES[extname(file)] ?? 'application/octet-stream',
    });
    createReadStream(file).pipe(response);
  });
  return new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(port, '127.0.0.1', () => {
      resolve(server);
    });
  });
}

/** The URL a listening server answers on. */
export function urlOf(server: Server): string {
  const address = server.address();
  if (address === null || typeof address === 'string') {
    throw new Error('the server is not listening on a TCP port');
  }
  return `http://127.0.0.1:${String(address.port)}/`;
}
