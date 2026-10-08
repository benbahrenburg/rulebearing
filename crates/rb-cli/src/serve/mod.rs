//! `rulebearing serve --mcp | --lsp`: the architecture as tools for an agent and as diagnostics
//! for an editor, over standard input and output, from one warm graph.
//!
//! - Architecture: [`docs/architecture.md#agent-surface`](../../../../docs/architecture.md#agent-surface)
//! - Plan: [Wave 3, Steps 18 to 20](../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#25-steps-for-sub-wave-3e-serve---mcp-and-serve---lsp)
//! - Decision: [ADR-0021](../../../../docs/adr/0021-agent-surface-cli-first.md)
//! - Requirement: [FR-CLI-06](../../../../docs/prd.md#fr-cli-06), [NFR-SEC-01](../../../../docs/prd.md#nfr-sec-01)

pub mod graph;
