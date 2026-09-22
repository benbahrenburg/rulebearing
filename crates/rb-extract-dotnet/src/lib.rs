//! `rb-extract-dotnet`: the .NET extractor over ECMA-335 metadata, IL operands and portable PDBs.
//!
//! - Architecture: [`docs/architecture.md#extractors`](../../../docs/architecture.md#extractors)
//! - Decisions: [ADR-0011](../../../docs/adr/0011-read-dotnet-assemblies-not-source.md),
//!   [ADR-0003](../../../docs/adr/0003-dotnet-extractor-fallback.md) (the C# fallback trigger)
//! - Plans: [Wave 0, Spike B](../../../docs/plans/pending/0000-wave-0-spike.md),
//!   [Wave 2, sub-wave 2A](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md),
//!   [Wave 3](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md) (`--mode source`)
//! - Requirements: [FR-EXT-DN-01](../../../docs/prd.md#fr-ext-dn-01) to [FR-EXT-DN-04](../../../docs/prd.md#fr-ext-dn-04)
//! - Specification: ECMA-335 partition II; the portable PDB format; `ArchUnitNET`'s `TestAssembly`
//!   ([ADR-0009](../../../docs/adr/0009-conformance-suites-as-specification.md))
//!
//! Rule of the boundary: this crate reads assemblies and PDBs and writes `rb_model` types only.

/// The edge kinds this extractor records, from
/// [design § Dependency rules](../../../docs/artifacts/design.md#dependency-rules-the-whole-of-dependency-cruiser-1820).
pub const DEPENDENCY_KINDS: &[&str] = &[
    "inherits",
    "implements",
    "field",
    "signature",
    "body",
    "attribute",
    "generic-argument",
    "typeof",
];

/// The `dependencyTypes` vocabulary for .NET, from
/// [design § One engine, three languages](../../../docs/artifacts/design.md#one-engine-three-languages-one-monorepo).
pub const DEPENDENCY_TYPES: &[&str] = &[
    "local",
    "project",
    "package",
    "framework",
    "test-only",
    "signature-only",
    "unresolved",
];

/// Whether the four-byte magic of a PDB stream is the portable PDB signature `BSJB`.
///
/// A classic Windows PDB gives no file attribution and makes the run untrustworthy
/// ([ADR-0008](../../../docs/adr/0008-exit-code-contract.md)).
pub fn is_portable_pdb(header: &[u8]) -> bool {
    header.starts_with(b"BSJB")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_portable_pdb_magic() {
        assert!(is_portable_pdb(b"BSJB\x01\x00\x01\x00"));
        assert!(!is_portable_pdb(b"Microsoft C/C++ MSF 7.00"));
        assert!(!is_portable_pdb(b""));
    }

    #[test]
    fn vocabularies_match_the_design() {
        assert_eq!(DEPENDENCY_KINDS.len(), 8);
        assert!(DEPENDENCY_TYPES.contains(&"signature-only"));
    }
}
