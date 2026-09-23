#!/usr/bin/env node
// Writes the synthetic 5,500-module TypeScript monorepo of plan 0001 § 2 Step 20, item 2
// (docs/plans/pending/0001-wave-1-typescript-parity.md): 4 apps and 40 packages, tsconfig `paths`
// for `@pkg/<name>`, workspace package.json files, relative, path-alias and type-only imports, and
// exactly two dependency cycles. Deterministic: a seeded PRNG, so every run writes the same bytes.
//
// Usage: node testbeds/synth/gen.mjs <target-directory>
// Node 22 or later, no dependencies. The tree is generated, never committed (testbeds/synth/README.md).

import { mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, posix } from 'node:path';

const APPS = 4;
const PACKAGES = 40;
const MODULES_PER_PACKAGE = 118; // plus src/index.ts
const MODULES_PER_APP = 183; // plus src/main.ts
const FEATURES = 6;
const SEED = 5500;

/** mulberry32: a small, well-known 32-bit PRNG; the same seed gives the same sequence. */
function prng(seed) {
    let state = seed >>> 0;
    return () => {
        state = (state + 0x6d2b79f5) >>> 0;
        let t = state;
        t = Math.imul(t ^ (t >>> 15), t | 1);
        t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
        return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
}

const random = prng(SEED);
const between = (low, high) => low + Math.floor(random() * (high - low + 1));
const pick = (count, limit) => {
    const chosen = new Set();
    const wanted = Math.min(count, limit);
    while (chosen.size < wanted) chosen.add(Math.floor(random() * limit));
    return [...chosen].sort((a, b) => a - b);
};

const pad = (n, width = 3) => String(n).padStart(width, '0');
const packageName = (p) => `p${pad(p, 2)}`;
const appName = (a) => `app-${pad(a, 1)}`;
const moduleFile = (i) => `feature-${i % FEATURES}/m${pad(i)}.ts`;

/** A relative specifier from one file to another inside the same `src`, without the extension. */
function relative(fromFile, toFile) {
    const spec = posix.relative(posix.dirname(fromFile), toFile).replace(/\.ts$/, '');
    return spec.startsWith('.') ? spec : `./${spec}`;
}

const target = process.argv[2];
if (!target) {
    console.error('usage: node testbeds/synth/gen.mjs <target-directory>');
    process.exit(2);
}
rmSync(target, { recursive: true, force: true });

let written = 0;
function write(file, text) {
    const path = join(target, file);
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, text);
    if (file.endsWith('.ts')) written += 1;
}
const json = (value) => `${JSON.stringify(value, null, 2)}\n`;

/**
 * One module: value imports from lower-numbered modules in the same `src` (so relative imports
 * never form a cycle), alias imports of lower-numbered packages (so package imports never do
 * either), and one type-only import.
 */
function moduleSource(file, index, lowerPackages) {
    const lines = [];
    const values = [];
    for (const j of pick(between(1, 3), index)) {
        lines.push(`import { v${pad(j)} } from '${relative(file, moduleFile(j))}';`);
        values.push(`v${pad(j)}`);
    }
    for (const q of pick(between(0, 2), lowerPackages)) {
        const name = packageName(q);
        lines.push(`import { ${name}Entry } from '@pkg/${name}';`);
        values.push(`${name}Entry`);
    }
    if (index > 0) {
        const j = Math.floor(random() * index);
        lines.push(`import type { T${pad(j)} } from '${relative(file, moduleFile(j))}';`);
        lines.push('');
        lines.push(
            `export type T${pad(index)} = T${pad(j)} & { readonly m${pad(index)}: number };`,
        );
    } else {
        lines.push(`export type T${pad(index)} = { readonly m${pad(index)}: number };`);
    }
    const sum = values.length > 0 ? values.map((v) => `Number(${v})`).join(' + ') : '0';
    lines.push(`export const v${pad(index)}: number = ${sum} + ${index};`);
    return `${lines.join('\n')}\n`;
}

// The root: workspaces and tsconfig paths.
const paths = {};
for (let p = 0; p < PACKAGES; p += 1) {
    const name = packageName(p);
    paths[`@pkg/${name}`] = [`packages/${name}/src`];
    paths[`@pkg/${name}/*`] = [`packages/${name}/src/*`];
}
write('package.json', json({ name: 'synth', private: true, workspaces: ['apps/*', 'packages/*'] }));
write(
    'tsconfig.json',
    json({
        compilerOptions: {
            target: 'ES2022',
            module: 'NodeNext',
            moduleResolution: 'NodeNext',
            strict: true,
            paths,
        },
        include: ['apps', 'packages'],
    }),
);

// The packages.
for (let p = 0; p < PACKAGES; p += 1) {
    const name = packageName(p);
    const root = `packages/${name}`;
    write(
        `${root}/package.json`,
        json({ name: `@pkg/${name}`, version: '0.0.0', main: 'src/index.ts' }),
    );
    const exported = [];
    for (let i = 0; i < MODULES_PER_PACKAGE; i += 1) {
        const file = moduleFile(i);
        write(`${root}/src/${file}`, moduleSource(file, i, p));
        exported.push(`export { v${pad(i)} } from '${relative('index.ts', file)}';`);
    }
    write(`${root}/src/index.ts`, `${exported.join('\n')}\n\nexport const ${name}Entry = ${p};\n`);
}

// Cycle one: two modules of one package that import each other.
write(
    'packages/p05/src/cycle/x.ts',
    "import { y } from './y';\n\nexport const x = (): number => y() + 1;\n",
);
write(
    'packages/p05/src/cycle/y.ts',
    "import { x } from './x';\n\nexport const y = (): number => (x.length > 0 ? 0 : 1);\n",
);
// Cycle two: across two packages, through the path aliases.
write(
    'packages/p10/src/cycle/a.ts',
    "import { b } from '@pkg/p30/cycle/b';\n\nexport const a = (): number => b() + 1;\n",
);
write(
    'packages/p30/src/cycle/b.ts',
    "import { a } from '@pkg/p10/cycle/a';\n\nexport const b = (): number => (a.length > 0 ? 0 : 1);\n",
);

// The apps: modules over the packages, and a main that pulls in every module.
for (let a = 0; a < APPS; a += 1) {
    const name = appName(a);
    const root = `apps/${name}`;
    const dependencies = {};
    for (let p = 0; p < PACKAGES; p += 1) dependencies[`@pkg/${packageName(p)}`] = 'workspace:*';
    write(`${root}/package.json`, json({ name, private: true, dependencies }));
    const imports = [];
    for (let i = 0; i < MODULES_PER_APP; i += 1) {
        const file = moduleFile(i);
        write(`${root}/src/${file}`, moduleSource(file, i, PACKAGES));
        imports.push(`import { v${pad(i)} } from '${relative('main.ts', file)}';`);
    }
    const all = Array.from({ length: MODULES_PER_APP }, (_, i) => `v${pad(i)}`);
    write(
        `${root}/src/main.ts`,
        `${imports.join('\n')}\n\nexport const total = [${all.join(', ')}].length;\n`,
    );
}

const expected = PACKAGES * (MODULES_PER_PACKAGE + 1) + 4 + APPS * (MODULES_PER_APP + 1);
if (written !== expected || written !== 5500) {
    console.error(`wrote ${written} modules, expected 5500`);
    process.exit(1);
}
console.log(`synth: ${written} modules, ${APPS} apps, ${PACKAGES} packages, 2 cycles in ${target}`);
