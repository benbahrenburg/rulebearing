# Fuzz targets

Nightly `cargo fuzz` over the inputs the tool must never panic on ([architecture § Security posture](../docs/architecture.md#security-posture), [NFR-SEC-01](../docs/prd.md#nfr-sec-01)): the ECMA-335 metadata and portable PDB reader (`rb-extract-dotnet`), the two configuration front-ends (`rb-config`), and the `cruise-result` reader (`rb-ingest`). Targets are added by the plan that lands each parser; a malformed input must produce exit 2 with a named reason, never a panic.
