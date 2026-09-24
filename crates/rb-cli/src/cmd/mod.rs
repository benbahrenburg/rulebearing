//! One module per subcommand.
//!
//! - Plan: [Wave 1 § 2](../../../../docs/plans/pending/0001-wave-1-typescript-parity.md#2-lead-developer-section-step-by-step-implementation)
//! - Plan: [Wave 2, Step 12](../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#212-step-12-agent-subcommands-2g)
//!   (`docs`, `propose`, `place`, `decisions`, `test --generate`)
//! - Requirement: [FR-CLI-01](../../../../docs/prd.md#fr-cli-01)

pub mod adopt;
pub mod attest;
pub mod baseline;
pub mod can_import;
pub mod catalogue;
pub mod config;
pub mod count;
pub mod cruise;
pub mod decisions;
pub mod docs;
pub mod explain;
pub mod fmt;
pub mod generate;
pub mod hooks;
pub mod impact;
pub mod import;
pub mod init;
pub mod place;
pub mod plain;
pub mod propose;
pub mod rules;
pub mod summary;
pub mod test_rules;
