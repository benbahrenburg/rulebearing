# Fuzz targets

`cargo fuzz` over the inputs the tool must never panic on ([architecture § Security posture](../docs/architecture.md#security-posture), [NFR-SEC-01](../docs/prd.md#nfr-sec-01)). A malformed input must produce exit 2 with a named reason; a crash, an arithmetic overflow or a runaway allocation found here is a bug. This folder is a Cargo workspace of its own, so the main workspace never needs a nightly toolchain.

| Target | Parser | Seed corpus | Added by |
| --- | --- | --- | --- |
| `metadata_reader` | the ECMA-335 assembly reader and the portable PDB reader (`rb-extract-dotnet`) | ArchUnitNET's `TestAssembly.dll` and `.pdb` | [Wave 0, Step 9](../docs/plans/pending/0000-wave-0-spike.md#step-9-spike-b-rb-extract-dotnet-0d) |

The configuration front-ends (`rb-config`) and the `cruise-result` reader (`rb-ingest`) get targets with the plans that build their parsers.

```sh
cargo install cargo-fuzz                    # once; needs a nightly toolchain
fuzz/run.sh metadata_reader 600             # ten minutes, seeded from conformance/archunitnet/fixtures/
```

The [fuzz workflow](../.github/workflows/fuzz.yml) runs every target nightly for ten minutes and uploads any reproducing input. Every reproducer becomes a unit test in the crate it broke before the fix lands.
