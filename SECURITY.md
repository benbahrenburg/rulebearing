# Security policy

## Reporting a vulnerability

Report privately through [GitHub's advisory form](https://github.com/benbahrenburg/rulebearing/security/advisories/new). Please do not open a public issue first. This is a personal project maintained in evenings, so expect an acknowledgement within a week.

## What the tool does, and does not do

Rulebearing reads source files, compiled assemblies and configuration, and writes reports. Its security posture is described in [docs/architecture.md](docs/architecture.md#security-posture) and is deliberately narrow:

- **No network.** The binary makes no outbound connection.
- **No code execution outside a sandbox.** A JavaScript configuration file runs in an embedded QuickJS interpreter with no filesystem access beyond the repository, no `process` and no timers ([ADR-0006](docs/adr/0006-embedded-quickjs-config-evaluator.md)). Node is spawned only with `--sidecar node` or `--config-via-node`, and the report records that it was.
- **Untrusted input.** Assemblies, portable PDBs, source files and configuration are parsed defensively. A malformed input must produce exit code 2 with a named reason, never a panic. Fuzz targets cover the metadata reader and the configuration parsers.

A sandbox escape, a network connection, a panic on malformed input, or a path traversal out of the repository is a vulnerability. A rule that produces a wrong finding is a bug; please open an issue for it.

## Supported versions

Until version 1.0, only the latest release receives fixes.
