//! One module per subcommand.
//!
//! - Plan: [Wave 1 § 2](../../../../docs/plans/pending/0001-wave-1-typescript-parity.md#2-lead-developer-section-step-by-step-implementation)
//! - Plan: [Wave 2, Step 12](../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#212-step-12-agent-subcommands-2g)
//!   (`docs`, `propose`, `place`, `decisions`, `test --generate`)
//! - Plan: [Wave 3, Step 4](../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#21-steps-for-sub-wave-3a-cache---affected-diff---exit-code-mode-strict)
//!   (`diff`)
//! - Plan: [Wave 3, Step 8](../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#22-steps-for-sub-wave-3b-the-remaining-reporters-and-the-sidecar)
//!   (`wrap-html`)
//! - Plan: [Wave 3, Steps 12 and 13](../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#23-steps-for-sub-wave-3c-presets-lifecycle-fields-snapshot-and-changelog)
//!   (`rules --unused`, `snapshot`, `changelog`)
//! - Requirement: [FR-CLI-01](../../../../docs/prd.md#fr-cli-01), [FR-CLI-07](../../../../docs/prd.md#fr-cli-07)

pub mod adopt;
pub mod attest;
pub mod baseline;
pub mod can_import;
pub mod catalogue;
pub mod changelog;
pub mod config;
pub mod count;
pub mod cruise;
pub mod decisions;
pub mod diff;
pub mod docs;
pub mod explain;
pub mod fmt;
pub mod generate;
pub mod hooks;
pub mod impact;
pub mod import;
pub mod init;
pub mod init_graph;
pub mod place;
pub mod plain;
pub mod propose;
pub mod rules;
pub mod snapshot;
pub mod summary;
pub mod test_rules;
pub mod wrap_html;
