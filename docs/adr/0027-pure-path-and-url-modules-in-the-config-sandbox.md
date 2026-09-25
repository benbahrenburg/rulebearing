# ADR-0027: The configuration sandbox offers pure `path` and `url` modules

- **Status:** Accepted
- **Date:** 2026-09-22
- **Derives from:** [ADR-0006](0006-embedded-quickjs-config-evaluator.md) (the sandboxed QuickJS evaluator), [design § The dependency-cruiser format](../artifacts/design.md#the-dependency-cruiser-format), [design § Test beds](../artifacts/design.md#test-beds-open-source-repositories-to-validate-against)
- **Constrains:** [architecture § Security posture](../architecture.md#security-posture), `crates/rb-config/src/js/`
- **Implemented by:** [Wave 1 plan](../plans/pending/0001-wave-1-typescript-parity.md), Step 2
- **Requirements:** [FR-CFG-03](../prd.md#fr-cfg-03), [NFR-SEC-01](../prd.md#nfr-sec-01)

## Context

[ADR-0006](0006-embedded-quickjs-config-evaluator.md) limits what a JavaScript configuration may `require` or `import` to JSON files, other configuration modules under the repository, and the bundled dependency-cruiser presets. Wave 1 measured that rule against the ten dependency-cruiser configurations the test-bed manifest names, fetched at their pinned commits:

| Configuration | Needs beyond ADR-0006 |
| --- | --- |
| eight of the ten | nothing |
| `sverweij/dependency-cruiser` `.dependency-cruiser.mjs` | `node:url` (`fileURLToPath`), `new URL(..., import.meta.url)` to name its own `configs/recommended-strict.cjs` |
| `invertase/react-native-firebase` `.dependency-cruiser.cjs` | `fs` (`readdirSync`, `statSync`, `existsSync`, and `writeFileSync` of a generated tsconfig) |

The first needs only string functions over paths: no file is opened and nothing leaves the process. The second genuinely reads and writes the filesystem.

## Decision

- The sandbox provides `path` (and `node:path`, `path/posix`) and `url` (and `node:url`) as modules implemented in JavaScript inside the sandbox, plus a global `URL` limited to `file:` URLs, `import.meta.url`, `import.meta.dirname`, `import.meta.filename`, `__filename` and `__dirname`. They compute strings; none of them reads, writes or lists anything.
- A no-op `console` is present so a configuration that logs does not throw; it writes nothing.
- Every other Node built-in (`fs`, `child_process`, `os`, `process` and the rest) stays refused, with a message naming `--config-via-node`. The react-native-firebase configuration is the reference case for that flag.
- Resolution still ends at the repository root, after symlinks are resolved.

## Consequences

- The dependency-cruiser oracle loads in the sandbox, so layer 5 on dependency-cruiser's own repository does not need Node for its configuration.
- The sandbox's host surface is unchanged: two functions, resolve and read, both enforcing the repository root. The added modules are data transformations, so the escape tests of ADR-0006 still describe the boundary.

## Alternatives considered

- **Require `--config-via-node` for dependency-cruiser's own configuration.** Rejected: the drop-in would then need Node on the one repository whose maintainer the design names first in the adoption order.
- **Offer a read-only `fs`.** Rejected: it makes the configuration's output depend on the directory listing, which is the non-hermetic behaviour ADR-0006 exists to prevent.
