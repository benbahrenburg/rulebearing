// ESLint flat config for every TypeScript and JavaScript package in the repository:
// wrappers/npm, adapters/vitest and frontends/eslint-plugin-rulebearing.
// Conventions: CLAUDE.md, TypeScript conventions. Gate: docs/adr/0023-documentation-link-and-lint-gates.md.
// Run through the one entry point: `cargo xtask lint` (or `npm run lint`).
import js from '@eslint/js';
import { defineConfig, globalIgnores } from 'eslint/config';
import tseslint from 'typescript-eslint';
import prettier from 'eslint-config-prettier';

export default defineConfig([
  globalIgnores([
    '**/node_modules/**',
    '**/dist/**',
    'target/**',
    'testbeds/checkouts/**',
    'conformance/**/upstream/**',
    'conformance/**/fixtures/**',
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
    // Tests may reach for the console and for fixtures typed loosely.
    files: ['**/*.test.ts', '**/*.spec.ts', '**/tests/**/*.ts'],
    rules: { 'no-console': 'off', '@typescript-eslint/no-non-null-assertion': 'off' },
  },
  prettier,
]);
