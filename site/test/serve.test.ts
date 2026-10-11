// The local server for the built site. Plan: docs/plans/pending/0004-wave-4-reach.md, Step 3.
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import type { Server } from 'node:http';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { fileFor, serve, urlOf } from '../src/serve.js';

let root: string;
let server: Server;

beforeAll(async () => {
  root = mkdtempSync(join(tmpdir(), 'rb-site-serve-'));
  mkdirSync(join(root, 'docs'));
  writeFileSync(join(root, 'index.html'), '<h1>home</h1>');
  writeFileSync(join(root, 'docs', 'index.html'), 'docs');
  writeFileSync(join(root, 'm.wasm'), 'wasm');
  writeFileSync(join(root, 'x.bin'), 'bin');
  mkdirSync(join(root, 'empty'));
  server = await serve(root, 0);
});

afterAll(async () => {
  await new Promise((done) => server.close(done));
  rmSync(root, { recursive: true, force: true });
});

describe('fileFor', () => {
  it('maps paths to files and folders to their index', () => {
    expect(fileFor(root, '/')).toBe(join(root, 'index.html'));
    expect(fileFor(root, '/docs/')).toBe(join(root, 'docs', 'index.html'));
    expect(fileFor(root, '/m.wasm?v=1')).toBe(join(root, 'm.wasm'));
  });

  it('refuses what is missing or outside the root', () => {
    expect(fileFor(root, '/missing.html')).toBeUndefined();
    expect(fileFor(root, '/empty/')).toBeUndefined();
    expect(fileFor(root, '/../etc/passwd')).toBeUndefined();
    expect(fileFor(root, '/%2e%2e/x')).toBeUndefined();
    expect(fileFor(root, '/%E0%A4%A')).toBeUndefined();
  });
});

describe('serve', () => {
  it('answers with the file and its media type', async () => {
    const page = await fetch(urlOf(server));
    expect(page.status).toBe(200);
    expect(page.headers.get('content-type')).toBe('text/html; charset=utf-8');
    expect(await page.text()).toBe('<h1>home</h1>');
    const wasm = await fetch(`${urlOf(server)}m.wasm`);
    expect(wasm.headers.get('content-type')).toBe('application/wasm');
    const other = await fetch(`${urlOf(server)}x.bin`);
    expect(other.headers.get('content-type')).toBe('application/octet-stream');
  });

  it('answers 404 for a missing file', async () => {
    expect((await fetch(`${urlOf(server)}nope`)).status).toBe(404);
  });

  it('names its URL only while listening', () => {
    const idle = { address: () => null } as unknown as Server;
    expect(() => urlOf(idle)).toThrow(/not listening/u);
  });
});
