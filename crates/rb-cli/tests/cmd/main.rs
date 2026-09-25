//! One CLI fixture per wave 2 agent command and form, run as a process against the wave 1
//! fixture tree (a TypeScript repository with one violation) and the `TestAssembly` graph.
//!
//! - Plan: [Wave 2, Step 12](../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#212-step-12-agent-subcommands-2g)
//!   ("one CLI fixture per command and form under `crates/rb-cli/tests/cmd/`")
//! - Contract: [ADR-0008](../../../../docs/adr/0008-exit-code-contract.md) (exit codes)
//! - Requirements: [FR-CLI-01](../../../../docs/prd.md#fr-cli-01), [FR-CLI-02](../../../../docs/prd.md#fr-cli-02)
//!
//! | Module | Command |
//! | --- | --- |
//! | [`docs`] | `docs --format agents-md / contributing / skill`, `--out`, `--verify` |
//! | [`propose`] | `propose --from/--to`, `--select/--where/--should`, `--from-example` |
//! | [`impact`] | `impact FILE [--depth N] [--json]` |
//! | [`place`] | `place --imports --imported-by --language` |
//! | [`generate`] | `test --generate [RULE] [--force]` |
//! | [`decisions`] | `decisions [--json]`, `decisions new` |

mod common;
mod decisions;
mod docs;
mod generate;
mod impact;
mod place;
mod propose;
