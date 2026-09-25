# rulebearing

One architecture rule set for TypeScript, .NET and Python: deterministic guardrails an agent can check its own changes against. This package puts the `rulebearing` binary on an npm project's path. Source, documentation and releases: https://github.com/benbahrenburg/rulebearing

```sh
npm install --save-dev rulebearing
npx rulebearing --version
npx rulebearing cruise --config .dependency-cruiser.cjs --output-type err src
```

The command-line surface is the binary's (`npx rulebearing --help`); the exit codes are those of [ADR-0008](../../docs/adr/0008-exit-code-contract.md). The package also exports `rulebearing/vitest`, which runs each rule as a vitest test ([adapters/vitest](../../adapters/vitest/README.md)); vitest is an optional peer dependency, needed only for that entry point.

## How the package works

The package is a launcher and nothing else ([architecture § Distribution](../../docs/architecture.md#distribution), [FR-DIST-01](../../docs/prd.md#fr-dist-01), [plan 0001 Step 19](../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-19-npm-package-github-action-release-1g)). The binary ships in one of six platform packages, listed under `optionalDependencies` at the same version as `rulebearing`. Each declares `os`, `cpu` and, on Linux, `libc`, so npm installs only the one that matches the machine. There is no `postinstall` script.

| Platform package | Release target | Installed on |
| --- | --- | --- |
| `rulebearing-cli-darwin-arm64` | `aarch64-apple-darwin` | macOS arm64 |
| `rulebearing-cli-darwin-x64` | `x86_64-apple-darwin` | macOS x64 |
| `rulebearing-cli-linux-x64-gnu` | `x86_64-unknown-linux-gnu` | Linux x64, glibc |
| `rulebearing-cli-linux-x64-musl` | `x86_64-unknown-linux-musl` | Linux x64, musl (Alpine) |
| `rulebearing-cli-linux-arm64-gnu` | `aarch64-unknown-linux-gnu` | Linux arm64, glibc |
| `rulebearing-cli-win32-x64-msvc` | `x86_64-pc-windows-msvc` | Windows x64 |

The platform packages are unscoped because the `@rulebearing` npm scope is not held ([ADR-0020](../../docs/adr/0020-single-name-across-registries.md), [plan 0000 sub-wave 0E](../../docs/plans/pending/0000-wave-0-spike.md#wave-0e-decision-and-release-plumbing)).

At run time, `bin/rulebearing.js` picks the platform package from `process.platform`, `process.arch` and, on Linux, the C library (musl when Node's diagnostic report has no `glibcVersionRuntime`). It resolves that package from its own location, runs `bin/rulebearing` (or `bin/rulebearing.exe`) with stdio inherited and every argument unchanged, and exits with the binary's exit code, or re-raises the signal that stopped it. It never re-implements a subcommand.

When the platform package is missing, for example after `npm install --omit=optional`, or on a platform with no release target, the launcher prints one line naming the package to install and the platform it detected, and exits 2.

| Environment variable | Effect |
| --- | --- |
| `RULEBEARING_BINARY` | Path to a `rulebearing` binary to run instead of the platform package's, for a local build (`cargo build --release -p rb-cli`) or a platform without a prebuilt binary. A path that does not exist is exit 2. |

## Finding the binary from another package

The package exports the launcher's resolution, typed, so a Node tool that calls the binary directly finds the same one `npx rulebearing` would run, `RULEBEARING_BINARY` included. [eslint-plugin-rulebearing](../../frontends/eslint-plugin-rulebearing/README.md) locates the binary this way ([plan 0002, Step 13](../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#213-step-13-worktree-aware-cache-and-the-eslint-plugin-2g)).

```javascript
import { detectHost, resolveBinary } from 'rulebearing';

const found = resolveBinary(detectHost(), process.env);
// { kind: 'binary', path } or { kind: 'missing', message }
```

## Developing this package

The launcher is TypeScript under `src/`, compiled to `dist/` (strict, ESM, Node 22); `npm run build` also compiles [adapters/vitest](../../adapters/vitest/README.md) into `dist/vitest/`, which `exports` publishes as `rulebearing/vitest`. Both are linted by the root configuration through `cargo xtask lint` ([ADR-0023](../../docs/adr/0023-documentation-link-and-lint-gates.md)). The tests hold the 70% line floor of [ADR-0018](../../docs/adr/0018-test-coverage-threshold.md), set in `vitest.config.ts`. Install the tooling once with `npm ci` at the repository root.

```sh
npm run build                                          # src/ to dist/
npm test                                               # vitest run --coverage
npm run stage -- <dist-dir> <version> <out-dir> [--partial]
```

`stage` turns a directory of release archives (`rulebearing-<target>.tar.gz`, as [release.yml](../../.github/workflows/release.yml) names them) into seven package folders: the six platform packages, each with its binary executable, and `rulebearing` with the version stamped into it and its `optionalDependencies`. `--partial` skips absent archives, for a check on one machine. The release flow is in [docs/release.md](../../docs/release.md#the-npm-wrapper-from-wave-1).
