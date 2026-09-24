// ESLint flat config for every TypeScript and JavaScript package in the repository:
// wrappers/npm, adapters/vitest and frontends/eslint-plugin-rulebearing, plus the Node scripts of
// the conformance harness (conformance/*/harness) and the test-bed runner (testbeds/).
// Conventions: CLAUDE.md, TypeScript conventions. Gate: docs/adr/0023-documentation-link-and-lint-gates.md.
// Run through the one entry point: `cargo xtask lint` (or `npm run lint`).
import js from '@eslint/js';
import { defineConfig, globalIgnores } from 'eslint/config';
import tseslint from 'typescript-eslint';
import prettier from 'eslint-config-prettier';
import globals from 'globals';

export default defineConfig([
    globalIgnores([
        '**/node_modules/**',
        '**/dist/**',
        '**/coverage/**',
        'target/**',
        'testbeds/checkouts/**',
        'conformance/**/upstream/**',
        'conformance/**/fixtures/**',
        // Vendored verbatim from dependency-cruiser 18.2.0 (MIT) as the bundled presets.
        'presets/dependency-cruiser/**',
        // The extractor's option fixtures are inputs, kept verbatim (plan 0001, sub-wave 1C).
        'crates/rb-extract-ts/tests/options/**',
        // The code-layer fixture tree, whose lines and columns the expectation records (plan 0002, 2C).
        'crates/rb-extract-ts/tests/fixtures/**',
        // Local only: fuzzing corpora (fuzz/README.md) and the worktrees of parallel agent sessions.
        'fuzz/corpus/**',
        '.claude/worktrees/**',
    ]),
    js.configs.recommended,
    tseslint.configs.strictTypeChecked,
    tseslint.configs.stylisticTypeChecked,
    {
        languageOptions: {
            ecmaVersion: 2023,
            sourceType: 'module',
            parserOptions: {
                // Type-aware linting. Package sources are covered by the root tsconfig.json; the
                // configuration files at the root belong to no project, so they use the default one.
                projectService: {
                    allowDefaultProject: ['*.mjs', '*.js', '*.cjs'],
                    defaultProject: 'tsconfig.json',
                },
                tsconfigRootDir: import.meta.dirname,
            },
        },
        rules: {
            // The wrappers shell out to one binary and must not swallow its failures.
            '@typescript-eslint/no-floating-promises': 'error',
            '@typescript-eslint/no-misused-promises': 'error',
            // A finding carries the rule's `fix` text; never silence one with a cast.
            '@typescript-eslint/no-unsafe-assignment': 'error',
            '@typescript-eslint/consistent-type-imports': 'error',
            eqeqeq: ['error', 'always'],
            'no-console': ['error', { allow: ['error', 'warn'] }],
        },
    },
    {
        // This file and its neighbours are configuration, typed loosely by the packages they load.
        files: ['*.mjs', '*.js', '*.cjs'],
        extends: [tseslint.configs.disableTypeChecked],
    },
    {
        // Plain Node scripts run by hand or by CI: no TypeScript project, Node's globals, and stdout
        // is their output, so console.log is how they report.
        files: ['conformance/**/*.mjs', 'testbeds/**/*.mjs'],
        extends: [tseslint.configs.disableTypeChecked],
        languageOptions: { globals: globals.node },
        rules: { 'no-console': 'off' },
    },
    {
        // The synthetic benchmark's dependency-cruiser configuration (plan 0001, Step 20).
        files: ['testbeds/**/*.cjs'],
        extends: [tseslint.configs.disableTypeChecked],
        languageOptions: { sourceType: 'commonjs', globals: globals.node },
    },
    {
        // The npm wrapper's two thin entry points, which only import compiled code from dist/
        // (plan 0001, Step 19); the logic they call is linted type-checked in src/.
        files: ['wrappers/npm/bin/*.js', 'wrappers/npm/scripts/*.mjs'],
        extends: [tseslint.configs.disableTypeChecked],
        languageOptions: { globals: globals.node },
    },
    {
        // The configuration sandbox's module system: a classic script QuickJS evaluates, with no
        // Node globals (docs/adr/0006-embedded-quickjs-config-evaluator.md). It defines `console`
        // as a no-op, so its own console use is a definition, not output.
        files: ['crates/rb-config/src/js/shim.js'],
        extends: [tseslint.configs.disableTypeChecked],
        languageOptions: { sourceType: 'script', globals: { globalThis: 'readonly' } },
    },
    {
        // Tests may reach for the console and for fixtures typed loosely.
        files: ['**/*.test.ts', '**/*.spec.ts', '**/tests/**/*.ts'],
        rules: { 'no-console': 'off', '@typescript-eslint/no-non-null-assertion': 'off' },
    },
    prettier,
]);
